//! Asynchronous font registry with background scanning and on-demand blocking.
//!
//! `FcFontRegistry` wraps an `FcFontCache` behind a `RwLock` and adds concurrent
//! background scanning. Background threads populate the cache while the main thread
//! reads from it. The main thread blocks at layout time (via `request_fonts()`) until
//! the specific fonts it needs are ready.
//!
//! # Architecture
//!
//! - **Scout** (1 thread): Enumerates font directories, guesses family names from
//!   filenames, and feeds paths to the Builder's priority queue. Takes ~5-20ms.
//! - **Builder Pool** (N threads): Parses font files from the priority queue, verifies
//!   CMAP tables, and writes results to the shared cache.
//! - **Registry** (shared state): Thread-safe wrapper around `FcFontCache`.
//!   The main thread reads from it; background threads write to it.
//!
//! # Usage
//!
//! ```rust,no_run
//! use rust_fontconfig::registry::FcFontRegistry;
//!
//! // Create and start the registry (returns immediately)
//! let registry = FcFontRegistry::new();
//! registry.spawn_scout_and_builders();
//!
//! // ... do other work (window creation, DOM construction, etc.) ...
//!
//! // Block until the fonts we need are ready
//! let families = vec![
//!     vec!["Arial".to_string(), "sans-serif".to_string()],
//!     vec!["Fira Code".to_string(), "monospace".to_string()],
//! ];
//! let chains = registry.request_fonts(&families);
//! ```

use alloc::collections::btree_map::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

use crate::{
    expand_font_families, FcFontCache, FcFontPath, FcParseFontBytes, FcPattern, FcWeight,
    FontFallbackChain, FontId, FontMatch, NamedFont, OperatingSystem, PatternMatch,
};
use crate::scoring::{
    family_exists_in_patterns, find_family_paths, find_incomplete_paths,
    FcBuildJob, Priority,
};
use crate::utils::normalize_family_name;

// ── The Registry ────────────────────────────────────────────────────────────

/// Thread-safe, incrementally-populated font registry.
///
/// Wraps an `FcFontCache` behind a `RwLock` so that background threads can
/// populate it concurrently while the main thread reads from it.
pub struct FcFontRegistry {
    /// The underlying font cache, populated incrementally by Builder threads.
    pub cache: RwLock<FcFontCache>,

    // ── Populated by Scout (fast, Phase 1) ──
    /// Maps guessed lowercase family name → file paths
    pub known_paths: RwLock<BTreeMap<String, Vec<PathBuf>>>,

    // ── Priority queue for Builder ──
    pub build_queue: Mutex<Vec<FcBuildJob>>,
    /// Notified when new jobs are added to `build_queue` or on shutdown.
    /// Builder threads wait on this (paired with `build_queue`).
    pub queue_condvar: Condvar,

    // ── Deduplication ──
    /// Paths claimed for parsing (set BEFORE parsing, for deduplication).
    pub processed_paths: Mutex<HashSet<PathBuf>>,
    /// Paths fully parsed and inserted into cache (set AFTER parsing).
    pub completed_paths: Mutex<HashSet<PathBuf>>,

    // ── Progress notification ──
    /// Notified when any progress occurs: font completed, scan done, build done.
    /// The main thread waits on this (paired with `completed_paths`).
    pub progress: Condvar,

    // ── Status ──
    pub scan_complete: AtomicBool,
    pub build_complete: AtomicBool,
    pub shutdown: AtomicBool,
    /// Whether a disk cache was successfully loaded (skip blocking in request_fonts)
    pub cache_loaded: AtomicBool,

    // ── Operating system (for font family expansion) ──
    pub os: OperatingSystem,
}

impl std::fmt::Debug for FcFontRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FcFontRegistry")
            .field("scan_complete", &self.scan_complete.load(Ordering::Relaxed))
            .field("build_complete", &self.build_complete.load(Ordering::Relaxed))
            .field("cache_loaded", &self.cache_loaded.load(Ordering::Relaxed))
            .finish()
    }
}

