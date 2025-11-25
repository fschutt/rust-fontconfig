use rust_fontconfig::{FcFontCache, FcWeight, PatternMatch};

fn main() {
    let cache = FcFontCache::build();

    // Example 1: Bold Japanese text with Latin fallback
    println!("=== Example 1: Bold Japanese + Latin ===");
    let text = "日本語 and English";
    let font_families = vec![
        "Hiragino Sans".to_string(),
        "sans-serif".to_string(),
    ];
    
    resolve_and_print_text(&cache, &font_families, text, FcWeight::Bold, PatternMatch::DontCare);
    println!();

    // Example 2: Italic serif text
    println!("=== Example 2: Italic Serif ===");
    let text = "The Quick Brown Fox";
    let font_families = vec!["serif".to_string()];
    
    resolve_and_print_text(&cache, &font_families, text, FcWeight::Normal, PatternMatch::True);
    println!();

    // Example 3: Bold monospace
    println!("=== Example 3: Bold Monospace ===");
    let text = "code_example() { }";
    let font_families = vec!["monospace".to_string()];
    
    resolve_and_print_text(&cache, &font_families, text, FcWeight::Bold, PatternMatch::DontCare);
    println!();

    // Example 4: Sans-serif with specific font
    println!("=== Example 4: Arial Bold ===");
    let text = "Bold Text Example";
    let font_families = vec!["Arial".to_string(), "sans-serif".to_string()];
    
    resolve_and_print_text(&cache, &font_families, text, FcWeight::Bold, PatternMatch::DontCare);
    println!();

    // Example 5: Mixed scripts with bold
    println!("=== Example 5: Bold Mixed Scripts ===");
    let text = "Cyrillic: Привет Japanese: こんにちは";
    let font_families = vec!["Helvetica".to_string(), "sans-serif".to_string()];
    
    resolve_and_print_text(&cache, &font_families, text, FcWeight::Bold, PatternMatch::DontCare);
}

fn resolve_and_print_text(
    cache: &FcFontCache,
    font_families: &[String],
    text: &str,
    weight: FcWeight,
    italic: PatternMatch,
) {
    println!("Font stack: {:?}", font_families.iter().take(3).collect::<Vec<_>>());
    println!("Text: \"{}\"", text);
    println!("Weight: {:?}, Italic: {:?}\n", weight, italic);
    
    let chain = cache.resolve_font_chain(
        font_families,
        text,
        weight,
        italic,
        PatternMatch::DontCare,
        &mut Vec::new(),
    );
    
    let resolved = chain.resolve_text(cache, text);
    
    let mut current_font: Option<(String, String)> = None;
    let mut current_segment = String::new();
    
    for (ch, font_info) in resolved {
        match font_info {
            Some((font_id, css_source)) => {
                let font_key = (font_id.to_string(), css_source.clone());
                
                if current_font.as_ref() != Some(&font_key) {
                    if !current_segment.is_empty() {
                        if let Some((ref prev_id, ref prev_source)) = current_font {
                            print_font_segment(cache, &current_segment, &parse_font_id(prev_id), prev_source);
                        }
                        current_segment.clear();
                    }
                    
                    current_font = Some(font_key);
                }
                
                current_segment.push(ch);
            }
            None => {
                if !current_segment.is_empty() {
                    if let Some((ref prev_id, ref prev_source)) = current_font {
                        print_font_segment(cache, &current_segment, &parse_font_id(prev_id), prev_source);
                    }
                    current_segment.clear();
                }
                
                println!("  '{}' -> [NO FONT]", ch);
                current_font = None;
            }
        }
    }
    
    if !current_segment.is_empty() {
        if let Some((ref prev_id, ref prev_source)) = current_font {
            print_font_segment(cache, &current_segment, &parse_font_id(prev_id), prev_source);
        }
    }
}

fn print_font_segment(
    cache: &FcFontCache, 
    text: &str, 
    font_id: &rust_fontconfig::FontId,
    css_source: &str,
) {
    if let Some(font_source) = cache.get_font_by_id(font_id) {
        let font_index = match font_source {
            rust_fontconfig::FontSource::Memory(m) => m.font_index,
            rust_fontconfig::FontSource::Disk(d) => d.font_index,
        };
        
        if let Some(pattern) = cache.get_metadata_by_id(font_id) {
            let font_name = pattern.name.as_ref()
                .or(pattern.family.as_ref())
                .map(|s| s.as_str())
                .unwrap_or("<unknown>");
            println!("  '{}' -> {} [index: {}] (CSS: '{}')", 
                     text, font_name, font_index, css_source);
        }
    }
}

fn parse_font_id(id_str: &str) -> rust_fontconfig::FontId {
    let parts: Vec<&str> = id_str.split('-').collect();
    if parts.len() != 5 {
        return rust_fontconfig::FontId(0);
    }
    
    let part1 = u128::from_str_radix(parts[0], 16).unwrap_or(0) << 96;
    let part2 = u128::from_str_radix(parts[1], 16).unwrap_or(0) << 80;
    let part3 = u128::from_str_radix(parts[2], 16).unwrap_or(0) << 64;
    let part4 = u128::from_str_radix(parts[3], 16).unwrap_or(0) << 48;
    let part5 = u128::from_str_radix(parts[4], 16).unwrap_or(0);
    
    rust_fontconfig::FontId(part1 | part2 | part3 | part4 | part5)
}
