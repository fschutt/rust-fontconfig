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
    ///
    /// 1. Walks the injected scan directories (`scan_config.font_dirs`)
    ///    recursively, collecting font file paths.
    /// 2. Tokenizes each filename and assigns a priority (High for the
    ///    injected priority families, Low for everything else).
    /// 3. Populates `known_paths` (family → file paths) and `build_queue`.
    /// 4. Signals `scan_complete` when done.
    ///
    /// Both the directory list and the priority token sets come from the
    /// [`crate::config::FcScanConfig`] injected at registry construction,
    /// never from the per-OS tables in `config` directly - the host
    /// decides where fonts live and which families matter, this thread
    /// only executes that decision. `self.os` remains in play solely for
    /// the iOS CoreText enumeration branch below.
    pub(crate) fn scout_thread(&self) {
        let font_dirs = self.scan_config.font_dirs.clone();
        let common_token_sets = self.scan_config.priority_token_sets();
        let lazy = self.lazy_scout.load(Ordering::Acquire);

        // iOS: the app sandbox denies `read_dir` on `/System/Library/...`
        // even though every individual font URL is openable. CoreText is
        // the only enumeration path. Branch off here, hand the resulting
        // PathBufs to the same `known_paths` / `build_queue` merge that
        // the per-directory walk uses.
        #[cfg(target_os = "ios")]
        {
            if self.os == OperatingSystem::IOS {
                let ios_paths = crate::mobile_ios::copy_available_font_urls();
                self.publish_ios_font_urls(ios_paths, &common_token_sets, lazy);
                self.mark_scan_complete();
                return;
            }
        }

        // Per-directory publish: walk one top-level font directory
        // at a time, collect its paths, then take a brief write
        // lock to merge into `known_paths`. Readers blocked on
        // `known_paths.read()` wake up between directories and can
        // immediately probe any family whose file already landed.
        //
        // Before this change the scout held the write lock for the
        // *entire* FS walk — ~130 ms on macOS cold — so every
        // consumer that called `request_fonts_fast` during init
        // stalled the whole time. Now the critical-section per
        // directory is just "insert N paths into a BTreeMap",
        // typically <2 ms per directory on macOS.
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

            let Ok(mut known_paths) = self.known_paths.write() else { return };
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

            // Notify callers waiting on `progress` that new paths
            // landed. `request_fonts_fast` re-checks its family
            // lookup on every wake-up; a DOM that only needs
            // Helvetica can proceed the moment the directory
            // containing HelveticaNeue.ttc has been merged.
            self.progress.notify_all();
        }

        self.mark_scan_complete();
    }

    /// Merge a batch of CoreText-discovered font URLs into the registry,
    /// mirroring the per-directory publish path used by `scout_thread`.
    ///
    /// iOS-only: the standard `read_dir` walk returns nothing inside the app
    /// sandbox, so this is the only way the async registry sees system fonts.
    #[cfg(target_os = "ios")]
    fn publish_ios_font_urls(
        &self,
        ios_paths: Vec<PathBuf>,
        common_token_sets: &[Vec<alloc::string::String>],
        lazy: bool,
    ) {
        // Filter to recognized font extensions. CoreText also returns app-bundled
        // resources occasionally, so the filter keeps us pruning anything not
        // parseable.
        let filtered: Vec<PathBuf> = ios_paths
            .into_iter()
            .filter(|p| is_font_file(p))
            .collect();

        if filtered.is_empty() {
            return;
        }

        let Ok(mut known_paths) = self.known_paths.write() else { return };
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
     /// Body of a builder thread. The registry is held only for the duration
    /// of one step, so a registry whose last `Arc` was dropped is freed — and
    /// its builders exit — within one step (at most one 100 ms wait or one
    /// font parse). Builders used to hold an `Arc` themselves, which made
    /// `Drop` unreachable and left N threads polling for the life of the
    /// process in lazy mode.
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

    /// Register this thread's handle and report whether anyone else still
    /// holds the registry. Threads upgrade their `Weak` at different times,
    /// so a plain upgrade would keep succeeding while another thread is
    /// inside its step — the threads would keep each other alive forever.
    /// Counting the threads' own handles tells them apart from an embedder's:
    /// when every strong handle is a thread's, nobody outside holds the
    /// registry and the thread exits. Pair with [`leave_step`](Self::leave_step).
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

        // Pop the highest-priority job. `in_flight` is bumped while the
        // queue lock is held, so a builder that finds the queue empty
        // also sees the job that just left it.
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

                // Eager mode: exit once the scout is done, everything it
                // queued has drained, AND no builder is still parsing.
                //
                // Lazy mode: keep waiting — `request_fonts` is the sole
                // source of jobs and can fire at any time during the
                // layout pass.
                if !lazy
                    && self.scan_complete.load(Ordering::Acquire)
                    && queue.is_empty()
                    && self.in_flight.load(Ordering::Acquire) == 0
                {
                    // Exactly one builder wins this transition; the others
                    // simply exit. Only the winner persists, so N builders
                    // cannot write the same manifest concurrently.
                    let is_winner = self.try_complete_build();
                    // Release the build-queue lock BEFORE touching the
                    // filesystem: persisting takes the cache state lock and
                    // does real I/O, and holding the queue lock across that
                    // would both stall `request_fonts` and invert the
                    // queue-then-state lock order used elsewhere.
                    drop(queue);
                    if is_winner && self.persist_on_complete.load(Ordering::Acquire) {
                        self.persist_cache_on_build_complete();
                    }
                    return BuilderStep::Exit;
                }

                // Wait for a job, or for an in-flight parse to finish
                // (`finish_job` notifies this condvar). A timed-out wait
                // ends the step so the thread re-checks whether the registry
                // still exists.
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

    /// Publish "the scout is done". The flag is a predicate that `progress`
    /// waiters read under `completed_paths`, so it is stored and the waiters
    /// notified under that same lock: a notify that lands between a waiter's
    /// check and its wait is otherwise lost, and the waiter sleeps to its
    /// deadline — up to 5 s for `wait_for_scout`.
    fn mark_scan_complete(&self) {
        {
            let _waiters = self.completed_paths.lock();
            self.scan_complete.store(true, Ordering::Release);
            self.progress.notify_all();
        }
        self.queue_condvar.notify_all();
    }

    /// Try to be the builder that completes the build; same locking argument
    /// as `mark_scan_complete`. Called with the queue lock held — queue then
    /// `completed_paths` is the order every other path keeps to.
    fn try_complete_build(&self) -> bool {
        let _waiters = self.completed_paths.lock();
        let won = self
            .build_complete
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        self.progress.notify_all();
        won
    }

    /// Balance the `in_flight` bump of a popped job. The decrement happens
    /// under the queue lock so it cannot interleave with another builder's
    /// empty-queue check, and the queue condvar is notified so a builder
    /// waiting for the last parse re-checks the completion condition at
    /// once instead of on its next 100 ms tick.
    fn finish_job(&self) {
        let guard = self.build_queue.lock();
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
        drop(guard);
        self.queue_condvar.notify_all();
    }
}

