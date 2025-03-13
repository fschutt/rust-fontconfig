use rust_fontconfig::*;

#[test]
fn test_unicode_range_matching() {
    // Create mock fonts with different Unicode ranges
    let latin_font = FcFont {
        bytes: vec![0, 1, 2, 3], // Dummy data
        font_index: 0,
        id: "latin-font".to_string(),
    };

    let cyrillic_font = FcFont {
        bytes: vec![4, 5, 6, 7], // Dummy data
        font_index: 0,
        id: "cyrillic-font".to_string(),
    };

    let cjk_font = FcFont {
        bytes: vec![8, 9, 10, 11], // Dummy data
        font_index: 0,
        id: "cjk-font".to_string(),
    };

    // Create patterns with Unicode ranges
    let latin_pattern = FcPattern {
        name: Some("Latin Font".to_string()),
        family: Some("Latin Family".to_string()),
        unicode_ranges: vec![
            UnicodeRange {
                start: 0x0000,
                end: 0x007F,
            }, // Basic Latin
            UnicodeRange {
                start: 0x0080,
                end: 0x00FF,
            }, // Latin-1 Supplement
        ],
        ..Default::default()
    };

    let cyrillic_pattern = FcPattern {
        name: Some("Cyrillic Font".to_string()),
        family: Some("Cyrillic Family".to_string()),
        unicode_ranges: vec![
            UnicodeRange {
                start: 0x0400,
                end: 0x04FF,
            }, // Cyrillic
        ],
        ..Default::default()
    };

    let cjk_pattern = FcPattern {
        name: Some("CJK Font".to_string()),
        family: Some("CJK Family".to_string()),
        unicode_ranges: vec![
            UnicodeRange {
                start: 0x4E00,
                end: 0x9FFF,
            }, // CJK Unified Ideographs
        ],
        ..Default::default()
    };

    // Create the font cache with our mock fonts
    let mut cache = FcFontCache::default();
    cache.with_memory_fonts(vec![
        (latin_pattern.clone(), latin_font),
        (cyrillic_pattern.clone(), cyrillic_font),
        (cjk_pattern.clone(), cjk_font),
    ]);

    // Get font IDs for assertions
    let font_list = cache.list();
    let latin_id = font_list
        .iter()
        .find(|(pattern, _)| pattern.name == Some("Latin Font".to_string()))
        .map(|(_, id)| *id)
        .expect("Latin font not found");

    let cyrillic_id = font_list
        .iter()
        .find(|(pattern, _)| pattern.name == Some("Cyrillic Font".to_string()))
        .map(|(_, id)| *id)
        .expect("Cyrillic font not found");

    // Test querying with Unicode ranges
    let mut trace = Vec::new();

    // Query for Latin characters
    let latin_query = FcPattern {
        unicode_ranges: vec![UnicodeRange {
            start: 0x0041,
            end: 0x005A,
        }], // A-Z
        ..Default::default()
    };

    let matches = cache.query_all(&latin_query, &mut trace);
    assert_eq!(matches.len(), 1);
    assert_eq!(cache.get_memory_font(&latin_id).is_some(), true);

    // Check trace messages for non-matches (Unicode range mismatches)
    trace.clear();

    // Query for Cyrillic characters
    let cyrillic_query = FcPattern {
        unicode_ranges: vec![UnicodeRange {
            start: 0x0410,
            end: 0x044F,
        }], // Cyrillic letters
        ..Default::default()
    };

    let matches = cache.query_all(&cyrillic_query, &mut trace);
    assert_eq!(matches.len(), 1);
    assert_eq!(cache.get_memory_font(&cyrillic_id).is_some(), true);

    // Check trace messages for non-matches (Unicode range mismatches)
    let range_mismatch_traces = trace
        .iter()
        .filter(|msg| matches!(msg.reason, MatchReason::UnicodeRangeMismatch { .. }))
        .count();
    assert!(
        range_mismatch_traces > 0,
        "Expected Unicode range mismatch traces"
    );

    trace.clear();

    // Query for text that needs multiple fonts
    let text = "Hello Привет 你好"; // Latin, Cyrillic, and CJK
    let matches = cache.query_for_text(&FcPattern::default(), text, &mut trace);
    assert_eq!(
        matches.len(),
        3,
        "Should match all three fonts for multilingual text"
    );
}

