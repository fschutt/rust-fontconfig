/// Example integration for azul-layout / printpdf
/// 
/// This shows how to cache CSS font stacks and resolve them to
/// rust-fontconfig font chains efficiently, including cross-platform
/// font resolution with explicit OS targeting.

use rust_fontconfig::{FcFontCache, FcWeight, PatternMatch, FontFallbackChain, OperatingSystem};
use std::collections::HashMap;
use std::sync::Mutex;

/// Cache for CSS font family stacks -> rust-fontconfig font chains
pub struct FontChainCache {
    /// The underlying fontconfig cache
    fc_cache: FcFontCache,
    
    /// Cache mapping: (CSS font stack, weight, italic, oblique) -> FontFallbackChain
    chain_cache: Mutex<HashMap<FontStackKey, FontFallbackChain>>,
}

/// Key for caching font chains
#[derive(Clone, PartialEq, Eq, Hash)]
struct FontStackKey {
    font_families: Vec<String>,
    weight: FcWeight,
    italic: bool,
    oblique: bool,
}

impl FontChainCache {
    /// Create a new font chain cache
    pub fn new() -> Self {
        println!("Building fontconfig cache...");
        let start = std::time::Instant::now();
        let fc_cache = FcFontCache::build();
        println!("✓ Fontconfig cache built in {:?}", start.elapsed());
        
        Self {
            fc_cache,
            chain_cache: Mutex::new(HashMap::new()),
        }
    }
    
    /// Resolve a CSS font-family stack to a font fallback chain
    /// This is cached, so subsequent calls with the same parameters are fast
    /// Uses the current OS for generic family expansion
    pub fn resolve_font_stack(
        &self,
        font_families: &[String],
        text: &str,
        weight: FcWeight,
        italic: bool,
        oblique: bool,
    ) -> FontFallbackChain {
        self.resolve_font_stack_with_os(
            font_families,
            text,
            weight,
            italic,
            oblique,
            OperatingSystem::current(),
        )
    }
    
    /// Resolve a CSS font-family stack with explicit OS specification
    /// Useful for cross-platform document rendering or testing
    /// Example: Render a document with Windows fonts on macOS
    pub fn resolve_font_stack_with_os(
        &self,
        font_families: &[String],
        text: &str,
        weight: FcWeight,
        italic: bool,
        oblique: bool,
        os: OperatingSystem,
    ) -> FontFallbackChain {
        let key = FontStackKey {
            font_families: font_families.to_vec(),
            weight,
            italic,
            oblique,
        };
        
        // Check cache first
        {
            let cache = self.chain_cache.lock().unwrap();
            if let Some(chain) = cache.get(&key) {
                return chain.clone();
            }
        }
        
        // Not in cache - resolve it
        let chain = self.fc_cache.resolve_font_chain_with_os(
            font_families,
            text,
            weight,
            if italic { PatternMatch::True } else { PatternMatch::DontCare },
            if oblique { PatternMatch::True } else { PatternMatch::DontCare },
            &mut Vec::new(),
            os,
        );
        
        // Store in cache
        {
            let mut cache = self.chain_cache.lock().unwrap();
            cache.insert(key, chain.clone());
        }
        
        chain
    }
    
    /// Get the underlying fontconfig cache (for character resolution)
    pub fn fc_cache(&self) -> &FcFontCache {
        &self.fc_cache
    }
}

