use rust_fontconfig::{FcFontCache, FcPattern, PatternMatch};

fn main() {
    let cache = FcFontCache::build();
    let fonts = cache.query_all(&FcPattern {
        monospace: PatternMatch::True,
        ..Default::default()
    }, &mut Vec::new());

    println!("total fonts: {}", fonts.len());

    for font in fonts {
        println!("{:?}", font);
    }
}
