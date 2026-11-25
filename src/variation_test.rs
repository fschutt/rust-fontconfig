//! Tests for variation helper functions and instance cache limits.

use super::*;

fn axis(tag: &[u8; 4], min: f32, default: f32, max: f32) -> FcAxis {
    FcAxis {
        tag: *tag,
        min,
        default,
        max,
        hidden: false,
    }
}

/// Test fixture with 5 variation axes.
fn five_axes() -> FcVariation {
    FcVariation {
        axes: vec![
            axis(b"wght", 100.0, 400.0, 900.0),
            axis(b"wdth", 50.0, 100.0, 200.0),
            axis(b"opsz", 8.0, 14.0, 144.0),
            axis(b"slnt", -15.0, 0.0, 0.0),
            axis(b"ital", 0.0, 0.0, 1.0),
        ],
    }
}

#[test]
fn coordinates_become_one_cell_per_axis_in_fvar_order() {
    let axes = five_axes();
    // Default coordinates
    assert_eq!(canonical_cells(&axes, &[]), vec![25600, 6400, 896, 0, 0]);
    // Preserves axis order
    assert_eq!(
        canonical_cells(
            &axes,
            &[(*b"ital", 1.0), (*b"wdth", 87.5), (*b"wght", 700.0)]
        ),
        vec![44800, 5600, 896, 0, 64]
    );
    // Ignore unknown tags; last value wins for duplicates
    assert_eq!(
        canonical_cells(
            &axes,
            &[(*b"GRAD", 3.0), (*b"wght", 300.0), (*b"wght", 500.0)]
        ),
        vec![32000, 6400, 896, 0, 0]
    );
}

#[test]
fn values_are_clamped_and_rounded_to_a_cell_of_1_64() {
    let axes = five_axes();
    // Clamping
    assert_eq!(
        canonical_cells(
            &axes,
            &[(*b"wght", 2000.0), (*b"slnt", -30.0), (*b"ital", -1.0)]
        ),
        vec![57600, 6400, 896, -960, 0]
    );
    assert_eq!(canonical_cells(&axes, &[(*b"slnt", 10.0)])[3], 0);
    // NaN defaults; infinities clamp to bounds
    assert_eq!(canonical_cells(&axes, &[(*b"wght", f32::NAN)])[0], 25600);
    assert_eq!(
        canonical_cells(&axes, &[(*b"wght", f32::INFINITY)])[0],
        57600
    );
    assert_eq!(
        canonical_cells(&axes, &[(*b"wght", f32::NEG_INFINITY)])[0],
        6400
    );
    // Rounding to nearest 1/64
    assert_eq!(canonical_cells(&axes, &[(*b"wght", 350.001)])[0], 22400);
    assert_eq!(canonical_cells(&axes, &[(*b"wght", 350.02)])[0], 22401);
    assert_eq!(canonical_cells(&axes, &[(*b"wght", 350.012)])[0], 22401);
    assert_eq!(canonical_cells(&axes, &[(*b"slnt", -7.51)])[3], -481);
    assert_eq!(canonical_cells(&axes, &[(*b"slnt", -7.5)])[3], -480);
    // Near-default values resolve to default
    assert_eq!(
        canonical_cells(&axes, &[(*b"wght", 400.003)]),
        canonical_cells(&axes, &[])
    );
}