/// Font files under `dir`, appended to `results`. See
/// [`crate::utils::collect_font_files`]: cycle-safe and depth-bounded, so a
/// symlink loop in a font directory no longer overflows the scout's stack
/// (which aborts the whole process — a stack overflow is not a panic).
fn collect_font_files_recursive(dir: PathBuf, results: &mut Vec<PathBuf>) {
    results.extend(crate::utils::collect_font_files(&dir));
}

impl FcFontRegistry {
    /// Write the freshly-completed scan to the on-disk manifest.
    ///
    /// **Why this lives here.** Before this, `save_to_disk_cache` had no caller
    /// anywhere: not in this crate, and not in the two known consumers. The
    /// manifest at `dirs::cache_dir()/rfc/fonts/manifest.bin` was therefore
    /// never created, so `load_from_disk_cache` missed on *every* launch and
    /// every process paid the full cold scan (~190 ms on macOS with ~370
    /// system fonts) that the cache exists to avoid. Making persistence a
    /// property of "the scan finished" rather than something each embedder has
    /// to remember to call is the only shape in which it cannot be forgotten
    /// again.
    ///
    /// Runs on the builder thread that just finished, immediately before that
    /// thread exits — never on a caller's thread, so it cannot add latency to
    /// layout.
    #[cfg(all(feature = "cache", not(target_family = "wasm")))]
    fn persist_cache_on_build_complete(&self) {
        // Nothing discovered (e.g. a registry that only ever held memory
        // fonts): writing an empty manifest would make the next launch load a
        // cache that claims the system has no fonts.
        if self.cache.state_read().disk_fonts.is_empty() {
            return;
        }
        let _ = self.save_to_disk_cache();
    }

    /// Persistence is compiled out without the `cache` feature.
    #[cfg(not(all(feature = "cache", not(target_family = "wasm"))))]
    fn persist_cache_on_build_complete(&self) {}
}
