//! Asynchronous font registry with background scanning and on-demand blocking.

use alloc::collections::btree_map::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use std::collections::HashSet;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
#[cfg(not(target_family = "wasm"))]
use std::time::Instant;

use crate::config::{FcFallbackConfig, FcScanConfig};
use crate::scoring::{
    family_exists_in_patterns, find_family_paths, find_incomplete_paths, FcBuildJob, Priority,
};
use crate::utils::normalize_family_name;
use crate::{
    FcFontCache, FcFontPath, FcParseFontBytes, FcPattern, FcWeight, FontFallbackChain, FontId,
    FontMatch, NamedFont, OperatingSystem, PatternMatch, UnicodeRange,
};

/// True when this target has no scout/builder threads.
#[cfg(target_family = "wasm")]
const WAITLESS_TARGET: bool = true;
#[cfg(not(target_family = "wasm"))]
const WAITLESS_TARGET: bool = false;

/// Thread-safe, incrementally-populated font registry.
pub struct FcFontRegistry {
    /// The underlying font cache, populated incrementally by Builder threads.
    pub cache: FcFontCache,

    // ── Populated by Scout (fast, Phase 1) ──
    /// Maps guessed lowercase family name → file paths.
    // [az-web-lift] queue RwLock spins in lock_contended in single-threaded lifted wasm
    // (Mutex is Leaf-stubbed and fine; only the pure-Rust queue RwLock is lifted). StLock = no-atomic single-threaded bypass. See lib.rs.
    pub known_paths: crate::StLock<BTreeMap<String, Vec<PathBuf>>>,

    // ── Priority queue for Builder ──
    /// Pending font files discovered by the scout thread, waiting to be parsed by builders.
    pub build_queue: Mutex<Vec<FcBuildJob>>,
    /// Notified when new jobs are added to `build_queue` or on shutdown.
    pub queue_condvar: Condvar,

    // ── Deduplication ──
    /// Paths claimed for parsing (set BEFORE parsing, for deduplication).
    pub processed_paths: Mutex<HashSet<PathBuf>>,
    /// Paths fully parsed and inserted into cache (set AFTER parsing).
    pub completed_paths: Mutex<HashSet<PathBuf>>,

    // ── Progress notification ──
    /// Notified when any progress occurs: font completed, scan done, build done.
    pub progress: Condvar,

    // ── Status ──
    /// True when the scout thread has finished enumerating files.
    pub scan_complete: AtomicBool,
    /// True when the scout is done and the build queue is fully exhausted.
    pub build_complete: AtomicBool,
    /// Jobs popped from `build_queue` whose parse has not finished yet.
    pub in_flight: std::sync::atomic::AtomicUsize,
    /// Strong handles currently held by the registry's own threads.
    pub thread_handles: std::sync::atomic::AtomicUsize,
    /// Whether the builder that completes the build persists the manifest.
    pub persist_on_complete: AtomicBool,
    pub shutdown: AtomicBool,
    /// Whether a disk cache was successfully loaded.
    pub cache_loaded: AtomicBool,
    /// When true, the scout finds paths but does not queue jobs unless requested.
    pub lazy_scout: AtomicBool,

    // ── Injected host knowledge ──
    /// Configuration defining where fonts live and which families to parse first.
    pub scan_config: FcScanConfig,

    // ── Operating system (for font family expansion) ──
    /// The OS identifier used to choose the default `scan_config` and generic fallbacks.
    pub os: OperatingSystem,
}

impl std::fmt::Debug for FcFontRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FcFontRegistry")
            .field("scan_complete", &self.scan_complete.load(Ordering::Relaxed))
            .field(
                "build_complete",
                &self.build_complete.load(Ordering::Relaxed),
            )
            .field("cache_loaded", &self.cache_loaded.load(Ordering::Relaxed))
            .finish()
    }
}