#[test]
fn the_label_names_the_axes_that_are_not_at_their_default() {
    let axes = five_axes();
    let source = FcPattern {
        name: Some("Foo Regular".to_string()),
        family: Some("Foo".to_string()),
        ..Default::default()
    };
    let label = |source: &FcPattern, coordinates: &[(FcAxisTag, f32)]| {
        synthetic_label(source, &axes, &canonical_cells(&axes, coordinates))
    };
    assert_eq!(
        label(&source, &[(*b"wght", 650.0), (*b"wdth", 87.5)]),
        "Foo @ wght=650 wdth=87.5"
    );
    assert_eq!(
        label(&source, &[(*b"ital", 1.0), (*b"slnt", -12.5)]),
        "Foo @ slnt=-12.5 ital=1"
    );
    assert_eq!(label(&source, &[]), "Foo @ default");
    // Default axes are omitted from label
    assert_eq!(label(&source, &[(*b"opsz", 14.0)]), "Foo @ default");
    assert_eq!(label(&source, &[(*b"wght", 12.5)]), "Foo @ wght=100");

    let name_only = FcPattern {
        name: Some("Foo Regular".to_string()),
        ..Default::default()
    };
    assert_eq!(label(&name_only, &[]), "Foo Regular @ default");
    assert_eq!(
        label(&FcPattern::default(), &[(*b"wght", 700.0)]),
        "font @ wght=700"
    );
}

#[test]
fn the_pattern_follows_the_axes_the_face_has() {
    let axes = five_axes();
    let source = FcPattern {
        name: Some("Foo".to_string()),
        family: Some("Foo".to_string()),
        monospace: PatternMatch::True,
        unicode_ranges: vec![crate::UnicodeRange {
            start: 0x20,
            end: 0x7E,
        }],
        ..Default::default()
    };
    let made = |coordinates: &[(FcAxisTag, f32)]| {
        synthetic_pattern(&source, &axes, &canonical_cells(&axes, coordinates))
    };

    // Weight mapping
    for (wght, weight, bold) in [
        (100.0, FcWeight::Thin, false),
        (300.0, FcWeight::Light, false),
        (400.0, FcWeight::Normal, false),
        (549.0, FcWeight::Medium, false),
        (550.0, FcWeight::SemiBold, true),
        (650.0, FcWeight::Bold, true),
        (900.0, FcWeight::Black, true),
    ] {
        let pattern = made(&[(*b"wght", wght)]);
        assert_eq!(pattern.weight, weight, "wght {wght}");
        assert_eq!(pattern.bold == PatternMatch::True, bold, "wght {wght}");
        assert_ne!(pattern.bold, PatternMatch::DontCare);
    }

    // Width mapping
    for (wdth, stretch, condensed) in [
        (50.0, FcStretch::UltraCondensed, true),
        (62.5, FcStretch::ExtraCondensed, true),
        (75.0, FcStretch::Condensed, true),
        (80.0, FcStretch::Condensed, true),
        (81.25, FcStretch::Condensed, true),
        (87.5, FcStretch::SemiCondensed, false),
        (100.0, FcStretch::Normal, false),
        (125.0, FcStretch::Expanded, false),
        (200.0, FcStretch::UltraExpanded, false),
    ] {
        let pattern = made(&[(*b"wdth", wdth)]);
        assert_eq!(pattern.stretch, stretch, "wdth {wdth}");
        assert_eq!(
            pattern.condensed == PatternMatch::True,
            condensed,
            "wdth {wdth}"
        );
        assert_ne!(pattern.condensed, PatternMatch::DontCare);
    }

    // Italic/oblique mapping
    assert_eq!(made(&[(*b"ital", 1.0)]).italic, PatternMatch::True);
    assert_eq!(made(&[(*b"ital", 0.5)]).italic, PatternMatch::True);
    assert_eq!(made(&[(*b"ital", 0.4)]).italic, PatternMatch::False);
    assert_eq!(made(&[]).italic, PatternMatch::False);
    assert_eq!(made(&[(*b"slnt", -10.0)]).oblique, PatternMatch::True);
    assert_eq!(made(&[(*b"slnt", 0.0)]).oblique, PatternMatch::False);
    assert_eq!(made(&[(*b"slnt", -10.0)]).italic, PatternMatch::False);
    assert_eq!(made(&[(*b"ital", 1.0)]).oblique, PatternMatch::False);

    // Inherit pattern metadata
    let pattern = made(&[(*b"wght", 650.0), (*b"wdth", 87.5), (*b"opsz", 20.0)]);
    assert_eq!(
        pattern.family.as_deref(),
        Some("Foo @ wght=650 wdth=87.5 opsz=20")
    );
    assert_eq!(pattern.name, pattern.family);
    assert_eq!(pattern.monospace, source.monospace);
    assert_eq!(pattern.unicode_ranges, source.unicode_ranges);
    assert_eq!(pattern.metadata, source.metadata);
    assert_eq!(pattern.render_config, source.render_config);

    // Missing axes inherit source values
    let only_weight = FcVariation {
        axes: vec![axis(b"wght", 100.0, 400.0, 900.0)],
    };
    let italic_source = FcPattern {
        italic: PatternMatch::True,
        stretch: FcStretch::Condensed,
        condensed: PatternMatch::True,
        oblique: PatternMatch::True,
        ..Default::default()
    };
    let pattern = synthetic_pattern(
        &italic_source,
        &only_weight,
        &canonical_cells(
            &only_weight,
            &[(*b"wght", 700.0), (*b"wdth", 150.0), (*b"ital", 0.0)],
        ),
    );
    assert_eq!(pattern.weight, FcWeight::Bold);
    assert_eq!(pattern.italic, PatternMatch::True);
    assert_eq!(pattern.oblique, PatternMatch::True);
    assert_eq!(pattern.stretch, FcStretch::Condensed);
    assert_eq!(pattern.condensed, PatternMatch::True);
}

