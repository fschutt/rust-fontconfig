use rust_fontconfig::*;

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// `PatternMatch` crosses the C boundary by value, and the header is
/// hand-maintained. Read the header and hold the enum to it, so the two
/// cannot drift apart again (they did: reordering the Rust variants once
/// made every C caller's `FC_MATCH_FALSE` arrive as `True`).
#[test]
fn pattern_match_discriminants_match_the_c_header() {
    let header = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/ffi/rust_fontconfig.h"))
        .expect("ffi/rust_fontconfig.h is part of the repository");
    let header_value = |name: &str| -> i32 {
        let at = header.find(name).unwrap_or_else(|| panic!("{name} is not declared in the header"));
        let rest = header[at + name.len()..].trim_start().trim_start_matches('=').trim_start();
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse().unwrap_or_else(|_| panic!("{name} has no numeric value in the header"))
    };
    assert_eq!(PatternMatch::True as i32, header_value("FC_MATCH_TRUE"));
    assert_eq!(PatternMatch::False as i32, header_value("FC_MATCH_FALSE"));
    assert_eq!(PatternMatch::DontCare as i32, header_value("FC_MATCH_DONT_CARE"));
    assert_eq!(PatternMatch::default(), PatternMatch::DontCare);
}

#[test]
fn test_operating_system_font_expansion() {
    // The tables the crate used to hard-code, now an explicit opt-in.
    let windows = FcFallbackConfig::os_defaults(OperatingSystem::Windows);
    assert_eq!(windows.generic_candidates(GenericFamily::Serif), names(&["Times New Roman"]));
    assert_eq!(
        windows.generic_candidates(GenericFamily::SansSerif),
        names(&["Segoe UI", "Tahoma", "Microsoft Sans Serif", "MS Sans Serif", "Helv"])
    );
    assert_eq!(
        windows.generic_candidates(GenericFamily::Monospace),
        names(&["Segoe UI Mono", "Courier New", "Cascadia Code", "Cascadia Mono", "Consolas"])
    );

    let macos = FcFallbackConfig::os_defaults(OperatingSystem::MacOS);
    assert_eq!(
        macos.generic_candidates(GenericFamily::Serif),
        names(&["Times New Roman", "Times", "New York", "Palatino"])
    );
    assert_eq!(
        macos.generic_candidates(GenericFamily::SansSerif),
        names(&["San Francisco", ".AppleSystemUIFont", ".SFUIText", ".SFUI-Regular", "Helvetica Neue", "Helvetica", "Lucida Grande"])
    );
    assert_eq!(
        macos.generic_candidates(GenericFamily::Monospace),
        names(&["SF Mono", "Menlo", "Monaco", "Courier", "Oxygen Mono", "Source Code Pro", "Fira Mono"])
    );

    let linux = FcFallbackConfig::os_defaults(OperatingSystem::Linux);
    assert_eq!(linux.generic_candidates(GenericFamily::Serif).len(), 8, "Linux should have 8 serif fonts");
    assert_eq!(
        linux.generic_candidates(GenericFamily::SansSerif),
        names(&["Ubuntu", "Arial", "DejaVu Sans", "Noto Sans", "Liberation Sans"])
    );

    // Generics without a table of their own borrow their parent's.
    assert_eq!(
        windows.generic_candidates(GenericFamily::SystemUi),
        windows.generic_candidates(GenericFamily::SansSerif)
    );

    // A script hint puts that script's preferences before the base list,
    // and the block decides the order: kana want the Japanese font first.
    let hiragana = [UnicodeRange { start: 0x3040, end: 0x309F }];
    let expanded = windows.expand_generic(GenericFamily::SansSerif, &hiragana);
    assert_eq!(expanded[0], "MS Gothic");
    assert!(expanded.contains(&"Segoe UI".to_string()));
    let hangul = [UnicodeRange { start: 0xAC00, end: 0xD7A3 }];
    assert_eq!(windows.expand_generic(GenericFamily::SansSerif, &hangul)[0], "Malgun Gothic");

    // Stack expansion keeps CSS order: the named family, then the generic's list.
    let families = vec!["Arial".to_string(), "sans-serif".to_string()];
    let expanded = macos.candidate_families(&families, &[]);
    assert_eq!(expanded[0], "Arial");
    assert_eq!(expanded[1], "San Francisco");
    assert_eq!(expanded[2], ".AppleSystemUIFont");
    assert_eq!(expanded[3], ".SFUIText");

    // A non-generic family passes through unchanged.
    let specific = vec!["MyCustomFont".to_string()];
    assert_eq!(windows.candidate_families(&specific, &[]), names(&["MyCustomFont"]));

    // The 4.x entry points are thin wrappers over the same tables.
    #[allow(deprecated)]
    {
        assert_eq!(
            OperatingSystem::Windows.get_sans_serif_fonts(&[]),
            windows.generic_candidates(GenericFamily::SansSerif)
        );
        assert_eq!(
            expand_font_families(&families, OperatingSystem::MacOS, &[]),
            macos.candidate_families(&families, &[])
        );
    }
}

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
    let cache = FcFontCache::default();
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
    let mut trace: Vec<TraceMsg> = Vec::new();

    // Query for Latin characters
    let latin_query = FcPattern {
        unicode_ranges: vec![UnicodeRange {
            start: 0x0041,
            end: 0x005A,
        }], // A-Z
        ..Default::default()
    };

    // Use list() and filter instead of query_all()
    let matches: Vec<_> = cache.list().into_iter()
        .filter(|(pattern, _)| {
            // Check if unicode ranges overlap
            if pattern.unicode_ranges.is_empty() { return false; }
            pattern.unicode_ranges.iter().any(|r| {
                latin_query.unicode_ranges.iter().any(|q| {
                    r.start <= q.end && q.start <= r.end
                })
            })
        })
        .collect();
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

    let matches: Vec<_> = cache.list().into_iter()
        .filter(|(pattern, _)| {
            if pattern.unicode_ranges.is_empty() { return false; }
            pattern.unicode_ranges.iter().any(|r| {
                cyrillic_query.unicode_ranges.iter().any(|q| {
                    r.start <= q.end && q.start <= r.end
                })
            })
        })
        .collect();
    assert_eq!(matches.len(), 1);
    assert_eq!(cache.get_memory_font(&cyrillic_id).is_some(), true);

    // Query for text that needs multiple fonts using resolve_font_chain + query_for_text
    #[cfg(feature = "std")]
    {
        let text = "Hello Привет 你好"; // Latin, Cyrillic, and CJK

        // Build a generic font chain from our in-memory fonts
        let families: Vec<String> = cache.list().iter()
            .filter_map(|(pattern, _)| pattern.family.clone())
            .collect();

        let chain = cache.resolve_font_chain(
            &families,
            FcWeight::Normal,
            PatternMatch::DontCare,
            PatternMatch::DontCare,
            &mut trace,
        );

        let runs = chain.query_for_text(&cache, text);

        // Collect unique fonts used
        let unique_fonts: std::collections::HashSet<_> = runs.iter()
            .filter_map(|r| r.font_id)
            .collect();

        assert!(
            unique_fonts.len() >= 2,
            "Should use multiple fonts for multilingual text"
        );
    }
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
    let cache = FcFontCache::default();
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

    let cache = FcFontCache::default();
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
                bold: PatternMatch::False,
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
    let cache = FcFontCache::default();
    for (id, pattern, font) in fonts {
        cache.with_memory_font_with_id(id, pattern, font);
    }

    // Test 2: Search for any monospace font using list() with filter
    let mut trace: Vec<TraceMsg> = Vec::new();
    
    let results: Vec<_> = cache.list().into_iter()
        .filter(|(pattern, _)| pattern.monospace == PatternMatch::True)
        .collect();
    assert_eq!(results.len(), 2, "Should find two monospace fonts");

    let result_ids: Vec<FontId> = results.into_iter().map(|(_, id)| id).collect();
    assert!(
        result_ids.contains(&courier_id),
        "Should include Courier New"
    );
    assert!(result_ids.contains(&fira_id), "Should include Fira Code");

    // Test 4: Search for a font that can render CJK text using resolve_font_chain
    #[cfg(feature = "std")]
    {
        let cjk_text = "你好"; // Hello in Chinese

        // Build font chain from all available fonts
        let families: Vec<String> = cache.list().iter()
            .filter_map(|(pattern, _)| pattern.family.clone())
            .collect();

        let chain = cache.resolve_font_chain(
            &families,
            FcWeight::Normal,
            PatternMatch::DontCare,
            PatternMatch::DontCare,
            &mut trace,
        );

        let runs = chain.query_for_text(&cache, cjk_text);
        assert!(!runs.is_empty(), "Should find fonts for CJK text");

        let result_ids: Vec<FontId> = runs.iter()
            .filter_map(|r| r.font_id)
            .collect();
        assert!(
            result_ids.contains(&noto_cjk_id),
            "Should include Noto Sans CJK"
        );

        // Test 5: Multiple fonts for mixed text
        trace.clear();
        let mixed_text = "Hello 你好"; // Latin and CJK

        let runs = chain.query_for_text(&cache, mixed_text);

        // Collect unique fonts
        let unique_fonts: std::collections::HashSet<_> = runs.iter()
            .filter_map(|r| r.font_id)
            .collect();

        assert!(
            unique_fonts.len() >= 1,
            "Should find at least one font for mixed text"
        );

        // Verify that we got both Latin and CJK capable fonts
        let cjk_found = unique_fonts.contains(&noto_cjk_id);
        assert!(cjk_found, "Should find a CJK-capable font");
    }
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
    let cache = FcFontCache::default();
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
    let cache = FcFontCache::default();
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

