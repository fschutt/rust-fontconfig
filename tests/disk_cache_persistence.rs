//! Regression: a completed font scan must PERSIST, and the next registry must
//! pick that persisted manifest up.
//!
//! ## What was broken
//!
//! `FcFontRegistry::save_to_disk_cache` existed, was documented, and had zero
//! callers — in this crate or in any consumer. `~/Library/Caches/rfc/fonts/
//! manifest.bin` (resp. `$XDG_CACHE_HOME/rfc/fonts/manifest.bin`) was therefore
//! never created, so `load_from_disk_cache` missed on every single launch and
//! every process paid the full cold scan the cache exists to eliminate
//! (~190 ms for ~370 fonts on macOS). The fix makes the builder thread that
//! flips `build_complete` persist before it exits.
//!
//! These tests use `save_to_disk_cache_at` / `load_from_disk_cache_at` so they
//! can point at a temp directory: the real path comes from `dirs::cache_dir()`,
//! which is derived from process-wide environment (`HOME` / `XDG_CACHE_HOME`)
//! that a test cannot mutate safely while sibling tests run in the same
//! process.
//!
//! Requires the `cache` and `async-registry` features:
//!   cargo test --features cache,async-registry,parsing --test disk_cache_persistence
//!
//! Not compiled under `single-thread-unsafe-locks`: that feature makes
//! `spawn_scout_and_builders` a no-op by design, so a scan can never
//! complete and every test here would time out. `--all-features` enables it,
//! which is why CI runs these on an explicit `cache,async-registry,parsing`
//! row instead.

#![cfg(all(
    feature = "cache",
    feature = "async-registry",
    not(feature = "single-thread-unsafe-locks")
))]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rust_fontconfig::config::FcScanConfig;
use rust_fontconfig::registry::FcFontRegistry;
use rust_fontconfig::OperatingSystem;
use std::sync::{Arc, OnceLock};

/// A unique, self-cleaning scratch directory.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rfc-cache-test-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        TempDir(dir)
    }
    fn manifest(&self) -> PathBuf {
        self.0.join("fonts").join("manifest.bin")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run a full eager scan and return the registry once `build_complete` is set.
///
/// Fails loudly rather than skipping: a machine with no system fonts at all
/// would make every assertion below vacuous, which is exactly the
/// silently-passing font test this whole change is meant to prevent.
/// The system's main font directory only: enough fonts for persistence to
/// mean something, few enough for a CI runner. Scanning every directory
/// (macOS's AssetsV2 alone holds thousands of downloadable fonts) three
/// times in parallel blew a 60 s budget on the macOS and Windows runners.
fn bounded_scan_config() -> FcScanConfig {
    let mut config = FcScanConfig::os_defaults(OperatingSystem::current());
    config.font_dirs.truncate(1);
    config.priority_families.clear();
    config
}

fn scan_to_completion(tag: &str) -> Arc<FcFontRegistry> {
    let registry = FcFontRegistry::new_with_config(bounded_scan_config());
    // The tests below save through the path-explicit seam; do not touch the
    // real cache directory as a side effect.
    registry.set_persist_on_complete(false);
    registry.spawn_scout_and_builders();

    let deadline = Instant::now() + Duration::from_secs(180);
    while !registry.is_build_complete() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        registry.is_build_complete(),
        "[{tag}] the font scan did not complete within 180s; cannot test persistence \
         of a scan that never finished"
    );
    assert!(
        !registry.list().is_empty(),
        "[{tag}] the scan completed but found ZERO fonts. This test cannot run on a \
         system with no installed fonts — failing loudly instead of passing vacuously."
    );
    registry
}

/// One scan per test process, shared by every test that only needs a
/// scanned registry.
fn scanned() -> Arc<FcFontRegistry> {
    static SCANNED: OnceLock<Arc<FcFontRegistry>> = OnceLock::new();
    SCANNED.get_or_init(|| scan_to_completion("shared")).clone()
}