#[test]
fn the_nearest_instance_counts_each_axis_as_a_part_of_its_range() {
    let axes = FcVariation {
        axes: vec![
            axis(b"wght", 100.0, 400.0, 900.0),
            axis(b"wdth", 50.0, 100.0, 150.0),
        ],
    };
    let ids = [FontId::new(), FontId::new(), FontId::new()];
    let instance = |wght: f32, wdth: f32, id: FontId| Instance {
        cells: canonical_cells(&axes, &[(*b"wght", wght), (*b"wdth", wdth)]),
        id,
    };
    let wanted = canonical_cells(&axes, &[(*b"wght", 400.0), (*b"wdth", 100.0)]);

    // Distance normalized by axis range
    let made = [
        instance(500.0, 100.0, ids[0]),
        instance(400.0, 125.0, ids[1]),
    ];
    assert_eq!(nearest(&made, &axes, &wanted), Some(ids[0]));

    // Ties break by FontId
    let made = [
        instance(500.0, 100.0, ids[2]),
        instance(300.0, 100.0, ids[1]),
    ];
    assert_eq!(nearest(&made, &axes, &wanted), Some(ids[1]));

    assert_eq!(nearest(&[], &axes, &wanted), None);

    // Zero-range axes ignored
    let flat = FcVariation {
        axes: vec![
            axis(b"wght", 100.0, 400.0, 900.0),
            axis(b"slnt", 0.0, 0.0, 0.0),
        ],
    };
    let made = [
        Instance {
            cells: vec![25600, 64],
            id: ids[1],
        },
        Instance {
            cells: vec![25600, 0],
            id: ids[0],
        },
    ];
    assert_eq!(nearest(&made, &flat, &[25600, 0]), Some(ids[0]));
}

/// Test fixture for 16.16 axis.
fn raw_axis(tag: &[u8; 4], min: i32, default: i32, max: i32) -> Axis {
    Axis {
        tag: *tag,
        min: Fixed::from_raw(min),
        default: Fixed::from_raw(default),
        max: Fixed::from_raw(max),
        hidden: false,
    }
}

fn raw(value: f32) -> i32 {
    (value * 65536.0) as i32
}

