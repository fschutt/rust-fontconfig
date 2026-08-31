//! Regression tests for GitHub issue #26:
//! "Chosen unicode fallback fonts make no sense and are terrible
//!  (e.g. unifont replacing proper fonts)".
//!
//! Fully hermetic: no system fonts, no `parsing` feature. Three mock fonts
//! shaped like a Windows box are registered with explicit coverage, and every
//! test asserts WHICH font the chain picks when more than one candidate covers
//! a script — the competition no earlier test set up.
//!
//! Before 5.0 the chain ranked fallback candidates by breadth of coverage and
//! greedily set-covered the requested scripts, so the pan-Unicode bitmap font
//! won every script it happened to include, and the per-script OS tables were
//! never consulted because the scripts hint was dropped before expansion.

use rust_fontconfig::{
    FcFallbackConfig, FcFont, FcFontCache, FcPattern, FcScriptFallback, FcWeight, FontFallbackChain,
    FontId, GenericFamily, OperatingSystem, PatternMatch, UnicodeRange,
    DEFAULT_UNICODE_FALLBACK_SCRIPTS,
};

const fn r(start: u32, end: u32) -> UnicodeRange {
    UnicodeRange { start, end }
}

const BASIC_LATIN: UnicodeRange = r(0x0000, 0x007F);
const LATIN_1: UnicodeRange = r(0x0080, 0x00FF);
const GREEK: UnicodeRange = r(0x0370, 0x03FF);
const CYRILLIC: UnicodeRange = r(0x0400, 0x04FF);
const HEBREW: UnicodeRange = r(0x0590, 0x05FF);
const ARABIC: UnicodeRange = r(0x0600, 0x06FF);
const CJK_SYMBOLS: UnicodeRange = r(0x3000, 0x303F);
const HIRAGANA: UnicodeRange = r(0x3040, 0x309F);
const KATAKANA: UnicodeRange = r(0x30A0, 0x30FF);
const CJK_UNIFIED: UnicodeRange = r(0x4E00, 0x9FFF);
const HALF_FULL_WIDTH: UnicodeRange = r(0xFF00, 0xFFEF);

fn mock(family: &str, ranges: &[UnicodeRange]) -> (FcPattern, FcFont) {
    let pattern = FcPattern {
        name: Some(family.to_string()),
        family: Some(family.to_string()),
        // Non-empty ranges are stored verbatim (no cmap parsing), so the
        // font bytes are never looked at.
        unicode_ranges: ranges.to_vec(),
        ..Default::default()
    };
    let font = FcFont {
        bytes: vec![0, 1, 2, 3],
        font_index: 0,
        id: family.to_string(),
    };
    (pattern, font)
}

/// A cache that looks like a stock Windows install that also has GNU Unifont,
/// resolving with `config`.
///
/// Coverage mirrors the real fonts at block granularity:
/// - Segoe UI: Latin, Greek, Cyrillic, Hebrew, Arabic — no CJK.
/// - MS Gothic: Latin, Greek, Cyrillic (JIS X 0208 includes both), kana,
///   CJK ideographs — the font Windows itself uses for Japanese.
/// - Unifont: the entire BMP. Legitimately covers everything, badly.
fn windows_like_cache(config: FcFallbackConfig) -> FcFontCache {
    let cache = FcFontCache::default().with_fallback_config(config);
    cache.with_memory_fonts(vec![
        mock("Segoe UI", &[BASIC_LATIN, LATIN_1, GREEK, CYRILLIC, HEBREW, ARABIC]),
        mock(
            "MS Gothic",
            &[
                BASIC_LATIN,
                LATIN_1,
                GREEK,
                CYRILLIC,
                CJK_SYMBOLS,
                HIRAGANA,
                KATAKANA,
                CJK_UNIFIED,
                HALF_FULL_WIDTH,
            ],
        ),
        mock("Unifont", &[r(0x0000, 0xFFFF)]),
    ]);
    cache
}

fn family_of(cache: &FcFontCache, id: FontId) -> String {
    cache
        .get_metadata_by_id(&id)
        .and_then(|m| m.family)
        .unwrap_or_else(|| format!("<unknown {id}>"))
}

fn chain(cache: &FcFontCache, stack: &[&str], scripts: Option<&[UnicodeRange]>) -> FontFallbackChain {
    let stack: Vec<String> = stack.iter().map(|s| s.to_string()).collect();
    cache.resolve_font_chain_with_scripts(
        &stack,
        FcWeight::Normal,
        PatternMatch::DontCare,
        PatternMatch::DontCare,
        scripts,
        &mut Vec::new(),
    )
}

/// `(family, tier)` the chain resolves `ch` to.
fn resolved(cache: &FcFontCache, chain: &FontFallbackChain, ch: char) -> (String, String) {
    match chain.resolve_char(cache, ch) {
        Some((id, source)) => (family_of(cache, id), source),
        None => ("<none>".to_string(), String::new()),
    }
}