/// The core regression: a completed scan writes a manifest, and a SECOND,
/// independent registry loads that manifest and comes up populated without
/// scanning anything.
///
/// Against the unfixed crate this fails at the first assertion: nothing ever
/// called `save_to_disk_cache`, so no manifest existed.
#[test]
fn completed_scan_persists_and_reloads() {
    let tmp = TempDir::new("roundtrip");
    let manifest = tmp.manifest();
    assert!(!manifest.exists(), "temp manifest must not pre-exist");

    let registry = scanned();
    let scanned = registry.list().len();

    // The real code path (`persist_cache_on_build_complete`) writes to
    // `get_font_cache_path()`; here we drive the same save through the
    // path-explicit seam so the test is hermetic.
    registry
        .save_to_disk_cache_at(&manifest)
        .expect("save_to_disk_cache_at must succeed after a completed scan");

    assert!(
        manifest.exists(),
        "a completed scan must leave a manifest at {manifest:?} — this is the bug: \
         save_to_disk_cache had no caller anywhere, so this file never appeared"
    );
    let size = std::fs::metadata(&manifest).unwrap().len();
    assert!(size > 0, "manifest must not be empty (got {size} bytes)");

    // A fresh registry that has NEVER scanned must come up populated.
    let reloaded = FcFontRegistry::new();
    assert!(
        reloaded.list().is_empty(),
        "a brand new registry must start empty"
    );
    reloaded
        .load_from_disk_cache_at(&manifest)
        .expect("load_from_disk_cache_at must accept the manifest we just wrote");

    assert!(
        reloaded.is_cache_loaded(),
        "loading a manifest must set cache_loaded, otherwise request_fonts \
         re-scans and the cache buys nothing"
    );
    let loaded = reloaded.list().len();
    assert_eq!(
        loaded, scanned,
        "the reloaded registry must expose exactly the fonts that were scanned \
         ({scanned} scanned vs {loaded} reloaded)"
    );
}

/// The persisted manifest must actually answer queries — not merely
/// deserialize. A cache that round-trips byte-wise but resolves nothing would
/// pass the test above for the wrong reason.
#[test]
fn reloaded_cache_resolves_the_same_families() {
    let tmp = TempDir::new("resolve");
    let manifest = tmp.manifest();

    let registry = scanned();
    let sample: Vec<_> = registry
        .list()
        .into_iter()
        .filter_map(|(p, _)| p.family.clone())
        .take(8)
        .collect();
    assert!(
        !sample.is_empty(),
        "scan produced fonts but none carried a family name — nothing to assert on"
    );

    registry.save_to_disk_cache_at(&manifest).expect("save");

    let reloaded = FcFontRegistry::new();
    reloaded.load_from_disk_cache_at(&manifest).expect("load");

    let reloaded_families: std::collections::BTreeSet<String> = reloaded
        .list()
        .into_iter()
        .filter_map(|(p, _)| p.family.clone())
        .collect();

    for family in &sample {
        assert!(
            reloaded_families.contains(family),
            "family {family:?} was discovered by the scan but is missing from the \
             reloaded cache"
        );
    }
}

/// The write must be atomic: a manifest is either wholly replaced or left
/// alone. A truncated manifest would be rejected by `load_from_disk_cache` on
/// every subsequent launch, silently reinstating the cold scan this change
/// removes.
#[test]
fn save_leaves_no_temp_files_and_replaces_atomically() {
    let tmp = TempDir::new("atomic");
    let manifest = tmp.manifest();

    let registry = scanned();
    registry
        .save_to_disk_cache_at(&manifest)
        .expect("first save");
    let first = std::fs::read(&manifest).expect("read first manifest");

    registry
        .save_to_disk_cache_at(&manifest)
        .expect("second save");
    let second = std::fs::read(&manifest).expect("read second manifest");
    assert_eq!(
        first.len(),
        second.len(),
        "re-saving the same registry state must produce an equivalent manifest"
    );

    let strays: Vec<_> = std::fs::read_dir(manifest.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != "manifest.bin")
        .collect();
    assert!(
        strays.is_empty(),
        "the atomic write must clean up after itself; found leftovers: {strays:?}"
    );

    // A truncated manifest must be REJECTED, not half-loaded.
    std::fs::write(&manifest, &second[..second.len() / 2]).unwrap();
    let victim = FcFontRegistry::new();
    assert!(
        victim.load_from_disk_cache_at(&manifest).is_none(),
        "a truncated manifest must be rejected outright"
    );
    assert!(
        !victim.is_cache_loaded(),
        "a rejected manifest must not mark the registry as cache-loaded"
    );
}

