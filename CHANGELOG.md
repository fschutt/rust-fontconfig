# Changelog

All notable changes to this project will be documented in this file.

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