fn family(cache: &FcFontCache, chain: &FontFallbackChain, ch: char) -> String {
    resolved(cache, chain, ch).0
}

/// The reporter's exact scenario: `resolve_font_chain(["sans-serif"])` on
/// Windows with the default seven fallback scripts. Japanese must go to the
/// Japanese font, not to the pan-Unicode bitmap font — and it does so through
/// the generic's per-script preferences, before any coverage ranking runs.
#[test]
fn sans_serif_with_windows_defaults_prefers_ms_gothic_over_unifont_for_japanese() {
    let cache = windows_like_cache(FcFallbackConfig::os_defaults(OperatingSystem::Windows));
    let chain = chain(&cache, &["sans-serif"], None);

    assert_eq!(resolved(&cache, &chain, 'A'), ("Segoe UI".into(), "sans-serif".into()));
    assert_eq!(resolved(&cache, &chain, 'Я'), ("Segoe UI".into(), "sans-serif".into()));
    assert_eq!(resolved(&cache, &chain, 'ب'), ("Segoe UI".into(), "sans-serif".into()));
    assert_eq!(resolved(&cache, &chain, 'あ'), ("MS Gothic".into(), "sans-serif".into()), "{chain:#?}");
    assert_eq!(resolved(&cache, &chain, '漢'), ("MS Gothic".into(), "sans-serif".into()), "{chain:#?}");

    let sans = &chain.css_fallbacks[0];
    let kana = sans
        .script_fonts
        .iter()
        .find(|g| g.range == HIRAGANA)
        .expect("the sans-serif group carries a Hiragana preference group");
    assert_eq!(family_of(&cache, kana.fonts[0].id), "MS Gothic");
    assert!(
        chain.unicode_fallbacks.iter().all(|g| g.fonts.iter().all(|m| family_of(&cache, m.id) != "MS Gothic")),
        "a font already in the CSS tier is not repeated in the script tier: {chain:#?}"
    );
}

/// No configuration at all: the coverage-ranked tier still prefers the font
/// dedicated to the script over the one that covers everything, because
/// dedication and narrowness break ties — breadth never wins.
#[test]
fn without_any_configuration_the_dedicated_font_beats_the_pan_unicode_font() {
    let cache = windows_like_cache(FcFallbackConfig::empty());
    let chain = chain(&cache, &["Segoe UI"], None);

    assert_eq!(resolved(&cache, &chain, 'A'), ("Segoe UI".into(), "Segoe UI".into()));
    assert_eq!(
        resolved(&cache, &chain, 'あ'),
        ("MS Gothic".into(), "(unicode-fallback)".into()),
        "{chain:#?}"
    );
    assert_eq!(family(&cache, &chain, '漢'), "MS Gothic", "{chain:#?}");

    let hiragana = chain
        .unicode_fallbacks
        .iter()
        .find(|g| g.range == HIRAGANA)
        .expect("a Hiragana fallback group");
    let order: Vec<String> = hiragana.fonts.iter().map(|m| family_of(&cache, m.id)).collect();
    assert_eq!(order, vec!["MS Gothic".to_string(), "Unifont".to_string()]);
}

/// An unconfigured generic on a headless cache resolves to *something*, and
/// among style ties the narrower font comes first, so plain ASCII does not
/// land on the widest font.
#[test]
fn unconfigured_generic_resolves_narrow_before_wide() {
    let cache = windows_like_cache(FcFallbackConfig::empty());
    let chain = chain(&cache, &["sans-serif"], None);

    let base: Vec<String> = chain.css_fallbacks[0].fonts.iter().map(|m| family_of(&cache, m.id)).collect();
    assert_eq!(base, vec!["Segoe UI".to_string(), "MS Gothic".to_string(), "Unifont".to_string()]);
    assert_eq!(family(&cache, &chain, 'A'), "Segoe UI");
    assert_eq!(family(&cache, &chain, 'あ'), "MS Gothic");
}

/// `Some(&[])` builds no script tier, and without a last resort an
/// uncovered character honestly resolves to no font.
#[test]
fn ascii_only_hint_builds_no_script_tier_and_reports_no_font() {
    let cache = windows_like_cache(FcFallbackConfig::os_defaults(OperatingSystem::Windows));
    let chain = chain(&cache, &["Segoe UI"], Some(&[]));

    assert!(chain.unicode_fallbacks.is_empty(), "{chain:#?}");
    assert!(chain.last_resort.is_empty());
    assert_eq!(chain.resolve_char(&cache, 'あ'), None);
    assert_eq!(family(&cache, &chain, 'A'), "Segoe UI");

    let runs = chain.query_for_text(&cache, "Aあ");
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1].font_id, None);
}