#[test]
fn values_become_one_number_of_the_font_per_axis_in_fvar_order() {
    let axes = [
        raw_axis(b"wdth", raw(62.5), raw(100.0), raw(125.0)),
        raw_axis(b"wght", raw(300.0), raw(400.0), raw(700.0)),
    ];
    let tuple = |coordinates: &[(FcAxisTag, f32)]| -> Vec<i32> {
        tuple_for_values(&axes, coordinates)
            .iter()
            .map(|value| value.raw_value())
            .collect()
    };
    let default = vec![raw(100.0), raw(400.0)];
    assert_eq!(tuple(&[]), default);
    // Preserves axis order
    assert_eq!(
        tuple(&[(*b"wght", 650.0), (*b"wdth", 87.5)]),
        vec![raw(87.5), raw(650.0)]
    );
    // Clamping and NaN handling
    assert_eq!(tuple(&[(*b"wght", 2000.0)]), vec![raw(100.0), raw(700.0)]);
    assert_eq!(tuple(&[(*b"wght", 1.0)]), vec![raw(100.0), raw(300.0)]);
    assert_eq!(tuple(&[(*b"wght", f32::INFINITY)])[1], raw(700.0));
    assert_eq!(tuple(&[(*b"wght", f32::NEG_INFINITY)])[1], raw(300.0));
    assert_eq!(tuple(&[(*b"wght", f32::NAN)]), default);
    // Last value wins; missing tags ignored
    assert_eq!(
        tuple(&[(*b"wght", 300.0), (*b"opsz", 9.0), (*b"wght", 500.0)]),
        vec![raw(100.0), raw(500.0)]
    );
}

#[test]
fn the_default_value_is_exactly_the_default_of_the_font() {
    // Exact match for default fixed point value
    let default = 26221159;
    let axes = [raw_axis(b"opsz", raw(8.0), default, raw(1000.0))];
    let as_f32 = f32::from(axes[0].default);
    assert_ne!((f64::from(as_f32) * 65536.0).round() as i32, default);

    // Default value handling
    assert_eq!(
        tuple_for_values(&axes, &[(*b"opsz", as_f32)])[0].raw_value(),
        default
    );
    assert_eq!(tuple_for_values(&axes, &[])[0].raw_value(), default);
    assert_eq!(
        tuple_for_values(&axes, &[(*b"opsz", 20.0)])[0].raw_value(),
        raw(20.0)
    );
    // Default cell handling
    let cell = to_cell(as_f32);
    assert_eq!(tuple_for_cells(&axes, &[cell])[0].raw_value(), default);
    assert_eq!(
        tuple_for_cells(&axes, &[cell + 1])[0].raw_value(),
        (cell + 1) * 1024
    );
}

#[test]
fn cells_become_numbers_of_the_font_limited_to_the_range() {
    let axes = [raw_axis(b"wght", raw(300.0), raw(400.0), raw(700.5))];
    let one = |cell: i32| tuple_for_cells(&axes, &[cell])[0].raw_value();
    assert_eq!(one(650 * 64), raw(650.0));
    assert_eq!(one(350 * 64 + 1), raw(350.0) + 1024);
    assert_eq!(one(400 * 64), raw(400.0));
    // Out-of-range cell clamping
    assert_eq!(one(701 * 64), raw(700.5));
    assert_eq!(one(i32::MAX), raw(700.5));
    assert_eq!(one(i32::MIN), raw(300.0));
}

mod with_a_font {
    use super::*;
    use crate::{FcFont, FcFontCache};

    const RED_HAT_MONO: &[u8] =
        include_bytes!("../tests/fixtures/RedHatMono-VariableFont_wght.ttf");
    const INSTRUMENT_SERIF: &[u8] = include_bytes!("../tests/fixtures/InstrumentSerif-Regular.ttf");
    const WGHT: FcAxisTag = *b"wght";

