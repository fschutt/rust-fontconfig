//! Compares the synchronous `FcFontCache::build()` (scans and parses all fonts upfront)
//! with the asynchronous `FcFontRegistry` (uses background threads and on-demand loading).
//!
//! Run with: cargo run --example registry --features "async-registry,cache"
//! (Run twice to see the disk cache improvement)

use rust_fontconfig::registry::FcFontRegistry;
use rust_fontconfig::{FcFontCache, FcPattern, FcWeight, FontId, PatternMatch};
use std::time::Instant;

fn main() {
    println!("Font Loading: Synchronous API vs Async Registry\n");

    // Part 1: Synchronous API
    println!("Synchronous API: FcFontCache::build() (loads all fonts upfront)\n");

    let t_old = Instant::now();
    let old_cache = FcFontCache::build();
    let old_time = t_old.elapsed();

    println!(
        "  Loaded {} fonts in {:?}",
        old_cache.list().len(),
        old_time
    );

    // Query the old cache
    let t_q = Instant::now();
    let old_result = old_cache.query(
        &FcPattern {
            name: Some("Helvetica".to_string()),
            ..Default::default()
        },
        &mut Vec::new(),
    );
    let old_query_time = t_q.elapsed();
    if let Some(fm) = &old_result {
        let name = old_cache
            .get_metadata_by_id(&fm.id)
            .and_then(|m| m.name.clone().or(m.family.clone()))
            .unwrap_or_default();
        println!("  Query 'Helvetica' -> {} in {:?}", name, old_query_time);
    }

    // Resolve font chain and text
    let old_chain = old_cache.resolve_font_chain(
        &["sans-serif".to_string()],
        FcWeight::Normal,
        PatternMatch::False,
        PatternMatch::False,
        &mut Vec::new(),
    );
    let css_count: usize = old_chain.css_fallbacks.iter().map(|g| g.fonts.len()).sum();
    println!(
        "  Font chain for 'sans-serif': {} CSS fonts, {} unicode fallbacks",
        css_count,
        old_chain.unicode_fallbacks.len()
    );

    // Show old results
    let test_text = "Hello 世界!";
    let old_resolved = old_chain.resolve_text(&old_cache, test_text);
    print!("  Text '{}': ", test_text);
    print_runs(&old_cache, &old_resolved);

    let old_total = t_old.elapsed();
    println!("  Total synchronous API time: {:?}\n", old_total);

    // Part 2: Async API
    println!("Async API: FcFontRegistry (background threads + on-demand)\n");

    let t_new = Instant::now();

    // Create registry (instant)
    let t1 = Instant::now();
    let registry = FcFontRegistry::new();
    println!("  1. Registry::new()             {:>10?}", t1.elapsed());

    // Load disk cache
    let t2 = Instant::now();
    let had_cache = registry.load_from_disk_cache();
    println!(
        "  2. load_from_disk_cache()      {:>10?}  ({})",
        t2.elapsed(),
        if had_cache.is_some() { "HIT" } else { "MISS" },
    );

    // Spawn background threads
    let t3 = Instant::now();
    registry.spawn_scout_and_builders();
    println!("  3. spawn_scout_and_builders()  {:>10?}", t3.elapsed());

    // Simulate app startup work (window creation, DOM build, etc.)
    let t_gap = Instant::now();
    println!("  4. (simulating 50ms of app startup work...)");
    std::thread::sleep(std::time::Duration::from_millis(50));
    println!("     ...done in {:?}", t_gap.elapsed());

    // Request specific fonts (blocks until ready)
    let t4 = Instant::now();
    let result = registry.query(&FcPattern {
        name: Some("Helvetica".to_string()),
        ..Default::default()
    });
    println!("  5. query('Helvetica')          {:>10?}", t4.elapsed());
    if let Some(fm) = &result {
        let name = registry
            .get_metadata_by_id(&fm.id)
            .and_then(|m| m.name.clone().or(m.family.clone()))
            .unwrap_or_default();
        println!("     -> {}", name);
    }

    // Resolve font chain
    let t5 = Instant::now();
    let new_chain = registry.resolve_font_chain(
        &["sans-serif".to_string()],
        FcWeight::Normal,
        PatternMatch::False,
        PatternMatch::False,
    );
    let css_count: usize = new_chain.css_fallbacks.iter().map(|g| g.fonts.len()).sum();
    println!("  6. resolve_font_chain()        {:>10?}", t5.elapsed());
    println!(
        "     {} CSS fonts, {} unicode fallbacks",
        css_count,
        new_chain.unicode_fallbacks.len()
    );

    // Snapshot to FcFontCache for text resolution
    let t6 = Instant::now();
    let new_cache = registry.shared_cache();
    println!("  7. shared_cache()              {:>10?}", t6.elapsed());

    let new_resolved = new_chain.resolve_text(&new_cache, test_text);
    print!("     Text '{}': ", test_text);
    print_runs(&new_cache, &new_resolved);

    // Status
    println!(
        "  8. scan_complete={}, build_complete={}, cache_loaded={}",
        registry.is_scan_complete(),
        registry.is_build_complete(),
        registry.is_cache_loaded(),
    );

    let new_total = t_new.elapsed();
    println!("  Total async API time: {:?}\n", new_total);

    // Save disk cache for next run
    #[cfg(feature = "cache")]
    {
        let t_save = Instant::now();
        registry.save_to_disk_cache();
        println!("  Disk cache saved in {:?}\n", t_save.elapsed());
    }

    registry.shutdown();

    // Summary
    println!("Summary");
    println!(
        "  Synchronous API (FcFontCache::build):  {:>10?}",
        old_total
    );
    println!("  Async API (FcFontRegistry):      {:>10?}", new_total);
    println!();
    println!("  Key: In a GUI app, steps 1-3 happen in App::create() and");
    println!("  steps 5-7 happen at first layout. The background threads");
    println!("  use the gap (window creation, DOM build) to pre-load fonts.");
    println!("  With disk cache from a previous run, queries are instant.");
}

fn print_runs(cache: &FcFontCache, resolved: &[(char, Option<(FontId, String)>)]) {
    let mut current_font: Option<String> = None;
    let mut seg = String::new();

    for (ch, info) in resolved {
        let font_name = info.as_ref().and_then(|(id, _css_src)| {
            cache
                .get_metadata_by_id(id)
                .and_then(|m| m.name.clone().or(m.family.clone()))
        });
        if font_name != current_font {
            if !seg.is_empty() {
                print!("[{}→{}] ", seg, current_font.as_deref().unwrap_or("?"));
                seg.clear();
            }
            current_font = font_name;
        }
        seg.push(*ch);
    }
    if !seg.is_empty() {
        print!("[{}→{}]", seg, current_font.as_deref().unwrap_or("?"));
    }
    println!();
}
