//! Faithful repro of the printpdf "Latin text renders as CJK NotoSansJP" bug.
//!
//! Mimics the WASM demo environment: an EMPTY base cache (no system-font scan,
//! exactly like `FcFontCache::default()` on wasm) into which we memory-register
//! two faces — the broad-coverage CJK megafont `NotoSansJP` and a plain Latin
//! `DejaVu Sans` / `Noto Sans`. A `<p>` with no font-family resolves the
//! `sans-serif` stack; on Linux that expands to
//! [Ubuntu, Arial, DejaVu Sans, Noto Sans, Liberation Sans]. The Latin faces
//! MUST win for 'E'. Before the `query_by_family_normalized` fix, the concrete
//! names resolve to nothing (fuzzy index is a no-op) and 'E' falls through to
//! the coverage-ranked megafont.
//!
//! Run with absolute font paths present on this dev box:
//!   cargo run --example repro_font_fallback

use rust_fontconfig::*;

fn load(cache: &FcFontCache, path: &str, id: &str) {
    match std::fs::read(path) {
        Ok(bytes) => {
            if let Some(parsed) = FcParseFontBytes(&bytes, id) {
                for (p, _) in &parsed {
                    println!(
                        "  registered {:<22} family={:?} name={:?} ranges={}",
                        id, p.family, p.name, p.unicode_ranges.len()
                    );
                }
                cache.with_memory_fonts(parsed);
            } else {
                println!("  FAILED to parse {}", path);
            }
        }
        Err(e) => println!("  MISSING {} ({e})", path),
    }
}

fn main() {
    let cache = FcFontCache::default(); // empty — no system scan (wasm-faithful)

    println!("Registering memory fonts:");
    // The CJK megafont first (highest Unicode coverage → the wrong winner).
    load(&cache, "/home/fs/Development/printpdf/examples/assets/fonts/NotoSansJP-Regular.otf", "NotoSansJP");
    // A plain Latin sans face whose family IS in the sans-serif expansion.
    load(&cache, "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "DejaVuSans");
    load(&cache, "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf", "NotoSans");

    for (label, w) in [("REGULAR", FcWeight::Normal), ("BOLD", FcWeight::Bold)] {
        let mut trace = Vec::new();
        let chain = cache.resolve_font_chain(
            &["sans-serif".to_string()],
            w, PatternMatch::False, PatternMatch::False, &mut trace,
        );
        println!("\n=== sans-serif {} ===", label);
        for g in &chain.css_fallbacks {
            print!("  group '{}':", g.css_name);
            for fm in g.fonts.iter().take(3) {
                let fam = cache.get_metadata_by_id(&fm.id).and_then(|m| m.family.clone()).unwrap_or_default();
                let nm = cache.get_metadata_by_id(&fm.id).and_then(|m| m.name.clone()).unwrap_or_default();
                print!(" [{}|{}]", fam, nm);
            }
            println!();
        }
        match chain.resolve_char(&cache, 'E') {
            Some((id, src)) => {
                let fam = cache.get_metadata_by_id(&id).and_then(|m| m.family.clone()).unwrap_or_default();
                let nm = cache.get_metadata_by_id(&id).and_then(|m| m.name.clone()).unwrap_or_default();
                let verdict = if fam.to_lowercase().contains("jp") || nm.to_lowercase().contains("jp") {
                    ">>> BUG: 'E' resolved to the CJK megafont"
                } else {
                    ">>> OK: 'E' resolved to a Latin face"
                };
                println!("  {} — family='{}' name='{}' via '{}'", verdict, fam, nm, src);
            }
            None => println!("  >>> 'E' resolves to: NONE"),
        }
    }
}