/// Regression test for the headless / wasm / embedder-bundled-font bug.
///
/// A bundled IN-MEMORY font, registered via `with_memory_fonts` with the
/// kind of NAIVE pattern a normal caller actually writes (a generic-ish
/// name and, crucially, an EMPTY `unicode_ranges`), must be usable to shape
/// text when the document asks for the generic CSS family `"serif"` and the
/// cache has NO system fonts at all.
///
/// Before the fix this returned `None` for two independent reasons:
///   1. `with_memory_fonts` stored the empty `unicode_ranges` verbatim, and
///      `resolve_char` skips fonts with no range info, so the font could
///      never be selected for any character.
///   2. The generic `"serif"` family was expanded to a hardcoded list of
///      real OS font names (Times, DejaVu Serif, ...) and the original
///      generic name was dropped, so a registered memory font was never
///      reached.
///
/// Requires the `parsing` feature: without it the crate cannot inspect the
/// font's cmap/OS2 to learn its Unicode coverage, so the empty ranges
/// cannot be auto-populated.
#[cfg(all(feature = "std", feature = "parsing"))]
#[test]
fn test_memory_font_generic_serif_resolves_char() {
    // A real Latin TTF, embedded into the test binary.
    let font_bytes = include_bytes!("fixtures/InstrumentSerif-Regular.ttf").to_vec();

    // Empty cache: no system fonts (headless / wasm / embedder scenario).
    let cache = FcFontCache::default();

    // Exactly what a normal caller writes: a name, and an EMPTY
    // unicode_ranges (they do NOT hand-compute the cmap).
    let pattern = FcPattern {
        name: Some("serif".to_string()),
        family: Some("serif".to_string()),
        unicode_ranges: Vec::new(),
        ..Default::default()
    };
    let font = FcFont {
        bytes: font_bytes,
        font_index: 0,
        id: "bundled-serif".to_string(),
    };
    cache.with_memory_fonts(vec![(pattern, font)]);

    // Resolve a chain for the generic CSS family "serif".
    let mut trace: Vec<TraceMsg> = Vec::new();
    let chain = cache.resolve_font_chain_with_scripts(
        &["serif".to_string()],
        FcWeight::Normal,
        PatternMatch::False,
        PatternMatch::False,
        None,
        &mut trace,
    );

    // The bundled font is the ONLY font available; it MUST be selected to
    // render an ASCII 'A'.
    let resolved = chain.resolve_char(&cache, 'A');
    assert!(
        resolved.is_some(),
        "bundled in-memory 'serif' font must resolve ASCII 'A' on a headless cache; \
         got None (chain = {:#?})",
        chain
    );
}

