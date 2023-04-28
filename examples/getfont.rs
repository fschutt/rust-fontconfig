use rust_fontconfig::{FcFontCache, FcPattern};
use std::time::Instant;

fn main() {
    let start = Instant::now();
    let cache = FcFontCache::build();
    let end = Instant::now();

    let start2 = Instant::now();
    let result = cache.query(&FcPattern {
        name: Some(String::from("Purisa")),
        ..Default::default()
    });
    let end2 = Instant::now();

    println!("built cache in: {:?}", end - start);
    println!("font path: {:?} - queried in {:?}", result, end2 - start2);
}