    fn cache_with(bytes: &[u8]) -> (FcFontCache, FontId) {
        let cache = FcFontCache::default();
        cache.with_memory_fonts(vec![(
            FcPattern {
                name: Some("Test".to_string()),
                family: Some("Test".to_string()),
                ..Default::default()
            },
            FcFont {
                bytes: bytes.into(),
                font_index: 0,
                id: "test".to_string(),
            },
        )]);
        let id = cache.list()[0].1;
        (cache, id)
    }

    fn size_of_instance(cache: &FcFontCache, id: &FontId, wght: f32) -> usize {
        let made = cache
            .variable_instance(id, &[(WGHT, wght)])
            .expect("an instance");
        cache.get_font_bytes(&made).expect("its bytes").len()
    }

    #[test]
    fn the_first_instance_is_kept_whatever_its_size() {
        let (cache, id) = cache_with(RED_HAT_MONO);
        cache.variation_write().max_bytes = 1;
        let first = cache
            .variable_instance(&id, &[(WGHT, 650.0)])
            .expect("kept");
        assert!(cache.get_font_bytes(&first).is_some());
        // Beyond limit: returns nearest instance
        assert_eq!(cache.variable_instance(&id, &[(WGHT, 300.0)]), Some(first));
        assert_eq!(cache.variable_instance(&id, &[(WGHT, 700.0)]), Some(first));
        assert_eq!(cache.variable_instance(&id, &[(WGHT, 650.0)]), Some(first));
        assert_eq!(cache.variable_instance_count(&id), 1);
    }

    #[test]
    fn instances_stop_at_the_byte_bound_and_the_nearest_one_is_returned() {
        let (probe, id) = cache_with(RED_HAT_MONO);
        let room = size_of_instance(&probe, &id, 300.0) + size_of_instance(&probe, &id, 400.0);

        // Verify byte capacity limits
        for max_bytes in [room, room + 1] {
            let (cache, id) = cache_with(RED_HAT_MONO);
            cache.variation_write().max_bytes = max_bytes;
            let light = cache.variable_instance(&id, &[(WGHT, 300.0)]).unwrap();
            let regular = cache.variable_instance(&id, &[(WGHT, 400.0)]).unwrap();
            assert_ne!(light, regular, "room {max_bytes}");
            assert_eq!(
                cache.variable_instance(&id, &[(WGHT, 700.0)]),
                Some(regular),
                "room {max_bytes}"
            );
            assert_eq!(
                cache.variable_instance(&id, &[(WGHT, 310.0)]),
                Some(light),
                "room {max_bytes}"
            );
            assert_eq!(cache.variable_instance_count(&id), 2, "room {max_bytes}");
            assert_eq!(
                cache.variable_instance(&id, &[(WGHT, 300.0)]),
                Some(light),
                "room {max_bytes}"
            );

            let variation = cache.variation(&id).unwrap();
            let key = cache.content_key(&id).unwrap();
            let wanted = canonical_cells(&variation, &[(WGHT, 500.0)]);
            assert!(
                matches!(
                    cache.lookup_instance(&key, &variation, &wanted),
                    Lookup::Found(found) if found == regular
                ),
                "room {max_bytes}"
            );
        }
    }

    #[test]
    fn the_count_bound_can_be_lowered() {
        let (cache, id) = cache_with(RED_HAT_MONO);
        cache.variation_write().max_instances = 2;
        let a = cache.variable_instance(&id, &[(WGHT, 300.0)]).unwrap();
        let b = cache.variable_instance(&id, &[(WGHT, 700.0)]).unwrap();
        assert_eq!(cache.variable_instance(&id, &[(WGHT, 600.0)]), Some(b));
        assert_eq!(cache.variable_instance(&id, &[(WGHT, 350.0)]), Some(a));
        assert_eq!(cache.variable_instance_count(&id), 2);
    }

