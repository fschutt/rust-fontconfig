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
#[cfg(target_os = "ios")]
use crate::utils::is_font_file;
use crate::FcParseFont;
#[cfg(target_os = "ios")]
use crate::OperatingSystem;

/// What a builder does after one step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuilderStep {
    /// Re-check that the registry still exists, then take another step.
    Continue,
    /// The build is complete or the registry is shutting down.
    Exit,
}

impl FcFontRegistry {
    /// Scout thread: enumerates font directories and populates the build queue.
    pub(crate) fn scout_thread(&self) {
        let font_dirs = self.scan_config.font_dirs.clone();
        let common_token_sets = self.scan_config.priority_token_sets();
        let lazy = self.lazy_scout.load(Ordering::Acquire);

        // iOS: use CoreText enumeration because sandbox denies read_dir.
        #[cfg(target_os = "ios")]
        {
            if self.os == OperatingSystem::IOS {
                let ios_paths = crate::mobile_ios::copy_available_font_urls();
                self.publish_ios_font_urls(ios_paths, &common_token_sets, lazy);
                self.mark_scan_complete();
                return;
            }
        }

        // Walk one directory at a time to minimize lock contention.
        for dir_path in font_dirs {
            if self.shutdown.load(Ordering::Relaxed) {
                return;
            }
            if std::fs::read_dir(&dir_path).is_err() {
                continue;
            }

            let mut dir_paths: Vec<PathBuf> = Vec::new();
            collect_font_files_recursive(dir_path, &mut dir_paths);

            if dir_paths.is_empty() {
                continue;
            }

            let Ok(mut known_paths) = self.known_paths.write() else {
                return;
            };
            let mut queue_opt = (!lazy).then(|| self.build_queue.lock().ok()).flatten();

            for path in &dir_paths {
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                let guessed_family = config::guess_family_from_filename(path);

                known_paths
                    .entry(guessed_family.clone())
                    .or_insert_with(Vec::new)
                    .push(path.clone());

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

            // Notify callers waiting on progress.
            self.progress.notify_all();
        }

        self.mark_scan_complete();
    }

    /// Merge a batch of CoreText-discovered font URLs into the registry.
    #[cfg(target_os = "ios")]
    fn publish_ios_font_urls(
        &self,
        ios_paths: Vec<PathBuf>,
        common_token_sets: &[Vec<alloc::string::String>],
        lazy: bool,
    ) {
        // Filter to recognized font extensions.
        let filtered: Vec<PathBuf> = ios_paths.into_iter().filter(|p| is_font_file(p)).collect();

        if filtered.is_empty() {
            return;
        }

        let Ok(mut known_paths) = self.known_paths.write() else {
            return;
        };
        let mut queue_opt = (!lazy).then(|| self.build_queue.lock().ok()).flatten();

        for path in &filtered {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let guessed_family = config::guess_family_from_filename(path);

            known_paths
                .entry(guessed_family.clone())
                .or_insert_with(Vec::new)
                .push(path.clone());

            if let Some(queue) = queue_opt.as_mut() {
                let all_tokens = config::tokenize_lowercase(stem);
                let priority = assign_scout_priority(&all_tokens, common_token_sets);
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

        self.progress.notify_all();
    }

    /// Builder thread loop: pops jobs from the priority queue, parses fonts,
    /// and inserts results into the registry.
    pub(crate) fn builder_thread(registry: std::sync::Weak<FcFontRegistry>) {
        while let Some(registry) = registry.upgrade() {
            if !Self::enter_step(&registry) {
                registry.leave_step();
                return;
            }
            let step = registry.builder_step();
            registry.leave_step();
            if step == BuilderStep::Exit {
                return;
            }
        }
    }

    /// Register this thread's handle and check if external holders exist.
    /// Threads exit if only threads hold the registry to avoid keeping it alive forever.
    pub(crate) fn enter_step(registry: &std::sync::Arc<Self>) -> bool {
        let thread_handles = registry.thread_handles.fetch_add(1, Ordering::AcqRel) + 1;
        std::sync::Arc::strong_count(registry) > thread_handles
    }

    /// Release the handle registered by [`enter_step`](Self::enter_step).
    pub(crate) fn leave_step(&self) {
        self.thread_handles.fetch_sub(1, Ordering::AcqRel);
    }

    /// One builder step: wait for a job (at most 100 ms) and process it.
    fn builder_step(&self) -> BuilderStep {
        if self.shutdown.load(Ordering::Relaxed) {
            return BuilderStep::Exit;
        }

        let lazy = self.lazy_scout.load(Ordering::Acquire);

        // Pop the highest-priority job.
        let job = {
            let mut queue = match self.build_queue.lock() {
                Ok(q) => q,
                Err(_) => return BuilderStep::Exit,
            };

            loop {
                if self.shutdown.load(Ordering::Relaxed) {
                    return BuilderStep::Exit;
                }

                if let Some(job) = queue.pop() {
                    self.in_flight.fetch_add(1, Ordering::AcqRel);
                    break job;
                }

                // Eager mode exits when done. Lazy mode waits indefinitely for jobs.
                if !lazy
                    && self.scan_complete.load(Ordering::Acquire)
                    && queue.is_empty()
                    && self.in_flight.load(Ordering::Acquire) == 0
                {
                    // Only the winner of this transition persists the cache.
                    let is_winner = self.try_complete_build();
                    // Release queue lock before touching the filesystem to avoid deadlocks and stalls.
                    drop(queue);
                    if is_winner && self.persist_on_complete.load(Ordering::Acquire) {
                        self.persist_cache_on_build_complete();
                    }
                    return BuilderStep::Exit;
                }

                // Wait for a job or in-flight parse to finish, with a timeout to re-check shutdown.
                queue = match self
                    .queue_condvar
                    .wait_timeout(queue, Duration::from_millis(100))
                {
                    Ok((queue, timeout)) if timeout.timed_out() => {
                        drop(queue);
                        return BuilderStep::Continue;
                    }
                    Ok((queue, _)) => queue,
                    Err(_) => return BuilderStep::Exit,
                };
            }
        };

        // Deduplication: the first builder to claim a path parses it.
        // Whatever happens below, the job is balanced by `finish_job`.
        let claimed = match self.processed_paths.lock() {
            Ok(mut processed) => processed.insert(job.path.clone()),
            Err(_) => false,
        };

        if claimed {
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

        self.finish_job();
        BuilderStep::Continue
    }

    /// Publish "the scout is done", notifying all waiters under lock.
    fn mark_scan_complete(&self) {
        {
            let _waiters = self.completed_paths.lock();
            self.scan_complete.store(true, Ordering::Release);
            self.progress.notify_all();
        }
        self.queue_condvar.notify_all();
    }

    /// Try to be the builder that completes the build.
    fn try_complete_build(&self) -> bool {
        let _waiters = self.completed_paths.lock();
        let won = self
            .build_complete
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        self.progress.notify_all();
        won
    }

    /// Decrement the `in_flight` counter for a popped job under the queue lock and notify waiters.
    fn finish_job(&self) {
        let guard = self.build_queue.lock();
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
        drop(guard);
        self.queue_condvar.notify_all();
    }
}

/// Append font files under `dir` to `results` (cycle-safe and depth-bounded).
fn collect_font_files_recursive(dir: PathBuf, results: &mut Vec<PathBuf>) {
    results.extend(crate::utils::collect_font_files(&dir));
}

impl FcFontRegistry {
    /// Write the freshly-completed scan to the on-disk manifest. Runs on the builder thread.
    #[cfg(all(feature = "cache", not(target_family = "wasm")))]
    fn persist_cache_on_build_complete(&self) {
        // Don't write an empty manifest if no disk fonts were found.
        if self.cache.state_read().disk_fonts.is_empty() {
            return;
        }
        let _ = self.save_to_disk_cache();
    }

    /// Persistence is compiled out without the `cache` feature.
    #[cfg(not(all(feature = "cache", not(target_family = "wasm"))))]
    fn persist_cache_on_build_complete(&self) {}
}
