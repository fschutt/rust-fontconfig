//! `build_complete` means every queued font is in the cache — not merely that
//! every job was handed to a builder.
//!
//! Before 5.0 the completion check was "scout done and queue empty". A builder
//! that had just popped the last job and released the queue lock was still
//! parsing it while another builder found the queue empty, won the
//! `build_complete` transition, woke every waiter and persisted the manifest.
//! `wait_for_scout` returned early and the manifest could be short by up to
//! N-1 fonts. The registry now counts in-flight parses under the queue lock
//! and completes only when that count is zero.
//!
//! Hermetic: scans no directory; the jobs are pushed by hand and point at
//! copies of the bundled test fixture. Not compiled under
//! `single-thread-unsafe-locks`, which spawns no threads by design.

#![cfg(all(
    feature = "async-registry",
    feature = "parsing",
    not(feature = "single-thread-unsafe-locks")
))]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rust_fontconfig::config::FcScanConfig;
use rust_fontconfig::registry::FcFontRegistry;
use rust_fontconfig::scoring::{FcBuildJob, Priority};
use rust_fontconfig::FcParseFontBytes;

const FIXTURE: &[u8] = include_bytes!("fixtures/InstrumentSerif-Regular.ttf");

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rfc-completion-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn build_complete_means_every_font_is_in_the_cache() {
    const FILES: usize = 48;

    let tmp = TempDir::new();
    let paths: Vec<PathBuf> = (0..FILES)
        .map(|i| {
            let path = tmp.0.join(format!("face-{i:02}.ttf"));
            std::fs::write(&path, FIXTURE).expect("write fixture copy");
            path
        })
        .collect();
    let patterns_per_file = FcParseFontBytes(FIXTURE, "fixture")
        .expect("the fixture parses")
        .len();
    assert!(patterns_per_file >= 1);

    // Scan nothing; every job is queued by hand before a builder exists, so
    // the builders start with a full queue and the scout's (empty) scan
    // completes almost immediately.
    let registry = FcFontRegistry::new_with_config(FcScanConfig::empty());
    registry.set_persist_on_complete(false);
    {
        let mut queue = registry.build_queue.lock().expect("queue lock");
        for path in &paths {
            queue.push(FcBuildJob {
                priority: Priority::Critical,
                path: path.clone(),
                font_index: None,
                guessed_family: "instrumentserif".to_string(),
            });
        }
    }
    registry.spawn_scout_and_builders();

    // Spin (no sleep) so the snapshot is taken as close as possible to the
    // moment `build_complete` flips: that is the window the old check left
    // open.
    let deadline = Instant::now() + Duration::from_secs(30);
    while !registry.is_build_complete() {
        assert!(Instant::now() < deadline, "the build did not complete within 30s");
        std::hint::spin_loop();
    }
    // `completed_paths` is written only after a file's patterns are in the
    // cache, so a full completed set at this instant means every font is
    // in the cache. (`list()` cannot count them: the copies carry identical
    // name tables and the pattern map is keyed by the pattern.)
    let completed_at_completion = registry.completed_paths.lock().expect("completed lock").len();
    let fonts_at_completion = registry.list().len();

    assert_eq!(
        completed_at_completion, FILES,
        "build_complete flipped with files still being parsed"
    );
    assert!(
        fonts_at_completion >= patterns_per_file,
        "the fixture's patterns must be in the cache at completion"
    );
    assert_eq!(
        registry.in_flight.load(std::sync::atomic::Ordering::Acquire),
        0,
        "no job may be in flight once the build is complete"
    );

    registry.shutdown();
}

/// Dropping the last handle frees the registry: builders hold only a `Weak`,
/// so they exit within one step instead of polling for the life of the
/// process. Lazy mode is the case that used to leak — nothing ever completed
/// the build, so the threads never exited and `Drop` never ran.
#[test]
fn dropping_the_registry_ends_its_threads() {
    let registry = FcFontRegistry::new_with_config(FcScanConfig::empty());
    registry.set_scout_lazy(true);
    registry.set_persist_on_complete(false);
    registry.spawn_scout_and_builders();
    let weak = std::sync::Arc::downgrade(&registry);
    drop(registry);

    let deadline = Instant::now() + Duration::from_secs(5);
    while weak.upgrade().is_some() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(weak.upgrade().is_none(), "a builder thread still holds the registry alive");
}

/// With nothing to scan, the only wake-up `wait_for_scout` can get is the
/// final one; it must not be lost (a lost one meant sleeping to the 5 s
/// deadline).
#[test]
fn wait_for_scout_returns_promptly_when_the_build_completes() {
    let registry = FcFontRegistry::new_with_config(FcScanConfig::empty());
    registry.set_persist_on_complete(false);
    registry.spawn_scout_and_builders();
    let started = Instant::now();
    registry.wait_for_scout();
    assert!(registry.is_build_complete());
    assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
}
