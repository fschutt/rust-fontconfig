//! Example demonstrating unicode-aware font resolution
//! 
//! This shows how to resolve font chains and then query them for specific text.

use rust_fontconfig::{FcFontCache, FcWeight, PatternMatch};

fn main() {
    // Initialize font cache
    let cache = FcFontCache::build();
    
    println!("=== Unicode-Aware Font Selection ===\n");
    
    // Step 1: Create a font chain for sans-serif fonts
    println!("Step 1: Resolve font chain for 'sans-serif'");
    let mut trace = Vec::new();
    let chain = cache.resolve_font_chain(
        &vec!["sans-serif".to_string()],
        FcWeight::Normal,
        PatternMatch::False,  // italic
        PatternMatch::False,  // oblique
        &mut trace,
    );
    
    println!("  Font chain has {} CSS fallbacks and {} unicode fallbacks\n", 
             chain.css_fallbacks.len(),
             chain.unicode_fallbacks.len());
    
    // Step 2: Resolve different texts against the chain
    println!("Step 2: Resolve various texts against the font chain\n");
    
    // Latin text
    let latin_text = "Hello World";
    println!("Latin text: '{}'", latin_text);
    print_text_resolution(&cache, &chain, latin_text);
    
    // CJK (Chinese) text
    let cjk_text = "你好世界";
    println!("\nCJK text: '{}'", cjk_text);
    print_text_resolution(&cache, &chain, cjk_text);
    
    // Japanese text
    let japanese_text = "こんにちは世界";
    println!("\nJapanese text: '{}'", japanese_text);
    print_text_resolution(&cache, &chain, japanese_text);
    
    // Arabic text
    let arabic_text = "مرحبا بالعالم";
    println!("\nArabic text: '{}'", arabic_text);
    print_text_resolution(&cache, &chain, arabic_text);
    
    // Cyrillic text
    let cyrillic_text = "Привет мир";
    println!("\nCyrillic text: '{}'", cyrillic_text);
    print_text_resolution(&cache, &chain, cyrillic_text);
    
    // Mixed text
    let mixed_text = "Hello 世界 Привет";
    println!("\nMixed text: '{}'", mixed_text);
    print_text_resolution(&cache, &chain, mixed_text);
    
    println!("\n=== Summary ===");
    println!("The workflow is:");
    println!("1. resolve_font_chain() - creates a fallback chain from CSS font-family");
    println!("2. chain.resolve_text() - maps each character to a font in the chain");
    println!("3. Use the font IDs to load and render glyphs");
}

fn print_text_resolution(
    cache: &FcFontCache,
    chain: &rust_fontconfig::FontFallbackChain,
    text: &str,
) {
    let resolved = chain.resolve_text(cache, text);
    
    // Group consecutive characters by font
    let mut current_font: Option<String> = None;
    let mut current_segment = String::new();
    
    for (ch, font_info) in resolved {
        let font_name = font_info.map(|(id, _)| {
            cache.get_metadata_by_id(&id)
                .and_then(|p| p.name.clone().or(p.family.clone()))
                .unwrap_or_else(|| format!("{:?}", id))
        });
        
        if font_name != current_font {
            if !current_segment.is_empty() {
                println!("  '{}' -> {}", 
                         current_segment, 
                         current_font.as_deref().unwrap_or("[NO FONT]"));
                current_segment.clear();
            }
            current_font = font_name;
        }
        current_segment.push(ch);
    }
    
    if !current_segment.is_empty() {
        println!("  '{}' -> {}", 
                 current_segment, 
                 current_font.as_deref().unwrap_or("[NO FONT]"));
    }
}
