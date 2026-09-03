//! The desktop font preference API: parsing what a desktop stores, and
//! putting the result into a fallback configuration.

use rust_fontconfig::{
    FcDesktopFont, FcFallbackConfig, FcFontCache, FcWeight, GenericFamily, OperatingSystem,
};

// --- Pango / GNOME ----------------------------------------------------------

#[test]
fn a_plain_family_and_size_is_the_common_case() {
    let f = FcDesktopFont::parse("Cantarell 11").unwrap();
    assert_eq!(f.family, "Cantarell");
    assert_eq!(f.weight, FcWeight::Normal);
    assert!(!f.italic);
    assert_eq!(f.size_pt, Some(11.0));
}

#[test]
fn the_quotes_gsettings_prints_are_stripped() {
    // `gsettings get` writes the value as a quoted GVariant string, with a
    // trailing newline.
    for raw in [
        "'DejaVu Sans 11'\n",
        "\"DejaVu Sans 11\"",
        "  DejaVu Sans 11 ",
    ] {
        let f = FcDesktopFont::parse(raw).unwrap();
        assert_eq!(f.family, "DejaVu Sans", "for {raw:?}");
        assert_eq!(f.size_pt, Some(11.0), "for {raw:?}");
    }
}

#[test]
fn style_keywords_are_read_and_kept_out_of_the_family() {
    let f = FcDesktopFont::parse("Noto Sans Semi-Bold Italic 12").unwrap();
    assert_eq!(f.family, "Noto Sans");
    assert_eq!(f.weight, FcWeight::SemiBold);
    assert!(f.italic);
    assert_eq!(f.size_pt, Some(12.0));

    let f = FcDesktopFont::parse("Cantarell Oblique").unwrap();
    assert_eq!(f.family, "Cantarell");
    assert!(f.oblique);
    assert!(!f.italic);
    assert_eq!(f.size_pt, None);

    // Stretch and variant are recognized so they cannot leak into the family,
    // even though they are not reported.
    let f = FcDesktopFont::parse("Ubuntu Condensed Small-Caps 10").unwrap();
    assert_eq!(f.family, "Ubuntu");
}

#[test]
fn a_numeric_weight_is_read() {
    let f = FcDesktopFont::parse("Inter Weight=380 11").unwrap();
    assert_eq!(f.family, "Inter");
    assert_eq!(f.weight, FcWeight::Normal); // 380 rounds into Normal's band
    let f = FcDesktopFont::parse("Inter Weight=800 11").unwrap();
    assert_eq!(f.weight, FcWeight::ExtraBold);
}

#[test]
fn a_family_that_ends_in_a_keyword_survives_when_nothing_follows() {
    // Peeling stops while one token is left, so the family wins the tie —
    // the same ambiguity Pango itself has.
    let f = FcDesktopFont::parse("Book Antiqua 11").unwrap();
    assert_eq!(f.family, "Book Antiqua");
    assert_eq!(f.weight, FcWeight::Normal);

    let f = FcDesktopFont::parse("Black 11").unwrap();
    assert_eq!(f.family, "Black");
}

#[test]
fn a_family_that_ends_in_digits_keeps_them() {
    // The bug in the usual `trim_end_matches(char::is_numeric)` approach:
    // only a whitespace-separated trailing token is a size.
    let f = FcDesktopFont::parse("M+ 1c 11").unwrap();
    assert_eq!(f.family, "M+ 1c");
    assert_eq!(f.size_pt, Some(11.0));

    let f = FcDesktopFont::parse("Iosevka Term SS08").unwrap();
    assert_eq!(f.family, "Iosevka Term SS08");
    assert_eq!(f.size_pt, None);
}

#[test]
fn a_family_list_keeps_every_entry() {
    let f = FcDesktopFont::parse("Cantarell, DejaVu Sans, sans-serif 11").unwrap();
    assert_eq!(f.family, "Cantarell");
    assert_eq!(f.families, ["Cantarell", "DejaVu Sans", "sans-serif"]);
}

#[test]
fn variations_and_pixel_sizes_are_stripped() {
    let f = FcDesktopFont::parse("Inter 11 @wght=700").unwrap();
    assert_eq!(f.family, "Inter");

    // A `px` size is absolute, not points, so it is dropped rather than
    // reported as if it were points.
    let f = FcDesktopFont::parse("Cantarell 16px").unwrap();
    assert_eq!(f.family, "Cantarell");
    assert_eq!(f.size_pt, None);
}

#[test]
fn nothing_usable_is_none() {
    assert!(FcDesktopFont::parse("").is_none());
    assert!(FcDesktopFont::parse("   ").is_none());
    assert!(FcDesktopFont::parse("''").is_none());
}

// --- Qt / KDE ---------------------------------------------------------------

#[test]
fn the_kdeglobals_format_is_read() {
    let f = FcDesktopFont::parse_qt("Noto Sans,10,-1,5,50,0,0,0,0,0").unwrap();
    assert_eq!(f.family, "Noto Sans");
    assert_eq!(f.weight, FcWeight::Normal);
    assert!(!f.italic);
    assert_eq!(f.size_pt, Some(10.0));
}

#[test]
fn qt_legacy_and_css_weight_scales_are_told_apart() {
    // 75 on Qt's 0-99 scale is Bold.
    let f = FcDesktopFont::parse_qt("Noto Sans,10,-1,5,75,1,0,0,0,0").unwrap();
    assert_eq!(f.weight, FcWeight::Bold);
    assert!(f.italic);

    // 700 is already the CSS scale.
    let f = FcDesktopFont::parse_qt("Noto Sans,10,-1,5,700,0,0,0,0,0").unwrap();
    assert_eq!(f.weight, FcWeight::Bold);
}

