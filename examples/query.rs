use rust_fontconfig::{FcFontCache, FcPattern, PatternMatch};

fn main() {
    println!("building cache...");
    let cache = FcFontCache::build();
    println!("cache built!");
    let list = cache.list();
    println!("{} fonts:", list.len());
    let fonts = cache.query_all(
        &FcPattern {
            monospace: PatternMatch::True,
            ..Default::default()
        },
        &mut Vec::new(),
    );

    println!("total fonts: {}", fonts.len());

    if let Some(first) = fonts.first() {
        println!("MATCH: {:#?}", first);
    }
}
