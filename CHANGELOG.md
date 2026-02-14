# Changelog

All notable changes to this project will be documented in this file.

## [2.0.0] - 2026-02-14

### Breaking Changes

- **`FontId` now uses atomic counter instead of `SystemTime`**: Font IDs are now
  assigned via a global atomic counter (`AtomicU128`), making them deterministic
  and reproducible across runs. Code that compared `FontId` values across sessions
  or relied on their magnitude encoding time will break.

### Added

- **`FcFontRegistry`**: New async font registry with background scanning and
  on-demand font loading. Requires the `async-registry` feature.
  - `FcFontRegistry::new()` — creates a new registry (returns `Arc<Self>`)
  - `register_memory_fonts()` — register in-memory fonts with priority
  - `spawn_scout_and_builders()` — start background directory scanning + font parsing
  - `request_fonts()` — request specific font families (prioritized loading)
  - `into_fc_font_cache()` — convert to `FcFontCache` for compatibility
  - `shutdown()`, `is_scan_complete()`, `is_build_complete()`, `progress()`

- **Disk cache** (`cache` feature): Serializes parsed font metadata to disk via
  `bincode`/`serde`, dramatically speeding up subsequent launches.
  - `FcFontRegistry::load_from_disk_cache()` / `save_to_disk_cache()`
  - `FontManifest`, `FontCacheEntry`, `FontIndexEntry` structs

- **`FcFontCache::build_with_families()`**: Build a cache that only scans and
  parses fonts matching specific family names, much faster than `build()` when
  you know which fonts you need.

- **`Debug` impl for `FcFontRegistry`**: Shows registry state (scan progress,
  font counts, memory fonts).

### Fixed

- **Italic font race condition**: `FcFontRegistry` now waits for all font file
  variants (regular, bold, italic, etc.) to be parsed before resolving font
  queries, preventing cases where italic variants were missing from results.

- **Font scoring**: When style is `DontCare`, prefer `Normal` over `Italic`
  variants. This fixes cases where italic fonts were incorrectly chosen as the
  default match.

- **Memory font preference**: `query()` now prefers memory fonts over disk fonts
  when both match equally, ensuring programmatically registered fonts take
  priority.

- **Test fix**: Arial Regular test pattern now explicitly sets `bold: False`
  instead of `DontCare` for correct scoring behavior.

## [1.2.2] - 2025-12-01

### Added

- `FcParseFontBytes`: Parse in-memory font data without building a full cache.

## [1.2.1] - 2025-11-26

### Fixed

- **Issue #15**: Windows font paths no longer assume C: drive. Now uses `SystemRoot`/`WINDIR` environment variable for system fonts and `USERPROFILE` for user fonts, with proper fallbacks.

- **Issue #17**: Removed duplicate `FcFontCache::build()` implementation that caused compilation errors when building without `std` or `parsing` features.

- **Issue #18**: Fixed compilation without `parsing` feature. All `allsorts` imports and dependent functions are now properly guarded with `#[cfg(feature = "parsing")]`.

## [1.2.0] - 2025-06-03

### Breaking Changes

- **`resolve_font_chain()` signature changed**: The `text` parameter has been removed. Font chains are now resolved based on CSS properties only (font-family, weight, italic, oblique), not text content.
  
  Old API:
  ```rust
  cache.resolve_font_chain(&families, text, weight, italic, oblique, &mut trace)
  ```
  
  New API:
  ```rust
  cache.resolve_font_chain(&families, weight, italic, oblique, &mut trace)
  ```

- **`query_all()` method removed**: Use `cache.list()` with filtering instead.
  
  Old API:
  ```rust
  let fonts = cache.query_all(&pattern, &mut trace);
  ```
  
  New API:
  ```rust
  let fonts: Vec<_> = cache.list().into_iter()
      .filter(|(pattern, _id)| /* your filter */)
      .collect();
  ```

- **`query_for_text()` moved to `FontFallbackChain`**: Text-to-font resolution now requires a font chain first.
  
  Old API:
  ```rust
  let fonts = cache.query_for_text(&pattern, text, &mut trace);
  ```
  
  New API:
  ```rust
  let chain = cache.resolve_font_chain(&families, weight, italic, oblique, &mut trace);
  let font_runs = chain.query_for_text(&cache, text);
  ```

### Added

- **`FontFallbackChain::resolve_text()`**: Returns per-character font assignments as `Vec<(char, Option<(FontId, String)>)>` for fine-grained control.

- **`FontFallbackChain::resolve_char()`**: Resolve a single character to its font.

- **`CssFallbackGroup` struct**: Groups fonts by their CSS source name, making it clear which CSS font-family each font came from.

- **Font chain caching**: Identical CSS font-family stacks now share cached font chains, improving performance when the same fonts are used with different text content.

### Changed

- **Architecture**: The new two-step workflow (chain resolution → text querying) better matches CSS/browser font handling semantics and enables better caching.

- **Performance**: Font chains are now cached by CSS properties, avoiding redundant font resolution for the same font-family declarations.

### Rationale

The API was refactored to separate concerns:
1. **Font chain resolution** (`resolve_font_chain`): Determines which fonts to use based on CSS font-family, weight, and style. This is typically done once per CSS declaration.
2. **Text-to-font mapping** (`resolve_text`/`query_for_text`): Maps text content to specific fonts in the chain. This is done per text string to render.

This separation enables:
- Better caching (same CSS fonts can be reused for different text)
- Clearer API semantics matching CSS behavior
- More efficient text layout pipelines

## [1.1.0] - 2025-11-25

### Added

- Better font resolution algorithms
- Performance improvements for font matching

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
