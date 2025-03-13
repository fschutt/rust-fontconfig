
use super::*;

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
            UnicodeRange { start: 0x0000, end: 0x007F }, // Basic Latin
            UnicodeRange { start: 0x0080, end: 0x00FF }, // Latin-1 Supplement
        ],
        ..Default::default()
    };
    
    let cyrillic_pattern = FcPattern {
        name: Some("Cyrillic Font".to_string()),
        family: Some("Cyrillic Family".to_string()),
        unicode_ranges: vec![
            UnicodeRange { start: 0x0400, end: 0x04FF }, // Cyrillic
        ],
        ..Default::default()
    };
    
    let cjk_pattern = FcPattern {
        name: Some("CJK Font".to_string()),
        family: Some("CJK Family".to_string()),
        unicode_ranges: vec![
            UnicodeRange { start: 0x4E00, end: 0x9FFF }, // CJK Unified Ideographs
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
    
    // Test querying with Unicode ranges
    let mut trace = Vec::new();
    
    // Query for Latin characters
    let latin_query = FcPattern {
        unicode_ranges: vec![UnicodeRange { start: 0x0041, end: 0x005A }], // A-Z
        ..Default::default()
    };
    
    let matches = cache.query_all(&latin_query, &mut trace);
    assert_eq!(matches.len(), 1);
    assert_eq!(cache.get_memory_font("latin-font").is_some(), true);
    
    // Check trace messages for non-matches (Unicode range mismatches)
    trace.clear();
    
    // Query for Cyrillic characters
    let cyrillic_query = FcPattern {
        unicode_ranges: vec![UnicodeRange { start: 0x0410, end: 0x044F }], // Cyrillic letters
        ..Default::default()
    };
    
    let matches = cache.query_all(&cyrillic_query, &mut trace);
    assert_eq!(matches.len(), 1);
    assert_eq!(cache.get_memory_font("cyrillic-font").is_some(), true);
    
    // Check trace messages for non-matches (Unicode range mismatches)
    let range_mismatch_traces = trace.iter()
        .filter(|msg| matches!(msg.reason, MatchReason::UnicodeRangeMismatch { .. }))
        .count();
    assert!(range_mismatch_traces > 0, "Expected Unicode range mismatch traces");
    
    trace.clear();
    
    // Query for text that needs multiple fonts
    let text = "Hello Привет 你好"; // Latin, Cyrillic, and CJK
    let matches = cache.query_for_text(&FcPattern::default(), text, &mut trace);
    assert_eq!(matches.len(), 3, "Should match all three fonts for multilingual text");
    
    // Check trace messages for fallback search
    let fallback_traces = trace.iter()
        .filter(|msg| msg.path == "<fallback search>")
        .count();
    assert!(fallback_traces > 0, "Expected fallback search traces");
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
    let family_mismatch_traces = trace.iter()
        .filter(|msg| matches!(msg.reason, MatchReason::FamilyMismatch { .. }))
        .count();
    assert!(family_mismatch_traces > 0, "Expected family mismatch trace messages");
    
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
    let weight_mismatch_traces = trace.iter()
        .filter(|msg| matches!(msg.reason, MatchReason::WeightMismatch { .. }))
        .count();
    assert!(weight_mismatch_traces > 0, "Expected weight mismatch trace messages");
    
    // Test weight matching algorithm
    let available_weights = [FcWeight::Light, FcWeight::Normal, FcWeight::Bold];
    
    // When exact match exists
    assert_eq!(FcWeight::Normal.find_best_match(&available_weights), Some(FcWeight::Normal),
                "Should find exact match when available");
    
    // When desired weight is less than 400
    assert_eq!(FcWeight::ExtraLight.find_best_match(&available_weights), Some(FcWeight::Light),
                "Should find closest lighter weight for weights < 400");
    
    // When desired weight is greater than 500
    assert_eq!(FcWeight::ExtraBold.find_best_match(&available_weights), Some(FcWeight::Bold),
                "Should find closest heavier weight for weights > 500");
    
    // For weight 400, try 500 first then lighter weights
    let available = [FcWeight::Light, FcWeight::Bold];
    assert_eq!(FcWeight::Normal.find_best_match(&available), Some(FcWeight::Light),
                "For weight 400, should prefer lightest weight when 500 unavailable");
    
    // For weight 500, try 400 first then lighter weights
    let available = [FcWeight::Light, FcWeight::SemiBold];
    assert_eq!(FcWeight::Medium.find_best_match(&available), Some(FcWeight::Light),
                "For weight 500, should prefer 400 first");
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
        unicode_ranges: vec![UnicodeRange { start: 0x0000, end: 0x007F }],
        ..Default::default()
    };
    
    let mut cache = FcFontCache::default();
    cache.with_memory_fonts(vec![
        (test_pattern.clone(), test_font),
    ]);
    
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
            requested.as_ref() == Some(&"Wrong Name".to_string()) &&
            found.as_ref() == Some(&"Test Font".to_string())
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
    
    let stretch_mismatch = trace.iter().any(|msg| {
        matches!(msg.reason, MatchReason::StretchMismatch { .. })
    });
    assert!(stretch_mismatch, "Stretch mismatch trace message not found");
    
    // Test unicode range mismatch
    trace.clear();
    let range_query = FcPattern {
        name: Some("Test Font".to_string()),
        unicode_ranges: vec![UnicodeRange { start: 0x0370, end: 0x03FF }], // Greek
        ..Default::default()
    };
    
    let matches = cache.query(&range_query, &mut trace);
    assert!(matches.is_none(), "Should not match with Unicode range mismatch");
    
    let range_mismatch = trace.iter().any(|msg| {
        matches!(msg.reason, MatchReason::UnicodeRangeMismatch { .. })
    });
    assert!(range_mismatch, "Unicode range mismatch trace message not found");
}