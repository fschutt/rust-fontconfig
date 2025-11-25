use rust_fontconfig::{FcFontCache, FcWeight, PatternMatch, OperatingSystem};

fn main() {
    // Initialize font cache
    let cache = FcFontCache::build();
    let os = OperatingSystem::current();
    
    println!("=== Unicode-Aware Font Selection ===\n");
    
    // Example 1: Latin text with sans-serif
    println!("Example 1: Latin text with 'sans-serif'");
    let latin_text = "Hello World";
    let chain = cache.resolve_font_chain(
        &vec!["sans-serif".to_string()],
        latin_text,
        FcWeight::Normal,
        PatternMatch::False,
        PatternMatch::False,
        &mut Vec::new(),
    );
    
    println!("  Text: '{}'", latin_text);
    print!("  Fonts: ");
    for (i, group) in chain.css_fallbacks.iter().enumerate() {
        if i > 0 { print!(", "); }
        if let Some(font_match) = group.fonts.first() {
            print!("{}", font_match.id);
        }
    }
    println!("\n");
    
    // Example 2: CJK text with sans-serif
    println!("Example 2: CJK (Chinese) text with 'sans-serif'");
    let cjk_text = "你好世界";
    let chain = cache.resolve_font_chain(
        &vec!["sans-serif".to_string()],
        cjk_text,
        FcWeight::Normal,
        PatternMatch::False,
        PatternMatch::False,
        &mut Vec::new(),
    );
    
    println!("  Text: '{}'", cjk_text);
    print!("  Fonts: ");
    for (i, group) in chain.css_fallbacks.iter().enumerate() {
        if i > 0 { print!(", "); }
        if let Some(font_match) = group.fonts.first() {
            print!("{}", font_match.id);
        }
    }
    println!("\n");
    
    // Example 3: Japanese text with sans-serif
    println!("Example 3: Japanese text with 'sans-serif'");
    let japanese_text = "こんにちは世界";
    let chain = cache.resolve_font_chain(
        &vec!["sans-serif".to_string()],
        japanese_text,
        FcWeight::Normal,
        PatternMatch::False,
        PatternMatch::False,
        &mut Vec::new(),
    );
    
    println!("  Text: '{}'", japanese_text);
    print!("  Fonts: ");
    for (i, group) in chain.css_fallbacks.iter().enumerate() {
        if i > 0 { print!(", "); }
        if let Some(font_match) = group.fonts.first() {
            print!("{}", font_match.id);
        }
    }
    println!("\n");
    
    // Example 4: Arabic text with sans-serif
    println!("Example 4: Arabic text with 'sans-serif'");
    let arabic_text = "مرحبا بالعالم";
    let chain = cache.resolve_font_chain(
        &vec!["sans-serif".to_string()],
        arabic_text,
        FcWeight::Normal,
        PatternMatch::False,
        PatternMatch::False,
        &mut Vec::new(),
    );
    
    println!("  Text: '{}'", arabic_text);
    print!("  Fonts: ");
    for (i, group) in chain.css_fallbacks.iter().enumerate() {
        if i > 0 { print!(", "); }
        if let Some(font_match) = group.fonts.first() {
            print!("{}", font_match.id);
        }
    }
    println!("\n");
    
    // Example 5: Cyrillic text with sans-serif
    println!("Example 5: Cyrillic text with 'sans-serif'");
    let cyrillic_text = "Привет мир";
    let chain = cache.resolve_font_chain(
        &vec!["sans-serif".to_string()],
        cyrillic_text,
        FcWeight::Normal,
        PatternMatch::False,
        PatternMatch::False,
        &mut Vec::new(),
    );
    
    println!("  Text: '{}'", cyrillic_text);
    print!("  Fonts: ");
    for (i, group) in chain.css_fallbacks.iter().enumerate() {
        if i > 0 { print!(", "); }
        if let Some(font_match) = group.fonts.first() {
            print!("{}", font_match.id);
        }
    }
    println!("\n");
    
    // Example 6: Mixed Latin + CJK text
    println!("Example 6: Mixed Latin + CJK text with 'sans-serif'");
    let mixed_text = "Hello 世界";
    let chain = cache.resolve_font_chain(
        &vec!["sans-serif".to_string()],
        mixed_text,
        FcWeight::Normal,
        PatternMatch::False,
        PatternMatch::False,
        &mut Vec::new(),
    );
    
    println!("  Text: '{}'", mixed_text);
    print!("  Fonts: ");
    for (i, group) in chain.css_fallbacks.iter().enumerate() {
        if i > 0 { print!(", "); }
        if let Some(font_match) = group.fonts.first() {
            print!("{}", font_match.id);
        }
    }
    println!("\n");
    
    // Example 7: Cross-platform - Windows with CJK text
    println!("Example 7: Cross-platform - Windows with CJK text");
    let chain = cache.resolve_font_chain_with_os(
        &vec!["sans-serif".to_string()],
        "你好世界",
        FcWeight::Normal,
        PatternMatch::False,
        PatternMatch::False,
        &mut Vec::new(),
        OperatingSystem::Windows,
    );
    
    println!("  Text: '你好世界'");
    println!("  OS: Windows");
    println!("  Expected fonts (prioritized): Microsoft YaHei (CJK), then Segoe UI (Latin)");
    print!("  Actual font IDs: ");
    for (i, group) in chain.css_fallbacks.iter().take(3).enumerate() {
        if i > 0 { print!(", "); }
        if let Some(font_match) = group.fonts.first() {
            print!("{}", font_match.id);
        }
    }
    println!("\n  Note: Actual fonts depend on system installation\n");
    
    // Example 8: Monospace with CJK
    println!("Example 8: Monospace with CJK text");
    let chain = cache.resolve_font_chain(
        &vec!["monospace".to_string()],
        "代码示例",
        FcWeight::Normal,
        PatternMatch::False,
        PatternMatch::False,
        &mut Vec::new(),
    );
    
    println!("  Text: '代码示例' (code example)");
    print!("  Fonts: ");
    for (i, group) in chain.css_fallbacks.iter().enumerate() {
        if i > 0 { print!(", "); }
        if let Some(font_match) = group.fonts.first() {
            print!("{}", font_match.id);
        }
    }
    println!("\n");
    
    println!("=== Summary ===");
    println!("Unicode-aware font selection prioritizes fonts based on the text content:");
    println!("- Latin text (U+0000-U+007F) → Standard system fonts (Helvetica, Arial, etc.)");
    println!("- CJK text (U+4E00-U+9FFF, U+3040-U+30FF, U+AC00-U+D7AF) → CJK fonts first");
    println!("- Arabic text (U+0600-U+06FF) → Arabic-capable fonts first");
    println!("- Cyrillic text (U+0400-U+04FF) → Cyrillic-capable fonts first");
    println!("\nThis ensures better text rendering for multilingual content!");
}
