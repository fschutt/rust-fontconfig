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
use crate::config;
use crate::utils::normalize_family_name;

// ── Priority Queue ──────────────────────────────────────────────────────────

/// Priority levels for font build jobs.
/// Critical > High > Medium > Low (higher numeric value = higher priority).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    /// Everything else found by Scout
    Low = 0,
    /// Disk cache hit (cheap deserialization)
    Medium = 1,
    /// Common OS default fonts (sans-serif, serif, monospace)
    High = 2,
    /// Main thread is blocked waiting for this font
    Critical = 3,
}

/// A job for the Builder pool to process.
#[derive(Debug, Clone)]
pub struct FcBuildJob {
    pub priority: Priority,
    pub path: PathBuf,
    pub font_index: Option<usize>,
    /// The guessed family name (lowercase, from filename)
    pub guessed_family: String,
}

impl PartialEq for FcBuildJob {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.path == other.path
    }
}
impl Eq for FcBuildJob {}

impl PartialOrd for FcBuildJob {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FcBuildJob {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority.cmp(&other.priority)
    }
}

// ── Font Request Tracking ───────────────────────────────────────────────────

/// A pending request from the main thread for a set of font families.
#[derive(Debug)]
pub struct FontRequest {
    /// Lowercased, normalized family names being requested
    families: Vec<String>,
    /// Set to true by a Builder thread when all families are satisfied
    satisfied: Arc<AtomicBool>,
}

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
    pub queue_condvar: Condvar,

    // ── Completion tracking ──
    pub pending_requests: Mutex<Vec<FontRequest>>,
    pub request_complete: Condvar,

    // ── Deduplication ──
    pub processed_paths: Mutex<HashSet<PathBuf>>,
    /// Paths that have been fully parsed and whose patterns are inserted.
    /// Unlike `processed_paths` (set before parsing for dedup), this is set
    /// AFTER parsing + insert_font(), so it's safe to wait on.
    pub completed_paths: Mutex<HashSet<PathBuf>>,

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
            pending_requests: Mutex::new(Vec::new()),
            request_complete: Condvar::new(),
            processed_paths: Mutex::new(HashSet::new()),
            completed_paths: Mutex::new(HashSet::new()),
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
            if let Some(parsed) = FcParseFontBytes(&named_font.bytes, &named_font.name) {
                if let Ok(mut cache) = self.cache.write() {
                    cache.with_memory_fonts(parsed);
                }
            }
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
                scout_thread(&registry);
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

        // 2. Wait for Scout to finish first (typically < 100ms).
        while !self.scan_complete.load(Ordering::Acquire) {
            if Instant::now() >= deadline {
                eprintln!(
                    "[rfc-font-registry] WARNING: Timed out waiting for font scout (5s). \
                     Proceeding with available fonts."
                );
                return self.resolve_chains(&expanded_stacks);
            }
            std::thread::sleep(Duration::from_millis(1));
        }

        // 3. Check which families are completely missing from the registry
        let mut missing: Vec<String> = Vec::new();
        {
            if let Ok(cache) = self.cache.read() {
                for family in &needed_families {
                    let found = cache.patterns.keys().any(|p| {
                        p.name
                            .as_ref()
                            .map(|n| normalize_family_name(n) == *family)
                            .unwrap_or(false)
                            || p.family
                                .as_ref()
                                .map(|f| normalize_family_name(f) == *family)
                                .unwrap_or(false)
                    });
                    if !found {
                        missing.push(family.clone());
                    }
                }
            }
        }

        // 4. Even if families have at least one pattern, some font FILES for
        //    that family may still be unprocessed (e.g., SFNSItalic.ttf parsed
        //    but SFNS.ttf not yet => only italic variant available).
        //    Use completed_paths (not processed_paths!) because processed_paths
        //    is set BEFORE parsing, while completed_paths is set AFTER parsing
        //    and insert_font().
        let mut incomplete_paths: Vec<(PathBuf, String)> = Vec::new();
        if let (Ok(known), Ok(completed)) = (self.known_paths.read(), self.completed_paths.lock()) {
            for family in &needed_families {
                if let Some(paths) = known.get(family) {
                    for path in paths {
                        if !completed.contains(path) {
                            incomplete_paths.push((path.clone(), family.clone()));
                        }
                    }
                }
                for (known_fam, paths) in known.iter() {
                    if known_fam != family
                        && (known_fam.contains(family.as_str())
                            || family.contains(known_fam.as_str()))
                    {
                        for path in paths {
                            if !completed.contains(path) {
                                incomplete_paths.push((path.clone(), known_fam.clone()));
                            }
                        }
                    }
                }
            }
        }

        // 5. If nothing is missing AND all files are processed, resolve immediately
        if missing.is_empty() && incomplete_paths.is_empty() {
            return self.resolve_chains(&expanded_stacks);
        }

        // 6. Push Critical jobs for missing families AND incomplete file variants
        if let (Ok(known_paths), Ok(mut queue)) = (self.known_paths.read(), self.build_queue.lock()) {

            for family in &missing {
                if let Some(paths) = known_paths.get(family) {
                    for path in paths {
                        queue.push(FcBuildJob {
                            priority: Priority::Critical,
                            path: path.clone(),
                            font_index: None,
                            guessed_family: family.clone(),
                        });
                    }
                }
                for (known_family, paths) in known_paths.iter() {
                    if known_family.contains(family.as_str())
                        || family.contains(known_family.as_str())
                    {
                        for path in paths {
                            queue.push(FcBuildJob {
                                priority: Priority::Critical,
                                path: path.clone(),
                                font_index: None,
                                guessed_family: known_family.clone(),
                            });
                        }
                    }
                }
            }

            for (path, fam) in &incomplete_paths {
                queue.push(FcBuildJob {
                    priority: Priority::Critical,
                    path: path.clone(),
                    font_index: None,
                    guessed_family: fam.clone(),
                });
            }

            queue.sort();
        }
        self.queue_condvar.notify_all();

        // 7. Wait for ALL specific files to be processed
        let wait_paths: HashSet<PathBuf> = {
            let mut paths = HashSet::new();
            for (path, _) in &incomplete_paths {
                paths.insert(path.clone());
            }
            if let Ok(known_paths) = self.known_paths.read() {
                for family in &missing {
                    if let Some(fam_paths) = known_paths.get(family) {
                        for p in fam_paths {
                            paths.insert(p.clone());
                        }
                    }
                    for (known_fam, fam_paths) in known_paths.iter() {
                        if known_fam.contains(family.as_str())
                            || family.contains(known_fam.as_str())
                        {
                            for p in fam_paths {
                                paths.insert(p.clone());
                            }
                        }
                    }
                }
            }
            paths
        };

        if !wait_paths.is_empty() {
            loop {
                let all_done = self.completed_paths.lock()
                    .map(|completed| wait_paths.iter().all(|p| completed.contains(p)))
                    .unwrap_or(true);
                if all_done {
                    break;
                }
                if Instant::now() >= deadline {
                    eprintln!(
                        "[rfc-font-registry] WARNING: Timed out waiting for font files (5s). \
                         Proceeding with available fonts."
                    );
                    break;
                }
                if self.build_complete.load(Ordering::Acquire) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
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
        if let Ok(cache) = self.cache.read() {
            let mut trace = Vec::new();
            cache.resolve_font_chain_with_os(
                font_families, weight, italic, oblique, &mut trace, self.os,
            )
        } else {
            FontFallbackChain {
                css_fallbacks: Vec::new(),
                unicode_fallbacks: Vec::new(),
                original_stack: font_families.to_vec(),
            }
        }
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
        if let Ok(mut cache) = self.cache.write() {
            let id = FontId::new();
            cache.index_pattern_tokens(&pattern, id);
            cache.patterns.insert(pattern.clone(), id);
            cache.disk_fonts.insert(id, path);
            cache.metadata.insert(id, pattern);

            // Invalidate chain cache since we have new fonts
            if let Ok(mut cc) = cache.chain_cache.lock() {
                cc.clear();
            }
        }
    }

    /// Check and signal any pending requests that are now satisfied.
    pub fn check_and_signal_pending_requests(&self) {
        let mut pending = match self.pending_requests.lock() {
            Ok(p) => p,
            Err(_) => return,
        };
        let cache = match self.cache.read() {
            Ok(c) => c,
            Err(_) => return,
        };

        for request in pending.iter() {
            if request.satisfied.load(Ordering::Relaxed) {
                continue;
            }

            let all_found = request.families.iter().all(|family| {
                cache.patterns.keys().any(|p| {
                    p.name
                        .as_ref()
                        .map(|n| normalize_family_name(n) == *family)
                        .unwrap_or(false)
                        || p.family
                            .as_ref()
                            .map(|f| normalize_family_name(f) == *family)
                            .unwrap_or(false)
                })
            });

            if all_found {
                request.satisfied.store(true, Ordering::Release);
            }
        }

        pending.retain(|r| !r.satisfied.load(Ordering::Relaxed));

        drop(pending);
        self.request_complete.notify_all();
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
        self.shutdown.store(true, Ordering::Release);
        self.queue_condvar.notify_all();
    }
}

