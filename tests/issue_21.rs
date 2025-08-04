use rust_fontconfig::*;

#[test]
fn test_emoji_font_matching() {
    // Create mock fonts
    let emoji_font = FcFont {
        bytes: vec![0; 100], // Dummy data
        font_index: 0,
        id: "emoji-font".to_string(),
    };

    let text_font = FcFont {
        bytes: vec![1; 100],
        font_index: 0,
        id: "text-font".to_string(),
    };

    // Create patterns
    let emoji_pattern = FcPattern {
        name: Some("Noto Color Emoji".to_string()),
        family: Some("Noto Color Emoji".to_string()),
        codepoints: vec![
            0x1F469, // WOMAN
            0x1F3FF, // EMOJI MODIFIER FITZPATRICK TYPE-6
            0x200D,  // ZERO WIDTH JOINER
            0x1F393, // GRADUATION CAP
            0x1F986, // DUCK
        ],
        ..Default::default()
    };

    let text_pattern = FcPattern {
        name: Some("Droid Sans Japanese".to_string()),
        family: Some("Droid Sans Japanese".to_string()),
        unicode_ranges: vec![UnicodeRange { start: 0, end: 0x10FFFF }], // Full range for simplicity
        ..Default::default()
    };

    // Create font cache
    let mut cache = FcFontCache::default();
    cache.with_memory_fonts(vec![
        (emoji_pattern.clone(), emoji_font),
        (text_pattern.clone(), text_font),
    ]);

    // Test query for ZWJ emoji
    let mut trace = Vec::new();
    let text = "👩🏿‍🎓🦆";
    let mut pattern = FcPattern::default();
    let matches = cache.query_for_text(&mut pattern, text, &mut trace);

    assert_eq!(matches.len(), 1, "Should find one font for the ZWJ emoji");
    assert_eq!(
        matches[0].id,
        cache.list().iter().find(|(p, _)| p.name == Some("Noto Color Emoji".to_string())).unwrap().1
    );
}

#[test]
fn test_mixed_language_font_matching() {
    // Create mock fonts
    let latin_font = FcFont {
        bytes: vec![0; 100],
        font_index: 0,
        id: "latin-font".to_string(),
    };

    let cjk_font = FcFont {
        bytes: vec![1; 100],
        font_index: 0,
        id: "cjk-font".to_string(),
    };

    // Create patterns
    let latin_pattern = FcPattern {
        name: Some("Latin Font".to_string()),
        family: Some("Latin Family".to_string()),
        unicode_ranges: vec![UnicodeRange { start: 0x0000, end: 0x007F }],
        ..Default::default()
    };

    let cjk_pattern = FcPattern {
        name: Some("CJK Font".to_string()),
        family: Some("CJK Family".to_string()),
        unicode_ranges: vec![UnicodeRange { start: 0x4E00, end: 0x9FFF }],
        ..Default::default()
    };

    // Create font cache
    let mut cache = FcFontCache::default();
    cache.with_memory_fonts(vec![
        (latin_pattern.clone(), latin_font),
        (cjk_pattern.clone(), cjk_font),
    ]);

    // Test query for mixed language string
    let mut trace = Vec::new();
    let text = "Hello 你好";
    let mut pattern = FcPattern::default();
    let matches = cache.query_for_text(&mut pattern, text, &mut trace);

    assert_eq!(matches.len(), 2, "Should find two fonts for the mixed language string");

    let latin_font_id = cache.list().iter().find(|(p, _)| p.name == Some("Latin Font".to_string())).unwrap().1;
    let cjk_font_id = cache.list().iter().find(|(p, _)| p.name == Some("CJK Font".to_string())).unwrap().1;

    assert!(matches.iter().any(|m| m.id == latin_font_id));
    assert!(matches.iter().any(|m| m.id == cjk_font_id));
}