impl FcFontRegistry {
    /// Create a new empty registry with default scan directories and fallback config.
    pub fn new() -> Arc<Self> {
        let os = OperatingSystem::current();
        let mut scan = FcScanConfig::os_defaults(os);
        let mut fallback = FcFallbackConfig::default();
        // Merge system fonts.conf directories and aliases.
        if let Some(system) = crate::FcSystemConfig::from_system() {
            for dir in system.font_dirs {
                if !scan.font_dirs.contains(&dir) {
                    scan.font_dirs.push(dir);
                }
            }
            fallback.absorb_system_aliases(system.aliases);
        }
        fallback.merge_defaults(&FcFallbackConfig::os_defaults(os));
        Self::new_with_configs(scan, fallback)
    }

    /// Create a new empty registry with a custom scan configuration.
    pub fn new_with_config(scan_config: FcScanConfig) -> Arc<Self> {
        Self::new_with_configs(
            scan_config,
            FcFallbackConfig::os_defaults(OperatingSystem::current()),
        )
    }

    /// Create a registry with custom scan and fallback configurations.
    pub fn new_with_configs(
        scan_config: FcScanConfig,
        fallback_config: FcFallbackConfig,
    ) -> Arc<Self> {
        Arc::new(Self {
            cache: FcFontCache::default().with_fallback_config(fallback_config),
            known_paths: crate::StLock::new(BTreeMap::new()),
            build_queue: Mutex::new(Vec::new()),
            queue_condvar: Condvar::new(),
            processed_paths: Mutex::new(HashSet::new()),
            completed_paths: Mutex::new(HashSet::new()),
            progress: Condvar::new(),
            scan_complete: AtomicBool::new(false),
            build_complete: AtomicBool::new(false),
            in_flight: std::sync::atomic::AtomicUsize::new(0),
            thread_handles: std::sync::atomic::AtomicUsize::new(0),
            persist_on_complete: AtomicBool::new(true),
            shutdown: AtomicBool::new(false),
            cache_loaded: AtomicBool::new(false),
            lazy_scout: AtomicBool::new(false),
            scan_config,
            os: OperatingSystem::current(),
        })
    }

    /// Returns the directories the scout is configured to walk.
    pub fn scan_dirs(&self) -> &[PathBuf] {
        &self.scan_config.font_dirs
    }

    /// Configure whether the final builder thread writes the on-disk cache.
    pub fn set_persist_on_complete(&self, persist: bool) {
        self.persist_on_complete.store(persist, Ordering::Release);
    }

    /// Enable/disable lazy scout mode.
    pub fn set_scout_lazy(&self, lazy: bool) {
        self.lazy_scout.store(lazy, Ordering::Release);
    }

    /// Register in-memory (bundled) fonts.
    pub fn register_memory_fonts(&self, fonts: Vec<NamedFont>) {
        for named_font in fonts {
            let Some(parsed) = FcParseFontBytes(&named_font.bytes, &named_font.name) else {
                continue;
            };
            self.cache.with_memory_fonts(parsed);
        }
    }

    /// Spawn the Scout thread and Builder pool.
    pub fn spawn_scout_and_builders(self: &Arc<Self>) {
        #[cfg(feature = "single-thread-unsafe-locks")]
        {
            // Not a panic: consumers on single-threaded targets are correct to call this.
            let _ = self;
            return;
        }

        #[cfg(not(feature = "single-thread-unsafe-locks"))]
        {
            let num_threads = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(2)
                .saturating_sub(1)
                .max(1);

            // Spawn Scout thread. Threads hold only a Weak handle.
            let scout = Arc::downgrade(self);
            std::thread::Builder::new()
                .name("rfc-font-scout".to_string())
                .spawn(move || {
                    if let Some(registry) = scout.upgrade() {
                        if FcFontRegistry::enter_step(&registry) {
                            registry.scout_thread();
                        }
                        registry.leave_step();
                    }
                })
                .expect("failed to spawn font scout thread");

            // Spawn Builder threads
            for i in 0..num_threads {
                let registry = Arc::downgrade(self);
                std::thread::Builder::new()
                    .name(format!("rfc-font-builder-{}", i))
                    .spawn(move || FcFontRegistry::builder_thread(registry))
                    .expect("failed to spawn font builder thread");
            }
        }
    }