    #[test]
    fn a_full_set_gives_the_nearest_at_the_lookup() {
        let (cache, id) = cache_with(RED_HAT_MONO);
        cache.variation_write().max_instances = 2;
        let variation = cache.variation(&id).unwrap();
        let key = cache.content_key(&id).unwrap();
        let wanted = canonical_cells(&variation, &[(WGHT, 600.0)]);
        // Cache miss
        assert!(matches!(
            cache.lookup_instance(&key, &variation, &wanted),
            Lookup::Make
        ));
        let light = cache.variable_instance(&id, &[(WGHT, 300.0)]).unwrap();
        assert!(matches!(
            cache.lookup_instance(&key, &variation, &wanted),
            Lookup::Make
        ));
        let bold = cache.variable_instance(&id, &[(WGHT, 700.0)]).unwrap();
        // Cache full: returns nearest
        assert!(matches!(
            cache.lookup_instance(&key, &variation, &wanted),
            Lookup::Found(found) if found == bold
        ));
        let near_light = canonical_cells(&variation, &[(WGHT, 320.0)]);
        assert!(matches!(
            cache.lookup_instance(&key, &variation, &near_light),
            Lookup::Found(found) if found == light
        ));
    }

    #[test]
    fn a_font_made_while_another_call_made_it_or_filled_the_set_is_dropped() {
        let (cache, id) = cache_with(RED_HAT_MONO);
        cache.variation_write().max_instances = 1;
        let first = cache.variable_instance(&id, &[(WGHT, 300.0)]).unwrap();
        let variation = cache.variation(&id).unwrap();
        let key = cache.content_key(&id).unwrap();
        let made = |wght: f32| {
            let cells = canonical_cells(&variation, &[(WGHT, wght)]);
            let font = FcCreateVariableInstance(RED_HAT_MONO, 0, &[(WGHT, wght)]).unwrap();
            (cells, Made::Font(font))
        };

        // Concurrent creation deduplication
        let (cells, font) = made(300.0);
        assert_eq!(
            cache.add_instance(&id, key, &variation, cells, font),
            Some(first)
        );
        // Set full: returns nearest
        let (cells, font) = made(700.0);
        assert_eq!(
            cache.add_instance(&id, key, &variation, cells, font),
            Some(first)
        );
        assert_eq!(cache.variable_instance_count(&id), 1);
        assert_eq!(cache.state_read().derived.len(), 1);
    }

    #[test]
    fn a_static_face_is_kept_as_static_and_an_unknown_id_is_not_kept() {
        let (cache, serif) = cache_with(INSTRUMENT_SERIF);
        assert_eq!(cache.variation(&serif), None);
        assert!(matches!(
            cache.variation_read().faces.get(&serif),
            Some(SourceFace {
                variation: None,
                ..
            })
        ));

        let unknown = FontId::new();
        assert_eq!(cache.variation(&unknown), None);
        assert!(!cache.variation_read().faces.contains_key(&unknown));

        // Synthetic faces are static
        let (cache, id) = cache_with(RED_HAT_MONO);
        let made = cache.variable_instance(&id, &[(WGHT, 650.0)]).unwrap();
        assert_eq!(cache.variation(&made), None);
        assert!(!cache.variation_read().faces.contains_key(&made));
    }

    #[test]
    fn a_font_that_cannot_be_instanced_is_kept_as_such() {
        // Missing gvar table
        let mut font = RED_HAT_MONO.to_vec();
        let at = font
            .windows(4)
            .position(|window| window == b"gvar")
            .expect("a table directory entry");
        font[at..at + 4].copy_from_slice(b"gvaZ");
        let (cache, id) = cache_with(&font);
        assert!(cache.variation(&id).is_some());
        assert_eq!(cache.variable_instance(&id, &[(WGHT, 650.0)]), None);
        let key = cache.content_key(&id).expect("a key");
        assert!(
            cache
                .variation_read()
                .sets
                .get(&key)
                .expect("a set")
                .unsupported
        );
        assert_eq!(cache.variable_instance(&id, &[(WGHT, 350.0)]), None);
        assert_eq!(cache.variable_instance_count(&id), 0);
    }
}