#[test]
fn test_weight_matching() {
    // Create fonts with different weights
    let normal_font = FcFont {
        bytes: vec![0, 1, 2, 3],
        font_index: 0,
        id: "normal-font".to_string(),
    };

    let bold_font = FcFont {
        bytes: vec![4, 5, 6, 7],
        font_index: 0,
        id: "bold-font".to_string(),
    };

    // Create patterns
    let normal_pattern = FcPattern {
        name: Some("Normal Font".to_string()),
        family: Some("Test Family".to_string()),
        weight: FcWeight::Normal,
        ..Default::default()
    };

    let bold_pattern = FcPattern {
        name: Some("Bold Font".to_string()),
        family: Some("Test Family".to_string()),
        weight: FcWeight::Bold,
        bold: PatternMatch::True,
        ..Default::default()
    };

    // Create the font cache
    let mut cache = FcFontCache::default();
    cache.with_memory_fonts(vec![
        (normal_pattern.clone(), normal_font),
        (bold_pattern.clone(), bold_font),
    ]);

    // Test querying with weights
    let mut trace = Vec::new();

    // Query for normal weight
    let normal_query = FcPattern {
        family: Some("Test Family".to_string()),
        weight: FcWeight::Normal,
        ..Default::default()
    };

    let matches = cache.query(&normal_query, &mut trace);
    assert!(matches.is_some(), "Should match normal weight font");

    // Query for bold weight
    let bold_query = FcPattern {
        family: Some("Test Family".to_string()),
        weight: FcWeight::Bold,
        ..Default::default()
    };

    let matches = cache.query(&bold_query, &mut trace);
    assert!(matches.is_some(), "Should match bold weight font");

    // Query that doesn't match - wrong family
    trace.clear();
    let wrong_family_query = FcPattern {
        family: Some("Wrong Family".to_string()),
        weight: FcWeight::Normal,
        ..Default::default()
    };

    let matches = cache.query(&wrong_family_query, &mut trace);
    assert!(matches.is_none(), "Should not match with wrong family");

    // Check trace messages for family mismatch
    let family_mismatch_traces = trace
        .iter()
        .filter(|msg| matches!(msg.reason, MatchReason::FamilyMismatch { .. }))
        .count();
    assert!(
        family_mismatch_traces > 0,
        "Expected family mismatch trace messages"
    );

    // Query that doesn't match - weight mismatch
    trace.clear();
    let light_query = FcPattern {
        family: Some("Test Family".to_string()),
        weight: FcWeight::Light,
        ..Default::default()
    };

    let matches = cache.query(&light_query, &mut trace);
    assert!(matches.is_none(), "Should not match with weight mismatch");

    // Check trace messages for weight mismatch
    let weight_mismatch_traces = trace
        .iter()
        .filter(|msg| matches!(msg.reason, MatchReason::WeightMismatch { .. }))
        .count();
    assert!(
        weight_mismatch_traces > 0,
        "Expected weight mismatch trace messages"
    );

    // Test weight matching algorithm
    let available_weights = [FcWeight::Light, FcWeight::Normal, FcWeight::Bold];

    // When exact match exists
    assert_eq!(
        FcWeight::Normal.find_best_match(&available_weights),
        Some(FcWeight::Normal),
        "Should find exact match when available"
    );

    // When desired weight is less than 400
    assert_eq!(
        FcWeight::ExtraLight.find_best_match(&available_weights),
        Some(FcWeight::Light),
        "Should find closest lighter weight for weights < 400"
    );

    // When desired weight is greater than 500
    assert_eq!(
        FcWeight::ExtraBold.find_best_match(&available_weights),
        Some(FcWeight::Bold),
        "Should find closest heavier weight for weights > 500"
    );

    // For weight 400, try 500 first then lighter weights
    let available = [FcWeight::Light, FcWeight::Bold];
    assert_eq!(
        FcWeight::Normal.find_best_match(&available),
        Some(FcWeight::Light),
        "For weight 400, should prefer lightest weight when 500 unavailable"
    );

    // For weight 500, try 400 first then lighter weights
    let available = [FcWeight::Light, FcWeight::SemiBold];
    assert_eq!(
        FcWeight::Medium.find_best_match(&available),
        Some(FcWeight::Light),
        "For weight 500, should prefer 400 first"
    );
}