/// `query` is fallible and `query_with_fallback` is total — the `fc-match`
/// contract.
///
/// `fc-match Cantarell` answers `NotoSans-Regular.ttf` on a machine with no
/// Cantarell, because fontconfig substitutes through its config chain. `query`
/// deliberately does not: it reports honestly that the exact request could not
/// be met. A RENDERER must not be handed that hole — either the text silently
/// vanishes, or the caller invents a fallback whose font is not registered
/// where the renderer later looks it up by hash.
#[test]
fn query_with_fallback_is_total_like_fc_match() {
    let installed = FcPattern {
        name: Some("Only Font".to_string()),
        family: Some("Only Family".to_string()),
        weight: FcWeight::Normal,
        unicode_ranges: vec![UnicodeRange { start: 0x0000, end: 0x007F }],
        ..Default::default()
    };
    let cache = FcFontCache::default();
    cache.with_memory_fonts(vec![(
        installed.clone(),
        FcFont { bytes: vec![0, 1, 2, 3], font_index: 0, id: "only-font".to_string() },
    )]);

    // A family that simply is not installed — the "Cantarell" case.
    let missing = FcPattern {
        family: Some("Cantarell".to_string()),
        ..Default::default()
    };

    let mut trace = Vec::new();
    assert!(
        cache.query(&missing, &mut trace).is_none(),
        "query must stay FALLIBLE — callers rely on it to report an unresolved family",
    );

    let mut trace = Vec::new();
    let fallback = cache.query_with_fallback(&missing, &mut trace);
    assert!(
        fallback.is_some(),
        "query_with_fallback must be TOTAL while any font exists (fc-match never fails)",
    );

    // It must fall back to the font we actually have, not to nothing-in-particular.
    let mut trace = Vec::new();
    let expected = cache.query(&installed, &mut trace).expect("the installed font matches itself");
    assert_eq!(
        fallback.unwrap().id,
        expected.id,
        "the fallback must resolve to the one font in the cache",
    );

    // Style is preserved where it can be: a BOLD request for a missing family
    // still resolves rather than failing.
    let missing_bold = FcPattern {
        family: Some("Cantarell".to_string()),
        weight: FcWeight::Bold,
        ..Default::default()
    };
    let mut trace = Vec::new();
    assert!(
        cache.query_with_fallback(&missing_bold, &mut trace).is_some(),
        "a bold request for a missing family must still resolve",
    );

    // The ONE case where it may fail: there is genuinely nothing to return.
    let mut trace = Vec::new();
    assert!(
        FcFontCache::default().query_with_fallback(&missing, &mut trace).is_none(),
        "an empty cache is the only legitimate None",
    );
}