fn main() {
    // Initialize the font chain cache (do this once at startup)
    let font_cache = FontChainCache::new();
    
    println!("\n=== Usage Example for azul-layout / printpdf ===\n");
    
    // Example 1: Resolve a simple font stack
    println!("Example 1: Basic font stack");
    let font_families = vec!["Arial".to_string(), "sans-serif".to_string()];
    let text = "Hello World";
    
    let start = std::time::Instant::now();
    let chain = font_cache.resolve_font_stack(
        &font_families,
        text,
        FcWeight::Normal,
        false,
        false,
    );
    println!("First call: {:?}", start.elapsed());
    
    // Second call should be instant (cached)
    let start = std::time::Instant::now();
    let _chain2 = font_cache.resolve_font_stack(
        &font_families,
        text,
        FcWeight::Normal,
        false,
        false,
    );
    println!("Second call (cached): {:?}", start.elapsed());
    println!("CSS fallbacks: {}", chain.css_fallbacks.len());
    println!("Unicode fallbacks: {}", chain.unicode_fallbacks.len());
    println!();
    
    // Example 2: Resolve characters
    println!("Example 2: Character-by-character resolution");
    let multilingual_text = "Hello 你好";
    
    // Resolve the chain for this text
    let chain = font_cache.resolve_font_stack(
        &font_families,
        multilingual_text,
        FcWeight::Normal,
        false,
        false,
    );
    
    // Now resolve each character
    println!("Text: \"{}\"", multilingual_text);
    for ch in multilingual_text.chars() {
        if let Some((font_id, css_source)) = chain.resolve_char(font_cache.fc_cache(), ch) {
            if let Some(pattern) = font_cache.fc_cache().get_metadata_by_id(&font_id) {
                let font_name = pattern.name.as_ref()
                    .or(pattern.family.as_ref())
                    .map(|s| s.as_str())
                    .unwrap_or("<unknown>");
                println!("  '{}' -> {} (from CSS: '{}')", ch, font_name, css_source);
            }
        } else {
            println!("  '{}' -> [NO FONT]", ch);
        }
    }
    println!();
    
    // Example 3: Different weights
    println!("Example 3: Bold text");
    let bold_chain = font_cache.resolve_font_stack(
        &font_families,
        "Bold Text",
        FcWeight::Bold,
        false,
        false,
    );
    println!("Bold chain resolved with {} CSS fallbacks", bold_chain.css_fallbacks.len());
    
    println!();
    
    // Example 4: Cross-platform font resolution
    println!("Example 4: Cross-platform font resolution");
    println!("Resolve 'sans-serif' for different operating systems:");
    
    let generic_families = vec!["sans-serif".to_string()];
    
    // macOS
    let macos_chain = font_cache.resolve_font_stack_with_os(
        &generic_families,
        "Hello",
        FcWeight::Normal,
        false,
        false,
        OperatingSystem::MacOS,
    );
    println!("\nmacOS sans-serif fonts:");
    for group in &macos_chain.css_fallbacks {
        for font in &group.fonts {
            if let Some(pattern) = font_cache.fc_cache().get_metadata_by_id(&font.id) {
                let name = pattern.name.as_ref().or(pattern.family.as_ref())
                    .map(|s| s.as_str()).unwrap_or("<unknown>");
                println!("  - {}", name);
            }
        }
    }
    
    // Windows
    let windows_chain = font_cache.resolve_font_stack_with_os(
        &generic_families,
        "Hello",
        FcWeight::Normal,
        false,
        false,
        OperatingSystem::Windows,
    );
    println!("\nWindows sans-serif fonts:");
    for group in &windows_chain.css_fallbacks {
        for font in &group.fonts {
            if let Some(pattern) = font_cache.fc_cache().get_metadata_by_id(&font.id) {
                let name = pattern.name.as_ref().or(pattern.family.as_ref())
                    .map(|s| s.as_str()).unwrap_or("<unknown>");
                println!("  - {}", name);
            }
        }
    }
    
    // Linux
    let linux_chain = font_cache.resolve_font_stack_with_os(
        &generic_families,
        "Hello",
        FcWeight::Normal,
        false,
        false,
        OperatingSystem::Linux,
    );
    println!("\nLinux sans-serif fonts:");
    for group in &linux_chain.css_fallbacks {
        for font in &group.fonts {
            if let Some(pattern) = font_cache.fc_cache().get_metadata_by_id(&font.id) {
                let name = pattern.name.as_ref().or(pattern.family.as_ref())
                    .map(|s| s.as_str()).unwrap_or("<unknown>");
                println!("  - {}", name);
            }
        }
    }
    
    println!();
    
    // Example 5: Rendering documents with target OS fonts
    println!("Example 5: Document rendering with target OS");
    println!("Scenario: Generating a PDF on macOS that should use Windows fonts\n");
    
    let document_fonts = vec!["Arial".to_string(), "sans-serif".to_string()];
    
    // Current OS (macOS in this example)
    let current_chain = font_cache.resolve_font_stack(
        &document_fonts,
        "Document Text",
        FcWeight::Normal,
        false,
        false,
    );
    println!("With current OS fonts:");
    if let Some(group) = current_chain.css_fallbacks.first() {
        if let Some(font) = group.fonts.first() {
            if let Some(pattern) = font_cache.fc_cache().get_metadata_by_id(&font.id) {
                let name = pattern.name.as_ref().or(pattern.family.as_ref())
                    .map(|s| s.as_str()).unwrap_or("<unknown>");
                println!("  First fallback: {}", name);
            }
        }
    }
    
    // Target OS (Windows)
    let windows_doc_chain = font_cache.resolve_font_stack_with_os(
        &document_fonts,
        "Document Text",
        FcWeight::Normal,
        false,
        false,
        OperatingSystem::Windows,
    );
    println!("\nWith Windows fonts:");
    if let Some(group) = windows_doc_chain.css_fallbacks.first() {
        if let Some(font) = group.fonts.first() {
            if let Some(pattern) = font_cache.fc_cache().get_metadata_by_id(&font.id) {
                let name = pattern.name.as_ref().or(pattern.family.as_ref())
                    .map(|s| s.as_str()).unwrap_or("<unknown>");
                println!("  First fallback: {}", name);
            }
        }
    }
    
    println!("\n=== Integration Summary ===");
    println!("1. Create FontChainCache once at startup");
    println!("2. Call resolve_font_stack() for each unique (font-family, weight, style) combination");
    println!("3. Use resolve_font_stack_with_os() for cross-platform document rendering");
    println!("4. Generic families (serif, sans-serif, monospace) are expanded based on OS");
    println!("5. Use chain.resolve_char() to find which font renders each character");
    println!("6. Results are cached automatically for performance");
}
