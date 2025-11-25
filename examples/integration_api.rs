//! Integration API example
//! 
//! Shows how to integrate rust-fontconfig into a text layout pipeline.

use rust_fontconfig::{FcFontCache, FcWeight, PatternMatch, FontId};

fn main() {
    println!("Font Integration API Example\n");
    
    // Step 1: Build the font cache once at application startup
    println!("Step 1: Building font cache...");
    let cache = FcFontCache::build();
    println!("  Loaded {} fonts\n", cache.list().len());
    
    // Step 2: Simulate CSS font-family resolution
    // This is what a browser/text renderer would do
    println!("Step 2: Resolving CSS font-family: 'Helvetica, Arial, sans-serif'\n");
    
    let css_families = vec![
        "Helvetica".to_string(),
        "Arial".to_string(),
        "sans-serif".to_string(),
    ];
    
    let mut trace = Vec::new();
    let chain = cache.resolve_font_chain(
        &css_families,
        FcWeight::Normal,
        PatternMatch::False,  // italic
        PatternMatch::False,  // oblique
        &mut trace,
    );
    
    println!("  CSS fallback groups: {}", chain.css_fallbacks.len());
    for (i, group) in chain.css_fallbacks.iter().enumerate() {
        println!("    {}: CSS '{}' -> {} fonts", i + 1, group.css_name, group.fonts.len());
        for font in group.fonts.iter().take(2) {
            if let Some(meta) = cache.get_metadata_by_id(&font.id) {
                let name = meta.name.as_ref().or(meta.family.as_ref());
                println!("       - {:?}", name);
            }
        }
        if group.fonts.len() > 2 {
            println!("       ... and {} more", group.fonts.len() - 2);
        }
    }
    
    println!("\n  Unicode fallback fonts: {}", chain.unicode_fallbacks.len());
    for (i, font) in chain.unicode_fallbacks.iter().take(3).enumerate() {
        if let Some(meta) = cache.get_metadata_by_id(&font.id) {
            println!("    {}: {:?}", 
                     i + 1, 
                     meta.name.as_ref().or(meta.family.as_ref()));
        }
    }
    if chain.unicode_fallbacks.len() > 3 {
        println!("    ... and {} more", chain.unicode_fallbacks.len() - 3);
    }
    
    // Step 3: Resolve text to fonts
    // This maps each character to a specific font
    println!("\n\nStep 3: Resolve text to fonts");
    
    let text = "Hello 世界! Привет мир";
    println!("  Input text: '{}'\n", text);
    
    let resolved = chain.resolve_text(&cache, text);
    
    // Group by runs of same font
    let mut runs = Vec::new();
    let mut current_run_text = String::new();
    let mut current_font_id: Option<FontId> = None;
    
    for (ch, font_info) in &resolved {
        let this_font_id = font_info.as_ref().map(|(id, _)| *id);
        
        if this_font_id != current_font_id {
            if !current_run_text.is_empty() {
                runs.push((current_run_text.clone(), current_font_id));
                current_run_text.clear();
            }
            current_font_id = this_font_id;
        }
        current_run_text.push(*ch);
    }
    if !current_run_text.is_empty() {
        runs.push((current_run_text, current_font_id));
    }
    
    println!("  Font runs:");
    for (run_text, font_id) in &runs {
        let font_name = font_id.as_ref()
            .and_then(|id| cache.get_metadata_by_id(id))
            .and_then(|m| m.name.clone().or(m.family.clone()))
            .unwrap_or_else(|| "[NO FONT]".to_string());
        println!("    '{}' -> {}", run_text, font_name);
    }
    
    // Step 4: Load font data for shaping
    // In a real application, you'd load font bytes here
    println!("\n\nStep 4: Loading fonts for shaping");
    
    // Collect unique font IDs needed for this text
    let unique_fonts: std::collections::HashSet<_> = runs.iter()
        .filter_map(|(_, id)| *id)
        .collect();
    
    println!("  Unique fonts needed: {}", unique_fonts.len());
    
    for font_id in &unique_fonts {
        if let Some(meta) = cache.get_metadata_by_id(font_id) {
            println!("    - {:?}", meta.name.as_ref().or(meta.family.as_ref()));
            // Get font path via get_font_by_id
            if let Some(source) = cache.get_font_by_id(font_id) {
                match source {
                    rust_fontconfig::FontSource::Disk(path) => {
                        // In real code, you'd load the font file here:
                        // let bytes = std::fs::read(&path.path)?;
                        // let parsed = ttf_parser::Face::parse(&bytes, path.font_index as u32)?;
                        println!("      Path: {}", path.path);
                    }
                    rust_fontconfig::FontSource::Memory(font) => {
                        println!("      Memory font (id: {})", font.id);
                    }
                }
            }
        }
    }
    
    println!("\nWorkflow summary:");
    println!("");
    println!("1. FcFontCache::build() - once at startup");
    println!("2. cache.resolve_font_chain() - per CSS font-family declaration");
    println!("3. chain.resolve_text(\”abc\”) -> [Run { font, glyphs: }] -”\" per text string to shape");
    println!("4. Load font bytes and shape each run with its font");
}
