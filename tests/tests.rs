use rust_fontconfig::*;

#[test]
fn test_zwj_emoji_support() {
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
        name: Some("Emoji Font".to_string()),
        family: Some("Emoji Family".to_string()),
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
        name: Some("Text Font".to_string()),
        family: Some("Text Family".to_string()),
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
        cache.list().iter().find(|(p, _)| p.name == Some("Emoji Font".to_string())).unwrap().1
    );
}