impl FcFontRegistry {
    /// Create a new empty registry.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            cache: RwLock::new(FcFontCache::default()),
            known_paths: RwLock::new(BTreeMap::new()),
            build_queue: Mutex::new(Vec::new()),
            queue_condvar: Condvar::new(),
            processed_paths: Mutex::new(HashSet::new()),
            completed_paths: Mutex::new(HashSet::new()),
            progress: Condvar::new(),
            scan_complete: AtomicBool::new(false),
            build_complete: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            cache_loaded: AtomicBool::new(false),
            os: OperatingSystem::current(),
        })
    }

    /// Register in-memory (bundled) fonts. These are available immediately.
    pub fn register_memory_fonts(&self, fonts: Vec<NamedFont>) {
        for named_font in fonts {
            let Some(parsed) = FcParseFontBytes(&named_font.bytes, &named_font.name) else {
                continue;
            };
            let Ok(mut cache) = self.cache.write() else { continue };
            cache.with_memory_fonts(parsed);
        }
    }

    /// Spawn the Scout thread and Builder pool. Returns immediately.
    pub fn spawn_scout_and_builders(self: &Arc<Self>) {
        let num_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2)
            .saturating_sub(1)
            .max(1);

        // Spawn Scout thread
        let registry = Arc::clone(self);
        std::thread::Builder::new()
            .name("rfc-font-scout".to_string())
            .spawn(move || {
                registry.scout_thread();
            })
            .expect("failed to spawn font scout thread");

        // Spawn Builder threads
        for i in 0..num_threads {
            let registry = Arc::clone(self);
            std::thread::Builder::new()
                .name(format!("rfc-font-builder-{}", i))
                .spawn(move || {
                    registry.builder_thread();
                })
                .expect("failed to spawn font builder thread");
        }
    }

    /// Block the calling thread until all requested font families are loaded
    /// (or confirmed to not exist on the system).
    ///
    /// This is called by the layout engine before the first layout pass.
    /// It boosts the priority of any not-yet-loaded fonts to Critical and
    /// waits for the Builder to process them.
    ///
    /// Hard timeout: 5 seconds.
    pub fn request_fonts(
        &self,
        family_stacks: &[Vec<String>],
    ) -> Vec<FontFallbackChain> {
        let deadline = Instant::now() + Duration::from_secs(5);

        // 1. Expand generic families and collect all unique family names we need
        let mut needed_families: Vec<String> = Vec::new();
        let mut expanded_stacks: Vec<Vec<String>> = Vec::new();

        for stack in family_stacks {
            let expanded = expand_font_families(stack, self.os, &[]);
            for family in &expanded {
                let normalized = normalize_family_name(family);
                if !needed_families.contains(&normalized) {
                    needed_families.push(normalized);
                }
            }
            expanded_stacks.push(expanded);
        }

        // Fast path: if disk cache was loaded, all previously-known fonts are
        // already in the patterns map. We can resolve chains immediately.
        if self.cache_loaded.load(Ordering::Acquire) {
            return self.resolve_chains(&expanded_stacks);
        }

        // 2. Wait for Scout to finish (typically < 100ms).
        //    Uses condvar instead of busy-polling.
        if !self.scan_complete.load(Ordering::Acquire) {
            let Ok(mut completed) = self.completed_paths.lock() else {
                return self.resolve_chains(&expanded_stacks);
            };
            while !self.scan_complete.load(Ordering::Acquire) {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    eprintln!(
                        "[rfc-font-registry] WARNING: Timed out waiting for font scout (5s). \
                         Proceeding with available fonts."
                    );
                    return self.resolve_chains(&expanded_stacks);
                }
                completed = match self.progress.wait_timeout(completed, remaining) {
                    Ok((c, _)) => c,
                    Err(_) => return self.resolve_chains(&expanded_stacks),
                };
            }
        }

        // 3. Check which families are completely missing from the cache
        let missing: Vec<String> = self.cache.read()
            .map(|cache| {
                needed_families
                    .iter()
                    .filter(|fam| !family_exists_in_patterns(fam, cache.patterns.keys()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_else(|_| needed_families.clone());

        // 4. Find font files that match needed families but haven't been
        //    fully parsed yet. Uses completed_paths (not processed_paths!)
        //    because processed_paths is set BEFORE parsing, while
        //    completed_paths is set AFTER parsing + insert_font().
        let incomplete_paths = self.known_paths.read().ok()
            .zip(self.completed_paths.lock().ok())
            .map(|(known, completed)| find_incomplete_paths(&needed_families, &known, &completed))
            .unwrap_or_default();

        // 5. If nothing is missing AND all files are processed, resolve immediately
        if missing.is_empty() && incomplete_paths.is_empty() {
            return self.resolve_chains(&expanded_stacks);
        }

        // 6. Boost all relevant paths to Critical priority
        let wait_paths: HashSet<PathBuf> = if let (Ok(known_paths), Ok(mut queue)) =
            (self.known_paths.read(), self.build_queue.lock())
        {
            // Paths for completely missing families
            let missing_paths: Vec<_> = missing
                .iter()
                .flat_map(|fam| {
                    find_family_paths(fam, &known_paths)
                        .into_iter()
                        .map(move |p| (p, fam.clone()))
                })
                .collect();

            // Push Critical jobs for both missing and incomplete paths
            for (path, family) in missing_paths.iter().chain(incomplete_paths.iter()) {
                queue.push(FcBuildJob {
                    priority: Priority::Critical,
                    path: path.clone(),
                    font_index: None,
                    guessed_family: family.clone(),
                });
            }

            queue.sort();

            // Collect all paths we need to wait for
            missing_paths
                .iter()
                .chain(incomplete_paths.iter())
                .map(|(p, _)| p.clone())
                .collect()
        } else {
            incomplete_paths.iter().map(|(p, _)| p.clone()).collect()
        };
        self.queue_condvar.notify_all();

        // 7. Wait for all wait_paths to be completed.
        //    Uses condvar instead of busy-polling with sleep(1ms).
        if !wait_paths.is_empty() {
            let Ok(mut completed) = self.completed_paths.lock() else {
                return self.resolve_chains(&expanded_stacks);
            };
            loop {
                if wait_paths.iter().all(|p| completed.contains(p)) {
                    break;
                }
                if self.build_complete.load(Ordering::Acquire) {
                    break;
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    eprintln!(
                        "[rfc-font-registry] WARNING: Timed out waiting for font files (5s). \
                         Proceeding with available fonts."
                    );
                    break;
                }
                completed = match self.progress.wait_timeout(completed, remaining) {
                    Ok((c, _)) => c,
                    Err(_) => break,
                };
            }
        }

        // 8. Resolve chains from the now-populated registry
        self.resolve_chains(&expanded_stacks)
    }

    // ── Delegated accessors ─────────────────────────────────────────────────

    /// Get font metadata by ID.
    pub fn get_metadata_by_id(&self, id: &FontId) -> Option<FcPattern> {
        self.cache.read().ok()?.metadata.get(id).cloned()
    }

    /// Get font bytes for a given font ID (either from memory or disk).
    pub fn get_font_bytes(&self, id: &FontId) -> Option<Vec<u8>> {
        self.cache.read().ok()?.get_font_bytes(id)
    }

    /// Get the disk font path for a font ID.
    pub fn get_disk_font_path(&self, id: &FontId) -> Option<FcFontPath> {
        self.cache.read().ok()?.disk_fonts.get(id).cloned()
    }

    /// Check if a font ID is a memory font.
    pub fn is_memory_font(&self, id: &FontId) -> bool {
        self.cache.read().ok()
            .map(|c| c.is_memory_font(id))
            .unwrap_or(false)
    }

    /// List all known fonts (pattern + ID pairs).
    pub fn list(&self) -> Vec<(FcPattern, FontId)> {
        self.cache.read().ok()
            .map(|c| c.list().into_iter().map(|(p, id)| (p.clone(), id)).collect())
            .unwrap_or_default()
    }

    /// Query the registry for a font matching the given pattern.
    pub fn query(&self, pattern: &FcPattern) -> Option<FontMatch> {
        let cache = self.cache.read().ok()?;
        let mut trace = Vec::new();
        cache.query(pattern, &mut trace)
    }

    /// Resolve a complete font fallback chain for a CSS font-family stack.
    pub fn resolve_font_chain(
        &self,
        font_families: &[String],
        weight: FcWeight,
        italic: PatternMatch,
        oblique: PatternMatch,
    ) -> FontFallbackChain {
        let Ok(cache) = self.cache.read() else {
            return FontFallbackChain {
                css_fallbacks: Vec::new(),
                unicode_fallbacks: Vec::new(),
                original_stack: font_families.to_vec(),
            };
        };
        let mut trace = Vec::new();
        cache.resolve_font_chain_with_os(
            font_families, weight, italic, oblique, &mut trace, self.os,
        )
    }

    /// Take a snapshot of the current cache state as an immutable `FcFontCache`.
    pub fn into_fc_font_cache(&self) -> FcFontCache {
        self.cache.read()
            .map(|c| c.clone())
            .unwrap_or_default()
    }

    /// Signal all background threads to shut down.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
        self.queue_condvar.notify_all();
        self.progress.notify_all();
    }

    /// Returns true if the Scout has finished enumerating all font directories.
    pub fn is_scan_complete(&self) -> bool {
        self.scan_complete.load(Ordering::Acquire)
    }

    /// Returns true if all fonts in the queue have been processed.
    pub fn is_build_complete(&self) -> bool {
        self.build_complete.load(Ordering::Acquire)
    }

    /// Returns true if a disk cache was successfully loaded at startup.
    pub fn is_cache_loaded(&self) -> bool {
        self.cache_loaded.load(Ordering::Acquire)
    }

    // ── Internal methods ────────────────────────────────────────────────────

    /// Insert a parsed font into the cache (called by Builder threads).
    pub fn insert_font(&self, pattern: FcPattern, path: FcFontPath) {
        let Ok(mut cache) = self.cache.write() else { return };
        let id = FontId::new();
        cache.index_pattern_tokens(&pattern, id);
        cache.patterns.insert(pattern.clone(), id);
        cache.disk_fonts.insert(id, path);
        cache.metadata.insert(id, pattern);
        // Invalidate chain cache — scope the inner lock so it drops before cache
        let _ = cache.chain_cache.lock().map(|mut cc| cc.clear());
    }

    /// Resolve font chains from the current state of the registry.
    fn resolve_chains(&self, expanded_stacks: &[Vec<String>]) -> Vec<FontFallbackChain> {
        expanded_stacks
            .iter()
            .map(|stack| {
                self.resolve_font_chain(
                    stack,
                    FcWeight::Normal,
                    PatternMatch::DontCare,
                    PatternMatch::DontCare,
                )
            })
            .collect()
    }
}

impl Drop for FcFontRegistry {
    fn drop(&mut self) {
        self.shutdown();
    }
}
