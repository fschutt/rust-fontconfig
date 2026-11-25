//! `FcFontCache::add_font_dir` (GitHub issue #23).

#![cfg(all(feature = "std", feature = "parsing"))]

use std::path::PathBuf;

use rust_fontconfig::*;

const RED_HAT_MONO: &[u8] = include_bytes!("fixtures/RedHatMono-VariableFont_wght.ttf");
const INSTRUMENT_SERIF: &[u8] = include_bytes!("fixtures/InstrumentSerif-Regular.ttf");

/// A fresh directory in the temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(test: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "rust-fontconfig-23-{}-{}",
            test,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn families(cache: &FcFontCache) -> Vec<String> {
    cache
        .list()
        .into_iter()
        .filter_map(|(pattern, _)| pattern.family)
        .collect()
}

#[test]
fn the_fonts_of_a_directory_are_added() {
    let dir = TempDir::new("added");
    std::fs::create_dir(dir.0.join("nested")).unwrap();
    std::fs::write(dir.0.join("RedHatMono.ttf"), RED_HAT_MONO).unwrap();
    std::fs::write(dir.0.join("nested/InstrumentSerif.ttf"), INSTRUMENT_SERIF).unwrap();
    std::fs::write(dir.0.join("readme.txt"), b"not a font").unwrap();

    let cache = FcFontCache::default();
    assert_eq!(cache.add_font_dir(&dir.0), 2);
    assert_eq!(cache.len(), 2);
    let families = families(&cache);
    assert!(families.iter().any(|f| f == "Red Hat Mono"), "{families:?}");
    assert!(
        families.iter().any(|f| f == "Instrument Serif"),
        "{families:?}"
    );
}

#[test]
fn a_directory_added_twice_adds_nothing_the_second_time() {
    let dir = TempDir::new("twice");
    std::fs::write(dir.0.join("RedHatMono.ttf"), RED_HAT_MONO).unwrap();

    let cache = FcFontCache::default();
    assert_eq!(cache.add_font_dir(&dir.0), 1);
    assert_eq!(cache.add_font_dir(&dir.0), 0);
    assert_eq!(cache.len(), 1);
}

#[test]
fn a_missing_directory_adds_nothing() {
    let dir = TempDir::new("missing");
    let cache = FcFontCache::default();
    assert_eq!(cache.add_font_dir(dir.0.join("missing")), 0);
    assert!(cache.is_empty());
}