#[test]
fn test_trace_messages() {
    // Create a simple font cache with one font
    let test_font = FcFont {
        bytes: vec![0, 1, 2, 3],
        font_index: 0,
        id: "test-font".to_string(),
    };

    let test_pattern = FcPattern {
        name: Some("Test Font".to_string()),
        family: Some("Test Family".to_string()),
        italic: PatternMatch::False,
        monospace: PatternMatch::True,
        weight: FcWeight::Normal,
        stretch: FcStretch::Normal,
        unicode_ranges: vec![UnicodeRange {
            start: 0x0000,
            end: 0x007F,
        }],
        ..Default::default()
    };

    let mut cache = FcFontCache::default();
    cache.with_memory_fonts(vec![(test_pattern.clone(), test_font)]);

    // Test name mismatch
    let mut trace = Vec::new();
    let name_query = FcPattern {
        name: Some("Wrong Name".to_string()),
        ..Default::default()
    };

    let matches = cache.query(&name_query, &mut trace);
    assert!(matches.is_none(), "Should not match with wrong name");

    assert!(!trace.is_empty(), "Trace should not be empty");
    let name_mismatch = trace.iter().any(|msg| {
        if let MatchReason::NameMismatch { requested, found } = &msg.reason {
            requested.as_ref() == Some(&"Wrong Name".to_string())
                && found.as_ref() == Some(&"Test Font".to_string())
        } else {
            false
        }
    });
    assert!(name_mismatch, "Name mismatch trace message not found");

    // Test style mismatch
    trace.clear();
    let style_query = FcPattern {
        name: Some("Test Font".to_string()),
        italic: PatternMatch::True,
        ..Default::default()
    };

    let matches = cache.query(&style_query, &mut trace);
    assert!(matches.is_none(), "Should not match with style mismatch");

    let style_mismatch = trace.iter().any(|msg| {
        if let MatchReason::StyleMismatch { property, .. } = &msg.reason {
            property == &"italic"
        } else {
            false
        }
    });
    assert!(style_mismatch, "Style mismatch trace message not found");

    // Test stretch mismatch
    trace.clear();
    let stretch_query = FcPattern {
        name: Some("Test Font".to_string()),
        stretch: FcStretch::Condensed,
        ..Default::default()
    };

    let matches = cache.query(&stretch_query, &mut trace);
    assert!(matches.is_none(), "Should not match with stretch mismatch");

    let stretch_mismatch = trace
        .iter()
        .any(|msg| matches!(msg.reason, MatchReason::StretchMismatch { .. }));
    assert!(stretch_mismatch, "Stretch mismatch trace message not found");

    // Test unicode range mismatch
    trace.clear();
    let range_query = FcPattern {
        name: Some("Test Font".to_string()),
        unicode_ranges: vec![UnicodeRange {
            start: 0x0370,
            end: 0x03FF,
        }], // Greek
        ..Default::default()
    };

    let matches = cache.query(&range_query, &mut trace);
    assert!(
        matches.is_none(),
        "Should not match with Unicode range mismatch"
    );

    let range_mismatch = trace
        .iter()
        .any(|msg| matches!(msg.reason, MatchReason::UnicodeRangeMismatch { .. }));
    assert!(
        range_mismatch,
        "Unicode range mismatch trace message not found"
    );
}

