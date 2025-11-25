use rust_fontconfig::{FcFontCache, FcPattern, FcWeight};
use std::time::Instant;

fn main() {
    let start = Instant::now();
    let cache = FcFontCache::build();
    let build_time = start.elapsed();

    println!("✓ Cache built with {} fonts in {:?}", cache.list().len(), build_time);
    println!();

    // Test various font queries to showcase fuzzy matching
    let test_queries = vec![
        ("Arial", FcWeight::Normal, "Common sans-serif font"),
        ("NotoSansJP", FcWeight::Normal, "Japanese font (fuzzy match)"),
        ("Helvetica", FcWeight::Bold, "Bold variant"),
        ("DejaVu Sans", FcWeight::Normal, "Font with spaces"),
        ("Courier", FcWeight::Normal, "Monospace font"),
    ];

    for (font_name, weight, description) in test_queries {
        println!("Searching for: '{}' ({})", font_name, description);
        
        let query_start = Instant::now();
        let result = cache.query(
            &FcPattern {
                name: Some(font_name.to_string()),
                weight,
                ..Default::default()
            },
            &mut Vec::new(),
        );
        let query_time = query_start.elapsed();

        match result {
            Some(font_match) => {
                if let Some(pattern) = cache.get_metadata_by_id(&font_match.id) {
                    let name = pattern.name.as_ref().or(pattern.family.as_ref())
                        .map(|s| s.as_str()).unwrap_or("<unknown>");
                    println!("  ✓ Found: {} (query time: {:?})", name, query_time);
                    println!("    - Weight: {:?}", pattern.weight);
                    println!("    - Unicode ranges: {}", font_match.unicode_ranges.len());
                    println!("    - Fallbacks: {}", font_match.fallbacks.len());
                } else {
                    println!("  ✓ Found font ID: {} (query time: {:?})", font_match.id, query_time);
                }
            }
            None => {
                println!("  ✗ Not found (query time: {:?})", query_time);
            }
        }
        println!();
    }
}
