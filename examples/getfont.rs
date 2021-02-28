use rust_fontconfig::{FcLocateFontInner, FcPattern};

fn main() {
    let start = std::time::Instant::now();
    let font = FcLocateFontInner(FcPattern {
        postscript_name: String::from("Purisa"),
        .. Default::default()
    });
    let end = std::time::Instant::now();
    println!("font path: {:?} - {:?}", font.map(|f| f.path), end - start);
}