fn getfonts(
    arial_id: FontId,
    arial_bold_id: FontId,
    courier_id: FontId,
    fira_id: FontId,
    noto_cjk_id: FontId,
) -> Vec<(FontId, FcPattern, FcFont)> {
    return vec![
        (
            arial_id,
            FcPattern {
                name: Some("Arial".to_string()),
                family: Some("Arial".to_string()),
                weight: FcWeight::Normal,
                monospace: PatternMatch::False,
                unicode_ranges: vec![UnicodeRange {
                    start: 0x0000,
                    end: 0x007F,
                }],
                ..Default::default()
            },
            FcFont {
                bytes: vec![1, 2, 3, 4],
                font_index: 0,
                id: "arial-regular".to_string(),
            },
        ),
        (
            arial_bold_id,
            FcPattern {
                name: Some("Arial Bold".to_string()),
                family: Some("Arial".to_string()),
                weight: FcWeight::Bold,
                bold: PatternMatch::True,
                monospace: PatternMatch::False,
                unicode_ranges: vec![UnicodeRange {
                    start: 0x0000,
                    end: 0x007F,
                }],
                ..Default::default()
            },
            FcFont {
                bytes: vec![5, 6, 7, 8],
                font_index: 0,
                id: "arial-bold".to_string(),
            },
        ),
        // Monospace fonts
        (
            courier_id,
            FcPattern {
                name: Some("Courier New".to_string()),
                family: Some("Courier New".to_string()),
                weight: FcWeight::Normal,
                monospace: PatternMatch::True,
                unicode_ranges: vec![UnicodeRange {
                    start: 0x0000,
                    end: 0x007F,
                }],
                ..Default::default()
            },
            FcFont {
                bytes: vec![9, 10, 11, 12],
                font_index: 0,
                id: "courier-new".to_string(),
            },
        ),
        (
            fira_id,
            FcPattern {
                name: Some("Fira Code".to_string()),
                family: Some("Fira Code".to_string()),
                weight: FcWeight::Normal,
                monospace: PatternMatch::True,
                unicode_ranges: vec![UnicodeRange {
                    start: 0x0000,
                    end: 0x007F,
                }],
                ..Default::default()
            },
            FcFont {
                bytes: vec![13, 14, 15, 16],
                font_index: 0,
                id: "fira-code".to_string(),
            },
        ),
        // CJK font
        (
            noto_cjk_id,
            FcPattern {
                name: Some("Noto Sans CJK".to_string()),
                family: Some("Noto Sans CJK".to_string()),
                weight: FcWeight::Normal,
                monospace: PatternMatch::False,
                unicode_ranges: vec![
                    UnicodeRange {
                        start: 0x0000,
                        end: 0x007F,
                    }, // Latin
                    UnicodeRange {
                        start: 0x4E00,
                        end: 0x9FFF,
                    }, // CJK
                ],
                ..Default::default()
            },
            FcFont {
                bytes: vec![17, 18, 19, 20],
                font_index: 0,
                id: "noto-sans-cjk".to_string(),
            },
        ),
    ];
}

