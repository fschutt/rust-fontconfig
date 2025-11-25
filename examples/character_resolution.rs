//! Character resolution example
//! 
//! Demonstrates how to resolve individual characters to fonts,
//! useful for debugging font coverage issues.

use rust_fontconfig::{FcFontCache, FcWeight, PatternMatch};

fn main() {
    let cache = FcFontCache::build();
        
    // Create a font chain with typical web defaults
    let families = vec![
        "system-ui".to_string(),
        "sans-serif".to_string(),
    ];
    
    let mut trace = Vec::new();
    let chain = cache.resolve_font_chain(
        &families,
        FcWeight::Normal,
        PatternMatch::False,
        PatternMatch::False,
        &mut trace,
    );
    
    // Test characters from different Unicode blocks
    let test_chars = vec![
        ('A', "Latin Capital Letter A"),
        ('a', "Latin Small Letter A"),
        ('0', "Digit Zero"),
        ('€', "Euro Sign"),
        ('→', "Rightwards Arrow"),
        ('中', "CJK Ideograph - China"),
        ('日', "CJK Ideograph - Sun/Day"),
        ('あ', "Hiragana Letter A"),
        ('ア', "Katakana Letter A"),
        ('한', "Hangul Syllable Han"),
        ('א', "Hebrew Letter Alef"),
        ('ا', "Arabic Letter Alef"),
        ('α', "Greek Small Letter Alpha"),
        ('Σ', "Greek Capital Letter Sigma"),
        ('я', "Cyrillic Small Letter Ya"),
        ('🙂', "Slightly Smiling Face"),
        ('♠', "Black Spade Suit"),
        ('∑', "N-ary Summation"),
        ('∞', "Infinity"),
        ('℃', "Degree Celsius"),
    ];
    
    println!("Character resolution results:\n");
    println!("{:<6} {:<30} {:<40}", "Char", "Description", "Font");
    println!("{}", "-".repeat(80));
    
    for (ch, description) in test_chars {
        let text = ch.to_string();
        let resolved = chain.resolve_text(&cache, &text);
        
        let font_name = resolved.first()
            .and_then(|(_, info)| info.as_ref())
            .and_then(|(id, _)| cache.get_metadata_by_id(id))
            .and_then(|m| m.name.clone().or(m.family.clone()))
            .unwrap_or_else(|| "⚠ NOT FOUND".to_string());
        
        println!("{:<6} {:<30} {}", ch, description, font_name);
    }
    
    // Show how to check if a specific font covers a character
    println!("\n\nFont Coverage Check\n");
    
    let pattern = rust_fontconfig::FcPattern {
        family: Some("Arial".to_string()),
        ..Default::default()
    };
    
    if let Some(match_result) = cache.query(&pattern, &mut Vec::new()) {
        println!("Checking Arial coverage:");
        
        // Create a chain just for Arial
        let arial_chain = cache.resolve_font_chain(
            &vec!["Arial".to_string()],
            FcWeight::Normal,
            PatternMatch::False,
            PatternMatch::False,
            &mut Vec::new(),
        );
        
        let check_chars = ['A', '中', '🙂', '→'];
        for ch in check_chars {
            let resolved = arial_chain.resolve_text(&cache, &ch.to_string());
            let found_in_arial = resolved.first()
                .and_then(|(_, info)| info.as_ref())
                .map(|(id, _)| id == &match_result.id)
                .unwrap_or(false);
            
            let status = if found_in_arial { "✓" } else { "✗" };
            println!("  {} '{}' (U+{:04X})", status, ch, ch as u32);
        }
    }
    
    // Show codepoint ranges supported
    println!("\n\nUnicode Block Coverage Summary\n");
    
    let blocks = [
        ("Basic Latin", 0x0020..0x007F),
        ("Latin Extended-A", 0x0100..0x017F),
        ("Greek", 0x0370..0x03FF),
        ("Cyrillic", 0x0400..0x04FF),
        ("Arabic", 0x0600..0x06FF),
        ("CJK Unified Ideographs", 0x4E00..0x9FFF),
        ("Hiragana", 0x3040..0x309F),
        ("Katakana", 0x30A0..0x30FF),
    ];
    
    for (name, range) in blocks {
        // Sample a few codepoints from each block
        let sample_points: Vec<char> = range.clone()
            .step_by(range.len() / 5)
            .take(5)
            .filter_map(|cp| char::from_u32(cp))
            .collect();
        
        let sample_text: String = sample_points.iter().collect();
        let resolved = chain.resolve_text(&cache, &sample_text);
        
        let fonts_used: std::collections::HashSet<_> = resolved.iter()
            .filter_map(|(_, info)| info.as_ref())
            .map(|(id, _)| id.clone())
            .collect();
        
        let coverage = resolved.iter()
            .filter(|(_, info)| info.is_some())
            .count() as f32 / resolved.len() as f32 * 100.0;
        
        println!("{:<30} {:>6.1}% coverage ({} fonts)", name, coverage, fonts_used.len());
    }
}