    /// Block until requested font families are loaded (5s timeout).
    pub fn request_fonts(&self, family_stacks: &[Vec<String>]) -> Vec<FontFallbackChain> {
        let deadline = Instant::now() + Duration::from_secs(5);

        let mut needed_families: Vec<String> = Vec::new();
        let config = self.cache.fallback_config();

        for stack in family_stacks {
            for family in config.candidate_families(stack, crate::DEFAULT_UNICODE_FALLBACK_SCRIPTS)
            {
                let normalized = normalize_family_name(&family);
                if !needed_families.contains(&normalized) {
                    needed_families.push(normalized);
                }
            }
        }

        if self.cache_loaded.load(Ordering::Acquire) || self.build_complete.load(Ordering::Acquire)
        {
            let result = self.resolve_chains(family_stacks);
            return result;
        }

        if !self.scan_complete.load(Ordering::Acquire) {
            let Ok(mut completed) = self.completed_paths.lock() else {
                return self.resolve_chains(family_stacks);
            };
            while !self.scan_complete.load(Ordering::Acquire) {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    if !WAITLESS_TARGET {
                        eprintln!(
                            "[rfc-font-registry] WARNING: Timed out waiting for font scout (5s). \
                             Proceeding with available fonts."
                        );
                    }
                    return self.resolve_chains(family_stacks);
                }
                completed = match self.progress.wait_timeout(completed, remaining) {
                    Ok((c, _)) => c,
                    Err(_) => return self.resolve_chains(family_stacks),
                };
            }
        }

        let missing: Vec<String> = {
            let state = self.cache.state_read();
            needed_families
                .iter()
                .filter(|fam| !family_exists_in_patterns(fam, state.metadata.values()))
                .cloned()
                .collect()
        };

        let incomplete_paths = self
            .known_paths
            .read()
            .ok()
            .zip(self.completed_paths.lock().ok())
            .map(|(known, completed)| find_incomplete_paths(&needed_families, &known, &completed))
            .unwrap_or_default();

        if missing.is_empty() && incomplete_paths.is_empty() {
            let r = self.resolve_chains(family_stacks);
            return r;
        }

        let wait_paths: HashSet<PathBuf> = if let (Ok(known_paths), Ok(mut queue)) =
            (self.known_paths.read(), self.build_queue.lock())
        {
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
        if !wait_paths.is_empty() {
            let Ok(mut completed) = self.completed_paths.lock() else {
                return self.resolve_chains(family_stacks);
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
                    if !WAITLESS_TARGET {
                        eprintln!(
                            "[rfc-font-registry] WARNING: Timed out waiting for font files (5s). \
                             Proceeding with available fonts."
                        );
                    }
                    break;
                }
                completed = match self.progress.wait_timeout(completed, remaining) {
                    Ok((c, _)) => c,
                    Err(_) => break,
                };
            }
        }

