use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::registry::FcFontRegistry;
use crate::FcParseFont;

impl FcFontRegistry {
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
                        self.request_complete.notify_all();
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

            // Check if any pending requests are now satisfied
            self.check_and_signal_pending_requests();
        }
    }
}
