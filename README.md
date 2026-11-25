# rust-fontconfig

Pure-Rust font discovery and font fallback library without system dependencies. Works on Linux, macOS, Windows, and WASM.

- **Pure Rust**: Safe font parsing via `allsorts` with zero C dependencies.
- **Fast**: In-memory caching, memory-mapped I/O, and optional multithreaded font discovery via `rayon`.
- **CSS Fallback Chains**: Character-by-character resolution across CSS stacks and Unicode script blocks.

## Installation

```toml
[dependencies]
rust-fontconfig = { version = "5.2", features = ["parsing"] }
```

The default build (`std` only) discovers system fonts via filename heuristics. Enable `parsing` to inspect actual font tables with `allsorts` for accurate family names, weights, Unicode coverage, and `.woff`/`.woff2`/`.ttc`/`.otf`/`.ttf` support.

### Cargo features

- `std` (default): Filesystem scanning and memory-mapped font loading (`mmapio`).
- `parsing`: Parses font tables via allsorts (accurate metadata; WOFF/WOFF2/TTC/OTF/TTF). Implies `std`.
- `multithreading`: Parallel font scanning and parsing via `rayon`.
- `cache`: Persists the parsed font cache to disk (serde + bincode).
- `async-registry`: Background and incremental font discovery via `FcFontRegistry`. Implies `parsing`.
- `desktop-detect`: Detects desktop font preferences on Linux (XDG Portal, GNOME, KDE).
- `ffi`: C API bindings. Implies `parsing` and `async-registry`.

> **WASM:** `wasm32-*` targets build out of the box - `mmapio` and `rayon` are
> excluded automatically via `cfg`. Build with `--features parsing`.

## Usage

### Basic Font Query

```rust
use rust_fontconfig::{FcFontCache, FcPattern};

fn main() {
    // Build the font cache (scans system fonts)
    let cache = FcFontCache::build();
    
    // Query a font by name
    let mut trace = Vec::new();
    let results = cache.query(
        &FcPattern {
            name: Some(String::from("Arial")),
            ..Default::default()
        },
        &mut trace
    );
    
    if let Some(font_match) = results {
        println!("Font match ID: {:?}", font_match.id);
        println!("Font unicode ranges: {:?}", font_match.unicode_ranges);
        
        // Get font metadata
        if let Some(meta) = cache.get_metadata_by_id(&font_match.id) {
            println!("Family: {:?}", meta.family);
        }
        
        // Get font file path
        if let Some(source) = cache.get_font_by_id(&font_match.id) {
            match source {
                rust_fontconfig::OwnedFontSource::Disk(path) => {
                    println!("Path: {}", path.path);
                }
                rust_fontconfig::OwnedFontSource::Memory(font) => {
                    println!("Memory font: {}", font.id);
                }
            }
        }
    } else {
        println!("No matching font found");
    }
}
```

### Font Fallback Chain for CSS font-family

The API separates font chain resolution from text querying:

1. **`resolve_font_chain()`** - Create a fallback chain from CSS font-family (without text)
2. **`chain.resolve_text()`** - Query which fonts to use for specific text

```rust
use rust_fontconfig::{FcFontCache, FcWeight, PatternMatch};

fn main() {
    let cache = FcFontCache::build();
    
    // Step 1: Build font fallback chain (without text parameter)
    let mut trace = Vec::new();
    let font_chain = cache.resolve_font_chain(
        &["Arial".to_string(), "sans-serif".to_string()],
        FcWeight::Normal,
        PatternMatch::DontCare,  // italic
        PatternMatch::DontCare,  // oblique
        &mut trace,
    );
    
    println!("CSS fallback groups: {}", font_chain.css_fallbacks.len());
    for group in &font_chain.css_fallbacks {
        println!("  CSS '{}' resolved to {} fonts", group.css_name, group.fonts.len());
    }
    
    // Step 2: Query which fonts to use for specific text
    let text = "Hello 你好 Здравствуйте";
    let font_runs = font_chain.query_for_text(&cache, text);
    
    println!("\nText '{}' split into {} font runs:", text, font_runs.len());
    for run in &font_runs {
        println!("  '{}' -> font {:?}", run.text, run.font_id);
    }
}
```

