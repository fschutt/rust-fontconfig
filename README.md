# rust-fontconfig

Pure-Rust rewrite of the Linux fontconfig library (no system dependencies) - using allsorts as a font parser to support `.woff`, `.woff2`, `.ttc`, `.otf` and `.ttf`

**NOTE**: Also works on Windows, macOS and WASM - without external dependencies!

## Motivation

There are a number of reasons why I want to have a pure-Rust version of fontconfig:

- fontconfig with all dependencies (expat and freetype) is ~190.000 lines of C (extremely bloated for what it does)
- fontconfig, freetype, expat and basically any kind of parsing in C is a common attack vector (via maliciously crafted fonts). The Rust version (allsorts) checks the boundaries before accessing memory, so attacks via font files should be less common.
- it gets rid of the cmake / cc dependencies necessary to build [azul](https://azul.rs) on Linux
- fontconfig isn't really a "hard" library to rewrite, it just parses fonts and selects fonts by name
- Rust has existing xml parsers and font parsers, just use those
- It allows fontconfig libraries to be purely statically linked
- Font parsing / loading can be easily multithreaded (parsing font files in parallel)
- It reduces the number of necessary non-Rust dependencies on Linux for azul to 0
- fontconfig (or at least the Rust bindings) do not allow you to store an in-memory cache, only an on-disk cache, requiring disk access on every query (= slow)
- `no_std` support ("bring your own font files") for WASM
 
Now for the more practical reasons:

- libfontconfig 0.12.x sometimes hangs and crashes ([see issue](https://github.com/maps4print/azul/issues/110))
- libfontconfig introduces build issues with cmake / cc ([see issue](https://github.com/maps4print/azul/issues/206))
- To support font fallback in CSS selectors and text runs based on Unicode ranges, you have to do several calls into C, since fontconfig doesn't handle that
- The rust rewrite uses multithreading and memory mapping, since that is faster than reading each file individually
- The rust rewrite only parses the font tables necessary to select the name, not the entire font
- The rust rewrite uses very few allocations (some are necessary because of UTF-16 / UTF-8 conversions and multithreading lifetime issues)

## Usage

### Basic Font Query

```rust
use rust_fontconfig::{FcFontCache, FcPattern};

fn main() {
    // Build the font cache
    let cache = FcFontCache::build();
    
    // Query a font by name
    let results = cache.query(
        &FcPattern {
            name: Some(String::from("Arial")),
            ..Default::default()
        },
        &mut Vec::new() // Trace messages container
    );
    
    if let Some(font_match) = results {
        println!("Font match ID: {:?}", font_match.id);
        println!("Font unicode ranges: {:?}", font_match.unicode_ranges);
        println!("Font fallbacks: {:?}", font_match.fallbacks.len());
    } else {
        println!("No matching font found");
    }
}
```

### Find All Monospace Fonts

```rust
use rust_fontconfig::{FcFontCache, FcPattern, PatternMatch};

fn main() {
    let cache = FcFontCache::build();
    let fonts = cache.query_all(
        &FcPattern {
            monospace: PatternMatch::True,
            ..Default::default()
        },
        &mut Vec::new()
    );

    println!("Found {} monospace fonts:", fonts.len());
    for font in fonts {
        println!("Font ID: {:?}", font.id);
    }
}
```

### Font Matching for Multilingual Text

```rust
use rust_fontconfig::{FcFontCache, FcPattern};

fn main() {
    let cache = FcFontCache::build();
    let text = "Hello 你好 Здравствуйте";
    
    // Find fonts that can render the mixed-script text
    let mut trace = Vec::new();
    let matched_fonts = cache.query_for_text(
        &FcPattern::default(),
        text,
        &mut trace
    );
    
    println!("Found {} fonts for the multilingual text", matched_fonts.len());
    for font in matched_fonts {
        println!("Font ID: {:?}", font.id);
    }
}
```

## Using from C

### Linking with the C API

The rust-fontconfig library provides C-compatible bindings that can be used from C/C++ applications.

#### Binary Downloads

You can download pre-built binary files from the [latest GitHub release](https://github.com/maps4print/rust-fontconfig/releases/latest):
- Windows: `rust_fontconfig.dll` and `rust_fontconfig.lib`
- macOS: `librust_fontconfig.dylib` and `librust_fontconfig.a`
- Linux: `librust_fontconfig.so` and `librust_fontconfig.a`

#### Building from Source

Alternatively, you can build the library from source:

```bash
# Clone the repository
git clone https://github.com/maps4print/rust-fontconfig.git
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

- cache building: ~90ms for ~530 fonts
- cache query: ~4µs

## Features

- Font matching by name, family, style properties, or Unicode ranges
- Support for font weights (thin, light, normal, bold, etc.)
- Support for font stretches (condensed, normal, expanded, etc.)
- Multilingual text support with automatic font fallback
- In-memory font loading and caching
- Optional `no_std` support
- C API for integration with non-Rust languages

## License

MIT