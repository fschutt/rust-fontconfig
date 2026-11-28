//! On-disk font cache serialization and deserialization.
//!
//! This entire module is gated on `feature = "cache"`.

use alloc::collections::btree_map::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use crate::registry::FcFontRegistry;
use crate::{FcFontPath, FcPattern, FontId};

/// Font cache manifest for on-disk serialization.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FontManifest {
    /// Cache format version (bump on breaking changes)
    pub version: u32,
    /// Entries: path → cached font data
    pub entries: BTreeMap<String, FontCacheEntry>,
}

impl FontManifest {
    /// Cache format version. Bump on breaking changes.
    pub const CURRENT_VERSION: u32 = 3;
}

/// A single cached font file entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FontCacheEntry {
    /// File modification time (seconds since epoch)
    pub mtime_secs: u64,
    /// File size in bytes
    pub file_size: u64,
    /// 64-bit content hash of the whole file. 0 = not computed.
    #[serde(default)]
    pub bytes_hash: u64,
    /// Parsed font data for each font index in the file
    pub font_indices: Vec<FontIndexEntry>,
}

/// A single font face within a font file, for disk cache serialization.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FontIndexEntry {
    /// Parsed font metadata (name, family, weight, italic, unicode ranges, etc.)
    pub pattern: FcPattern,
    /// Zero-based index of this face within the font file (0 for single-face files)
    pub font_index: usize,
}

/// What loading a manifest came to: how many faces went into the registry, and how many manifest entries were left
/// to the scan because their file is no longer the one the manifest recorded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheLoadReport {
    /// Faces put in the registry.
    pub loaded: usize,
    /// Entries whose file has another size or modification time than the manifest recorded (an OS update, a font
    /// installed over another): not loaded, so the scan parses the file again.
    pub changed: usize,
    /// Entries whose file cannot be opened any more: not loaded.
    pub missing: usize,
}

impl CacheLoadReport {
    /// Whether every entry of the manifest was still good.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.changed == 0 && self.missing == 0
    }
}

impl FcFontRegistry {
    /// Load font metadata from the on-disk cache.
    #[cfg(not(target_family = "wasm"))]
    pub fn load_from_disk_cache(&self) -> Option<()> {
        self.load_from_disk_cache_at(&get_font_cache_path()?)
    }

    /// Load font metadata from the on-disk cache at a specific path.
    ///
    /// Returns `Some(())` when the manifest was read and EVERY entry in it is still good (its font file opens, with
    /// the size and modification time the manifest recorded): then the registry is marked as loaded from the cache
    /// and a layout does not wait for the scan. `None` when there is no usable manifest (missing, cut, damaged, of
    /// another format) or when some entry is stale: the entries that are still good stay in the registry and are
    /// not parsed again, the stale ones are left to the scan, and a layout waits for it as it does without a cache.
    /// [`Self::load_from_disk_cache_checked_at`] says how many were which.
    #[cfg(not(target_family = "wasm"))]
    pub fn load_from_disk_cache_at(&self, cache_path: &std::path::Path) -> Option<()> {
        self.load_from_disk_cache_checked_at(cache_path)?
            .is_complete()
            .then_some(())
    }

    /// [`Self::load_from_disk_cache_at`], with the numbers: `None` when there is no usable manifest, else what was
    /// loaded and what was left to the scan. Each entry's file is opened once (outside every lock) and its size and
    /// modification time are compared with the manifest's.
    #[cfg(not(target_family = "wasm"))]
    pub fn load_from_disk_cache_checked_at(
        &self,
        cache_path: &std::path::Path,
    ) -> Option<CacheLoadReport> {
        use std::io::Read;
        let mut file = std::fs::File::open(cache_path).ok()?;
        let mut data = Vec::new();
        file.read_to_end(&mut data).ok()?;
        let manifest: FontManifest = bincode::deserialize(&data).ok()?;

        if manifest.version != FontManifest::CURRENT_VERSION {
            return None;
        }

        // The files first, with no lock held: a manifest lists hundreds of them.
        let mut report = CacheLoadReport::default();
        let good: Vec<(&String, &FontCacheEntry)> = manifest
            .entries
            .iter()
            .filter(|(path_str, entry)| match get_file_metadata(path_str) {
                None => {
                    report.missing += 1;
                    false
                }
                Some((mtime_secs, file_size))
                    if mtime_secs != entry.mtime_secs || file_size != entry.file_size =>
                {
                    report.changed += 1;
                    false
                }
                Some(_) => true,
            })
            .collect();

        let mut state = self.cache.state_write();
        let mut processed = self.processed_paths.lock().ok()?;
        let mut completed = self.completed_paths.lock().ok()?;

        for (path_str, entry) in good {
            let pb = PathBuf::from(path_str);
            processed.insert(pb.clone());
            completed.insert(pb);
            for idx_entry in &entry.font_indices {
                state.insert_disk_font(
                    idx_entry.pattern.clone(),
                    FontId::new(),
                    FcFontPath {
                        path: path_str.clone(),
                        font_index: idx_entry.font_index,
                        bytes_hash: entry.bytes_hash,
                    },
                );
                report.loaded += 1;
            }
        }

        drop(state);
        if report.is_complete() {
            self.cache_loaded.store(true, Ordering::Release);
        }

        Some(report)
    }

