//! Background thread implementations for the async font registry.
//!
//! - [`FcFontRegistry::scout_thread`]: Enumerates font directories and populates the build queue.
//! - [`FcFontRegistry::builder_thread`]: Pops jobs from the queue, parses fonts, inserts results.

use alloc::string::String;
use alloc::vec::Vec;

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::config;
use crate::registry::FcFontRegistry;
use crate::scoring::{assign_scout_priority, FcBuildJob};
use crate::utils::is_font_file;
use crate::FcFontCache;
use crate::FcParseFont;

impl FcFontRegistry {
    /// Scout thread: enumerates font directories and populates the build queue.
    ///
    /// 1. Walks all OS font directories recursively, collecting font file paths.
    /// 2. Tokenizes each filename and assigns a priority (High for common
    ///    OS fonts, Low for everything else).
    /// 3. Populates `known_paths` (family → file paths) and `build_queue`.
    /// 4. Signals `scan_complete` when done.
    pub fn scout_thread(&self) {
        let font_dirs = config::font_directories(self.os);

        let mut all_font_paths: Vec<PathBuf> = Vec::new();

        for dir_path in font_dirs {
            if self.shutdown.load(Ordering::Relaxed) {
                return;
            }
            if std::fs::read_dir(&dir_path).is_err() {
                continue;
            }
            collect_font_files_recursive(dir_path, &mut all_font_paths);
        }

        // Pre-tokenize common families once (not per-file)
        let common_token_sets = config::tokenize_common_families(self.os);

        if let (Ok(mut known_paths), Ok(mut queue)) =
            (self.known_paths.write(), self.build_queue.lock())
        {
            for path in &all_font_paths {
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                let all_tokens: Vec<String> = FcFontCache::extract_font_name_tokens(stem)
                    .into_iter()
                    .map(|t| t.to_lowercase())
                    .collect();

                let guessed_family = config::guess_family_from_filename(path);

                known_paths
                    .entry(guessed_family.clone())
                    .or_insert_with(Vec::new)
                    .push(path.clone());

                let priority = assign_scout_priority(&all_tokens, &common_token_sets);

                queue.push(FcBuildJob {
                    priority,
                    path: path.clone(),
                    font_index: None,
                    guessed_family,
                });
            }

            queue.sort();
        }

        self.scan_complete.store(true, Ordering::Release);
        self.queue_condvar.notify_all();
        self.progress.notify_all();
    }

    /// Builder thread loop: pops jobs from the priority queue, parses fonts,
    /// and inserts results into the registry.
    pub fn builder_thread(&self) {
        loop {
            if self.shutdown.load(Ordering::Relaxed) {
                return;
            }

            // Pop the highest-priority job
            let job = {
                let mut queue = match self.build_queue.lock() {
                    Ok(q) => q,
                    Err(_) => return,
                };

                loop {
                    if self.shutdown.load(Ordering::Relaxed) {
                        return;
                    }

                    if let Some(job) = queue.pop() {
                        break job;
                    }

                    // If scan is complete and queue is empty, we're done
                    if self.scan_complete.load(Ordering::Acquire) && queue.is_empty() {
                        self.build_complete.store(true, Ordering::Release);
                        self.progress.notify_all();
                        return;
                    }

                    // Wait for new jobs
                    queue = match self
                        .queue_condvar
                        .wait_timeout(queue, Duration::from_millis(100))
                    {
                        Ok(result) => result.0,
                        Err(_) => return,
                    };
                }
            };

            // Deduplication: skip if already processed
            {
                let mut processed = match self.processed_paths.lock() {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                if processed.contains(&job.path) {
                    continue;
                }
                processed.insert(job.path.clone());
            }

            // Parse the font file
            if let Some(results) = FcParseFont(&job.path) {
                for (pattern, font_path) in results {
                    self.insert_font(pattern, font_path);
                }
            }

            // Mark this file as fully completed (patterns inserted)
            if let Ok(mut completed) = self.completed_paths.lock() {
                completed.insert(job.path.clone());
            }

            // Notify waiting threads that a font has been completed
            self.progress.notify_all();
        }
    }
}

/// Recursively collect font files from a directory.
fn collect_font_files_recursive(dir: PathBuf, results: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            collect_font_files_recursive(path, results);
        } else if is_font_file(&path) {
            results.push(path);
        }
    }
}
