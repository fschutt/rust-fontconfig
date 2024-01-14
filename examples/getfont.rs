use rust_fontconfig::{FcFontCache, FcPattern};
use std::time::Instant;

fn main() {
    let start = Instant::now();
    let cache = FcFontCache::build();
    let end = Instant::now();

    let start2 = Instant::now();
    let results = cache.query(&FcPattern {
        name: Some(String::from("Purisa")),
        ..Default::default()
    });
    let end2 = Instant::now();

    println!("built cache in: {:?}", end - start);
    println!("font results: {:?} - queried in {:?}", results, end2 - start2);
}