### Controlling fallback

`FcFallbackConfig` controls fallback behavior: CSS generic mappings, per-script preferences, missing family substitutions, and last-resort fonts. `FcFontCache::build()` automatically uses platform defaults (and Linux `fonts.conf`), while `FcFontCache::default()` starts empty.

```rust
use rust_fontconfig::{
    FcFallbackConfig, FcFontCache, FcWeight, GenericFamily,
    OperatingSystem, PatternMatch, UnicodeRange,
};

let mut config = FcFallbackConfig::os_defaults(OperatingSystem::current());

// 1. Substitute a missing named font
config.substitutions.insert("Helvetica".to_string(), vec!["Arial".to_string()]);

// 2. Prepend a preferred font to a CSS generic
config.prefer(GenericFamily::SansSerif, "Open Sans");

// 3. Set a catch-all last resort font
config.last_resort = vec!["Noto Sans".to_string()];

let cache = FcFontCache::build().with_fallback_config(config);

// `scripts_hint` limits which Unicode blocks are precomputed in the chain.
// `None` builds the default 7 fallback scripts; `Some(&[])` builds none.
let chain = cache.resolve_font_chain_with_scripts(
    &["Helvetica".to_string(), "sans-serif".to_string()],
    FcWeight::Normal,
    PatternMatch::DontCare,
    PatternMatch::DontCare,
    Some(&[UnicodeRange { start: 0x3040, end: 0x309F }]),
    &mut Vec::new(),
);
```

Font fallback resolves in three tiers (first hit wins):
1. **CSS stack**: Generics and their per-script preferred fonts.
2. **Script block**: Configured script preferences, then installed fonts ranked by coverage of that specific block, style match, and dedication (narrow coverage beats pan-Unicode fonts).
3. **Last resort**: The configured fallback for uncovered characters.

Text resolution (`resolve_char`, `query_for_text`) reads only the precomputed chain, requiring no locks or cache access per character.

For the async registry, use `FcFontRegistry::new_with_configs(scan, fallback)`.

### Desktop Font Detection

Enable the `desktop-detect` feature to read the desktop's configured UI font and prepend it to generic fallbacks:

```rust
use rust_fontconfig::{FcDesktopFonts, GenericFamily};

if let Some(ui) = FcDesktopFonts::detect().ui {
    cache.modify_fallback_config(|c| {
        c.prefer_for(&[GenericFamily::SystemUi, GenericFamily::UiSansSerif], ui.family);
    });
}
```

### Character-by-Character Font Resolution

For fine-grained control, use `resolve_text()` to get per-character font assignments:

```rust
use rust_fontconfig::{FcFontCache, FcWeight, PatternMatch};

fn main() {
    let cache = FcFontCache::build();
    
    let chain = cache.resolve_font_chain(
        &["sans-serif".to_string()],
        FcWeight::Normal,
        PatternMatch::False,
        PatternMatch::False,
        &mut Vec::new(),
    );
    
    // Get font assignment for each character
    let text = "Hello 世界";
    let resolved = chain.resolve_text(&cache, text);
    
    for (ch, font_info) in resolved {
        match font_info {
            Some((font_id, css_source)) => {
                let font_name = cache.get_metadata_by_id(&font_id)
                    .and_then(|m| m.name.clone().or(m.family.clone()))
                    .unwrap_or_default();
                println!("'{}' -> {} (from CSS '{}')", ch, font_name, css_source);
            }
            None => println!("'{}' -> NO FONT FOUND", ch),
        }
    }
}
```

### List All Fonts Matching a Pattern

```rust
use rust_fontconfig::{FcFontCache, FcWeight};

fn main() {
    let cache = FcFontCache::build();
    
    // List all fonts - filter by properties
    let bold_fonts: Vec<_> = cache.list().into_iter()
        .filter(|(pattern, _id)| {
            matches!(pattern.weight, FcWeight::Bold | FcWeight::ExtraBold)
        })
        .collect();

    println!("Found {} bold fonts:", bold_fonts.len());
    for (pattern, id) in bold_fonts.iter().take(5) {
        println!("  {:?}: {:?}", id, pattern.name.as_ref().or(pattern.family.as_ref()));
    }
}
```

### Variable fonts