/// A font's coverage is unioned from two sources whose block boundaries do not
/// align: the OS/2 `ulUnicodeRange` bit mappings and the cmap block probe.
/// `calculate_unicode_coverage` ranks fallback candidates by SUMMING range
/// widths, so an un-coalesced union counts the shared codepoints twice and
/// hands the font a score it did not earn — which is how a CJK megafont ends up
/// winning a Latin run it has no business winning.
#[test]
fn normalize_unicode_ranges_coalesces_so_coverage_is_not_double_counted() {
    let r = |start, end| UnicodeRange { start, end };

    let raw = vec![
        r(0x0100, 0x017F), // Latin Extended-A, listed first to prove sorting
        r(0x0000, 0x007F), // Basic Latin
        r(0x0040, 0x00FF), // overlaps Basic Latin, then TOUCHES Latin Ext-A
        r(0x0000, 0x007F), // exact duplicate
    ];

    // Overlapping, touching and duplicated ranges collapse to one contiguous run.
    let merged = FcFontCache::normalize_unicode_ranges(raw.clone());
    assert_eq!(merged, vec![r(0x0000, 0x017F)]);

    // The whole point: the ranking sum equals the codepoints actually covered.
    assert_eq!(FcFontCache::calculate_unicode_coverage(&merged), 0x180);
    // What the raw union would have claimed instead — 1.5x inflated.
    assert_eq!(FcFontCache::calculate_unicode_coverage(&raw), 0x240);

    // A genuine gap must NOT be bridged.
    let disjoint =
        FcFontCache::normalize_unicode_ranges(vec![r(0x0200, 0x02FF), r(0x0000, 0x007F)]);
    assert_eq!(disjoint, vec![r(0x0000, 0x007F), r(0x0200, 0x02FF)]);

    // An `end` at u32::MAX must not wrap while testing adjacency.
    let maxed = FcFontCache::normalize_unicode_ranges(vec![r(0x0000, u32::MAX), r(0x0010, 0x0020)]);
    assert_eq!(maxed, vec![r(0x0000, u32::MAX)]);
}

/// The coverage a parsed font reports must be a normalized set, which is what
/// keeps the ranking sum above honest for REAL fonts and not just hand-built
/// range vectors.
///
/// Before coverage became cmap-authoritative this vector was whatever the OS/2
/// bits claimed, minus what the cmap disproved. Now the cmap's own blocks are
/// unioned in, so without coalescing this font would report overlapping ranges.
#[cfg(all(feature = "std", feature = "parsing"))]
#[test]
fn parsed_font_coverage_is_a_normalized_set() {
    let font_bytes = include_bytes!("fixtures/InstrumentSerif-Regular.ttf").to_vec();

    let cache = FcFontCache::default();
    let pattern = FcPattern {
        name: Some("instrument".to_string()),
        // Empty: the crate must derive coverage from the font itself.
        unicode_ranges: Vec::new(),
        ..Default::default()
    };
    cache.with_memory_fonts(vec![(
        pattern.clone(),
        FcFont { bytes: font_bytes, font_index: 0, id: "instrument".to_string() },
    )]);

    let mut trace = Vec::new();
    let matched = cache
        .query(&pattern, &mut trace)
        .expect("the registered font matches itself");

    let ranges = &matched.unicode_ranges;
    assert!(!ranges.is_empty(), "a parsed Latin font must report some coverage");

    for pair in ranges.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        assert!(
            a.end.saturating_add(1) < b.start,
            "ranges must be sorted, disjoint and non-touching; {a:?} then {b:?} in {ranges:?}",
        );
    }

    // Sanity: a Latin serif face covers Basic Latin.
    assert!(
        ranges.iter().any(|r| r.start <= 'A' as u32 && 'A' as u32 <= r.end),
        "a Latin font must cover 'A', got {ranges:?}",
    );
}

