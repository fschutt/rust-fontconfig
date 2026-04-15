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
    UnicodeRange,
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
    ///
    /// As of v4.1, `FcFontCache` carries its own internal `RwLock` and
    /// `Arc`, so the registry can hand out handles (via `shared_cache`)
    /// that live-update with builder writes — no outer lock needed,
    /// no staleness for snapshot-holders downstream.
    pub cache: FcFontCache,

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
    /// When true, the scout populates `known_paths` + sets
    /// `scan_complete` but does NOT push every path onto
    /// `build_queue`. Builders therefore idle until a caller runs
    /// [`FcFontRegistry::request_fonts`] or
    /// [`FcFontRegistry::request_and_resolve_with_scripts`], which
    /// priority-bumps *only* the requested families into the queue.
    /// Cuts steady-state memory: the ~300 system fonts on macOS
    /// each cost ~50 KiB of parsed NAME + OS/2 metadata in the
    /// cache's pattern map — skipping those that the current
    /// workload never touches saves ~15 MiB on a short-lived
    /// headless render.
    ///
    /// Set via [`FcFontRegistry::set_scout_lazy`] before
    /// [`FcFontRegistry::spawn_scout_and_builders`]. Defaults to
    /// `false` to preserve the existing eager-scout behaviour for
    /// long-running embedders who want the disk cache to populate
    /// in the background.
    pub lazy_scout: AtomicBool,

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
            cache: FcFontCache::default(),
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
            lazy_scout: AtomicBool::new(false),
            os: OperatingSystem::current(),
        })
    }

    /// Enable/disable lazy scout mode. See [`FcFontRegistry::lazy_scout`]
    /// for what this changes. Must be called before
    /// [`FcFontRegistry::spawn_scout_and_builders`] — the scout thread
    /// reads the flag once when it starts iterating the build queue.
    pub fn set_scout_lazy(&self, lazy: bool) {
        self.lazy_scout.store(lazy, Ordering::Release);
    }

    /// Register in-memory (bundled) fonts. These are available immediately.
    pub fn register_memory_fonts(&self, fonts: Vec<NamedFont>) {
        for named_font in fonts {
            let Some(parsed) = FcParseFontBytes(&named_font.bytes, &named_font.name) else {
                continue;
            };
            self.cache.with_memory_fonts(parsed);
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
        let missing: Vec<String> = {
            let state = self.cache.state_read();
            needed_families
                .iter()
                .filter(|fam| !family_exists_in_patterns(fam, state.patterns.keys()))
                .cloned()
                .collect()
        };

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
        self.cache.get_metadata_by_id(id)
    }

    /// Get font bytes for a given font ID — disk-backed fonts come
    /// back as a shared mmap; in-memory fonts as `Owned`. See
    /// [`FcFontCache::get_font_bytes`] for the lifetime semantics.
    pub fn get_font_bytes(&self, id: &FontId) -> Option<std::sync::Arc<crate::FontBytes>> {
        self.cache.get_font_bytes(id)
    }

    /// Get the disk font path for a font ID.
    pub fn get_disk_font_path(&self, id: &FontId) -> Option<FcFontPath> {
        self.cache.state_read().disk_fonts.get(id).cloned()
    }

    /// Check if a font ID is a memory font.
    pub fn is_memory_font(&self, id: &FontId) -> bool {
        self.cache.is_memory_font(id)
    }

    /// List all known fonts (pattern + ID pairs).
    pub fn list(&self) -> Vec<(FcPattern, FontId)> {
        self.cache.list()
    }

    /// Query the registry for a font matching the given pattern.
    pub fn query(&self, pattern: &FcPattern) -> Option<FontMatch> {
        let mut trace = Vec::new();
        self.cache.query(pattern, &mut trace)
    }

    /// Resolve a complete font fallback chain for a CSS font-family stack.
    pub fn resolve_font_chain(
        &self,
        font_families: &[String],
        weight: FcWeight,
        italic: PatternMatch,
        oblique: PatternMatch,
    ) -> FontFallbackChain {
        let mut trace = Vec::new();
        self.cache.resolve_font_chain_with_os(
            font_families, weight, italic, oblique, &mut trace, self.os,
        )
    }

    /// On-demand font-chain resolution: triggers the scout + builder
    /// to parse the requested families (if not already parsed), waits
    /// for them via condvar, then resolves a full fallback chain with
    /// the caller-supplied weight / italic / oblique / scripts_hint.
    ///
    /// This is the "scout-on-demand" entry point: callers can skip
    /// the eager `request_fonts(common_stacks)` at init and pay the
    /// per-family parse only when a DOM actually needs that family.
    /// On excel.html that cuts the init cost from ~150 ms to ~10 ms
    /// and peak RSS from ~71 MiB to ~55 MiB because only the
    /// ~2 families excel uses get parsed, not the full common-stack
    /// set (~35 fonts across Helvetica/Lucida/Menlo/Times/NewYork/
    /// Courier/SFNS).
    ///
    /// Re-entrant from layout: holds no locks for the duration of the
    /// call, and `request_fonts` internally handles the scan_complete
    /// wait + priority-bump + completed_paths wait.
    #[cfg(feature = "std")]
    pub fn request_and_resolve_with_scripts(
        &self,
        font_families: &[String],
        weight: FcWeight,
        italic: PatternMatch,
        oblique: PatternMatch,
        scripts_hint: Option<&[UnicodeRange]>,
    ) -> FontFallbackChain {
        // Trigger parse + wait for these families. The returned
        // `FontFallbackChain` uses Normal/DontCare, which isn't what
        // we want — discard it and do a full re-resolve below.
        let _ = self.request_fonts(std::slice::from_ref(&font_families.to_vec()));
        // With the v4.1 shared cache, the registry's `cache` handle
        // and any previously-handed-out clone of it point at the
        // same `Arc<RwLock<FcFontCacheInner>>`, so this read sees
        // exactly the families the builder just parsed.
        let mut trace = Vec::new();
        self.cache.resolve_font_chain_with_scripts(
            font_families, weight, italic, oblique, scripts_hint, &mut trace,
        )
    }

    /// Get a shared handle on the cache. The returned `FcFontCache`
    /// shares state with this registry (and with every other holder
    /// of the handle): writes by builder threads via [`insert_font`]
    /// are immediately visible to all readers.
    ///
    /// Replaces v4.0's `into_fc_font_cache` (which took a deep
    /// snapshot) — the deep copy was the source of the stale-state
    /// bug in lazy-scout mode, since builders kept writing to the
    /// registry's cache while downstream holders were stuck on a
    /// frozen copy.
    pub fn shared_cache(&self) -> FcFontCache {
        self.cache.clone()
    }

    /// Block until the background scout + builder threads have
    /// populated the in-memory pattern map with every font's NAME +
    /// OS/2 metadata (most importantly `unicode_ranges`). Returns
    /// immediately if a disk cache was loaded, both scan + build
    /// already completed, or the 5 s deadline elapses.
    ///
    /// Callers that skip [`request_fonts`] but still need a fully
    /// populated [`FcFontCache`] snapshot (e.g. headless renderers
    /// that do their own font-chain resolution) must invoke this
    /// first — otherwise `into_fc_font_cache` may capture the cache
    /// mid-build and every `resolve_char` call will return `None`
    /// because `unicode_ranges` is empty for not-yet-parsed fonts.
    ///
    /// This waits for `build_complete` (not just `scan_complete`) —
    /// the scout finishes `readdir` quickly but the builder threads
    /// do the actual header parsing, and it is the builder output
    /// that populates `unicode_ranges`.
    pub fn wait_for_scout(&self) {
        use std::time::{Duration, Instant};
        if self.cache_loaded.load(Ordering::Acquire) {
            return;
        }
        if self.build_complete.load(Ordering::Acquire) {
            return;
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        let Ok(mut completed) = self.completed_paths.lock() else {
            return;
        };
        while !self.build_complete.load(Ordering::Acquire) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                eprintln!(
                    "[rfc-font-registry] WARNING: wait_for_scout timed out (5s)."
                );
                return;
            }
            completed = match self.progress.wait_timeout(completed, remaining) {
                Ok((c, _)) => c,
                Err(_) => return,
            };
        }
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
        self.cache.insert_builder_font(pattern, path);
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
