//! Background thread implementations for the async font registry.
//!
//! - [`FcFontRegistry::scout_thread`]: Enumerates font directories and populates the build queue.
//! - [`FcFontRegistry::builder_thread`]: Pops jobs from the queue, parses fonts, inserts results.

use alloc::vec::Vec;

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::config;
use crate::registry::FcFontRegistry;
use crate::scoring::{assign_scout_priority, FcBuildJob};
use crate::utils::is_font_file;
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

        let lazy = self.lazy_scout.load(Ordering::Acquire);

        let Ok(mut known_paths) = self.known_paths.write() else { return };
        let mut queue_opt = (!lazy).then(|| self.build_queue.lock().ok()).flatten();

        for path in &all_font_paths {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let guessed_family = config::guess_family_from_filename(path);

            known_paths
                .entry(guessed_family.clone())
                .or_insert_with(Vec::new)
                .push(path.clone());

            // Lazy-scout mode (scout-on-demand): skip the eager
            // `queue.push` so builders only parse fonts the caller
            // explicitly requests via `request_fonts` /
            // `request_and_resolve_with_scripts`. In eager mode the
            // old behaviour is preserved — push everything at
            // Scout/Common priority so background threads keep
            // running and the on-disk cache auto-populates.
            if let Some(queue) = queue_opt.as_mut() {
                let all_tokens = config::tokenize_lowercase(stem);
                let priority = assign_scout_priority(&all_tokens, &common_token_sets);
                queue.push(FcBuildJob {
                    priority,
                    path: path.clone(),
                    font_index: None,
                    guessed_family,
                });
            }
        }

        if let Some(mut queue) = queue_opt {
            queue.sort();
            drop(queue);
        }
        drop(known_paths);

        self.scan_complete.store(true, Ordering::Release);
        self.queue_condvar.notify_all();
        self.progress.notify_all();
    }

    /// Builder thread loop: pops jobs from the priority queue, parses fonts,
    /// and inserts results into the registry.
    ///
    /// Exit conditions:
    ///
    /// - `shutdown` is set (registry is dropping).
    /// - In **eager** mode: once the scout finishes the initial
    ///   directory walk, queue empties, and every queued path is
    ///   processed. At that point `build_complete` flips and the
    ///   thread returns.
    /// - In **lazy-scout** mode: the thread keeps waiting on
    ///   `queue_condvar` indefinitely, because the scout does not
    ///   pre-queue anything — all jobs come in later from
    ///   [`FcFontRegistry::request_fonts`]. Exiting on the
    ///   "queue empty + scan complete" condition (as the eager
    ///   path does) would race the Critical job push and cause the
    ///   request to hang forever.
    pub fn builder_thread(&self) {
        loop {
            if self.shutdown.load(Ordering::Relaxed) {
                return;
            }

            let lazy = self.lazy_scout.load(Ordering::Acquire);

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

                    // Eager mode: exit once the scout is done and
                    // everything it queued has drained.
                    //
                    // Lazy mode: keep waiting — `request_fonts` is
                    // the sole source of jobs and can fire at any
                    // time during the layout pass.
                    if !lazy
                        && self.scan_complete.load(Ordering::Acquire)
                        && queue.is_empty()
                    {
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