/// A configured per-script preference wins over the ranking, even when it
/// prefers the font the ranking would put last.
#[test]
fn configured_script_preference_beats_the_ranking() {
    let mut config = FcFallbackConfig::empty();
    config.generic_families.insert(GenericFamily::SansSerif, vec!["Segoe UI".to_string()]);
    config.script_fallbacks.push(FcScriptFallback {
        range: HIRAGANA,
        generic: Some(GenericFamily::SansSerif),
        families: vec!["Unifont".to_string()],
    });
    let cache = windows_like_cache(config);
    let chain = chain(&cache, &["sans-serif"], None);

    assert_eq!(resolved(&cache, &chain, 'あ'), ("Unifont".into(), "sans-serif".into()), "{chain:#?}");
    // Katakana has no preference: the ranking picks the dedicated font.
    assert_eq!(resolved(&cache, &chain, 'ア'), ("MS Gothic".into(), "(unicode-fallback)".into()), "{chain:#?}");
}

/// A stack that names no generic takes its per-script preferences from the
/// configured default generic (fontconfig appends `sans-serif` to every
/// pattern without one).
#[test]
fn stack_without_a_generic_uses_the_default_generic_preferences() {
    let mut config = FcFallbackConfig::empty();
    config.script_fallbacks.push(FcScriptFallback {
        range: HIRAGANA,
        generic: Some(GenericFamily::SansSerif),
        families: vec!["Unifont".to_string()],
    });
    let cache = windows_like_cache(config);
    let chain = chain(&cache, &["Segoe UI"], None);

    assert_eq!(resolved(&cache, &chain, 'あ'), ("Unifont".into(), "(unicode-fallback)".into()), "{chain:#?}");
}

/// The last resort is used for any character nothing else covers — without
/// a coverage check, because its purpose is to draw `.notdef`.
#[test]
fn last_resort_is_used_without_a_coverage_check() {
    let mut config = FcFallbackConfig::empty();
    config.last_resort = vec!["Segoe UI".to_string()];
    let cache = windows_like_cache(config);
    let chain = chain(&cache, &["Segoe UI"], Some(&[]));

    assert_eq!(resolved(&cache, &chain, 'あ'), ("Segoe UI".into(), "(last-resort)".into()));
    assert_eq!(resolved(&cache, &chain, 'A'), ("Segoe UI".into(), "Segoe UI".into()));
}

/// A named family that is not installed resolves to its configured
/// substitutions, reported under the requested name.
#[test]
fn substitutions_stand_in_for_missing_named_families() {
    let mut config = FcFallbackConfig::empty();
    config.substitutions.insert("arial".to_string(), vec!["Segoe UI".to_string()]);
    let cache = windows_like_cache(config);
    let chain = chain(&cache, &["Arial"], Some(&[]));

    assert_eq!(resolved(&cache, &chain, 'A'), ("Segoe UI".into(), "Arial".into()), "{chain:#?}");
}

/// Replacing the configuration drops memoized chains.
#[test]
fn changing_the_configuration_invalidates_memoized_chains() {
    let cache = windows_like_cache(FcFallbackConfig::os_defaults(OperatingSystem::Windows));
    let before = chain(&cache, &["sans-serif"], None);
    assert_eq!(family(&cache, &before, 'あ'), "MS Gothic");

    let mut config = FcFallbackConfig::os_defaults(OperatingSystem::Windows);
    config.script_fallbacks.insert(
        0,
        FcScriptFallback {
            range: HIRAGANA,
            generic: Some(GenericFamily::SansSerif),
            families: vec!["Unifont".to_string()],
        },
    );
    cache.set_fallback_config(config);

    let after = chain(&cache, &["sans-serif"], None);
    assert_eq!(family(&cache, &after, 'あ'), "Unifont", "{after:#?}");
}

/// What the async registry parses ahead of time is exactly what the chain
/// builder can look up: base candidates, per-script preferences, last resort.
#[test]
fn registry_prefetch_list_covers_the_chain() {
    let mut config = FcFallbackConfig::os_defaults(OperatingSystem::Windows);
    config.last_resort = vec!["Unifont".to_string()];
    let families = config.candidate_families(&["sans-serif".to_string()], DEFAULT_UNICODE_FALLBACK_SCRIPTS);

    for expected in ["Segoe UI", "MS Gothic", "Malgun Gothic", "Segoe UI Arabic", "Unifont"] {
        assert!(families.iter().any(|f| f == expected), "missing {expected} in {families:?}");
    }
    let cache = windows_like_cache(config);
    let chain = chain(&cache, &["sans-serif"], None);
    for m in chain.fonts() {
        let name = family_of(&cache, m.id);
        assert!(families.iter().any(|f| f.eq_ignore_ascii_case(&name)), "{name} resolved but was not prefetched");
    }
}
