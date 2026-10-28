#[cfg(test)]
mod tests {
    use crate::config::*;
    use crate::OperatingSystem;
    use std::path::Path;

    /// Verifies that generic CSS font families like "sans-serif" are correctly parsed,
    /// including case-insensitive checks.
    #[test]
    fn generic_families_recognized() {
        assert!(is_generic_family("sans-serif"));
        assert!(is_generic_family("Sans-Serif")); // case-insensitive
        assert!(is_generic_family("monospace"));
        assert!(is_generic_family("SERIF"));
        assert!(!is_generic_family("Arial"));
        assert!(!is_generic_family("Noto Sans"));
    }

    /// Ensures all common font style tokens (e.g. Bold, Italic) are present in the ignore list.
    #[test]
    fn font_style_tokens_covers_common_styles() {
        for token in &[
            "Regular", "Bold", "Italic", "Light", "Medium", "Thin", "Black", "Oblique", "SemiBold",
        ] {
            assert!(
                FONT_STYLE_TOKENS.contains(token),
                "missing style token: {}",
                token
            );
        }
    }

    /// Verifies that desktop operating systems return a non-empty list of priority font families.
    #[test]
    fn common_font_families_nonempty_for_desktop() {
        assert!(
            !crate::config::FcScanConfig::os_defaults(OperatingSystem::MacOS)
                .priority_families
                .is_empty()
        );
        assert!(
            !crate::config::FcScanConfig::os_defaults(OperatingSystem::Linux)
                .priority_families
                .is_empty()
        );
        assert!(
            !crate::config::FcScanConfig::os_defaults(OperatingSystem::Windows)
                .priority_families
                .is_empty()
        );
        assert!(
            crate::config::FcScanConfig::os_defaults(OperatingSystem::Wasm)
                .priority_families
                .is_empty()
        );
    }

    /// Checks that the OS-specific fallback configuration correctly maps to the expected
    /// hardcoded system font directories and priority families.
    #[test]
    fn os_defaults_carry_the_legacy_tables() {
        for os in [
            OperatingSystem::Linux,
            OperatingSystem::Windows,
            OperatingSystem::MacOS,
        ] {
            let config = FcScanConfig::os_defaults(os);
            assert!(!config.font_dirs.is_empty(), "no dirs for {:?}", os);
            assert!(
                !config.priority_families.is_empty(),
                "no families for {:?}",
                os
            );
            assert_eq!(config.font_dirs, font_directories(os));
        }
    }

    /// Ensures an empty configuration initializes cleanly with no default fallback data.
    #[test]
    fn empty_scan_config_scans_and_prioritizes_nothing() {
        let config = FcScanConfig::empty();
        assert!(config.font_dirs.is_empty());
        assert!(config.priority_families.is_empty());
        assert!(config.priority_token_sets().is_empty());
    }

    /// Validates that common style suffixes are stripped out when guessing font families from filenames.
    #[test]
    fn guess_family_strips_style_suffixes() {
        assert_eq!(
            guess_family_from_filename(Path::new("ArialBold.ttf")),
            "arial"
        );
        assert_eq!(
            guess_family_from_filename(Path::new("NotoSansJP-Regular.otf")),
            "notosansjp"
        );
        assert_eq!(
            guess_family_from_filename(Path::new("Helvetica Neue Bold Italic.ttf")),
            "helveticaneue"
        );
    }

    /// Validates that underscore delimiters are stripped out when guessing font families from filenames.
    #[test]
    fn guess_family_handles_underscores() {
        assert_eq!(
            guess_family_from_filename(Path::new("Liberation_Sans_Bold.ttf")),
            "liberationsans"
        );
    }

    /// Validates that compound hyphenated style suffixes are correctly stripped.
    #[test]
    fn guess_family_handles_compound_styles() {
        assert_eq!(
            guess_family_from_filename(Path::new("LiberationSans-BoldItalic.ttf")),
            "liberationsans"
        );
        assert_eq!(
            guess_family_from_filename(Path::new("DejaVuSansMono-ExtraBold.ttf")),
            "dejavusansmono"
        );
        assert_eq!(
            guess_family_from_filename(Path::new("SFMono-SemiBold.otf")),
            "sfmono"
        );
    }