/// The one that actually catches the shipped bug: **nobody has to call save**.
///
/// The three tests above drive `save_to_disk_cache_at` by hand, so they would
/// keep passing in a world where the manifest is still never written in
/// practice — which is precisely the world we were in. This one calls nothing:
/// it starts a registry, lets the scan finish, and then asserts that the file
/// at `get_font_cache_path()` exists.
///
/// It runs in a child process with `HOME` / `XDG_CACHE_HOME` redirected at a
/// temp directory, both so it cannot clobber the developer's real font cache
/// and so "the manifest exists" cannot be satisfied by a manifest that was
/// already there.
///
/// Unix only: on Windows `dirs::cache_dir()` comes from the known-folder API
/// (`%LOCALAPPDATA%` as the shell sees it), which no environment variable
/// redirects, so the child would write into the real cache and the parent's
/// assertion would fail for the wrong reason.
#[cfg(unix)]
#[test]
fn build_completion_writes_the_real_manifest_without_any_explicit_save() {
    const CHILD_ENV: &str = "RFC_DISK_CACHE_AUTOSAVE_CHILD";
    const TEST_NAME: &str = "build_completion_writes_the_real_manifest_without_any_explicit_save";

    if std::env::var_os(CHILD_ENV).is_some() {
        // ---- child: scan, and DO NOT call save ----
        let registry = FcFontRegistry::new_with_config(bounded_scan_config());
        registry.spawn_scout_and_builders();
        let deadline = Instant::now() + Duration::from_secs(180);
        while !registry.is_build_complete() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            registry.is_build_complete(),
            "child: scan did not complete in 180s"
        );
        assert!(!registry.list().is_empty(), "child: scan found zero fonts");

        let path = rust_fontconfig::disk_cache::get_font_cache_path()
            .expect("child: get_font_cache_path returned None");
        println!("child cache path: {path:?}");

        // The persisting builder thread exits right after the rename; give it a
        // bounded moment to land rather than racing it.
        let deadline = Instant::now() + Duration::from_secs(20);
        while !path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            path.exists(),
            "child: a completed scan did NOT write {path:?}. This is the bug: \
             save_to_disk_cache has no caller, so every launch re-scans."
        );
        return;
    }

    // ---- parent ----
    let tmp = TempDir::new("autosave-home");
    let home = tmp.0.join("home");
    std::fs::create_dir_all(home.join("Library").join("Caches")).unwrap();
    let xdg = tmp.0.join("xdg-cache");
    std::fs::create_dir_all(&xdg).unwrap();

    let exe = std::env::current_exe().expect("current_exe");
    let out = std::process::Command::new(exe)
        .args(["--exact", TEST_NAME, "--nocapture", "--test-threads", "1"])
        .env(CHILD_ENV, "1")
        .env("HOME", &home)
        .env("XDG_CACHE_HOME", &xdg)
        .output()
        .expect("spawn child test process");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "child process failed.\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );

    // Belt and braces: the manifest must be under one of the redirected roots,
    // proving the child wrote a NEW file rather than finding a pre-existing one.
    let candidates = [
        home.join("Library")
            .join("Caches")
            .join("rfc")
            .join("fonts")
            .join("manifest.bin"),
        xdg.join("rfc").join("fonts").join("manifest.bin"),
        home.join(".cache")
            .join("rfc")
            .join("fonts")
            .join("manifest.bin"),
    ];
    let found: Vec<_> = candidates.iter().filter(|p| p.exists()).collect();
    assert!(
        !found.is_empty(),
        "no manifest was written under the redirected cache roots.\n\
         checked: {candidates:?}\n--- child stdout ---\n{stdout}"
    );
    for p in found {
        assert!(
            std::fs::metadata(p).unwrap().len() > 0,
            "manifest {p:?} is empty"
        );
    }
}
