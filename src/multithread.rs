use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::registry::FcFontRegistry;
use crate::FcParseFont;

/// A Builder thread: pops jobs from the priority queue, parses fonts, and inserts
/// results into the registry.
pub fn builder_thread(registry: &FcFontRegistry) {
    loop {
        if registry.shutdown.load(Ordering::Relaxed) {
            return;
        }

        // Pop the highest-priority job
        let job = {
            let mut queue = registry.build_queue.lock().unwrap();

            loop {
                if registry.shutdown.load(Ordering::Relaxed) {
                    return;
                }

                if let Some(job) = queue.pop() {
                    break job;
                }

                // If scan is complete and queue is empty, we're done
                if registry.scan_complete.load(Ordering::Acquire) && queue.is_empty() {
                    registry.build_complete.store(true, Ordering::Release);
                    registry.request_complete.notify_all();
                    return;
                }

                // Wait for new jobs
                queue = registry
                    .queue_condvar
                    .wait_timeout(queue, Duration::from_millis(100))
                    .unwrap()
                    .0;
            }
        };

        // Deduplication: skip if already processed
        {
            let mut processed = registry.processed_paths.lock().unwrap();
            if processed.contains(&job.path) {
                continue;
            }
            processed.insert(job.path.clone());
        }

        // Parse the font file
        if let Some(results) = FcParseFont(&job.path) {
            for (pattern, font_path) in results {
                registry.insert_font(pattern, font_path);
            }
        }

        // Mark this file as fully completed (patterns inserted)
        {
            let mut completed = registry.completed_paths.lock().unwrap();
            completed.insert(job.path.clone());
        }

        // Check if any pending requests are now satisfied
        registry.check_and_signal_pending_requests();
    }
}
