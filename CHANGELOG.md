# Changelog

All notable changes to this project will be documented in this file.

## [5.0.0] - 2026-09-03

### Breaking
- `FontFallbackChain::unicode_fallbacks` is `Vec<ScriptFallbackGroup>`; new `last_resort`; `CssFallbackGroup::script_fonts`.
- `resolve_char` returns `None` when nothing covers the character; set `FcFallbackConfig::last_resort` for a `.notdef` font.
- `query` ranks style before coverage and never prefers wider fonts.
- `FcPattern::unicode_ranges` is exact cmap coverage; disk manifest v3 (a v2 manifest is rescanned).
- `list()` is registration order; identical patterns from different files are separate records.
- `PatternMatch` discriminants pinned to the C header (`True=0`, `False=1`, `DontCare=2`).
- `scout_thread`/`builder_thread` are `pub(crate)`.

### Added
- `FcFallbackConfig`: generic families, substitutions, per-script preferences, last resort, default generic. `os_defaults`, `empty`, `merge_defaults`, `absorb_system_aliases`, `candidate_families`.
- `GenericFamily` (13 CSS generics: `from_css`, `as_css`, `parent`); `FcScriptFallback`.
- `FcFontCache::{fallback_config, set_fallback_config, with_fallback_config}`; `FcFontRegistry::new_with_configs`.
- `FcSystemConfig::{parse_tree, from_system}`: fonts.conf tree, on every platform.
- `FontFallbackChain::{empty, resolve_codepoint, fonts}`; `fallback::RankKey`.
- `FcFontRegistry::set_persist_on_complete`.
- `utils::collect_font_files`.
- CI row `cache,async-registry,parsing`; tag-triggered release workflow.

