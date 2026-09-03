//! Tests for `build_complete` and registry thread lifecycle.

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
        let dir =
            std::env::temp_dir().join(format!("rfc-completion-{}-{nanos}", std::process::id()));
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

    // Queue jobs directly before a builder exists
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

    // Spin until build_complete flips
    let deadline = Instant::now() + Duration::from_secs(30);
    while !registry.is_build_complete() {
        assert!(Instant::now() < deadline, "timeout");
        std::hint::spin_loop();
    }

    let completed_at_completion = registry
        .completed_paths
        .lock()
        .expect("completed lock")
        .len();
    let fonts_at_completion = registry.list().len();

    assert_eq!(completed_at_completion, FILES, "not all files processed");
    assert!(fonts_at_completion >= patterns_per_file, "missing patterns");
    assert_eq!(
        registry
            .in_flight
            .load(std::sync::atomic::Ordering::Acquire),
        0,
        "jobs still in flight"
    );

    registry.shutdown();
}

/// Verify dropping the registry ends background threads.
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
    assert!(weak.upgrade().is_none(), "thread still alive");
}

/// Verify wait_for_scout returns promptly.
#[test]
fn wait_for_scout_returns_promptly_when_the_build_completes() {
    let registry = FcFontRegistry::new_with_config(FcScanConfig::empty());
    registry.set_persist_on_complete(false);
    registry.spawn_scout_and_builders();
    let started = Instant::now();
    registry.wait_for_scout();
    assert!(registry.is_build_complete());
    assert!(started.elapsed() < Duration::from_secs(2), "took too long");
}
