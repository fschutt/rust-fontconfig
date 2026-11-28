//! A manifest is believed only as far as its files are what it recorded.
//!
//! `load_from_disk_cache_at` used to insert EVERY entry of a manifest: the modification time and the size it
//! records for each font file were written and never read. After an OS update that replaced a system font, or
//! a font that was removed, the registry kept the old family names, weights and coverage for ever (and since the
//! next save wrote the registry back, the stale entries were never dropped either).
//!
//! These tests build manifests by hand (the files need no font in them: the load reads sizes and times) and
//! check what each way a file can differ does: a changed size, a changed time, a missing file. The entries that
//! are still good load and are not parsed again; the stale ones are left to the scan; and `cache_loaded`
//! (what makes a layout skip waiting for the scan) is set only when nothing was stale.
//!
//!   cargo test --features cache --test disk_cache_validation

#![cfg(all(feature = "cache", not(target_family = "wasm")))]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use rust_fontconfig::config::FcScanConfig;
use rust_fontconfig::disk_cache::{get_file_metadata, FontCacheEntry, FontIndexEntry, FontManifest};
use rust_fontconfig::registry::FcFontRegistry;
use rust_fontconfig::FcPattern;

/// A unique, self-cleaning scratch directory.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rfc-validation-test-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        TempDir(dir)
    }
    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
    fn manifest(&self) -> PathBuf {
        self.0.join("manifest.bin")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A registry that scans nothing and writes nothing of its own.
fn registry() -> Arc<FcFontRegistry> {
    let registry = FcFontRegistry::new_with_config(FcScanConfig {
        font_dirs: Vec::new(),
        priority_families: Vec::new(),
    });
    registry.set_persist_on_complete(false);
    registry
}

/// The manifest entry of the file at `path`, as a save would have written it just now.
fn entry_of(path: &Path, family: &str) -> FontCacheEntry {
    let (mtime_secs, file_size) =
        get_file_metadata(path.to_str().unwrap()).expect("the test file exists");
    FontCacheEntry {
        mtime_secs,
        file_size,
        bytes_hash: 0,
        font_indices: vec![FontIndexEntry {
            pattern: FcPattern {
                name: Some(family.to_string()),
                family: Some(family.to_string()),
                ..FcPattern::default()
            },
            font_index: 0,
        }],
    }
}

fn write_manifest(path: &Path, version: u32, files: &[(&Path, &str)]) {
    let entries: BTreeMap<String, FontCacheEntry> = files
        .iter()
        .map(|(file, family)| (file.to_str().unwrap().to_string(), entry_of(file, family)))
        .collect();
    let bytes = bincode::serialize(&FontManifest { version, entries }).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// Three files and the manifest that lists them.
fn three(dir: &TempDir) -> [PathBuf; 3] {
    let files = [
        dir.file("A.ttf", &[1u8; 100]),
        dir.file("B.ttf", &[2u8; 200]),
        dir.file("C.ttf", &[3u8; 300]),
    ];
    write_manifest(
        &dir.manifest(),
        FontManifest::CURRENT_VERSION,
        &[(&files[0], "Alpha"), (&files[1], "Beta"), (&files[2], "Gamma")],
    );
    files
}

fn processed(registry: &FcFontRegistry, path: &Path) -> bool {
    registry.processed_paths.lock().unwrap().contains(path)
}

#[test]
fn a_manifest_whose_files_are_as_recorded_loads_whole() {
    let dir = TempDir::new("whole");
    let files = three(&dir);

    let a = registry();
    let report = a
        .load_from_disk_cache_checked_at(&dir.manifest())
        .expect("a readable manifest");
    assert_eq!((report.loaded, report.changed, report.missing), (3, 0, 0));
    assert!(report.is_complete());
    assert!(a.is_cache_loaded(), "a whole manifest is a loaded cache");
    assert_eq!(a.list().len(), 3);
    for file in &files {
        assert!(processed(&a, file), "{file:?} would be parsed again");
    }

    // The plain entry point answers the same.
    let b = registry();
    assert_eq!(b.load_from_disk_cache_at(&dir.manifest()), Some(()));
    assert!(b.is_cache_loaded());
}

#[test]
fn a_file_of_another_size_is_left_to_the_scan() {
    let dir = TempDir::new("size");
    let files = three(&dir);
    // Replaced by another font of another size (an OS update).
    std::fs::write(&files[1], [9u8; 250]).unwrap();

    let a = registry();
    let report = a.load_from_disk_cache_checked_at(&dir.manifest()).unwrap();
    assert_eq!((report.loaded, report.changed, report.missing), (2, 1, 0));
    assert!(!report.is_complete());
    assert!(
        !a.is_cache_loaded(),
        "a layout must wait for the scan that parses the changed file"
    );
    assert_eq!(a.list().len(), 2, "the stale entry's face is not in the registry");
    assert!(processed(&a, &files[0]) && processed(&a, &files[2]));
    assert!(
        !processed(&a, &files[1]),
        "the changed file is not marked as parsed: the scan parses it"
    );

    let b = registry();
    assert_eq!(b.load_from_disk_cache_at(&dir.manifest()), None);
    assert!(!b.is_cache_loaded());
    assert_eq!(b.list().len(), 2, "what is still good stays");
}

#[test]
fn a_file_with_another_time_is_left_to_the_scan() {
    let dir = TempDir::new("time");
    let files = three(&dir);
    // The same size, written again an hour from now.
    let later = SystemTime::now() + Duration::from_secs(3600);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&files[0])
        .unwrap()
        .set_modified(later)
        .unwrap();

    let a = registry();
    let report = a.load_from_disk_cache_checked_at(&dir.manifest()).unwrap();
    assert_eq!((report.loaded, report.changed, report.missing), (2, 1, 0));
    assert!(!a.is_cache_loaded());
    assert!(!processed(&a, &files[0]));
}

#[test]
fn a_file_that_is_gone_is_left_out() {
    let dir = TempDir::new("gone");
    let files = three(&dir);
    std::fs::remove_file(&files[2]).unwrap();

    let a = registry();
    let report = a.load_from_disk_cache_checked_at(&dir.manifest()).unwrap();
    assert_eq!((report.loaded, report.changed, report.missing), (2, 0, 1));
    assert!(!a.is_cache_loaded());
    assert_eq!(a.list().len(), 2);
    assert!(!processed(&a, &files[2]));
}

#[test]
fn a_manifest_of_another_format_or_a_cut_one_loads_nothing() {
    let dir = TempDir::new("format");
    let files = three(&dir);
    let good = std::fs::read(dir.manifest()).unwrap();

    // Another format number.
    write_manifest(
        &dir.manifest(),
        FontManifest::CURRENT_VERSION + 1,
        &[(&files[0], "Alpha")],
    );
    let a = registry();
    assert!(a.load_from_disk_cache_checked_at(&dir.manifest()).is_none());
    assert_eq!(a.load_from_disk_cache_at(&dir.manifest()), None);
    assert!(a.list().is_empty() && !a.is_cache_loaded());

    // Cut in the middle, and cut to nothing.
    for cut in [good.len() / 2, 3, 0] {
        std::fs::write(dir.manifest(), &good[..cut]).unwrap();
        let b = registry();
        assert!(
            b.load_from_disk_cache_checked_at(&dir.manifest()).is_none(),
            "cut at {cut} of {}",
            good.len()
        );
        assert!(b.list().is_empty() && !b.is_cache_loaded());
    }

    // No file at all.
    let c = registry();
    assert!(c.load_from_disk_cache_checked_at(&dir.0.join("none.bin")).is_none());
}
