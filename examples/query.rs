//! Example demonstrating basic font querying
//! 
//! Shows how to query for specific fonts using patterns.

use rust_fontconfig::{FcFontCache, FcPattern, FcWeight, PatternMatch};

fn main() {
    // Initialize font cache - scans system fonts
    println!("Building font cache...");
    let cache = FcFontCache::build();
    println!("Font cache built with {} fonts\n", cache.list().len());
    
    // Example 1: Query by family name
    println!("=== Query by Family Name ===");
    let mut trace = Vec::new();
    let pattern = FcPattern {
        family: Some("Arial".to_string()),
        ..Default::default()
    };
    
    if let Some(match_result) = cache.query(&pattern, &mut trace) {
        println!("Found Arial:");
        println!("  Font ID: {:?}", match_result.id);
        if let Some(meta) = cache.get_metadata_by_id(&match_result.id) {
            println!("  Family: {:?}", meta.family);
            println!("  Weight: {:?}", meta.weight);
            println!("  Italic: {:?}", meta.italic);
        }
        // To get the path, use get_font_by_id
        if let Some(source) = cache.get_font_by_id(&match_result.id) {
            match source {
                rust_fontconfig::FontSource::Disk(path) => {
                    println!("  Path: {}", path.path);
                }
                rust_fontconfig::FontSource::Memory(font) => {
                    println!("  Memory font: {}", font.id);
                }
            }
        }
    } else {
        println!("Arial not found, trace:");
        for t in &trace {
            println!("  {:?}", t);
        }
    }
    
    // Example 2: Query by generic family
    println!("\n=== Query Generic 'serif' ===");
    trace.clear();
    let pattern = FcPattern {
        family: Some("serif".to_string()),
        ..Default::default()
    };
    
    if let Some(match_result) = cache.query(&pattern, &mut trace) {
        println!("Found serif font:");
        if let Some(meta) = cache.get_metadata_by_id(&match_result.id) {
            println!("  Name: {:?}", meta.name.as_ref().or(meta.family.as_ref()));
        }
    }
    
    // Example 3: Query by style (bold + italic)
    println!("\n=== Query Bold Italic ===");
    trace.clear();
    let pattern = FcPattern {
        family: Some("sans-serif".to_string()),
        weight: FcWeight::Bold,
        italic: PatternMatch::True,
        ..Default::default()
    };
    
    if let Some(match_result) = cache.query(&pattern, &mut trace) {
        println!("Found bold italic sans-serif:");
        if let Some(meta) = cache.get_metadata_by_id(&match_result.id) {
            println!("  Name: {:?}", meta.name);
            println!("  Family: {:?}", meta.family);
            println!("  Weight: {:?}", meta.weight);
            println!("  Italic: {:?}", meta.italic);
        }
    }
    
    // Example 4: List all fonts with a specific weight
    println!("\n=== Listing Bold Fonts ===");
    let bold_fonts: Vec<_> = cache.list().into_iter()
        .filter(|(meta, _id)| {
            matches!(meta.weight, FcWeight::Bold | FcWeight::ExtraBold | FcWeight::Black)
        })
        .take(5)
        .collect();
    
    println!("First 5 bold fonts:");
    for (meta, id) in bold_fonts {
        println!("  {:?}: {:?}", id, meta.name.as_ref().or(meta.family.as_ref()));
    }
    
    // Example 5: Search by name pattern
    println!("\n=== Fonts with 'Mono' in name ===");
    let mono_fonts: Vec<_> = cache.list().into_iter()
        .filter(|(meta, _id)| {
            meta.name.as_ref().map(|n| n.contains("Mono")).unwrap_or(false) ||
            meta.family.as_ref().map(|f| f.contains("Mono")).unwrap_or(false)
        })
        .take(5)
        .collect();
    
    println!("First 5 monospace fonts:");
    for (meta, id) in mono_fonts {
        println!("  {:?}: {:?}", id, meta.name.as_ref().or(meta.family.as_ref()));
    }
}
