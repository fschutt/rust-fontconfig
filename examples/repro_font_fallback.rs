//! Mimics an empty cache (no system-font scan, similar to WASM) and registers
//! a CJK font alongside a Latin font. Verifies that Latin characters are correctly
//! resolved to the Latin font rather than falling back to the broader CJK font.
//!
//! Run with absolute font paths present on your machine:
//! cargo run --example repro_font_fallback

use rust_fontconfig::*;

fn load(cache: &FcFontCache, path: &str, id: &str) {
    match std::fs::read(path) {
        Ok(bytes) => {
            if let Some(parsed) = FcParseFontBytes(&bytes, id) {
                for (p, _) in &parsed {
                    println!(
                        "  registered {:<22} family={:?} name={:?} ranges={}",
                        id,
                        p.family,
                        p.name,
                        p.unicode_ranges.len()
                    );
                }
                cache.with_memory_fonts(parsed);
            } else {
                println!("  Failed to parse {}", path);
            }
        }
        Err(e) => println!("  Missing {} ({e})", path),
    }
}

fn main() {
    let cache = FcFontCache::default(); // empty cache without system scan

    println!("Registering memory fonts:");
    // CJK font with high unicode coverage
    load(
        &cache,
        "/home/fs/Development/printpdf/examples/assets/fonts/NotoSansJP-Regular.otf",
        "NotoSansJP",
    );
    // Plain Latin sans face
    load(
        &cache,
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "DejaVuSans",
    );
    load(
        &cache,
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "NotoSans",
    );

    for (label, w) in [("Regular", FcWeight::Normal), ("Bold", FcWeight::Bold)] {
        let mut trace = Vec::new();
        let chain = cache.resolve_font_chain(
            &["sans-serif".to_string()],
            w,
            PatternMatch::False,
            PatternMatch::False,
            &mut trace,
        );
        println!("\nsans-serif {}", label);
        for g in &chain.css_fallbacks {
            print!("  group '{}':", g.css_name);
            for fm in g.fonts.iter().take(3) {
                let fam = cache
                    .get_metadata_by_id(&fm.id)
                    .and_then(|m| m.family.clone())
                    .unwrap_or_default();
                let nm = cache
                    .get_metadata_by_id(&fm.id)
                    .and_then(|m| m.name.clone())
                    .unwrap_or_default();
                print!(" [{}|{}]", fam, nm);
            }
            println!();
        }
        match chain.resolve_char(&cache, 'E') {
            Some((id, src)) => {
                let fam = cache
                    .get_metadata_by_id(&id)
                    .and_then(|m| m.family.clone())
                    .unwrap_or_default();
                let nm = cache
                    .get_metadata_by_id(&id)
                    .and_then(|m| m.name.clone())
                    .unwrap_or_default();
                let verdict =
                    if fam.to_lowercase().contains("jp") || nm.to_lowercase().contains("jp") {
                        "Error: 'E' resolved to CJK font"
                    } else {
                        "Success: 'E' resolved to Latin font"
                    };
                println!(
                    "  {} (family='{}' name='{}' via '{}')",
                    verdict, fam, nm, src
                );
            }
            None => println!("  Error: 'E' resolves to none"),
        }
    }
}