### Changed
- Fallback chains are per script (#26): CSS tier (generics carry per-script preferred fonts) → per-block script tier → last resort. Ranked by coverage of the block, style, dedication, narrowness — never breadth.
- `resolve_char`/`query_for_text` read only the chain: no lock, no clone per character.
- Coverage comes from cmap segments (formats 4/12 exact); OS/2 `ulUnicodeRange` is not consulted; block probes removed.
- Registry prefetch is `candidate_families(stack, scripts)` — what the chain can contain.
- `FcFontRegistry::new()` reads fonts.conf dirs and aliases.
- Both scanners use one cycle-safe, extension-filtered walk.
- One record per font: `metadata` is the store; dedup on insert; `by_path` index.

### Fixed
- Relative fonts.conf `<include>` resolved against the CWD; now `$FONTCONFIG_PATH`, then the config dir. Deterministic include order, cycle guard.
- `build_complete` could flip while parses were in flight (`in_flight` counter).
- Lost condvar wake-ups on `scan_complete`/`build_complete` (published under `completed_paths`).
- Lazy-mode builder threads leaked; threads hold a `Weak` now.
- Fonts loaded from the manifest were missing from the family index.
- Duplicate registrations overwrote and orphaned ids.
- `FcFontRenderConfig` `Eq`/`Ord` consistency (clippy `derive_ord_xor_partial_ord`).
- OS/2 bit table was misaligned from bit 12 (table removed).
- Panics no longer unwind into C: `catch_unwind` in every export; `clippy --all-features` passes.
- Disk-cache tests are compiled out under `single-thread-unsafe-locks`; autosave test is Unix-only.
- `has_*_ranges` test overlap with the block.

### Removed
- Token-fuzzy path (`fuzzy_query_by_name`, token index), `find_unicode_fallbacks`, `calculate_font_similarity_score`, `query_internal*`, `system_aliases` state, OS/2 range table, cmap block probes, web-lift last-resort branches, the pattern-keyed map.
- Workflows `c-bindings.yml` and `rust.yml` (duplicates of `ci.yml`).

### Deprecated
- `OperatingSystem::{get_serif_fonts, get_sans_serif_fonts, get_monospace_fonts, expand_generic_family}`, `expand_font_families`, `FcFontCache::{expand_font_families_config_first, system_alias_prefs, resolve_font_chain_with_os}` — thin wrappers over `FcFallbackConfig::os_defaults`.

### Upgrade notes
- Inject the tables: `FcFontRegistry::new_with_configs(FcScanConfig::os_defaults(os), FcFallbackConfig::os_defaults(os))` or `cache.set_fallback_config(..)`.
- A font for uncovered characters: `FcFallbackConfig::last_resort`.
- First run after upgrading rescans (manifest v3).

### Tests
- `tests/issue_26_unicode_fallback_ranking.rs`, `tests/registry_completion.rs`, fonts.conf tree, coverage exactness, walk cycle, header parity, one-record-per-font.

## [4.6.0] - 2026-08-29

### Added
- `FcScanConfig { font_dirs, priority_families }` and `FcFontRegistry::new_with_config` to inject scan targets instead of guessing them.
- `FcScanConfig::os_defaults`, `FcScanConfig::empty`, `FcScanConfig::priority_token_sets`, and `FcFontRegistry::scan_dirs()`.

## [4.5.0] - 2026-08-06

### Performance
- Indexed family lookups (`family_index: BTreeMap<String, Vec<FontId>>`), turning O(fonts) lookups into O(1).

## [4.4.11] - 2026-08-04

- Parse fonts.conf `<alias><prefer>`; generic families resolve config-first (`expand_font_families_config_first`, `system_alias_prefs`).

## [4.4.10] - 2026-07-29

- `single-thread-unsafe-locks`: `spawn_scout_and_builders` is a no-op; spawning threads under the feature is UB.

## [4.4.9] - 2026-07-29

### Fixed
- Fonts without an OS/2 table are no longer rejected during parsing (OS/2 is optional in TrueType).

## [4.4.8] - 2026-07-28

### Fixed
- Font coverage is now cmap-authoritative rather than capped by OS/2 claims, fixing missing fonts.

### Added
- `FcFontCache::query_with_fallback`: A total resolve mirroring the `fc-match` contract.
- `FcFontCache::normalize_unicode_ranges`: Coalesce ranges into a sorted, disjoint set.

### Performance
- Fast-path cmap block probing stops as soon as a block's verdict is decided.

## [4.4.7] - 2026-07-23

- Concrete family names resolve by normalized exact family match; no more substring leaks ("Noto Sans" → "Noto Sans JP").

## [4.4.6] - 2026-07-17

- Registry waits are wasm-safe (`Instant::now` aborts on browser wasm; deadlines are born expired there).

## [4.4.5] - 2026-07-14

- Depend on `allsorts-azul` 0.17 so consumers link one copy.

## [4.4.4] - 2026-06-11

- In-memory fonts registered with empty `unicode_ranges` get coverage from their cmap and resolve on caches without system fonts.

## [4.4.3] - 2026-06-06

- ASCII-lowercase font-name matching.
- `single-thread-unsafe-locks` feature: `StLock` no-atomic lock bypass for single-threaded (azul web) builds.

## [4.4.2] - 2026-05-30

- iOS: enumerate fonts via `CTFontCollection` (iOS 7+), not the macOS-only API.

## [4.4.1] - 2026-05-26

- No rayon on wasm (target-gated off).
- README and crate docs updated for the 4.x API.

## [4.4.0] - 2026-05-23

### Changed
- Bumped `allsorts-azul` to 0.16.4.

### Fixed
- Fixed WASM builds by properly gating `mmapio` usage.
- Realigned C bindings (`ffi`) to the v4.x shared-cache API.
- Fixed dead-code warnings.
- Updated macOS/iOS `test_operating_system_font_expansion` assertions.

## [4.3.0] - 2026-05-23

- iOS and Android system-font discovery.

## [4.2.1] - 2026-04-18

### Fixed
- Fixed `build_queue` leak in `FcFontRegistry::request_fonts` after `build_complete`.

### Added
- Optional fine-grained heap probes inside `request_fonts` via `AZ_PROFILE` and `AZ_PROFILE_OUT`.
- `FcFontCache::chain_cache_len()`.

## [4.2.0] - 2026-04-16

- cmap-probe fast path (`request_fonts_fast`, `FcParseFontFaceFast`).
- Scout publishes per directory.

## [4.1.0] - 2026-04-16

- `FcFontCache` is shared state behind `Arc`; `shared_cache()` replaces `into_fc_font_cache`.
- Lazy-scout builder fix.
- `std` is always on.

## [4.0.0] - 2026-04-15

- Breaking: `allsorts-azul` dependency.
- Scripts-hint chain: `resolve_font_chain_with_scripts`, `DEFAULT_UNICODE_FALLBACK_SCRIPTS`.
- `FcFontRegistry::wait_for_scout`.

## [3.3.0] - 2026-04-14

### Added
- `FcFontCache::get_font_bytes_arc`: Returns font bytes as a deduplicated shared `Arc<[u8]>`.
- `FcFontPath::bytes_hash`: 64-bit content hash of file byte contents for deduplication.
- `DEFAULT_UNICODE_FALLBACK_SCRIPTS`: The 7 script blocks pulled in by default.
- `FcFontCache::resolve_font_chain_with_scripts`: Allows overriding or omitting the default Unicode fallbacks via `scripts_hint`.
- `utils::content_hash_u64`: Stable 64-bit byte hash.

### Changed
- Bumped `FontManifest::CURRENT_VERSION` to `2` to persist `bytes_hash`.
- Added `bytes_hash` field to `FontCacheEntry`.

## [3.2.1] - 2026-04-11

- `process_path` gated to `std`+`parsing` (used cross-platform).

## [3.2.0] - 2026-04-11

- `parsing`, `multithreading`, `cache` are opt-in features, not defaults.

## [3.1.0] - 2026-04-07

- Populate `unicode_fallbacks` for CJK, Arabic, Cyrillic, Devanagari.
- `parsing` implies `std`; `xmlparser` Linux-only; CI fixes for MSVC/Windows.

## [3.0.0] - 2026-04-02

- Per-font DPI/rendering config from fonts.conf (#16).
- FFI: `#[repr(C)]` on `UnicodeRange`; Vec capacity UB fixed.
- Dedup of lib.rs/ffi.rs/multithread.rs (-1057 lines); `scoring.rs`, `utils.rs` extracted.
- Single `progress` condvar replaces the FontRequest system.
- C registry example built and run in CI on all platforms.

## [2.0.0] - 2026-02-14

### Breaking Changes
- **`FontId`**: Now uses `AtomicU128` instead of `SystemTime` for deterministic IDs.

### Added
- **`FcFontRegistry`**: Async font registry (`async-registry` feature) with `request_fonts`, `spawn_scout_and_builders`, `progress`.
- **Disk cache**: Bincode-based disk cache (`cache` feature).
- **`FcFontCache::build_with_families`**: Selective font scanning.
- **`Debug` impl for `FcFontRegistry`**.

### Fixed
- Fixed italic font race condition in `FcFontRegistry`.
- Prefer `Normal` over `Italic` variants when style is `DontCare`.
- `query()` prefers memory fonts over disk fonts on equal match.
- Fixed Arial Regular test pattern expectations.

## [1.2.2] - 2025-12-01

### Added

- `FcParseFontBytes`: Parse in-memory font data without building a full cache.

## [1.2.1] - 2025-11-26

### Fixed
- **Issue #15**: Windows font paths use `SystemRoot`/`WINDIR` instead of assuming `C:`.
- **Issue #17**: Removed duplicate `FcFontCache::build()` implementation.
- **Issue #18**: Fixed compilation when the `parsing` feature is disabled.

## [1.2.0] - 2025-06-03

### Breaking Changes
- **`resolve_font_chain()`**: Signature changed, `text` parameter removed.
- **`query_all()` removed**: Replaced with `cache.list().into_iter().filter(...)`.
- **`query_for_text()` moved**: Now belongs to `FontFallbackChain` instead of `FcFontCache`.

### Added
- **`FontFallbackChain::resolve_text()`** and **`FontFallbackChain::resolve_char()`**.
- **`CssFallbackGroup` struct**: Groups fonts by their CSS source name.
- **Font chain caching**: Caches chains based on identical CSS font-family stacks.

### Changed
- Refactored API to separate font chain resolution from text-to-font mapping (for better caching and semantic parity with CSS).

## [1.1.0] - 2025-11-25

### Added

- Better font resolution algorithms
- Performance improvements for font matching

## [1.0.4] - 2025-11-24

- Update to regular allsorts.

## [1.0.3] - Previous

### Added

- Derive `Hash` on public types

## [1.0.2] - Previous

- Bug fixes and improvements

## [1.0.1] - Previous

- Bug fixes

## [1.0.0] - Previous

- Initial stable release
- Font matching by name, family, and style properties
- Unicode range support
- In-memory font loading
- C API bindings
- Cross-platform support (Windows, macOS, Linux, WASM)