// ── Scout Thread ────────────────────────────────────────────────────────────

/// The Scout thread: enumerates font directories and populates the build queue.
fn scout_thread(registry: &FcFontRegistry) {
    let font_dirs = config::font_directories(registry.os);

    let mut all_font_paths: Vec<PathBuf> = Vec::new();

    for dir_path in font_dirs {
        if registry.shutdown.load(Ordering::Relaxed) {
            return;
        }
        if std::fs::read_dir(&dir_path).is_err() {
            continue;
        }
        collect_font_files_recursive(dir_path, &mut all_font_paths);
    }

    // Pre-tokenize common families once (not per-file)
    let common_token_sets = config::tokenize_common_families(registry.os);

    if let (Ok(mut known_paths), Ok(mut queue)) = (registry.known_paths.write(), registry.build_queue.lock()) {
        for path in &all_font_paths {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let all_tokens = crate::FcFontCache::extract_font_name_tokens(stem)
                .into_iter()
                .map(|t| t.to_lowercase())
                .collect::<Vec<_>>();

            // Guessed family: style-filtered tokens joined
            let guessed_family: String = all_tokens
                .iter()
                .filter(|t| !config::FONT_STYLE_TOKENS.iter().any(|s| s.eq_ignore_ascii_case(t)))
                .cloned()
                .collect::<Vec<_>>()
                .join("");

            known_paths
                .entry(guessed_family.clone())
                .or_insert_with(Vec::new)
                .push(path.clone());

            // Priority: token-based matching against common families
            let priority = if config::matches_common_family_tokens(&all_tokens, &common_token_sets) {
                Priority::High
            } else {
                Priority::Low
            };

            queue.push(FcBuildJob {
                priority,
                path: path.clone(),
                font_index: None,
                guessed_family,
            });
        }

        queue.sort();
    }

    registry.scan_complete.store(true, Ordering::Release);
    registry.queue_condvar.notify_all();
}

/// Recursively collect font files from a directory.
fn collect_font_files_recursive(dir: PathBuf, results: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();

        if path.is_dir() {
            collect_font_files_recursive(path, results);
        } else if crate::utils::is_font_file(&path) {
            results.push(path);
        }
    }
}