    /// Checks that macOS UI and standard fonts successfully tokenize and match against the common fallback list.
    #[test]
    fn matches_common_family_macos() {
        let common = FcScanConfig::os_defaults(OperatingSystem::MacOS).priority_token_sets();
        // "SFNSDisplay" → tokens ["sfns", "display"] → matches "SFNS"
        let tokens = tokenize_all("SFNSDisplay");
        assert!(matches_common_family_tokens(&tokens, &common));
        // "HelveticaNeue" → tokens ["helvetica", "neue"] → matches "Helvetica Neue"
        let tokens = tokenize_all("HelveticaNeue");
        assert!(matches_common_family_tokens(&tokens, &common));
        // "Helvetica" → matches "Helvetica"
        let tokens = tokenize_all("Helvetica");
        assert!(matches_common_family_tokens(&tokens, &common));
        // "SomeRandomFont" → no match
        let tokens = tokenize_all("SomeRandomFont");
        assert!(!matches_common_family_tokens(&tokens, &common));
    }

    /// Checks that Linux standard fonts successfully tokenize and match against the common fallback list.
    #[test]
    fn matches_common_family_linux() {
        let common = FcScanConfig::os_defaults(OperatingSystem::Linux).priority_token_sets();
        let tokens = tokenize_all("DejaVuSans");
        assert!(matches_common_family_tokens(&tokens, &common));
        let tokens = tokenize_all("NotoSansCJK");
        assert!(matches_common_family_tokens(&tokens, &common));
        let tokens = tokenize_all("UbuntuMono-Regular");
        assert!(matches_common_family_tokens(&tokens, &common));
    }

    /// Checks that Windows standard fonts successfully tokenize and match against the common fallback list.
    #[test]
    fn matches_common_family_windows() {
        let common = FcScanConfig::os_defaults(OperatingSystem::Windows).priority_token_sets();
        let tokens = tokenize_all("SegoeUI-Regular");
        assert!(matches_common_family_tokens(&tokens, &common));
        let tokens = tokenize_all("Consolas");
        assert!(matches_common_family_tokens(&tokens, &common));
    }

    /// Validates that filename stems correctly drop recognized style tokens (e.g. "Bold") but keep family tokens intact.
    #[test]
    fn tokenize_font_stem_filters_styles() {
        assert_eq!(tokenize_font_stem("ArialBold"), vec!["arial"]);
        assert_eq!(
            tokenize_font_stem("NotoSansJP-Regular"),
            vec!["noto", "sans", "jp"]
        );
        // "SFMono" stays as one token (consecutive uppercase → no CamelCase split)
        assert_eq!(tokenize_font_stem("SFMono-SemiBold"), vec!["sfmono"]);
    }

    /// Helper: tokenize a stem into all lowercase tokens (including style tokens).
    fn tokenize_all(stem: &str) -> Vec<String> {
        tokenize_lowercase(stem)
    }

    /// Apple's Geeza Pro (Arabic), Thonburi (Thai) and Devanagari MT shape
    /// only through AAT `morx` tables, which an OpenType shaper does not read:
    /// a page that names no font of its own came out with unjoined Arabic and
    /// unstacked Thai marks. The script preferences try a face with OpenType
    /// GSUB for the script first and keep an AAT-only face last, for its
    /// coverage.
    #[test]
    fn apple_script_fallbacks_try_opentype_faces_before_aat_only_ones() {
        let devanagari = crate::UnicodeRange {
            start: 0x0900,
            end: 0x097F,
        };
        for os in [OperatingSystem::MacOS, OperatingSystem::IOS] {
            let config = FcFallbackConfig::os_defaults(os);
            for (generic, block, script, opentype) in [
                (GenericFamily::SansSerif, blocks::ARABIC, "Arabic", "Arial"),
                (GenericFamily::Serif, blocks::ARABIC, "Arabic", "Times New Roman"),
                (GenericFamily::SansSerif, blocks::THAI, "Thai", "Sukhumvit Set"),
                (GenericFamily::SansSerif, devanagari, "Devanagari", "Kohinoor Devanagari"),
            ] {
                let candidates = config.script_candidates(Some(generic), &block);
                assert_eq!(
                    candidates.first().map(String::as_str),
                    Some(opentype),
                    "{os:?} {generic:?} {script} should try {opentype}, which carries OpenType \
                     GSUB for {script}, first: {candidates:?}"
                );
            }
            for (generic, block, aat_only) in [
                (GenericFamily::SansSerif, blocks::ARABIC, "Geeza Pro"),
                (GenericFamily::Serif, blocks::ARABIC, "Geeza Pro"),
                (GenericFamily::SansSerif, blocks::THAI, "Thonburi"),
            ] {
                let candidates = config.script_candidates(Some(generic), &block);
                assert_eq!(
                    candidates.last().map(String::as_str),
                    Some(aat_only),
                    "{os:?} {generic:?} should keep the AAT-only {aat_only} last, for its \
                     coverage: {candidates:?}"
                );
            }
        }
    }
}