With the `parsing` feature enabled, `FcFontCache::variation` reads a face's variation axes, and `variable_instance` generates a static synthetic face at specific coordinates:

```rust
// Inspect axes (None for static fonts)
let axes = cache.variation(&font_id)?;

// Generate a static synthetic font at wght=650
let semibold_id = cache.variable_instance(&font_id, &[(*b"wght", 650.0)])?;
let bytes = cache.get_font_bytes(&semibold_id)?;
```

Standalone functions (`FcReadVariation` and `FcCreateVariableInstance`) are also available for raw font bytes without an `FcFontCache`.

## Using from C

### Linking with the C API

The rust-fontconfig library provides C-compatible bindings that can be used from C/C++ applications.

#### Binary Downloads

You can download pre-built binary files from the [latest GitHub release](https://github.com/fschutt/rust-fontconfig/releases/latest):
- Windows: `rust_fontconfig.dll` and `rust_fontconfig.lib`
- macOS: `librust_fontconfig.dylib` and `librust_fontconfig.a`
- Linux: `librust_fontconfig.so` and `librust_fontconfig.a`

#### Building from Source

Alternatively, you can build the library from source:

```bash
# Clone the repository
git clone https://github.com/fschutt/rust-fontconfig.git
cd rust-fontconfig

# Build with FFI support
cargo build --release --features ffi

# The generated libraries will be in target/release
```

#### Including in Your C Project

1. Copy the header file from `ffi/rust_fontconfig.h` to your include directory
2. Link against the static or dynamic library
3. Include the header file in your C code:

```c
#include "rust_fontconfig.h"
```

### Minimal C Example

```c
#include <stdio.h>
#include "rust_fontconfig.h"

int main() {
    // Build the font cache
    FcFontCache cache = fc_cache_build();
    if (!cache) {
        fprintf(stderr, "Failed to build font cache\n");
        return 1;
    }
    
    // Create a pattern to search for Arial
    FcPattern* pattern = fc_pattern_new();
    fc_pattern_set_name(pattern, "Arial");
    
    // Search for the font
    FcTraceMsg* trace = NULL;
    size_t trace_count = 0;
    FcFontMatch* match = fc_cache_query(cache, pattern, &trace, &trace_count);
    
    if (match) {
        char id_str[40];
        fc_font_id_to_string(&match->id, id_str, sizeof(id_str));
        printf("Found font! ID: %s\n", id_str);
        
        // Get the font path
        FcFontPath* font_path = fc_cache_get_font_path(cache, &match->id);
        if (font_path) {
            printf("Font path: %s (index: %zu)\n", font_path->path, font_path->font_index);
            fc_font_path_free(font_path);
        }
        
        fc_font_match_free(match);
    } else {
        printf("Font not found\n");
    }
    
    // Clean up
    fc_pattern_free(pattern);
    if (trace) fc_trace_free(trace, trace_count);
    fc_cache_free(cache);
    
    return 0;
}
```

For a more comprehensive example, see the [example.c](ffi/example.c) file included in the repository.

#### Compiling the C Example

On Linux:
```bash
gcc -I./include -L. -o font_example example.c -lrust_fontconfig
```

On macOS:
```bash
clang -I./include -L. -o font_example example.c -lrust_fontconfig
```

On Windows:
```bash
cl.exe /I./include /Fe:font_example.exe example.c rust_fontconfig.lib
```

## Performance

- cache building (cold start, multithreaded): ~1.2s for ~760 fonts
- cache building (from disk cache): ~12ms
- cache query: ~4µs

## Features

- **Font matching** by name, family, style properties, or Unicode ranges
- **CSS font-family resolution** with `resolve_font_chain()` for proper fallback handling
- **Per-character font resolution** with `chain.resolve_text()` for multilingual text
- **Font run grouping** with `chain.query_for_text()` for text shaping pipelines
- Support for font weights (thin, light, normal, bold, etc.)
- Support for font stretches (condensed, normal, expanded, etc.)
- In-memory font loading and caching
- Variable font support (axes inspection and synthetic static instances)
- WASM support (`wasm32-*` targets; `mmapio`/`rayon` auto-excluded via `cfg`)
- C API for integration with non-Rust languages

## License

MIT
