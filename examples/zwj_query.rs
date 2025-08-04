use rust_fontconfig::{FcFontCache, FcPattern};

fn main() {
    let cache = FcFontCache::build();
    let text = "👩🏿‍🎓🦆";

    // Find fonts that can render the mixed-script text
    let mut trace = Vec::new();
    let mut pattern = FcPattern::default();

    // The `query_for_text` function will automatically add the codepoints from the text
    // to the pattern and prioritize fonts that support the full ZWJ sequence.
    let matched_fonts = cache.query_for_text(&mut pattern, text, &mut trace);

    println!("Found {} fonts for the multilingual text", matched_fonts.len());
    for font in matched_fonts {
        println!("Font ID: {:?}", font.id);
        let metadata = cache.get_metadata_by_id(&font.id).unwrap();
        println!("Metadata, name: {:?}", metadata.name);
        println!("Metadata, unicode ranges: {:?}", metadata.unicode_ranges);
        println!();
    }
}
