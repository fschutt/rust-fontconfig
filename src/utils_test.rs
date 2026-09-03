
use super::*;

#[test]
fn font_extensions_covers_common_formats() {
    for ext in &["ttf", "otf", "ttc", "woff", "woff2"] {
        assert!(FONT_EXTENSIONS.contains(ext), "missing extension: {}", ext);
    }
}

#[cfg(feature = "std")]
#[test]
fn is_font_file_recognizes_fonts() {
    use std::path::Path;
    assert!(is_font_file(Path::new("Arial.ttf")));
    assert!(is_font_file(Path::new("NotoSans.otf")));
    assert!(is_font_file(Path::new("Font.TTC"))); // case insensitive
    assert!(is_font_file(Path::new("web.woff2")));
}

#[cfg(feature = "std")]
#[test]
fn is_font_file_rejects_non_fonts() {
    use std::path::Path;
    assert!(!is_font_file(Path::new("readme.txt")));
    assert!(!is_font_file(Path::new("image.png")));
    assert!(!is_font_file(Path::new("no_extension")));
}
