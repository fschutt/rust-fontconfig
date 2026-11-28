//! How many builder threads a scan starts: `set_builder_threads` sets it, `0` is what it always was (one fewer than the
//! machine's cores, at least one), and a scan with ONE builder finds what a scan with the default finds.
//!
//!   cargo test --features cache,async-registry,parsing --test builder_threads

#![cfg(all(
    feature = "async-registry",
    feature = "parsing",
    not(feature = "single-thread-unsafe-locks")
))]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rust_fontconfig::config::FcScanConfig;
use rust_fontconfig::registry::FcFontRegistry;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

/// A scan of the fixtures with `threads` builders (0: the default): how many faces it found.
fn scan(threads: usize) -> usize {
    let registry = FcFontRegistry::new_with_config(FcScanConfig {
        font_dirs: vec![fixtures()],
        priority_families: Vec::new(),
    });
    registry.set_persist_on_complete(false);
    registry.set_builder_threads(threads);
    registry.spawn_scout_and_builders();
    let deadline = Instant::now() + Duration::from_secs(60);
    while !registry.is_build_complete() {
        assert!(Instant::now() < deadline, "the scan with {threads} builders did not complete");
        std::thread::sleep(Duration::from_millis(5));
    }
    registry.list().len()
}

#[test]
fn the_default_is_one_fewer_than_the_cores_and_at_least_one() {
    let registry = FcFontRegistry::new_with_config(FcScanConfig {
        font_dirs: Vec::new(),
        priority_families: Vec::new(),
    });
    let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
    assert_eq!(registry.builder_thread_count(), cores.saturating_sub(1).max(1));
}

#[test]
fn the_count_that_was_set_is_the_count_and_zero_is_the_machines_again() {
    let registry = FcFontRegistry::new_with_config(FcScanConfig {
        font_dirs: Vec::new(),
        priority_families: Vec::new(),
    });
    let machines = registry.builder_thread_count();
    registry.set_builder_threads(1);
    assert_eq!(registry.builder_thread_count(), 1);
    registry.set_builder_threads(5);
    assert_eq!(registry.builder_thread_count(), 5);
    registry.set_builder_threads(0);
    assert_eq!(registry.builder_thread_count(), machines);
}

#[test]
fn a_scan_with_one_builder_finds_what_a_scan_with_the_default_finds() {
    let default = scan(0);
    assert!(default > 0, "the fixtures hold fonts");
    assert_eq!(scan(1), default);
}
