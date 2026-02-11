//! Asynchronous font registry with background scanning and on-demand blocking.
//!
//! `FcFontRegistry` replaces the monolithic `FcFontCache::build()` with a concurrent
//! system where background threads race to load fonts ahead of when the main thread
//! needs them. The main thread blocks at layout time (via `request_fonts()`) until the
//! specific fonts it needs are ready. This guarantees no flash of unstyled content (FOUC).
//!
//! # Architecture
//!
//! - **Scout** (1 thread): Enumerates font directories, guesses family names from
//!   filenames, and feeds paths to the Builder's priority queue. Takes ~5-20ms.
//! - **Builder Pool** (N threads): Parses font files from the priority queue, verifies
//!   CMAP tables, and writes results to the shared Registry.
//! - **Registry** (shared state): Thread-safe, incrementally-populated font database.
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
use alloc::collections::BTreeSet;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

use crate::{
    CssFallbackGroup, FcFont, FcFontCache, FcFontPath, FcPattern,
    FcWeight, FontChainCacheKey, FontFallbackChain, FontId, FontMatch, NamedFont, OperatingSystem,
    PatternMatch,
};

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
struct FontRequest {
    /// Lowercased, normalized family names being requested
    families: Vec<String>,
    /// Set to true by a Builder thread when all families are satisfied
    satisfied: Arc<AtomicBool>,
}

// ── The Registry ────────────────────────────────────────────────────────────

/// Thread-safe, incrementally-populated font registry.
///
/// Background threads (Scout + Builder pool) populate this concurrently while
/// the main thread reads from it. The main thread can block via `request_fonts()`
/// until specific fonts are available.
pub struct FcFontRegistry {
    // ── Populated by Scout (fast, Phase 1) ──
    /// Maps guessed lowercase family name → file paths
    /// e.g. "arial" → ["/System/Library/Fonts/Arial.ttf", "/System/Library/Fonts/Arial Bold.ttf"]
    known_paths: RwLock<BTreeMap<String, Vec<PathBuf>>>,

    // ── Populated by Builder (incremental, Phase 2+) ──
    /// Pattern → FontId mapping (query index)
    patterns: RwLock<BTreeMap<FcPattern, FontId>>,
    /// FontId → disk font path
    disk_fonts: RwLock<BTreeMap<FontId, FcFontPath>>,
    /// FontId → parsed pattern (metadata)
    metadata: RwLock<BTreeMap<FontId, FcPattern>>,
    /// Lowercase token → set of FontIds (inverted index for fuzzy search)
    token_index: RwLock<BTreeMap<String, BTreeSet<FontId>>>,
    /// FontId → pre-tokenized lowercase name tokens
    font_tokens: RwLock<BTreeMap<FontId, Vec<String>>>,

    // ── In-memory fonts (bundled, embedded) ──
    memory_fonts: RwLock<BTreeMap<FontId, FcFont>>,

    // ── Chain cache (computed lazily) ──
    chain_cache: Mutex<HashMap<FontChainCacheKey, FontFallbackChain>>,

    // ── Priority queue for Builder ──
    build_queue: Mutex<Vec<FcBuildJob>>,
    queue_condvar: Condvar,

    // ── Completion tracking ──
    pending_requests: Mutex<Vec<FontRequest>>,
    request_complete: Condvar,

    // ── Deduplication ──
    processed_paths: Mutex<HashSet<PathBuf>>,

    // ── Status ──
    scan_complete: AtomicBool,
    build_complete: AtomicBool,
    shutdown: AtomicBool,
    /// Whether a disk cache was successfully loaded (skip blocking in request_fonts)
    cache_loaded: AtomicBool,
    /// Number of font files successfully parsed by Builder threads
    files_parsed: AtomicUsize,
    /// Number of individual font faces loaded (a .ttc file can yield multiple)
    faces_loaded: AtomicUsize,
    /// Total number of font files discovered by Scout
    files_discovered: AtomicUsize,

    // ── Operating system (for font family expansion) ──
    os: OperatingSystem,
}

