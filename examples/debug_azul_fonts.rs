//! Debug script to check why Azul's system font names fail to resolve

use rust_fontconfig::{FcFontCache, FcWeight, PatternMatch};

fn main() {
    println!("Building font cache...");
    let cache = FcFontCache::build();
    println!("Font cache built with {} fonts\n", cache.list().len());

    // These are the font names that Azul passes for system:ui and system:title:bold
    let test_names = vec![
        "System Font",
        "Helvetica Neue",
        "Lucida Grande",
        ".AppleSystemUIFont",
        "SF Pro",
        "SF Pro Text",
        "San Francisco",
        "Helvetica",
        "Arial",  // known-good reference
    ];

    for name in &test_names {
        println!("=== Testing: \"{}\" ===", name);
        let mut trace = Vec::new();
        let chain = cache.resolve_font_chain(
            &[name.to_string()],
            FcWeight::Normal,
            PatternMatch::DontCare,
            PatternMatch::DontCare,
            &mut trace,
        );
        
        let total_fonts: usize = chain.css_fallbacks.iter().map(|g| g.fonts.len()).sum();
        println!("  CSS fallbacks: {} groups, {} total fonts", chain.css_fallbacks.len(), total_fonts);
        for group in &chain.css_fallbacks {
            println!("  Group '{}': {} fonts", group.css_name, group.fonts.len());
            for (i, fm) in group.fonts.iter().take(3).enumerate() {
                if let Some(meta) = cache.get_metadata_by_id(&fm.id) {
                    println!("    [{}] name={:?} family={:?} weight={:?}", 
                        i, meta.name, meta.family, meta.weight);
                }
            }
        }
        if total_fonts == 0 {
            println!("  *** NO FONTS FOUND ***");
            if !trace.is_empty() {
                println!("  Trace:");
                for t in trace.iter().take(5) {
                    println!("    {:?}", t);
                }
            }
        }
        println!();
    }

    // Also check: what font names contain "helvetica" or "system" in the cache?
    println!("=== Fonts containing 'helvetica' (case-insensitive) ===");
    for (meta, id) in cache.list() {
        let name_str = meta.name.as_deref().unwrap_or("");
        let family_str = meta.family.as_deref().unwrap_or("");
        if name_str.to_lowercase().contains("helvetica") || family_str.to_lowercase().contains("helvetica") {
            println!("  {:?}: name={:?} family={:?}", id, meta.name, meta.family);
        }
    }

    println!("\n=== Fonts containing 'system' (case-insensitive) ===");
    for (meta, id) in cache.list() {
        let name_str = meta.name.as_deref().unwrap_or("");
        let family_str = meta.family.as_deref().unwrap_or("");
        if name_str.to_lowercase().contains("system") || family_str.to_lowercase().contains("system") {
            println!("  {:?}: name={:?} family={:?}", id, meta.name, meta.family);
        }
    }

    println!("\n=== Fonts containing 'lucida' (case-insensitive) ===");
    for (meta, id) in cache.list() {
        let name_str = meta.name.as_deref().unwrap_or("");
        let family_str = meta.family.as_deref().unwrap_or("");
        if name_str.to_lowercase().contains("lucida") || family_str.to_lowercase().contains("lucida") {
            println!("  {:?}: name={:?} family={:?}", id, meta.name, meta.family);
        }
    }

    // Check what the DEFAULT font resolves to (sans-serif with no specific name)
    println!("\n=== Default sans-serif (what counter text uses) ===");
    let mut trace = Vec::new();
    let chain = cache.resolve_font_chain(
        &["sans-serif".to_string()],
        FcWeight::Normal,
        PatternMatch::DontCare,
        PatternMatch::DontCare,
        &mut trace,
    );
    for group in &chain.css_fallbacks {
        println!("  Group '{}': {} fonts", group.css_name, group.fonts.len());
        for (i, fm) in group.fonts.iter().take(5).enumerate() {
            if let Some(meta) = cache.get_metadata_by_id(&fm.id) {
                println!("    [{}] name={:?} family={:?}", i, meta.name, meta.family);
            }
        }
    }
}
