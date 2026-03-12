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
    FcFontCache, FcFontPath, FcPattern, FcWeight, FontFallbackChain, FontId, FontMatch,
    NamedFont, OperatingSystem, PatternMatch,
};
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
struct FontRequest {
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
    cache: RwLock<FcFontCache>,

    // ── Populated by Scout (fast, Phase 1) ──
    /// Maps guessed lowercase family name → file paths
    known_paths: RwLock<BTreeMap<String, Vec<PathBuf>>>,

    // ── Priority queue for Builder ──
    build_queue: Mutex<Vec<FcBuildJob>>,
    queue_condvar: Condvar,

    // ── Completion tracking ──
    pending_requests: Mutex<Vec<FontRequest>>,
    request_complete: Condvar,

    // ── Deduplication ──
    processed_paths: Mutex<HashSet<PathBuf>>,
    /// Paths that have been fully parsed and whose patterns are inserted.
    /// Unlike `processed_paths` (set before parsing for dedup), this is set
    /// AFTER parsing + insert_font(), so it's safe to wait on.
    completed_paths: Mutex<HashSet<PathBuf>>,

    // ── Status ──
    scan_complete: AtomicBool,
    build_complete: AtomicBool,
    shutdown: AtomicBool,
    /// Whether a disk cache was successfully loaded (skip blocking in request_fonts)
    cache_loaded: AtomicBool,

    // ── Operating system (for font family expansion) ──
    os: OperatingSystem,
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
            if let Some(parsed) = crate::FcParseFontBytes(&named_font.bytes, &named_font.name) {
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
        {
            let known = self.known_paths.read().unwrap();
            let completed = self.completed_paths.lock().unwrap();
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
            let known_paths = self.known_paths.read().unwrap();
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
            paths
        };

        if !wait_paths.is_empty() {
            loop {
                let all_done = {
                    let completed = self.completed_paths.lock().unwrap();
                    wait_paths.iter().all(|p| completed.contains(p))
                };
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
    fn insert_font(&self, pattern: FcPattern, path: FcFontPath) {
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
    fn check_and_signal_pending_requests(&self) {
        let mut pending = self.pending_requests.lock().unwrap();
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

    let common_families = get_common_font_families_for_os(registry.os);

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

        queue.sort();
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
            "sfns".to_string(),
            "systemfont".to_string(),
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

        // Mark this file as fully completed (patterns inserted)
        {
            let mut completed = registry.completed_paths.lock().unwrap();
            completed.insert(job.path.clone());
        }

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
    /// Reads and deserializes the bincode font manifest from the platform
    /// cache directory, then populates the inner `FcFontCache` with all cached
    /// patterns, font paths, and token indices. Marks all cached file paths as
    /// processed/completed so builder threads skip them.
    ///
    /// Returns `Some(())` on success, `None` if the cache is missing,
    /// unreadable, malformed, or has a version mismatch.
    /// On WASM this is a no-op that always returns `None`.
    #[cfg(not(target_family = "wasm"))]
    pub fn load_from_disk_cache(&self) -> Option<()> {
        let cache_path = get_font_cache_path()?;
        let data = std::fs::read(&cache_path).ok()?;
        let manifest: FontManifest = bincode::deserialize(&data).ok()?;

        if manifest.version != FontManifest::CURRENT_VERSION {
            return None;
        }

        let mut cache = self.cache.write().ok()?;
        let mut processed = self.processed_paths.lock().ok()?;
        let mut completed = self.completed_paths.lock().ok()?;

        manifest.entries.iter()
            .flat_map(|(path_str, entry)| {
                let pb = PathBuf::from(path_str);
                processed.insert(pb.clone());
                completed.insert(pb);
                entry.font_indices.iter().map(move |idx_entry| (path_str, idx_entry))
            })
            .for_each(|(path_str, idx_entry)| {
                let id = FontId::new();
                cache.index_pattern_tokens(&idx_entry.pattern, id);
                cache.patterns.insert(idx_entry.pattern.clone(), id);
                cache.disk_fonts.insert(id, FcFontPath {
                    path: path_str.clone(),
                    font_index: idx_entry.font_index,
                });
                cache.metadata.insert(id, idx_entry.pattern.clone());
            });

        self.cache_loaded.store(true, Ordering::Release);

        Some(())
    }

    /// No-op on WASM — no filesystem access available.
    #[cfg(target_family = "wasm")]
    pub fn load_from_disk_cache(&self) -> Option<()> {
        None
    }

    /// Serialize the current registry state to the on-disk font cache.
    ///
    /// Collects all discovered font paths and their parsed metadata into a
    /// [`FontManifest`], then writes it as bincode to the platform cache
    /// directory (e.g. `~/.cache/rfc/fonts/manifest.bin` on Linux).
    ///
    /// Returns `None` if the cache path cannot be determined, the parent
    /// directory cannot be created, or serialization / writing fails.
    /// On WASM this is a no-op that always returns `None` (no filesystem access).
    #[cfg(not(target_family = "wasm"))]
    pub fn save_to_disk_cache(&self) -> Option<()> {
        let cache_path = get_font_cache_path()?;
        std::fs::create_dir_all(cache_path.parent()?).ok()?;

        let cache = self.cache.read().ok()?;

        let mut entries: BTreeMap<String, FontCacheEntry> = BTreeMap::new();

        cache.disk_fonts.iter()
            .filter_map(|(id, font_path)| {
                cache.metadata.get(id).map(|pattern| (font_path, pattern))
            })
            .for_each(|(font_path, pattern)| {
                entries
                    .entry(font_path.path.clone())
                    .or_insert_with(|| {
                        let (mtime_secs, file_size) = get_file_metadata(&font_path.path)
                            .unwrap_or((0, 0));
                        FontCacheEntry {
                            mtime_secs,
                            file_size,
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

        let data = bincode::serialize(&manifest).ok()?;
        std::fs::write(&cache_path, data).ok()?;

        Some(())
    }

    /// No-op on WASM — no filesystem access available.
    #[cfg(target_family = "wasm")]
    pub fn save_to_disk_cache(&self) -> Option<()> {
        None
    }
}

/// Get file mtime (seconds since epoch) and size in bytes.
#[cfg(feature = "cache")]
fn get_file_metadata(path: &str) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some((mtime, meta.len()))
}

/// Get the path to the font cache manifest file.
#[cfg(feature = "cache")]
fn get_font_cache_path() -> Option<PathBuf> {
    let base = get_cache_base_dir()?;
    Some(base.join("fonts").join("manifest.bin"))
}

/// Get the base cache directory for rust-fontconfig.
#[cfg(all(feature = "cache", not(target_family = "wasm")))]
fn get_cache_base_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("rfc"))
}

/// Returns `None` on platforms without a conventional cache directory (e.g. WASM).
#[cfg(all(feature = "cache", target_family = "wasm"))]
fn get_cache_base_dir() -> Option<PathBuf> {
    None
}