impl std::fmt::Debug for FcFontRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FcFontRegistry")
            .field("faces_loaded", &self.faces_loaded.load(Ordering::Relaxed))
            .field("files_parsed", &self.files_parsed.load(Ordering::Relaxed))
            .field("files_discovered", &self.files_discovered.load(Ordering::Relaxed))
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
            known_paths: RwLock::new(BTreeMap::new()),
            patterns: RwLock::new(BTreeMap::new()),
            disk_fonts: RwLock::new(BTreeMap::new()),
            metadata: RwLock::new(BTreeMap::new()),
            token_index: RwLock::new(BTreeMap::new()),
            font_tokens: RwLock::new(BTreeMap::new()),
            memory_fonts: RwLock::new(BTreeMap::new()),
            chain_cache: Mutex::new(HashMap::new()),
            build_queue: Mutex::new(Vec::new()),
            queue_condvar: Condvar::new(),
            pending_requests: Mutex::new(Vec::new()),
            request_complete: Condvar::new(),
            processed_paths: Mutex::new(HashSet::new()),
            scan_complete: AtomicBool::new(false),
            build_complete: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            cache_loaded: AtomicBool::new(false),
            files_parsed: AtomicUsize::new(0),
            faces_loaded: AtomicUsize::new(0),
            files_discovered: AtomicUsize::new(0),
            os: OperatingSystem::current(),
        })
    }

    /// Register in-memory (bundled) fonts. These are available immediately.
    pub fn register_memory_fonts(&self, fonts: Vec<NamedFont>) {
        for named_font in fonts {
            if let Some(parsed) = crate::FcParseFontBytes(&named_font.bytes, &named_font.name) {
                let mut patterns = self.patterns.write().unwrap();
                let mut metadata = self.metadata.write().unwrap();
                let mut memory_fonts = self.memory_fonts.write().unwrap();
                let mut token_index = self.token_index.write().unwrap();
                let mut font_tokens = self.font_tokens.write().unwrap();

                for (pattern, fc_font) in parsed {
                    let id = FontId::new();
                    Self::index_pattern_tokens_static(
                        &mut token_index,
                        &mut font_tokens,
                        &pattern,
                        id,
                    );
                    patterns.insert(pattern.clone(), id);
                    metadata.insert(id, pattern);
                    memory_fonts.insert(id, fc_font);
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
            .name("azul-font-scout".to_string())
            .spawn(move || {
                scout_thread(&registry);
            })
            .expect("failed to spawn font scout thread");

        // Spawn Builder threads
        for i in 0..num_threads {
            let registry = Arc::clone(self);
            std::thread::Builder::new()
                .name(format!("azul-font-builder-{}", i))
                .spawn(move || {
                    builder_thread(&registry);
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
            let expanded = crate::expand_font_families(stack, self.os, &[]);
            for family in &expanded {
                let normalized = normalize_family_name(family);
                if !needed_families.contains(&normalized) {
                    needed_families.push(normalized);
                }
            }
            expanded_stacks.push(expanded);
        }

        // Fast path: if disk cache was loaded, all previously-known fonts are
        // already in the patterns map.  We can resolve chains immediately.
        // Background builders will pick up any newly installed fonts later.
        if self.cache_loaded.load(Ordering::Acquire) {
            return self.resolve_chains(&expanded_stacks);
        }

        // 2. Check which families are already in the registry
        let mut missing: Vec<String> = Vec::new();
        {
            let patterns = self.patterns.read().unwrap();
            for family in &needed_families {
                let found = patterns.keys().any(|p| {
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

        // 3. If nothing is missing, resolve chains immediately
        if missing.is_empty() {
            return self.resolve_chains(&expanded_stacks);
        }

        // 4. Wait for Scout to finish (so we can look up file paths)
        while !self.scan_complete.load(Ordering::Acquire) {
            if Instant::now() >= deadline {
                eprintln!(
                    "[azul-font-registry] WARNING: Timed out waiting for font scout (5s). \
                     Proceeding with available fonts."
                );
                return self.resolve_chains(&expanded_stacks);
            }
            std::thread::sleep(Duration::from_millis(1));
        }

        // 5. For each missing family, look up known_paths and push Critical jobs
        {
            let known_paths = self.known_paths.read().unwrap();
            let mut queue = self.build_queue.lock().unwrap();

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
                // Also try partial matches (e.g. "arial" matches "arialblack")
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

            // Sort so Critical jobs are at the end (popped first with pop())
            queue.sort();
        }
        self.queue_condvar.notify_all();

        // 6. Register a pending request and wait for completion
        let satisfied = Arc::new(AtomicBool::new(false));
        {
            let mut pending = self.pending_requests.lock().unwrap();
            pending.push(FontRequest {
                families: missing.clone(),
                satisfied: Arc::clone(&satisfied),
            });
        }

        // 7. Wait for the Builder threads to satisfy our request (or timeout)
        while !satisfied.load(Ordering::Acquire) {
            if Instant::now() >= deadline {
                eprintln!(
                    "[azul-font-registry] WARNING: Timed out waiting for fonts: {:?} (5s). \
                     Proceeding with available fonts.",
                    missing
                );
                break;
            }

            // Also check if build is complete (all fonts processed)
            if self.build_complete.load(Ordering::Acquire) {
                break;
            }

            let pending = self.pending_requests.lock().unwrap();
            let remaining = deadline.saturating_duration_since(Instant::now());
            let _result = self.request_complete.wait_timeout(pending, remaining);
        }

        // 8. Resolve chains from the now-populated registry
        self.resolve_chains(&expanded_stacks)
    }

    /// Get font metadata by ID.
    pub fn get_metadata_by_id(&self, id: &FontId) -> Option<FcPattern> {
        self.metadata.read().unwrap().get(id).cloned()
    }

    /// Get font bytes for a given font ID (either from memory or disk).
    pub fn get_font_bytes(&self, id: &FontId) -> Option<Vec<u8>> {
        // Check memory fonts first
        if let Some(font) = self.memory_fonts.read().unwrap().get(id) {
            return Some(font.bytes.clone());
        }
        // Then check disk fonts
        if let Some(path) = self.disk_fonts.read().unwrap().get(id) {
            return std::fs::read(&path.path).ok();
        }
        None
    }

    /// Get the disk font path for a font ID.
    pub fn get_disk_font_path(&self, id: &FontId) -> Option<FcFontPath> {
        self.disk_fonts.read().unwrap().get(id).cloned()
    }

    /// Check if a font ID is a memory font.
    pub fn is_memory_font(&self, id: &FontId) -> bool {
        self.memory_fonts.read().unwrap().contains_key(id)
    }

    /// List all known fonts (pattern + ID pairs).
    pub fn list(&self) -> Vec<(FcPattern, FontId)> {
        self.patterns
            .read()
            .unwrap()
            .iter()
            .map(|(p, id)| (p.clone(), *id))
            .collect()
    }

    /// Query the registry for a font matching the given pattern.
    pub fn query(&self, pattern: &FcPattern) -> Option<FontMatch> {
        let patterns = self.patterns.read().unwrap();
        let metadata_map = self.metadata.read().unwrap();
        let memory_fonts = self.memory_fonts.read().unwrap();

        let mut matches = Vec::new();
        let mut trace = Vec::new();

        for (stored_pattern, id) in patterns.iter() {
            if FcFontCache::query_matches_internal(stored_pattern, pattern, &mut trace) {
                let meta = metadata_map.get(id).unwrap_or(stored_pattern);
                let unicode_compatibility = if pattern.unicode_ranges.is_empty() {
                    FcFontCache::calculate_unicode_coverage(&meta.unicode_ranges) as i32
                } else {
                    FcFontCache::calculate_unicode_compatibility(
                        &pattern.unicode_ranges,
                        &meta.unicode_ranges,
                    )
                };
                let style_score = FcFontCache::calculate_style_score(pattern, meta);
                let is_memory = memory_fonts.contains_key(id);
                matches.push((*id, unicode_compatibility, style_score, meta.clone(), is_memory));
            }
        }

        matches.sort_by(|a, b| {
            b.4.cmp(&a.4)
                .then_with(|| b.1.cmp(&a.1))
                .then_with(|| a.2.cmp(&b.2))
        });

        matches.first().map(|(id, _, _, meta, _)| FontMatch {
            id: *id,
            unicode_ranges: meta.unicode_ranges.clone(),
            fallbacks: Vec::new(),
        })
    }

    /// Resolve a complete font fallback chain for a CSS font-family stack.
    pub fn resolve_font_chain(
        &self,
        font_families: &[String],
        weight: FcWeight,
        italic: PatternMatch,
        oblique: PatternMatch,
    ) -> FontFallbackChain {
        // Check chain cache first
        let cache_key = FontChainCacheKey {
            font_families: font_families.to_vec(),
            weight,
            italic,
            oblique,
        };

        if let Ok(cache) = self.chain_cache.lock() {
            if let Some(cached) = cache.get(&cache_key) {
                return cached.clone();
            }
        }

        // Expand generic families
        let expanded = crate::expand_font_families(font_families, self.os, &[]);

        // Build chain
        let chain = self.resolve_font_chain_uncached(&expanded, weight, italic, oblique);

        // Cache it
        if let Ok(mut cache) = self.chain_cache.lock() {
            cache.insert(cache_key, chain.clone());
        }

        chain
    }

    /// Convert the registry into an immutable `FcFontCache` snapshot.
    pub fn into_fc_font_cache(&self) -> FcFontCache {
        let mut cache = FcFontCache::default();

        // Copy patterns
        let patterns = self.patterns.read().unwrap();
        let disk_fonts = self.disk_fonts.read().unwrap();
        let metadata_map = self.metadata.read().unwrap();
        let memory_fonts_map = self.memory_fonts.read().unwrap();

        for (pattern, id) in patterns.iter() {
            cache.patterns.insert(pattern.clone(), *id);
        }
        for (id, path) in disk_fonts.iter() {
            cache.disk_fonts.insert(*id, path.clone());
        }
        for (id, meta) in metadata_map.iter() {
            cache.metadata.insert(*id, meta.clone());
        }
        for (id, font) in memory_fonts_map.iter() {
            cache.memory_fonts.insert(*id, font.clone());
        }

        // Rebuild token index
        let token_index = self.token_index.read().unwrap();
        for (token, ids) in token_index.iter() {
            cache
                .token_index
                .insert(token.clone(), ids.clone());
        }
        let font_tokens = self.font_tokens.read().unwrap();
        for (id, tokens) in font_tokens.iter() {
            cache.font_tokens.insert(*id, tokens.clone());
        }

        cache
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

    /// Returns (files_parsed, files_discovered, faces_loaded).
    pub fn progress(&self) -> (usize, usize, usize) {
        (
            self.files_parsed.load(Ordering::Relaxed),
            self.files_discovered.load(Ordering::Relaxed),
            self.faces_loaded.load(Ordering::Relaxed),
        )
    }

    // ── Internal methods ────────────────────────────────────────────────────

    /// Insert a parsed font into the registry (called by Builder threads).
    fn insert_font(&self, pattern: FcPattern, path: FcFontPath) {
        let id = FontId::new();

        let mut patterns = self.patterns.write().unwrap();
        let mut disk_fonts = self.disk_fonts.write().unwrap();
        let mut metadata = self.metadata.write().unwrap();
        let mut token_index = self.token_index.write().unwrap();
        let mut font_tokens = self.font_tokens.write().unwrap();

        Self::index_pattern_tokens_static(&mut token_index, &mut font_tokens, &pattern, id);
        patterns.insert(pattern.clone(), id);
        disk_fonts.insert(id, path);
        metadata.insert(id, pattern);

        self.faces_loaded.fetch_add(1, Ordering::Relaxed);

        // Invalidate chain cache since we have new fonts
        if let Ok(mut cache) = self.chain_cache.lock() {
            cache.clear();
        }
    }

    /// Static helper for token indexing (doesn't need &self, works with mutable refs)
    fn index_pattern_tokens_static(
        token_index: &mut BTreeMap<String, BTreeSet<FontId>>,
        font_tokens: &mut BTreeMap<FontId, Vec<String>>,
        pattern: &FcPattern,
        id: FontId,
    ) {
        let mut all_tokens = Vec::new();
        if let Some(name) = &pattern.name {
            all_tokens.extend(FcFontCache::extract_font_name_tokens(name));
        }
        if let Some(family) = &pattern.family {
            all_tokens.extend(FcFontCache::extract_font_name_tokens(family));
        }

        let tokens_lower: Vec<String> = all_tokens.iter().map(|t| t.to_lowercase()).collect();

        for token_lower in &tokens_lower {
            token_index
                .entry(token_lower.clone())
                .or_insert_with(BTreeSet::new)
                .insert(id);
        }

        font_tokens.insert(id, tokens_lower);
    }

    /// Check and signal any pending requests that are now satisfied.
    fn check_and_signal_pending_requests(&self) {
        let mut pending = self.pending_requests.lock().unwrap();
        let patterns = self.patterns.read().unwrap();

        for request in pending.iter() {
            if request.satisfied.load(Ordering::Relaxed) {
                continue;
            }

            let all_found = request.families.iter().all(|family| {
                patterns.keys().any(|p| {
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

        // Remove satisfied requests
        pending.retain(|r| !r.satisfied.load(Ordering::Relaxed));

        // Signal waiting threads
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

    /// Internal chain resolution without caching.
    fn resolve_font_chain_uncached(
        &self,
        font_families: &[String],
        weight: FcWeight,
        italic: PatternMatch,
        oblique: PatternMatch,
    ) -> FontFallbackChain {
        let patterns = self.patterns.read().unwrap();
        let metadata_map = self.metadata.read().unwrap();
        let token_index = self.token_index.read().unwrap();
        let font_tokens_map = self.font_tokens.read().unwrap();
        let memory_fonts = self.memory_fonts.read().unwrap();

        let mut css_fallbacks = Vec::new();
        let mut trace = Vec::new();

        for family in font_families {
            let is_generic = matches!(
                family.to_lowercase().as_str(),
                "serif" | "sans-serif" | "monospace" | "cursive" | "fantasy" | "system-ui"
            );

            let matches = if is_generic {
                // Generic families need full pattern matching
                let pattern = match family.as_str() {
                    "monospace" => FcPattern {
                        name: None,
                        weight,
                        italic,
                        oblique,
                        monospace: PatternMatch::True,
                        unicode_ranges: Vec::new(),
                        ..Default::default()
                    },
                    _ => FcPattern {
                        name: None,
                        weight,
                        italic,
                        oblique,
                        monospace: PatternMatch::False,
                        unicode_ranges: Vec::new(),
                        ..Default::default()
                    },
                };

                let mut found = Vec::new();
                for (stored_pattern, id) in patterns.iter() {
                    if FcFontCache::query_matches_internal(stored_pattern, &pattern, &mut trace) {
                        let meta = metadata_map.get(id).unwrap_or(stored_pattern);
                        found.push(FontMatch {
                            id: *id,
                            unicode_ranges: meta.unicode_ranges.clone(),
                            fallbacks: Vec::new(),
                        });
                    }
                }
                if found.len() > 5 {
                    found.truncate(5);
                }
                found
            } else {
                // Specific font: use token-based fuzzy matching
                self.fuzzy_query_by_name_internal(
                    family,
                    weight,
                    italic,
                    oblique,
                    &patterns,
                    &metadata_map,
                    &token_index,
                    &font_tokens_map,
                    &memory_fonts,
                )
            };

            css_fallbacks.push(CssFallbackGroup {
                css_name: family.clone(),
                fonts: matches,
            });
        }

        FontFallbackChain {
            css_fallbacks,
            unicode_fallbacks: Vec::new(),
            original_stack: font_families.to_vec(),
        }
    }

    /// Token-based fuzzy matching (same algorithm as FcFontCache but using read locks).
    fn fuzzy_query_by_name_internal(
        &self,
        requested_name: &str,
        weight: FcWeight,
        italic: PatternMatch,
        oblique: PatternMatch,
        _patterns: &BTreeMap<FcPattern, FontId>,
        metadata_map: &BTreeMap<FontId, FcPattern>,
        token_index: &BTreeMap<String, BTreeSet<FontId>>,
        font_tokens_map: &BTreeMap<FontId, Vec<String>>,
        _memory_fonts: &BTreeMap<FontId, FcFont>,
    ) -> Vec<FontMatch> {
        let tokens = FcFontCache::extract_font_name_tokens(requested_name);
        if tokens.is_empty() {
            return Vec::new();
        }

        let tokens_lower: Vec<String> = tokens.iter().map(|t| t.to_lowercase()).collect();

        // Progressive token matching
        let first_token = &tokens_lower[0];
        let mut candidate_ids = match token_index.get(first_token) {
            Some(ids) if !ids.is_empty() => ids.clone(),
            _ => return Vec::new(),
        };

        for token in &tokens_lower[1..] {
            if let Some(token_ids) = token_index.get(token) {
                let intersection: BTreeSet<FontId> =
                    candidate_ids.intersection(token_ids).copied().collect();
                if intersection.is_empty() {
                    break;
                }
                candidate_ids = intersection;
            } else {
                break;
            }
        }

        let mut candidates = Vec::new();
        for id in candidate_ids {
            let pattern = match metadata_map.get(&id) {
                Some(p) => p,
                None => continue,
            };
            let ft = match font_tokens_map.get(&id) {
                Some(t) => t,
                None => continue,
            };
            if ft.is_empty() {
                continue;
            }

            let token_matches = tokens_lower
                .iter()
                .filter(|req_token| ft.iter().any(|font_token| font_token.contains(req_token.as_str())))
                .count();
            if token_matches == 0 {
                continue;
            }

            let token_similarity = (token_matches * 100 / tokens.len()) as i32;
            let style_score = FcFontCache::calculate_style_score(
                &FcPattern {
                    weight,
                    italic,
                    oblique,
                    ..Default::default()
                },
                pattern,
            );

            candidates.push((id, token_similarity, style_score, pattern.clone()));
        }

        candidates.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2)));
        candidates.truncate(5);

        candidates
            .into_iter()
            .map(|(id, _, _, pattern)| FontMatch {
                id,
                unicode_ranges: pattern.unicode_ranges.clone(),
                fallbacks: Vec::new(),
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
    let font_dirs = get_font_directories();

    let mut all_font_paths: Vec<(PathBuf, String)> = Vec::new();

    for dir_path in font_dirs {
        if registry.shutdown.load(Ordering::Relaxed) {
            return;
        }
        let _dir = match std::fs::read_dir(&dir_path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        collect_font_files_recursive(dir_path, &mut all_font_paths);
    }

    // Determine common OS fonts for high priority
    let common_families = get_common_font_families_for_os(registry.os);

    // Populate known_paths and build queue
    {
        let mut known_paths = registry.known_paths.write().unwrap();
        let mut queue = registry.build_queue.lock().unwrap();

        for (path, guessed_family) in &all_font_paths {
            known_paths
                .entry(guessed_family.clone())
                .or_insert_with(Vec::new)
                .push(path.clone());

            let priority = if common_families
                .iter()
                .any(|cf| guessed_family.contains(cf))
            {
                Priority::High
            } else {
                Priority::Low
            };

            queue.push(FcBuildJob {
                priority,
                path: path.clone(),
                font_index: None,
                guessed_family: guessed_family.clone(),
            });
        }

        // Sort queue so highest priority is at the end (pop from end)
        queue.sort();

        registry
            .files_discovered
            .store(all_font_paths.len(), Ordering::Relaxed);
    }

    registry.scan_complete.store(true, Ordering::Release);
    registry.queue_condvar.notify_all();
}

/// Recursively collect font files from a directory, guessing family names from filenames.
fn collect_font_files_recursive(dir: PathBuf, results: &mut Vec<(PathBuf, String)>) {
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
        } else if is_font_file(&path) {
            let guessed = guess_family_from_filename(&path);
            results.push((path, guessed));
        }
    }
}

/// Check if a file has a font extension.
fn is_font_file(path: &PathBuf) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => matches!(
            ext.to_lowercase().as_str(),
            "ttf" | "otf" | "ttc" | "woff" | "woff2" | "dfont"
        ),
        None => false,
    }
}

/// Guess the font family name from a filename.
///
/// Examples:
/// - `"ArialBold.ttf"` → `"arial"`
/// - `"NotoSansJP-Regular.otf"` → `"notosansjp"`
/// - `"Helvetica Neue Bold Italic.ttf"` → `"helveticaneue"`
fn guess_family_from_filename(path: &PathBuf) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    // Strip common style suffixes
    let cleaned = stem
        .replace("-Regular", "")
        .replace("-Bold", "")
        .replace("-Italic", "")
        .replace("-Light", "")
        .replace("-Medium", "")
        .replace("-Thin", "")
        .replace("-Black", "")
        .replace("-ExtraLight", "")
        .replace("-ExtraBold", "")
        .replace("-SemiBold", "")
        .replace("-DemiBold", "")
        .replace("-Heavy", "")
        .replace("-Oblique", "")
        .replace("_Regular", "")
        .replace("_Bold", "")
        .replace("_Italic", "")
        .replace("Bold", "")
        .replace("Italic", "")
        .replace("Regular", "")
        .replace("Light", "")
        .replace("Medium", "")
        .replace("Thin", "")
        .replace("Black", "")
        .replace("Oblique", "");

    // Remove non-alphanumeric, lowercase
    cleaned
        .chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Get OS-specific font directories.
fn get_font_directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/Library/Fonts"));
        dirs.push(PathBuf::from("/System/Library/AssetsV2"));
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(format!("{}/Library/Fonts", home)));
        }
    }

    #[cfg(target_os = "linux")]
    {
        // Parse /etc/fonts/fonts.conf for font directories
        // For simplicity, use the common locations directly
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(format!("{}/.fonts", home)));
            dirs.push(PathBuf::from(format!("{}/.local/share/fonts", home)));
        }
    }

    #[cfg(target_os = "windows")]
    {
        let system_root = std::env::var("SystemRoot")
            .or_else(|_| std::env::var("WINDIR"))
            .unwrap_or_else(|_| "C:\\Windows".to_string());
        let user_profile = std::env::var("USERPROFILE")
            .unwrap_or_else(|_| "C:\\Users\\Default".to_string());
        dirs.push(PathBuf::from(format!("{}\\Fonts", system_root)));
        dirs.push(PathBuf::from(format!(
            "{}\\AppData\\Local\\Microsoft\\Windows\\Fonts",
            user_profile
        )));
    }

    dirs
}

/// Get common font families that should be loaded at High priority.
///
/// Returns normalized (lowercased, no spaces) family names for the current OS.
pub fn get_common_font_families() -> Vec<String> {
    get_common_font_families_for_os(OperatingSystem::current())
}

fn get_common_font_families_for_os(os: OperatingSystem) -> Vec<String> {
    match os {
        OperatingSystem::MacOS => vec![
            "sanfrancisco".to_string(),
            "helveticaneue".to_string(),
            "helvetica".to_string(),
            "arial".to_string(),
            "timesnewroman".to_string(),
            "georgia".to_string(),
            "menlo".to_string(),
            "sfmono".to_string(),
            "courier".to_string(),
            "lucidagrande".to_string(),
        ],
        OperatingSystem::Linux => vec![
            "dejavusans".to_string(),
            "dejavuserif".to_string(),
            "dejavusansmono".to_string(),
            "liberation".to_string(),
            "noto".to_string(),
            "ubuntu".to_string(),
            "roboto".to_string(),
            "droidsans".to_string(),
            "arial".to_string(),
        ],
        OperatingSystem::Windows => vec![
            "segoeui".to_string(),
            "arial".to_string(),
            "timesnewroman".to_string(),
            "calibri".to_string(),
            "consolas".to_string(),
            "couriernew".to_string(),
            "tahoma".to_string(),
            "verdana".to_string(),
        ],
        OperatingSystem::Wasm => vec![],
    }
}

// ── Builder Thread ──────────────────────────────────────────────────────────

/// A Builder thread: pops jobs from the priority queue, parses fonts, and inserts
/// results into the registry.
fn builder_thread(registry: &FcFontRegistry) {
    loop {
        if registry.shutdown.load(Ordering::Relaxed) {
            return;
        }

        // Pop the highest-priority job
        let job = {
            let mut queue = registry.build_queue.lock().unwrap();

            loop {
                if registry.shutdown.load(Ordering::Relaxed) {
                    return;
                }

                if let Some(job) = queue.pop() {
                    break job;
                }

                // If scan is complete and queue is empty, we're done
                if registry.scan_complete.load(Ordering::Acquire) && queue.is_empty() {
                    registry.build_complete.store(true, Ordering::Release);
                    // Signal any waiting requests that we're done
                    registry.request_complete.notify_all();
                    return;
                }

                // Wait for new jobs
                queue = registry
                    .queue_condvar
                    .wait_timeout(queue, Duration::from_millis(100))
                    .unwrap()
                    .0;
            }
        };

        // Deduplication: skip if already processed
        {
            let mut processed = registry.processed_paths.lock().unwrap();
            if processed.contains(&job.path) {
                continue;
            }
            processed.insert(job.path.clone());
        }

        // Parse the font file
        if let Some(results) = crate::FcParseFont(&job.path) {
            for (pattern, font_path) in results {
                registry.insert_font(pattern, font_path);
            }
        }

        // Count this file as parsed regardless of whether it yielded faces
        registry.files_parsed.fetch_add(1, Ordering::Relaxed);

        // Check if any pending requests are now satisfied
        registry.check_and_signal_pending_requests();
    }
}

// ── Disk Cache ──────────────────────────────────────────────────────────────

/// Font cache manifest for on-disk serialization.
#[cfg(feature = "cache")]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FontManifest {
    /// Cache format version (bump on breaking changes)
    pub version: u32,
    /// Entries: path → cached font data
    pub entries: BTreeMap<String, FontCacheEntry>,
}

#[cfg(feature = "cache")]
impl FontManifest {
    pub const CURRENT_VERSION: u32 = 1;
}

/// A single cached font file entry.
#[cfg(feature = "cache")]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FontCacheEntry {
    /// File modification time (seconds since epoch)
    pub mtime_secs: u64,
    /// File size in bytes
    pub file_size: u64,
    /// Parsed font data for each font index in the file
    pub font_indices: Vec<FontIndexEntry>,
}

/// Parsed metadata for a single font index within a font file.
#[cfg(feature = "cache")]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FontIndexEntry {
    pub pattern: FcPattern,
    pub font_index: usize,
}

#[cfg(feature = "cache")]
impl FcFontRegistry {
    /// Load font metadata from the on-disk cache.
    ///
    /// If a valid cache exists, all font patterns are loaded into the registry
    /// immediately. The Scout thread will verify staleness in the background.
    pub fn load_from_disk_cache(&self) -> bool {
        let cache_path = match get_font_cache_path() {
            Some(p) => p,
            None => return false,
        };

        let data = match std::fs::read(&cache_path) {
            Ok(d) => d,
            Err(_) => return false,
        };

        let manifest: FontManifest = match bincode::deserialize(&data) {
            Ok(m) => m,
            Err(_) => return false,
        };

        if manifest.version != FontManifest::CURRENT_VERSION {
            return false;
        }

        let mut patterns = self.patterns.write().unwrap();
        let mut disk_fonts = self.disk_fonts.write().unwrap();
        let mut metadata = self.metadata.write().unwrap();
        let mut token_index = self.token_index.write().unwrap();
        let mut font_tokens = self.font_tokens.write().unwrap();

        let mut count = 0usize;
        let mut processed = self.processed_paths.lock().unwrap();
        for (path_str, entry) in &manifest.entries {
            // Mark this file as already processed so builder threads skip it
            processed.insert(PathBuf::from(path_str));
            for idx_entry in &entry.font_indices {
                let id = FontId::new();
                Self::index_pattern_tokens_static(
                    &mut token_index,
                    &mut font_tokens,
                    &idx_entry.pattern,
                    id,
                );
                patterns.insert(idx_entry.pattern.clone(), id);
                disk_fonts.insert(
                    id,
                    FcFontPath {
                        path: path_str.clone(),
                        font_index: idx_entry.font_index,
                    },
                );
                metadata.insert(id, idx_entry.pattern.clone());
                count += 1;
            }
        }
        drop(processed);

        self.faces_loaded.store(count, Ordering::Relaxed);
        // Don't set files_parsed here — that counter tracks builder thread work.
        // files_discovered will be set by the scout thread when it runs.
        self.cache_loaded.store(true, Ordering::Release);

        true
    }

    /// Save the current registry state to the on-disk cache.
    pub fn save_to_disk_cache(&self) {
        let cache_path = match get_font_cache_path() {
            Some(p) => p,
            None => return,
        };

        // Create parent directories
        if let Some(parent) = cache_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let disk_fonts = self.disk_fonts.read().unwrap();
        let metadata_map = self.metadata.read().unwrap();

        let mut entries: BTreeMap<String, FontCacheEntry> = BTreeMap::new();

        for (id, font_path) in disk_fonts.iter() {
            if let Some(pattern) = metadata_map.get(id) {
                let entry = entries
                    .entry(font_path.path.clone())
                    .or_insert_with(|| {
                        let (mtime_secs, file_size) = get_file_metadata(&font_path.path);
                        FontCacheEntry {
                            mtime_secs,
                            file_size,
                            font_indices: Vec::new(),
                        }
                    });

                entry.font_indices.push(FontIndexEntry {
                    pattern: pattern.clone(),
                    font_index: font_path.font_index,
                });
            }
        }

        let manifest = FontManifest {
            version: FontManifest::CURRENT_VERSION,
            entries,
        };

        if let Ok(data) = bincode::serialize(&manifest) {
            let _ = std::fs::write(&cache_path, data);
        }
    }
}

/// Get file mtime and size.
#[cfg(feature = "cache")]
fn get_file_metadata(path: &str) -> (u64, u64) {
    match std::fs::metadata(path) {
        Ok(meta) => {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            (mtime, meta.len())
        }
        Err(_) => (0, 0),
    }
}

/// Get the path to the font cache manifest file.
#[cfg(feature = "cache")]
fn get_font_cache_path() -> Option<PathBuf> {
    let base = get_cache_base_dir()?;
    Some(base.join("fonts").join("manifest.bin"))
}

/// Get the base cache directory for azul.
#[cfg(feature = "cache")]
fn get_cache_base_dir() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let xdg = std::env::var("XDG_CACHE_HOME").ok();
        let base = match xdg {
            Some(dir) => PathBuf::from(dir),
            None => {
                let home = std::env::var("HOME").ok()?;
                PathBuf::from(home).join(".cache")
            }
        };
        Some(base.join("azul"))
    }

    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").ok()?;
        Some(PathBuf::from(home).join("Library").join("Caches").join("azul"))
    }

    #[cfg(target_os = "windows")]
    {
        let local_app_data = std::env::var("LOCALAPPDATA").ok()?;
        Some(PathBuf::from(local_app_data).join("azul"))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Normalize a family name for comparison: lowercase, strip spaces/hyphens/underscores.
fn normalize_family_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}