// Update the test code to use deterministic IDs
#[test]
fn test_font_search() {
    // Create fixed font IDs for deterministic testing
    let arial_id = FontId(1);
    let arial_bold_id = FontId(2);
    let courier_id = FontId(3);
    let fira_id = FontId(4);
    let noto_cjk_id = FontId(5);

    // Create a set of fonts with various properties for testing search functionality
    let fonts = getfonts(arial_id, arial_bold_id, courier_id, fira_id, noto_cjk_id);

    // Create font cache with all our test fonts using deterministic IDs
    let mut cache = FcFontCache::default();
    for (id, pattern, font) in fonts {
        cache.with_memory_font_with_id(id, pattern, font);
    }

    // Test 2: Search for any monospace font
    let mut trace = Vec::new();
    let monospace_query = FcPattern {
        monospace: PatternMatch::True,
        ..Default::default()
    };

    let results = cache.query_all(&monospace_query, &mut trace);
    assert_eq!(results.len(), 2, "Should find two monospace fonts");

    let result_ids: Vec<FontId> = results.into_iter().map(|m| m.id).collect();
    assert!(
        result_ids.contains(&courier_id),
        "Should include Courier New"
    );
    assert!(result_ids.contains(&fira_id), "Should include Fira Code");

    // Test 4: Search for a font that can render CJK text
    let mut trace = Vec::new();
    let cjk_text = "你好"; // Hello in Chinese

    let results = cache.query_for_text(&FcPattern::default(), cjk_text, &mut trace);
    assert!(!results.is_empty(), "Should find fonts for CJK text");

    let result_ids: Vec<FontId> = results.into_iter().map(|m| m.id).collect();
    assert!(
        result_ids.contains(&noto_cjk_id),
        "Should include Noto Sans CJK"
    );

    // Test 5: Multiple fonts for mixed text
    let mut trace = Vec::new();
    let mixed_text = "Hello 你好"; // Latin and CJK

    let results = cache.query_for_text(&FcPattern::default(), mixed_text, &mut trace);
    assert!(
        results.len() >= 2,
        "Should find multiple fonts for mixed text"
    );

    // Verify that we got both Latin and CJK capable fonts
    let latin_found = results.iter().any(|m| {
        let id = m.id;
        id == arial_id || id == arial_bold_id || id == courier_id || id == fira_id
    });
    let cjk_found = results.iter().any(|m| m.id == noto_cjk_id);

    assert!(latin_found, "Should find a Latin-capable font");
    assert!(cjk_found, "Should find a CJK-capable font");
}

#[test]
fn test_failing_isolated() {
    // Create fixed font IDs for deterministic testing
    let arial_id = FontId(1);
    let arial_bold_id = FontId(2);
    let courier_id = FontId(3);
    let fira_id = FontId(4);
    let noto_cjk_id = FontId(5);

    // Create a set of fonts with various properties for testing search functionality
    let fonts = getfonts(arial_id, arial_bold_id, courier_id, fira_id, noto_cjk_id);

    // Create font cache with all our test fonts using deterministic IDs
    let mut cache = FcFontCache::default();
    for (id, pattern, font) in fonts {
        cache.with_memory_font_with_id(id, pattern, font);
    }

    // Test 1: Search for Arial font
    let mut trace = Vec::new();
    let arial_query = FcPattern {
        name: Some("Arial".to_string()),
        ..Default::default()
    };

    let result = cache.query(&arial_query, &mut trace);
    assert!(result.is_some(), "Should find Arial font");
    assert_eq!(result.unwrap().id, arial_id, "Should match Arial font ID");
}

#[test]
fn test_failing_isolated_2() {
    // Create fixed font IDs for deterministic testing
    let arial_id = FontId(1);
    let arial_bold_id = FontId(2);
    let courier_id = FontId(3);
    let fira_id = FontId(4);
    let noto_cjk_id = FontId(5);

    // Create a set of fonts with various properties for testing search functionality
    let fonts = getfonts(arial_id, arial_bold_id, courier_id, fira_id, noto_cjk_id);

    // Create font cache with all our test fonts using deterministic IDs
    let mut cache = FcFontCache::default();
    for (id, pattern, font) in fonts {
        cache.with_memory_font_with_id(id, pattern, font);
    }

    // Test 3: Search for bold Arial font
    let mut trace = Vec::new();
    let arial_bold_query = FcPattern {
        family: Some("Arial".to_string()),
        bold: PatternMatch::True,
        ..Default::default()
    };

    let result = cache.query(&arial_bold_query, &mut trace);
    assert!(result.is_some(), "Should find Arial Bold font");
    assert_eq!(
        result.unwrap().id,
        arial_bold_id,
        "Should match Arial Bold font ID"
    );
}