#[test]
fn a_qt_description_may_be_short() {
    let f = FcDesktopFont::parse_qt("Hack,9").unwrap();
    assert_eq!(f.family, "Hack");
    assert_eq!(f.size_pt, Some(9.0));
    assert_eq!(f.weight, FcWeight::Normal);

    assert!(FcDesktopFont::parse_qt("").is_none());
    assert!(FcDesktopFont::parse_qt(",10,-1").is_none());
}

// --- Putting it into a configuration ----------------------------------------

#[test]
fn prefer_puts_the_family_first_and_keeps_the_rest() {
    let mut config = FcFallbackConfig::os_defaults(OperatingSystem::Linux);
    let before = config.generic_candidates(GenericFamily::SansSerif).to_vec();
    assert!(before.len() > 1);

    config.prefer(GenericFamily::SansSerif, "Cantarell");
    let after = config.generic_candidates(GenericFamily::SansSerif);
    assert_eq!(after[0], "Cantarell");
    assert_eq!(&after[1..], &before[..]);
}

#[test]
fn prefer_moves_an_existing_entry_instead_of_duplicating_it() {
    let mut config = FcFallbackConfig::os_defaults(OperatingSystem::Linux);
    config.prefer(GenericFamily::SansSerif, "dejavu sans"); // different case
    let after = config.generic_candidates(GenericFamily::SansSerif);
    assert_eq!(after[0], "dejavu sans");
    assert_eq!(
        after
            .iter()
            .filter(|f| f.eq_ignore_ascii_case("DejaVu Sans"))
            .count(),
        1
    );
}

#[test]
fn preferring_for_an_inheriting_generic_copies_the_inherited_list_in_behind() {
    let mut config = FcFallbackConfig::os_defaults(OperatingSystem::Linux);
    // Linux has no system-ui list of its own; it inherits sans-serif's.
    let inherited = config.generic_candidates(GenericFamily::SystemUi).to_vec();
    assert_eq!(
        inherited,
        config.generic_candidates(GenericFamily::SansSerif)
    );

    config.prefer(GenericFamily::SystemUi, "Cantarell");
    let after = config.generic_candidates(GenericFamily::SystemUi);
    assert_eq!(after[0], "Cantarell");
    assert_eq!(&after[1..], &inherited[..]);
    // sans-serif is untouched.
    assert_ne!(
        config.generic_candidates(GenericFamily::SansSerif)[0],
        "Cantarell"
    );
}

#[test]
fn set_generic_replaces_the_list() {
    let mut config = FcFallbackConfig::os_defaults(OperatingSystem::Linux);
    config.set_generic(GenericFamily::SansSerif, vec!["Only This".to_string()]);
    assert_eq!(
        config.generic_candidates(GenericFamily::SansSerif),
        ["Only This".to_string()]
    );
}

#[test]
fn prefer_for_covers_every_named_generic() {
    let ui = FcDesktopFont::parse("'Cantarell 11'").unwrap();
    let mut config = FcFallbackConfig::os_defaults(OperatingSystem::Linux);
    config.prefer_for(
        &[GenericFamily::SystemUi, GenericFamily::UiSansSerif],
        ui.family,
    );

    for g in [GenericFamily::SystemUi, GenericFamily::UiSansSerif] {
        assert_eq!(config.generic_candidates(g)[0], "Cantarell");
    }
    // The document generic is deliberately untouched.
    assert_ne!(
        config.generic_candidates(GenericFamily::SansSerif)[0],
        "Cantarell"
    );
}

#[test]
fn modify_fallback_config_edits_the_live_cache() {
    let cache = FcFontCache::default();
    cache.set_fallback_config(FcFallbackConfig::os_defaults(OperatingSystem::Linux));

    cache.modify_fallback_config(|c| {
        c.prefer_for(
            &[GenericFamily::SystemUi, GenericFamily::UiSansSerif],
            "Cantarell",
        );
    });

    let config = cache.fallback_config();
    assert_eq!(
        config.generic_candidates(GenericFamily::SystemUi)[0],
        "Cantarell"
    );
    assert_eq!(
        config.generic_candidates(GenericFamily::UiSansSerif)[0],
        "Cantarell"
    );
}

#[test]
fn a_kdeglobals_file_is_read_section_by_section() {
    let text = "\
[ColorEffects:Disabled]
font=Not This,10,-1,5,50,0,0,0,0,0

[General]
XftAntialias=true
font=Noto Sans,10,-1,5,50,0,0,0,0,0
fixed=Hack,9,-1,5,50,0,0,0,0,0
menuFont=Noto Sans,10,-1,5,50,0,0,0,0,0
";
    let fonts = rust_fontconfig::FcDesktopFonts::from_kdeglobals_str(text);
    assert_eq!(fonts.ui.as_ref().unwrap().family, "Noto Sans");
    assert_eq!(fonts.monospace.as_ref().unwrap().family, "Hack");
    assert_eq!(fonts.monospace.as_ref().unwrap().size_pt, Some(9.0));
    // No `document-font-name` equivalent in kdeglobals.
    assert!(fonts.document.is_none());
}

#[test]
fn an_empty_kdeglobals_yields_nothing() {
    let fonts =
        rust_fontconfig::FcDesktopFonts::from_kdeglobals_str("[General]\nXftHinting=true\n");
    assert_eq!(fonts, rust_fontconfig::FcDesktopFonts::default());
}