        // 8. Resolve chains from the now-populated registry
        let r = self.resolve_chains(family_stacks);
        r
    }

    // ── Delegated accessors ─────────────────────────────────────────────────

    /// Get font metadata by ID.
    pub fn get_metadata_by_id(&self, id: &FontId) -> Option<FcPattern> {
        self.cache.get_metadata_by_id(id)
    }

    /// Get font bytes for a given font ID — disk-backed fonts come.
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
        self.cache
            .resolve_font_chain(font_families, weight, italic, oblique, &mut trace)
    }

    /// On-demand font-chain resolution for layout.
    #[cfg(feature = "std")]
    pub fn request_and_resolve_with_scripts(
        &self,
        font_families: &[String],
        weight: FcWeight,
        italic: PatternMatch,
        oblique: PatternMatch,
        scripts_hint: Option<&[UnicodeRange]>,
    ) -> FontFallbackChain {
        // Trigger parse + wait for these families.
        let _ = self.request_fonts(std::slice::from_ref(&font_families.to_vec()));
        // Resolve using the newly updated cache.
        let mut trace = Vec::new();
        self.cache.resolve_font_chain_with_scripts(
            font_families,
            weight,
            italic,
            oblique,
            scripts_hint,
            &mut trace,
        )
    }

    /// Get a shared handle on the cache.
    pub fn shared_cache(&self) -> FcFontCache {
        self.cache.clone()
    }

    /// Block until background threads have fully populated the cache with font metadata.
    pub fn wait_for_scout(&self) {
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
                if !WAITLESS_TARGET {
                    eprintln!("[rfc-font-registry] WARNING: wait_for_scout timed out (5s).");
                }
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

    #[cfg(feature = "std")]
    #[doc(hidden)]
    pub fn chain_cache_len(&self) -> usize {
        self.cache.chain_cache_len()
    }

    /// Resolve font fallback chains by cmap-probing candidate files directly.
    #[cfg(all(feature = "std", feature = "parsing"))]
    pub fn request_fonts_fast(
        &self,
        requests: &[(Vec<String>, alloc::collections::BTreeSet<char>)],
        weight: FcWeight,
        italic: PatternMatch,
    ) -> Vec<FontFallbackChain> {
        use crate::{
            CssFallbackGroup, FcCountFontFaces, FcFontPath, FcParseFontFaceFast, FontMatch,
        };
        use std::sync::atomic::Ordering;

        let config = self.cache.fallback_config();

        // Try to resolve against known_paths immediately without waiting for scan_complete.
        let wait_start = Instant::now();
        let mut waited = false;
        let current_known_paths;
        loop {
            let Ok(paths) = self.known_paths.read() else {
                return requests
                    .iter()
                    .map(|(stack, _)| FontFallbackChain::empty(stack))
                    .collect();
            };
            // Heuristic: if any request has a match, we can make progress.
            let any_matches = requests.iter().any(|(stack, _)| {
                let expanded =
                    config.candidate_families(stack, crate::DEFAULT_UNICODE_FALLBACK_SCRIPTS);
                expanded.iter().any(|fam| {
                    let fam_norm = crate::utils::normalize_family_name(fam);
                    !crate::scoring::find_family_paths(&fam_norm, &*paths).is_empty()
                })
            });
            if any_matches
                || self.scan_complete.load(Ordering::Acquire)
                || wait_start.elapsed() >= Duration::from_millis(500)
            {
                drop(paths);
                if let Ok(p) = self.known_paths.read() {
                    current_known_paths = p;
                    break;
                } else {
                    return requests
                        .iter()
                        .map(|(stack, _)| FontFallbackChain::empty(stack))
                        .collect();
                }
            }
            drop(paths);
            waited = true;
            let Ok(completed) = self.completed_paths.lock() else {
                if let Ok(p) = self.known_paths.read() {
                    current_known_paths = p;
                    break;
                } else {
                    return Vec::new();
                }
            };
            let remaining = Duration::from_millis(500).saturating_sub(wait_start.elapsed());
            if remaining.is_zero() {
                drop(completed);
                if let Ok(p) = self.known_paths.read() {
                    current_known_paths = p;
                    break;
                } else {
                    return Vec::new();
                }
            }
            let _ = self.progress.wait_timeout(completed, remaining);
        }
        let known_paths = current_known_paths;
        let scan_wait_us = wait_start.elapsed().as_micros();
        static RFC_DBG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *RFC_DBG.get_or_init(|| std::env::var_os("RFC_REGISTRY_DEBUG").is_some()) {
            eprintln!(
                "[RFC] request_fonts_fast: scan_wait = {} µs (waited={})",
                scan_wait_us, waited,
            );
        }

        let want_bold = weight >= FcWeight::Bold;
        let want_italic = italic == PatternMatch::True;

        let mut chains = Vec::with_capacity(requests.len());

        for (stack, codepoints) in requests {
            let expanded =
                config.candidate_families(stack, crate::DEFAULT_UNICODE_FALLBACK_SCRIPTS);
            let mut css_fallbacks: Vec<CssFallbackGroup> = Vec::new();
            let mut uncovered: alloc::collections::BTreeSet<char> = codepoints.clone();

            'families: for family in &expanded {
                if uncovered.is_empty() {
                    break;
                }
                let family_norm = crate::utils::normalize_family_name(family);
                let paths = crate::scoring::find_family_paths(&family_norm, &known_paths);
                let mut group = CssFallbackGroup {
                    css_name: family.clone(),
                    fonts: Vec::new(),
                    script_fonts: Vec::new(),
                };

                for path in paths {
                    let path_str = path.to_string_lossy().to_string();

                    // Reuse cached FontId if it covers the current uncovered set.
                    if let Some(cached_ids) = self.cache.lookup_paths_cached(&path_str) {
                        let mut picked: Option<(
                            crate::FontId,
                            crate::FcPattern,
                            alloc::collections::BTreeSet<char>,
                        )> = None;
                        for id in cached_ids {
                            let Some(pattern) = self.cache.get_metadata_by_id(&id) else {
                                continue;
                            };
                            let covers: alloc::collections::BTreeSet<char> = uncovered
                                .iter()
                                .copied()
                                .filter(|ch| {
                                    let cp = *ch as u32;
                                    pattern
                                        .unicode_ranges
                                        .iter()
                                        .any(|r| cp >= r.start && cp <= r.end)
                                })
                                .collect();
                            if covers.is_empty() {
                                continue;
                            }
                            let is_bold = pattern.weight >= FcWeight::Bold;
                            let is_italic = pattern.italic == PatternMatch::True;
                            let style_dist =
                                (is_bold != want_bold) as u8 + (is_italic != want_italic) as u8;
                            let replace = match &picked {
                                None => true,
                                Some((_, pat, _)) => {
                                    let pb = pat.weight >= FcWeight::Bold;
                                    let pi = pat.italic == PatternMatch::True;
                                    let pd = (pb != want_bold) as u8 + (pi != want_italic) as u8;
                                    style_dist < pd
                                }
                            };
                            if replace {
                                picked = Some((id, pattern, covers));
                            }
                        }

                        if let Some((id, pattern, covers)) = picked {
                            for ch in &covers {
                                uncovered.remove(ch);
                            }
                            group.fonts.push(FontMatch {
                                id,
                                unicode_ranges: pattern.unicode_ranges,
                                fallbacks: Vec::new(),
                            });
                            if !group.fonts.is_empty() {
                                css_fallbacks.push(group);
                            }
                            continue 'families;
                        }
                        // Fall through and probe fresh cmaps below.
                    }

                    // Cold path: mmap + probe faces by best style match.
                    let Some(bytes) = read_or_mmap_font(&path) else {
                        continue;
                    };
                    let num_faces = FcCountFontFaces(bytes.as_slice());
                    let bytes_hash = crate::utils::content_dedup_hash_u64(bytes.as_slice());

                    // Skip style sort for single-face files.
                    let face_order: Vec<usize> = if num_faces == 1 {
                        vec![0]
                    } else {
                        collect_face_style_order(
                            bytes.as_slice(),
                            num_faces,
                            want_bold,
                            want_italic,
                        )
                    };

                    for face_index in face_order {
                        let Some(cov) =
                            FcParseFontFaceFast(bytes.as_slice(), face_index, &uncovered)
                        else {
                            continue;
                        };
                        if cov.covered.is_empty() {
                            continue;
                        }

                        let mut pat = cov.pattern.clone();
                        let family_guessed = crate::config::guess_family_from_filename(&path);
                        pat.name = Some(family.clone());
                        pat.family = Some(family_guessed);

                        let id = self.cache.insert_fast_pattern(
                            pat.clone(),
                            FcFontPath {
                                path: path_str.clone(),
                                font_index: face_index,
                                bytes_hash,
                            },
                        );
                        for ch in &cov.covered {
                            uncovered.remove(ch);
                        }
                        group.fonts.push(FontMatch {
                            id,
                            unicode_ranges: pat.unicode_ranges,
                            fallbacks: Vec::new(),
                        });
                        // CSS semantic: one face per family.
                        if !group.fonts.is_empty() {
                            css_fallbacks.push(group);
                        }
                        continue 'families;
                    }
                }

                // No file in this family covered anything new.
            }

            chains.push(FontFallbackChain {
                css_fallbacks,
                unicode_fallbacks: Vec::new(),
                last_resort: Vec::new(),
                original_stack: stack.clone(),
            });
        }

        chains
    }

    // Internal methods

    /// Insert a parsed font into the cache (called by Builder threads).
    pub fn insert_font(&self, pattern: FcPattern, path: FcFontPath) {
        self.cache.insert_builder_font(pattern, path);
    }

    /// Resolve font chains from the current state of the registry.
    fn resolve_chains(&self, family_stacks: &[Vec<String>]) -> Vec<FontFallbackChain> {
        family_stacks
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

/// Stub for `Instant` on wasm where it's unimplemented and unused.
#[cfg(target_family = "wasm")]
#[derive(Clone, Copy)]
struct Instant;
#[cfg(target_family = "wasm")]
impl Instant {
    fn now() -> Self {
        Instant
    }
    fn elapsed(&self) -> Duration {
        Duration::MAX
    }
    fn saturating_duration_since(&self, _earlier: Instant) -> Duration {
        Duration::ZERO
    }
}
#[cfg(target_family = "wasm")]
impl core::ops::Add<Duration> for Instant {
    type Output = Instant;
    fn add(self, _rhs: Duration) -> Instant {
        Instant
    }
}

/// Read or mmap font file.
#[cfg(all(feature = "std", feature = "parsing"))]
fn read_or_mmap_font(path: &std::path::Path) -> Option<std::sync::Arc<crate::FontBytes>> {
    #[cfg(all(not(target_family = "wasm"), feature = "std"))]
    {
        crate::open_font_bytes_mmap(&path.to_string_lossy())
    }
    #[cfg(target_family = "wasm")]
    {
        let bytes = std::fs::read(path).ok()?;
        Some(std::sync::Arc::new(crate::FontBytes::Owned(
            std::sync::Arc::from(bytes.as_slice()),
        )))
    }
}

/// For a multi-face TTC, return face indices ordered by best style match.
#[cfg(all(feature = "std", feature = "parsing"))]
fn collect_face_style_order(
    bytes: &[u8],
    num_faces: usize,
    want_bold: bool,
    want_italic: bool,
) -> Vec<usize> {
    use allsorts::{
        binary::read::ReadScope,
        font_data::FontData,
        tables::{FontTableProvider, HeadTable},
        tag,
    };

    let scope = ReadScope::new(bytes);
    let Ok(font_file) = scope.read::<FontData<'_>>() else {
        return (0..num_faces).collect();
    };

    let mut styles: Vec<(usize, bool, bool)> = Vec::with_capacity(num_faces);
    for fi in 0..num_faces {
        let Ok(provider) = font_file.table_provider(fi) else {
            continue;
        };
        let Ok(Some(head_data)) = provider.table_data(tag::HEAD) else {
            continue;
        };
        let Ok(head) = ReadScope::new(&head_data).read::<HeadTable>() else {
            continue;
        };
        styles.push((fi, head.is_bold(), head.is_italic()));
    }
    if styles.is_empty() {
        return (0..num_faces).collect();
    }
    styles.sort_by_key(|(_, is_bold, is_italic)| {
        let bold_mismatch = (*is_bold != want_bold) as u8;
        let italic_mismatch = (*is_italic != want_italic) as u8;
        (bold_mismatch, italic_mismatch)
    });
    styles.into_iter().map(|(fi, _, _)| fi).collect()
}