    /// No-op on WASM — no filesystem access available.
    #[cfg(target_family = "wasm")]
    pub fn load_from_disk_cache(&self) -> Option<()> {
        None
    }

    /// No-op on WASM — no filesystem access available.
    #[cfg(target_family = "wasm")]
    pub fn load_from_disk_cache_at(&self, _cache_path: &std::path::Path) -> Option<()> {
        None
    }

    /// No-op on WASM — no filesystem access available.
    #[cfg(target_family = "wasm")]
    pub fn load_from_disk_cache_checked_at(
        &self,
        _cache_path: &std::path::Path,
    ) -> Option<CacheLoadReport> {
        None
    }

    /// Serialize the current registry state to the on-disk font cache.
    #[cfg(not(target_family = "wasm"))]
    pub fn save_to_disk_cache(&self) -> Option<()> {
        self.save_to_disk_cache_at(&get_font_cache_path()?)
    }

    /// Serialize the current registry state to the on-disk font cache at a specific path.
    #[cfg(not(target_family = "wasm"))]
    pub fn save_to_disk_cache_at(&self, cache_path: &std::path::Path) -> Option<()> {
        std::fs::create_dir_all(cache_path.parent()?).ok()?;

        let state = self.cache.state_read();

        let mut entries: BTreeMap<String, FontCacheEntry> = BTreeMap::new();

        state
            .disk_fonts
            .iter()
            .filter_map(|(id, font_path)| {
                state.metadata.get(id).map(|pattern| (font_path, pattern))
            })
            .for_each(|(font_path, pattern)| {
                entries
                    .entry(font_path.path.clone())
                    .or_insert_with(|| {
                        let (mtime_secs, file_size) =
                            get_file_metadata(&font_path.path).unwrap_or((0, 0));
                        FontCacheEntry {
                            mtime_secs,
                            file_size,
                            bytes_hash: font_path.bytes_hash,
                            font_indices: Vec::new(),
                        }
                    })
                    .font_indices
                    .push(FontIndexEntry {
                        pattern: pattern.clone(),
                        font_index: font_path.font_index,
                    });
            });

        let manifest = FontManifest {
            version: FontManifest::CURRENT_VERSION,
            entries,
        };

        // Drop the lock before touching the filesystem to avoid stalling readers/writers.
        drop(state);

        let data = bincode::serialize(&manifest).ok()?;

        let mut tmp_name = cache_path.file_name()?.to_os_string();
        tmp_name.push(alloc::format!(".tmp-{}", std::process::id()));
        let tmp_path = cache_path.with_file_name(tmp_name);

        use std::io::Write;
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
        {
            Ok(f) => f,
            Err(_) => return None,
        };

        if file.write_all(&data).is_err() || file.sync_all().is_err() {
            drop(file);
            let _ = std::fs::remove_file(&tmp_path);
            return None;
        }
        drop(file);

        if std::fs::rename(&tmp_path, cache_path).is_err() {
            let _ = std::fs::remove_file(&tmp_path);
            return None;
        }

        Some(())
    }

    /// No-op on WASM — no filesystem access available.
    #[cfg(target_family = "wasm")]
    pub fn save_to_disk_cache(&self) -> Option<()> {
        None
    }

    /// No-op on WASM — no filesystem access available.
    #[cfg(target_family = "wasm")]
    pub fn save_to_disk_cache_at(&self, _cache_path: &std::path::Path) -> Option<()> {
        None
    }
}

/// Get file mtime (seconds since epoch) and size in bytes.
pub fn get_file_metadata(path: &str) -> Option<(u64, u64)> {
    let meta = std::fs::File::open(path).and_then(|f| f.metadata()).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some((mtime, meta.len()))
}

/// Get the path to the font cache manifest file.
pub fn get_font_cache_path() -> Option<PathBuf> {
    let base = get_cache_base_dir()?;
    Some(base.join("fonts").join("manifest.bin"))
}

/// Get the base cache directory for rust-fontconfig.
#[cfg(not(target_family = "wasm"))]
pub fn get_cache_base_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("rfc"))
}

/// Returns `None` on platforms without a conventional cache directory (e.g. WASM).
#[cfg(target_family = "wasm")]
pub fn get_cache_base_dir() -> Option<PathBuf> {
    None
}
