use alloc::string::String;

/// Known font file extensions (lowercase).
pub const FONT_EXTENSIONS: &[&str] = &["ttf", "otf", "ttc", "woff", "woff2", "dfont"];

/// Normalize a family/font name for comparison: lowercase, strip all non-alphanumeric characters.
///
/// This ensures consistent matching regardless of spaces, hyphens, underscores, or casing.
pub fn normalize_family_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Check if a file has a recognized font extension.
#[cfg(feature = "std")]
pub fn is_font_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| {
            let lower = ext.to_lowercase();
            FONT_EXTENSIONS.contains(&lower.as_str())
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
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
}