/// Rebuild `font` without the table `drop_tag`, recomputing the table directory
/// so the result is a valid sfnt rather than a file with dangling offsets.
#[cfg(all(feature = "std", feature = "parsing"))]
fn strip_table(font: &[u8], drop_tag: &[u8; 4]) -> Vec<u8> {
    let num = u16::from_be_bytes([font[4], font[5]]) as usize;
    let mut tables: Vec<([u8; 4], u32, Vec<u8>)> = Vec::new();

    for i in 0..num {
        let rec = 12 + i * 16;
        let tag: [u8; 4] = font[rec..rec + 4].try_into().unwrap();
        let checksum = u32::from_be_bytes(font[rec + 4..rec + 8].try_into().unwrap());
        let offset = u32::from_be_bytes(font[rec + 8..rec + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(font[rec + 12..rec + 16].try_into().unwrap()) as usize;
        if &tag != drop_tag {
            tables.push((tag, checksum, font[offset..offset + len].to_vec()));
        }
    }
    tables.sort_by_key(|(tag, _, _)| *tag);

    let n = tables.len();
    let entry_selector = (usize::BITS - 1 - n.leading_zeros()) as u16;
    let search_range = (1u16 << entry_selector) * 16;

    let mut out = Vec::new();
    out.extend_from_slice(&font[0..4]); // sfntVersion
    out.extend_from_slice(&(n as u16).to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&((n as u16) * 16 - search_range).to_be_bytes());

    let mut body = Vec::new();
    let mut offset = 12 + n * 16;
    for (tag, checksum, data) in &tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        let pad = (4 - data.len() % 4) % 4;
        body.extend(std::iter::repeat(0).take(pad));
        offset += data.len() + pad;
    }
    out.extend_from_slice(&body);
    out
}

/// Set or clear the `head.macStyle` bold bit, which is the only weight signal a
/// font without an OS/2 table has.
#[cfg(all(feature = "std", feature = "parsing"))]
fn set_head_bold(font: &mut [u8]) {
    let num = u16::from_be_bytes([font[4], font[5]]) as usize;
    for i in 0..num {
        let rec = 12 + i * 16;
        if &font[rec..rec + 4] == b"head" {
            let offset = u32::from_be_bytes(font[rec + 8..rec + 12].try_into().unwrap()) as usize;
            let mac_style = offset + 44; // head.macStyle, bit 0 = bold
            let cur = u16::from_be_bytes([font[mac_style], font[mac_style + 1]]);
            font[mac_style..mac_style + 2].copy_from_slice(&(cur | 1).to_be_bytes());
            return;
        }
    }
    panic!("no head table");
}

/// A font without an OS/2 table must still parse.
///
/// OS/2 is optional in TrueType - only OpenType requires it - and fonts that
/// omit it are common enough to matter: printpdf's embedded base-14 PDF font
/// subsets (Helvetica, Times, Courier, ...) have no OS/2 table at all. Reading
/// it with `??` made every one of them fail to parse, so `font-family:
/// Helvetica` resolved to nothing and text silently fell back.
#[test]
#[cfg(all(feature = "std", feature = "parsing"))]
fn parses_a_font_without_an_os2_table() {
    let original = include_bytes!("fixtures/InstrumentSerif-Regular.ttf").to_vec();

    let baseline = FcParseFontBytes(&original, "InstrumentSerif")
        .expect("fixture itself must parse");
    let (baseline_pattern, _) = &baseline[0];

    let stripped = strip_table(&original, b"OS/2");
    assert!(
        stripped.len() < original.len(),
        "fixture had no OS/2 table to strip, so this test proves nothing"
    );

    let parsed = FcParseFontBytes(&stripped, "InstrumentSerif")
        .expect("a font without OS/2 must still parse");
    let (pattern, _) = &parsed[0];

    // The name table still names it, and coverage is cmap-derived so it survives
    // losing OS/2's unicode-range hints entirely.
    assert_eq!(pattern.family, baseline_pattern.family);
    assert!(
        !pattern.unicode_ranges.is_empty(),
        "coverage comes from the cmap, so it must survive the loss of OS/2"
    );

    // Without OS/2 the weight falls back to head.macStyle: regular here...
    assert_eq!(pattern.weight, FcWeight::Normal);

    // ...and Bold once the macStyle bit is set.
    let mut bolded = strip_table(&original, b"OS/2");
    set_head_bold(&mut bolded);
    let parsed_bold = FcParseFontBytes(&bolded, "InstrumentSerif")
        .expect("a bold font without OS/2 must still parse");
    assert_eq!(parsed_bold[0].0.weight, FcWeight::Bold);
}

/// A family lookup must find the fonts that carry that family, and must
/// answer "nobody has it" without walking the cache.
///
/// `query_by_family_normalized` used to be the ONLY path a specific family
/// name could take (`fuzzy_query_by_name` is a no-op on the azul web fork),
/// and it walked every registered pattern allocating a normalized `String`
/// per face per call. azul measured ~0.52 ms per lookup against a system
/// font set, and a CSS stack with generic expansion asks ~150 times — 74 ms
/// of a 177 ms cold pagination went here. It is a `family_index` probe now.
///
/// NEGATIVE CONTROL: making `index_pattern_family` a no-op (so the index is
/// always empty) makes every resolve below come back with no fonts — run
/// and seen.
#[test]
fn a_family_lookup_finds_its_faces_and_misses_cheaply() {
    let mk = |id: &str| FcFont {
        bytes: vec![0, 1, 2, 3],
        font_index: 0,
        id: id.to_string(),
    };
    let pat = |family: &str, name: &str| FcPattern {
        family: Some(family.to_string()),
        name: Some(name.to_string()),
        ..Default::default()
    };

    let cache = FcFontCache::default();
    cache.with_memory_fonts(vec![
        (pat("Test Sans", "Test Sans Regular"), mk("ts-regular")),
        (pat("Test Sans", "Test Sans Bold"), mk("ts-bold")),
        (pat("Other Family", "Other Regular"), mk("other")),
    ]);

    let mut trace = Vec::new();
    let resolve = |fam: &str, trace: &mut Vec<TraceMsg>| {
        cache
            .resolve_font_chain_with_scripts(
                &[fam.to_string()],
                FcWeight::Normal,
                PatternMatch::False,
                PatternMatch::False,
                None,
                trace,
            )
            .css_fallbacks
            .iter()
            .find(|g| g.css_name == fam)
            .map_or(0, |g| g.fonts.len())
    };

    assert!(
        resolve("Test Sans", &mut trace) >= 1,
        "a registered family must resolve to at least one face"
    );
    // Normalization strips case and separators, so the CSS spelling and the
    // stored spelling do not have to match byte for byte.
    assert!(
        resolve("test  sans", &mut trace) >= 1,
        "family matching is normalized (case and separators)"
    );
    assert!(
        resolve("Other Family", &mut trace) >= 1,
        "the second family resolves independently"
    );

    // The miss is the case the index exists for: it must not leak another
    // family's faces just because they are the only thing in the cache.
    assert_eq!(
        resolve("Nonexistent Family Name", &mut trace),
        0,
        "a family nobody has must resolve to NO fonts for its own CSS group \
         - matching something else here is what makes a missing font render \
         as an arbitrary one"
    );
}

/// fonts.conf handling, hermetic: a configuration tree in a temp directory.
///
/// The parser is not Linux-specific; only the default location is. What
/// these pin down is the part that was wrong before 5.0: a relative
/// `<include>` (the stock `conf.d`) resolved against the process's working
/// directory, so on a stock distribution the whole `conf.d` tree — where
/// every `<alias>` lives — was silently skipped unless the process happened
/// to run from `/etc/fonts`; and includes were walked last-in-first-out over
/// an unsorted directory listing, so which alias won depended on hash order.
#[cfg(feature = "parsing")]
mod fonts_conf_tree {
    use rust_fontconfig::{FcFallbackConfig, FcSystemConfig, GenericFamily};
    use std::path::{Path, PathBuf};

    struct TempTree(PathBuf);

    impl TempTree {
        fn new() -> Self {
            // Tests run concurrently and the clock is not unique enough to
            // tell them apart; a counter is.
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!("rfc-fontsconf-{}-{nanos}-{n}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            TempTree(dir)
        }
        fn write(&self, rel: &str, contents: &str) -> PathBuf {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, contents).unwrap();
            path
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn alias(family: &str, prefer: &[&str]) -> String {
        let prefer: String = prefer.iter().map(|p| format!("<family>{p}</family>")).collect();
        format!("<alias><family>{family}</family><prefer>{prefer}</prefer></alias>")
    }

    fn name_of(path: &Path) -> String {
        path.file_name().unwrap().to_string_lossy().into_owned()
    }

    #[test]
    fn relative_includes_resolve_against_the_config_directory_in_document_order() {
        let tree = TempTree::new();
        let font_dir = tree.0.join("fonts");
        std::fs::create_dir_all(&font_dir).unwrap();

        // The stock shape: a bare `conf.d`, plus a `relative` include.
        let root = tree.write(
            "etc/fonts/fonts.conf",
            &format!(
                r#"<?xml version="1.0"?><fontconfig>
                    <dir>{}</dir>
                    <dir prefix="relative">local-fonts</dir>
                    <include ignore_missing="yes">conf.d</include>
                    <include prefix="relative">extra/more.conf</include>
                    <include ignore_missing="yes">does-not-exist.conf</include>
                </fontconfig>"#,
                font_dir.display()
            ),
        );
        tree.write(
            "etc/fonts/conf.d/20-second.conf",
            &format!(
                r#"<fontconfig>{}<include>../fonts.conf</include></fontconfig>"#,
                alias("sans-serif", &["Second Sans"])
            ),
        );
        tree.write(
            "etc/fonts/conf.d/10-first.conf",
            &format!(
                "<fontconfig>{}{}</fontconfig>",
                alias("sans-serif", &["First Sans"]),
                alias("Arial", &["First Arial"])
            ),
        );
        tree.write("etc/fonts/conf.d/README", "not a configuration file");
        tree.write(
            "etc/fonts/extra/more.conf",
            &format!("<fontconfig>{}</fontconfig>", alias("monospace", &["Rel Mono"])),
        );

        // Run from somewhere that is NOT the config directory: the old code
        // would have looked for ./conf.d here and found nothing.
        let config = FcSystemConfig::parse_tree(&root).expect("the root parses");

        let read: Vec<String> = config.files.iter().map(|p| name_of(p)).collect();
        assert_eq!(
            read,
            vec!["fonts.conf", "10-first.conf", "20-second.conf", "more.conf"],
            "root, then the directory's numbered files in name order, then the \
             relative include; the cycle back to fonts.conf is ignored"
        );

        assert_eq!(
            config.aliases.get("sansserif").map(Vec::as_slice),
            Some(&["First Sans".to_string(), "Second Sans".to_string()][..]),
            "preferences append across files in include order: 10 before 20"
        );
        assert_eq!(
            config.aliases.get("arial").map(Vec::as_slice),
            Some(&["First Arial".to_string()][..])
        );
        assert_eq!(
            config.aliases.get("monospace").map(Vec::as_slice),
            Some(&["Rel Mono".to_string()][..]),
            "prefix=\"relative\" resolves against the including file's directory"
        );

        assert_eq!(
            config.font_dirs,
            vec![font_dir.clone(), root.parent().unwrap().join("local-fonts")],
            "<dir> entries resolve in order; prefix=\"relative\" against the config file"
        );

        // The parsed aliases are what the fallback configuration absorbs.
        let mut fallback = FcFallbackConfig::default();
        fallback.absorb_system_aliases(config.aliases);
        assert_eq!(
            fallback.generic_candidates(GenericFamily::SansSerif),
            &["First Sans".to_string(), "Second Sans".to_string()][..]
        );
        assert_eq!(fallback.substitutions_for("Arial"), &["First Arial".to_string()][..]);
    }

    #[test]
    fn an_include_cycle_terminates_and_reads_each_file_once() {
        let tree = TempTree::new();
        let a = tree.write(
            "a.conf",
            &format!("<fontconfig>{}<include>b.conf</include></fontconfig>", alias("serif", &["A Serif"])),
        );
        tree.write(
            "b.conf",
            &format!("<fontconfig>{}<include>a.conf</include></fontconfig>", alias("serif", &["B Serif"])),
        );
        let config = FcSystemConfig::parse_tree(&a).expect("parses");
        let read: Vec<String> = config.files.iter().map(|p| name_of(p)).collect();
        assert_eq!(read, vec!["a.conf", "b.conf"]);
        assert_eq!(
            config.aliases.get("serif").map(Vec::as_slice),
            Some(&["A Serif".to_string(), "B Serif".to_string()][..])
        );
    }

    #[test]
    fn a_missing_root_is_none_and_an_unreadable_include_is_skipped() {
        let tree = TempTree::new();
        assert!(FcSystemConfig::parse_tree(&tree.0.join("nope.conf")).is_none());
        let root = tree.write(
            "fonts.conf",
            "<fontconfig><include ignore_missing=\"yes\">missing-dir</include></fontconfig>",
        );
        let config = FcSystemConfig::parse_tree(&root).expect("parses");
        assert_eq!(config.files.len(), 1);
        assert!(config.aliases.is_empty());
    }
}

/// Coverage is what the cmap says, codepoint for codepoint — not a union of
/// whole Unicode blocks decided by a handful of probes. Before 5.0 a face
/// that mapped three of six sampled ideographs was recorded as covering all
/// 20,992 of them, and a face whose script was not in the fixed 50-block probe
/// list (Tibetan, Braille, every emoji) was recorded as covering nothing.
#[cfg(feature = "parsing")]
#[test]
fn parsed_coverage_is_exact_not_block_rounded() {
    let bytes = include_bytes!("fixtures/InstrumentSerif-Regular.ttf");
    let faces = FcParseFontBytes(bytes, "fixture").expect("the fixture parses");
    let ranges = &faces[0].0.unicode_ranges;

    // Normalized: sorted, disjoint, no touching neighbours.
    for pair in ranges.windows(2) {
        assert!(pair[0].end + 1 < pair[1].start, "ranges are sorted, disjoint and coalesced: {ranges:?}");
    }
    let contains = |cp: u32| ranges.iter().any(|r| r.start <= cp && cp <= r.end);
    assert!(contains('A' as u32) && contains('z' as u32));
    assert!(!contains(0x4E00), "a Latin face does not claim CJK");

    // Not block-rounded: Latin Extended-A (U+0100–U+017F) is partially covered.
    let in_block = ranges
        .iter()
        .map(|r| {
            let s = r.start.max(0x0100);
            let e = r.end.min(0x017F);
            if s <= e { e - s + 1 } else { 0 }
        })
        .sum::<u32>();
    assert!(in_block > 0 && in_block < 128, "partial block coverage survives: {in_block}/128");
}

/// A pattern registered with unsorted, overlapping coverage is stored
/// normalized, so every reader can rely on the invariant.
#[test]
fn registered_coverage_is_normalized_on_insert() {
    let cache = FcFontCache::default();
    cache.with_memory_fonts(vec![(
        FcPattern {
            name: Some("Messy".to_string()),
            family: Some("Messy".to_string()),
            unicode_ranges: vec![
                UnicodeRange { start: 0x0400, end: 0x04FF },
                UnicodeRange { start: 0x0020, end: 0x007E },
                UnicodeRange { start: 0x0040, end: 0x00FF },
                UnicodeRange { start: 0x0100, end: 0x017F },
            ],
            ..Default::default()
        },
        FcFont { bytes: vec![0], font_index: 0, id: "messy".to_string() },
    )]);
    let (pattern, _) = &cache.list()[0];
    assert_eq!(
        pattern.unicode_ranges,
        vec![
            UnicodeRange { start: 0x0020, end: 0x017F },
            UnicodeRange { start: 0x0400, end: 0x04FF },
        ]
    );
}

/// One record per font. The same face of the same file registered twice — a
/// directory scanned twice, a manifest loaded on top of a scan — is one
/// record; two different files that happen to carry identical name tables are
/// two. (The cache used to key its pattern map by the pattern itself, so the
/// second silently overwrote the first and orphaned its id.)
#[test]
fn fonts_are_one_record_each() {
    let cache = FcFontCache::default();
    let pattern = FcPattern {
        name: Some("Twin".to_string()),
        family: Some("Twin".to_string()),
        unicode_ranges: vec![UnicodeRange { start: 0x20, end: 0x7E }],
        ..Default::default()
    };
    let at = |path: &str, face: usize| FcFontPath { path: path.to_string(), font_index: face, bytes_hash: 0 };

    cache.insert_builder_font(pattern.clone(), at("/fonts/a.ttf", 0));
    cache.insert_builder_font(pattern.clone(), at("/fonts/a.ttf", 0));
    assert_eq!(cache.len(), 1, "the same face registered twice is one record");
    cache.insert_builder_font(pattern.clone(), at("/fonts/a.ttf", 1));
    cache.insert_builder_font(pattern.clone(), at("/fonts/b.ttf", 0));
    assert_eq!(cache.len(), 3, "another face, and another file, are records of their own");
    assert_eq!(cache.list().iter().filter(|(p, _)| p.name.as_deref() == Some("Twin")).count(), 3);
    assert_eq!(cache.lookup_paths_cached("/fonts/a.ttf").map(|ids| ids.len()), Some(2));
    assert_eq!(cache.lookup_paths_cached("/fonts/none.ttf"), None);

    let font = |bytes: &[u8]| FcFont { bytes: bytes.to_vec(), font_index: 0, id: "mem".to_string() };
    cache.with_memory_fonts(vec![(pattern.clone(), font(&[1, 2, 3]))]);
    cache.with_memory_fonts(vec![(pattern.clone(), font(&[1, 2, 3]))]);
    assert_eq!(cache.len(), 4, "identical pattern and bytes: one memory record");
    cache.with_memory_fonts(vec![(pattern, font(&[9, 9, 9]))]);
    assert_eq!(cache.len(), 5, "different bytes under the same pattern: a record of its own");
}

/// The font-file walk follows directory symlinks but visits each directory
/// once, so a link cycle terminates and a second route to the same directory
/// does not list its files twice. (Both scanners used to recurse unguarded; a
/// cycle overflowed the scout thread's stack, which aborts the process.)
#[cfg(unix)]
#[test]
fn font_file_walk_survives_a_symlink_cycle_and_lists_each_file_once() {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("rfc-walk-{}-{nanos}", std::process::id()));
    let fonts = root.join("a");
    std::fs::create_dir_all(&fonts).unwrap();
    std::fs::write(fonts.join("one.ttf"), include_bytes!("fixtures/InstrumentSerif-Regular.ttf")).unwrap();
    std::fs::write(fonts.join("notes.txt"), "not a font").unwrap();
    std::os::unix::fs::symlink(&root, fonts.join("loop")).unwrap(); // a/loop -> root: a cycle
    std::os::unix::fs::symlink(&fonts, root.join("twin")).unwrap(); // twin -> a: a second route

    let files = rust_fontconfig::utils::collect_font_files(&root);
    let _ = std::fs::remove_dir_all(&root);

    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].ends_with("one.ttf"));
}
