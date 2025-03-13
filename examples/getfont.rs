use rust_fontconfig::{FcFontCache, FcPattern};
use std::time::Instant;

fn main() {
    let start = Instant::now();
    let cache = FcFontCache::build();
    let end = Instant::now();

    println!("cache list: found {} fonts", cache.list().len());

    let start2 = Instant::now();
    let results = cache.query(
        &FcPattern {
            name: Some(String::from("Gilroy")),
            ..Default::default()
        },
        &mut Vec::new(),
    );
    let end2 = Instant::now();

    println!("built cache in: {:?}", end - start);
    println!("queried in {:?}", end2 - start2);
}
