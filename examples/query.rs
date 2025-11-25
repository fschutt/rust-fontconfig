use rust_fontconfig::{FcFontCache, FcPattern, PatternMatch, FcWeight, UnicodeRange};
use std::time::Instant;

fn main() {
    println!("Building font cache...");
    let start = Instant::now();
    let cache = FcFontCache::build();
    let build_time = start.elapsed();
    
    println!("✓ Cache built with {} fonts in {:?}\n", cache.list().len(), build_time);

    // Query 1: Find all monospace fonts
    println!("=== Query 1: All Monospace Fonts ===");
    let query_start = Instant::now();
    let monospace_fonts = cache.query_all(
        &FcPattern {
            monospace: PatternMatch::True,
            ..Default::default()
        },
        &mut Vec::new(),
    );
    println!("Found {} monospace fonts in {:?}", monospace_fonts.len(), query_start.elapsed());
    
    // Show first 5 monospace fonts
    for (i, font) in monospace_fonts.iter().take(5).enumerate() {
        if let Some(pattern) = cache.get_metadata_by_id(&font.id) {
            let name = pattern.name.as_ref().or(pattern.family.as_ref())
                .map(|s| s.as_str()).unwrap_or("<unknown>");
            println!("  {}. {} (weight: {:?})", i + 1, name, pattern.weight);
        }
    }
    if monospace_fonts.len() > 5 {
        println!("  ... and {} more", monospace_fonts.len() - 5);
    }
    println!();

    // Query 2: Find bold fonts
    println!("=== Query 2: Bold Fonts ===");
    let query_start = Instant::now();
    let bold_fonts = cache.query_all(
        &FcPattern {
            bold: PatternMatch::True,
            weight: FcWeight::Bold,
            ..Default::default()
        },
        &mut Vec::new(),
    );
    println!("Found {} bold fonts in {:?}", bold_fonts.len(), query_start.elapsed());
    
    for (i, font) in bold_fonts.iter().take(5).enumerate() {
        if let Some(pattern) = cache.get_metadata_by_id(&font.id) {
            let name = pattern.name.as_ref().or(pattern.family.as_ref())
                .map(|s| s.as_str()).unwrap_or("<unknown>");
            println!("  {}. {}", i + 1, name);
        }
    }
    if bold_fonts.len() > 5 {
        println!("  ... and {} more", bold_fonts.len() - 5);
    }
    println!();

    // Query 3: Find fonts with CJK (Chinese/Japanese/Korean) support
    println!("=== Query 3: Fonts with CJK Unicode Support ===");
    let query_start = Instant::now();
    let cjk_fonts = cache.query_all(
        &FcPattern {
            unicode_ranges: vec![
                UnicodeRange { start: 0x4E00, end: 0x9FFF }, // CJK Unified Ideographs
            ],
            ..Default::default()
        },
        &mut Vec::new(),
    );
    println!("Found {} fonts with CJK support in {:?}", cjk_fonts.len(), query_start.elapsed());
    
    for (i, font) in cjk_fonts.iter().take(5).enumerate() {
        if let Some(pattern) = cache.get_metadata_by_id(&font.id) {
            let name = pattern.name.as_ref().or(pattern.family.as_ref())
                .map(|s| s.as_str()).unwrap_or("<unknown>");
            println!("  {}. {} ({} unicode ranges)", i + 1, name, pattern.unicode_ranges.len());
        }
    }
    if cjk_fonts.len() > 5 {
        println!("  ... and {} more", cjk_fonts.len() - 5);
    }
}
