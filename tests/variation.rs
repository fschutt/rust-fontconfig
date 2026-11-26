//! Integration tests for variable font variation and instances.

#![cfg(all(feature = "std", feature = "parsing"))]

use std::path::PathBuf;
use std::sync::Arc;

use rust_fontconfig::*;

/// Red Hat Mono test fixture (TrueType, wght 300..700).
const RED_HAT_MONO: &[u8] = include_bytes!("fixtures/RedHatMono-VariableFont_wght.ttf");
/// Cantarell test fixture (CFF2, wght 100..800).
const CANTARELL: &[u8] = include_bytes!("fixtures/Cantarell-VF.otf");
/// Static font test fixture.
const INSTRUMENT_SERIF: &[u8] = include_bytes!("fixtures/InstrumentSerif-Regular.ttf");

const WGHT: FcAxisTag = *b"wght";

/// Registers `bytes` as a memory font named `family`.
fn add_font(cache: &FcFontCache, bytes: &[u8], family: &str) -> FontId {
    let id = FontId::new();
    let pattern = FcPattern {
        name: Some(family.to_string()),
        family: Some(family.to_string()),
        ..Default::default()
    };
    let font = FcFont {
        bytes: bytes.into(),
        font_index: 0,
        id: family.to_string(),
    };
    cache.with_memory_font_with_id(id, pattern, font);
    id
}

fn cache_with(bytes: &[u8], family: &str) -> (FcFontCache, FontId) {
    let cache = FcFontCache::default();
    let id = add_font(&cache, bytes, family);
    (cache, id)
}

/// Reads font bytes from cache for `id`.
fn bytes_of(cache: &FcFontCache, id: &FontId) -> Vec<u8> {
    cache
        .get_font_bytes(id)
        .expect("the cache holds the face")
        .as_slice()
        .to_vec()
}

/// Extracts sfnt table tags from font bytes.
fn table_tags(font: &[u8]) -> Vec<String> {
    let num = u16::from_be_bytes([font[4], font[5]]) as usize;
    (0..num)
        .map(|i| String::from_utf8_lossy(&font[12 + i * 16..16 + i * 16]).into_owned())
        .collect()
}

/// Constructs an sfnt font from table data.
fn sfnt(version: &[u8], tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut tables = tables.to_vec();
    tables.sort_by_key(|(tag, _)| *tag);
    let n = tables.len();
    let entry_selector = (usize::BITS - 1 - n.leading_zeros()) as u16;
    let search_range = (1u16 << entry_selector) * 16;

    let mut out = Vec::new();
    out.extend_from_slice(version);
    out.extend_from_slice(&(n as u16).to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&((n as u16) * 16 - search_range).to_be_bytes());

    let mut body = Vec::new();
    let mut offset = 12 + n * 16;
    for (tag, data) in &tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        let pad = (4 - data.len() % 4) % 4;
        body.resize(body.len() + pad, 0);
        offset += data.len() + pad;
    }
    out.extend_from_slice(&body);
    out
}

/// Replaces or drops sfnt tables in `font`.
fn rebuild(font: &[u8], drop: &[&[u8; 4]], add: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    let num = u16::from_be_bytes([font[4], font[5]]) as usize;
    let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    for i in 0..num {
        let rec = 12 + i * 16;
        let tag: [u8; 4] = font[rec..rec + 4].try_into().unwrap();
        let offset = u32::from_be_bytes(font[rec + 8..rec + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(font[rec + 12..rec + 16].try_into().unwrap()) as usize;
        if !drop.contains(&&tag) && !add.iter().any(|(added, _)| *added == tag) {
            tables.push((tag, font[offset..offset + len].to_vec()));
        }
    }
    tables.extend(add);
    sfnt(&font[0..4], &tables)
}

/// Constructs a TTC font collection from font faces.
fn collection(fonts: &[&[u8]]) -> Vec<u8> {
    type Table = ([u8; 4], [u8; 4], Vec<u8>); // tag, checksum, data
    let faces: Vec<([u8; 4], Vec<Table>)> = fonts
        .iter()
        .map(|font| {
            let num = u16::from_be_bytes([font[4], font[5]]) as usize;
            let tables = (0..num)
                .map(|i| {
                    let rec = 12 + i * 16;
                    let offset =
                        u32::from_be_bytes(font[rec + 8..rec + 12].try_into().unwrap()) as usize;
                    let len =
                        u32::from_be_bytes(font[rec + 12..rec + 16].try_into().unwrap()) as usize;
                    (
                        font[rec..rec + 4].try_into().unwrap(),
                        font[rec + 4..rec + 8].try_into().unwrap(),
                        font[offset..offset + len].to_vec(),
                    )
                })
                .collect();
            (font[0..4].try_into().unwrap(), tables)
        })
        .collect();

    let mut out = Vec::new();
    out.extend_from_slice(b"ttcf");
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(faces.len() as u32).to_be_bytes());
    let mut directory_at = 12 + 4 * faces.len();
    for (_, tables) in &faces {
        out.extend_from_slice(&(directory_at as u32).to_be_bytes());
        directory_at += 12 + 16 * tables.len();
    }
    let mut data_at = directory_at;
    let mut data = Vec::new();
    for (version, tables) in &faces {
        let n = tables.len();
        let entry_selector = (usize::BITS - 1 - n.leading_zeros()) as u16;
        let search_range = (1u16 << entry_selector) * 16;
        out.extend_from_slice(version);
        out.extend_from_slice(&(n as u16).to_be_bytes());
        out.extend_from_slice(&search_range.to_be_bytes());
        out.extend_from_slice(&entry_selector.to_be_bytes());
        out.extend_from_slice(&((n as u16) * 16 - search_range).to_be_bytes());
        for (tag, checksum, table) in tables {
            out.extend_from_slice(tag);
            out.extend_from_slice(checksum);
            out.extend_from_slice(&(data_at as u32).to_be_bytes());
            out.extend_from_slice(&(table.len() as u32).to_be_bytes());
            data.extend_from_slice(table);
            let pad = (4 - table.len() % 4) % 4;
            data.resize(data.len() + pad, 0);
            data_at += table.len() + pad;
        }
    }
    out.extend_from_slice(&data);
    out
}

/// Constructs an `fvar` table from axis definitions.
fn fvar_table(axes: &[(FcAxisTag, f32, f32, f32, u16)]) -> Vec<u8> {
    let fixed = |value: f32| ((value * 65536.0) as i32).to_be_bytes();
    let mut out = Vec::new();
    out.extend_from_slice(&1u16.to_be_bytes()); // major version
    out.extend_from_slice(&0u16.to_be_bytes()); // minor version
    out.extend_from_slice(&16u16.to_be_bytes()); // axes array offset
    out.extend_from_slice(&2u16.to_be_bytes()); // reserved
    out.extend_from_slice(&(axes.len() as u16).to_be_bytes());
    out.extend_from_slice(&20u16.to_be_bytes()); // axis size
    out.extend_from_slice(&0u16.to_be_bytes()); // instance count
    out.extend_from_slice(&(4 + 4 * axes.len() as u16).to_be_bytes()); // instance size
    for (tag, min, default, max, flags) in axes {
        out.extend_from_slice(tag);
        out.extend_from_slice(&fixed(*min));
        out.extend_from_slice(&fixed(*default));
        out.extend_from_slice(&fixed(*max));
        out.extend_from_slice(&flags.to_be_bytes());
        out.extend_from_slice(&256u16.to_be_bytes()); // name id
    }
    out
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("rfc-variation-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_variable_font_has_its_axes_and_a_static_font_has_none() {
    let (cache, id) = cache_with(RED_HAT_MONO, "Red Hat Mono");
    let variation = cache.variation(&id).expect("Red Hat Mono is variable");
    assert_eq!(variation.axes.len(), 1);
    let axis = &variation.axes[0];
    assert_eq!(axis.tag, WGHT);
    assert_eq!((axis.min, axis.default, axis.max), (300.0, 400.0, 700.0));
    assert!(!axis.hidden);
    assert_eq!(variation.axis(WGHT), Some(axis));
    assert_eq!(variation.axis(*b"wdth"), None);

    // Cache and free function return identical axes
    assert_eq!(*variation, FcReadVariation(RED_HAT_MONO, 0).unwrap());
    assert!(Arc::ptr_eq(&variation, &cache.variation(&id).unwrap()));

    // Negative test cases (static, invalid, missing)
    let serif = add_font(&cache, INSTRUMENT_SERIF, "Instrument Serif");
    assert_eq!(cache.variation(&serif), None);
    assert_eq!(cache.variation(&FontId::new()), None);
    assert_eq!(FcReadVariation(INSTRUMENT_SERIF, 0), None);
    assert_eq!(FcReadVariation(b"this is not a font", 0), None);
    assert_eq!(FcReadVariation(RED_HAT_MONO, 1), None);
}

#[test]
fn an_fvar_table_gives_its_axes_in_order_with_range_and_hidden_flag() {
    let font = sfnt(
        &[0, 1, 0, 0],
        &[(
            *b"fvar",
            fvar_table(&[
                (*b"wdth", 62.5, 100.0, 125.0, 0),
                (*b"wght", 100.0, 400.0, 900.0, 0),
                (*b"opsz", 8.0, 14.0, 144.0, 0),
                (*b"slnt", -15.0, 0.0, 0.0, 1),
                (*b"ital", 0.0, 0.0, 1.0, 0xFFFE),
            ]),
        )],
    );
    let variation = FcReadVariation(&font, 0).expect("five axes");
    let axis = |tag, min, default, max, hidden| FcAxis {
        tag,
        min,
        default,
        max,
        hidden,
    };
    assert_eq!(
        variation.axes,
        vec![
            axis(*b"wdth", 62.5, 100.0, 125.0, false),
            axis(*b"wght", 100.0, 400.0, 900.0, false),
            axis(*b"opsz", 8.0, 14.0, 144.0, false),
            axis(*b"slnt", -15.0, 0.0, 0.0, true),
            axis(*b"ital", 0.0, 0.0, 1.0, false),
        ]
    );
    assert_eq!(variation.axis(*b"opsz"), Some(&variation.axes[2]));
    assert_eq!(variation.axis(*b"GRAD"), None);

    let wdth = variation.axis(*b"wdth").unwrap();
    assert_eq!(wdth.clamp(90.0), 90.0);
    assert_eq!(wdth.clamp(50.0), 62.5);
    assert_eq!(wdth.clamp(200.0), 125.0);
    assert_eq!(wdth.clamp(f32::NEG_INFINITY), 62.5);
    assert_eq!(wdth.clamp(f32::INFINITY), 125.0);
    assert_eq!(wdth.clamp(f32::NAN), 100.0);
    let slnt = variation.axis(*b"slnt").unwrap();
    assert_eq!(slnt.clamp(-30.0), -15.0);
    assert_eq!(slnt.clamp(10.0), 0.0);
    assert_eq!(slnt.clamp(f32::NAN), 0.0);

    // Cache read test
    let cache = FcFontCache::default();
    let id = add_font(&cache, &font, "Only fvar");
    assert_eq!(*cache.variation(&id).unwrap(), variation);
}

#[test]
fn an_fvar_table_that_cannot_be_used_is_not_variable() {
    let with_axes = |axes: &[(FcAxisTag, f32, f32, f32, u16)]| {
        sfnt(&[0, 1, 0, 0], &[(*b"fvar", fvar_table(axes))])
    };
    // Invalid axis range (min > max)
    assert_eq!(
        FcReadVariation(&with_axes(&[(WGHT, 700.0, 400.0, 300.0, 0)]), 0),
        None
    );
    assert_eq!(
        FcReadVariation(&with_axes(&[(WGHT, 300.0, 800.0, 700.0, 0)]), 0),
        None
    );
    // Empty axes array
    assert_eq!(FcReadVariation(&with_axes(&[]), 0), None);
    // Valid axes
    assert!(FcReadVariation(&with_axes(&[(WGHT, 300.0, 400.0, 700.0, 0)]), 0).is_some());
}

#[test]
fn the_same_coordinates_give_the_same_instance() {
    let (cache, id) = cache_with(RED_HAT_MONO, "Red Hat Mono");
    let made = cache
        .variable_instance(&id, &[(WGHT, 350.0)])
        .expect("an instance at wght=350");
    assert_ne!(made, id);
    assert_eq!(cache.variable_instance(&id, &[(WGHT, 350.0)]), Some(made));

    // Coordinates rounded to 1/64th units
    assert_eq!(cache.variable_instance(&id, &[(WGHT, 350.001)]), Some(made));
    let next = cache.variable_instance(&id, &[(WGHT, 350.02)]).unwrap();
    assert_ne!(next, made);
    assert_eq!(cache.variable_instance_count(&id), 2);

    // Missing axes ignored
    assert_eq!(
        cache.variable_instance(&id, &[(*b"wdth", 80.0), (WGHT, 350.0)]),
        Some(made)
    );
    // Duplicate tags take last value
    assert_eq!(
        cache.variable_instance(&id, &[(WGHT, 300.0), (WGHT, 350.0)]),
        Some(made)
    );
    assert_eq!(cache.variable_instance_count(&id), 2);

    // Distinct coordinates produce distinct instances
    let bold = cache.variable_instance(&id, &[(WGHT, 650.0)]).unwrap();
    assert!(bold != made && bold != next);
    assert_eq!(cache.variable_instance_count(&id), 3);
}

#[test]
fn coordinates_are_clamped_and_the_default_coordinates_are_an_instance() {
    let (cache, id) = cache_with(RED_HAT_MONO, "Red Hat Mono");
    let widest = cache.variable_instance(&id, &[(WGHT, 700.0)]).unwrap();
    let thinnest = cache.variable_instance(&id, &[(WGHT, 300.0)]).unwrap();
    assert_ne!(widest, thinnest);
    assert_eq!(
        cache.variable_instance(&id, &[(WGHT, 1000.0)]),
        Some(widest)
    );
    assert_eq!(
        cache.variable_instance(&id, &[(WGHT, f32::INFINITY)]),
        Some(widest)
    );
    assert_eq!(cache.variable_instance(&id, &[(WGHT, 1.0)]), Some(thinnest));
    assert_eq!(
        cache.variable_instance(&id, &[(WGHT, f32::NEG_INFINITY)]),
        Some(thinnest)
    );
    assert_eq!(cache.variable_instance_count(&id), 2);

    // Default coordinates test
    let default = cache
        .variable_instance(&id, &[])
        .expect("the default instance");
    assert!(default != widest && default != thinnest && default != id);
    assert_eq!(
        cache.variable_instance(&id, &[(WGHT, 400.0)]),
        Some(default)
    );
    assert_eq!(
        cache.variable_instance(&id, &[(WGHT, f32::NAN)]),
        Some(default)
    );
    assert_eq!(
        cache.variable_instance(&id, &[(*b"wdth", 80.0)]),
        Some(default)
    );
    assert_eq!(cache.variable_instance_count(&id), 3);

    // Instance produces a static font without variation tables
    let bytes = bytes_of(&cache, &default);
    let tags = table_tags(&bytes);
    for variable_only in ["fvar", "gvar", "avar", "HVAR", "MVAR"] {
        assert!(
            !tags.iter().any(|t| t == variable_only),
            "{variable_only} in {tags:?}"
        );
    }
    assert!(tags.iter().any(|t| t == "glyf"), "{tags:?}");
    assert_eq!(FcReadVariation(&bytes, 0), None);
    assert_ne!(bytes, RED_HAT_MONO);
    assert!(FcParseFontBytes(&bytes, "default instance").is_some());
}

#[test]
fn a_static_or_unknown_source_has_no_instance() {
    let (cache, serif) = cache_with(INSTRUMENT_SERIF, "Instrument Serif");
    let variable = add_font(&cache, RED_HAT_MONO, "Red Hat Mono");
    assert!(cache
        .variable_instance(&variable, &[(WGHT, 650.0)])
        .is_some());

    assert_eq!(cache.variable_instance(&serif, &[(WGHT, 650.0)]), None);
    assert_eq!(cache.variable_instance(&serif, &[]), None);
    assert_eq!(cache.variable_instance_count(&serif), 0);
    assert_eq!(
        cache.variable_instance(&FontId::new(), &[(WGHT, 650.0)]),
        None
    );
    assert_eq!(cache.variable_instance_count(&FontId::new()), 0);
}

#[test]
fn a_font_that_cannot_be_instanced_gives_none() {
    // Font missing gvar table cannot be instanced
    let font = rebuild(RED_HAT_MONO, &[b"gvar"], Vec::new());
    let (cache, id) = cache_with(&font, "Without gvar");
    assert!(cache.variation(&id).is_some());
    assert_eq!(cache.variable_instance(&id, &[(WGHT, 650.0)]), None);
    assert_eq!(cache.variable_instance(&id, &[]), None);
    assert_eq!(cache.variable_instance(&id, &[(WGHT, 650.0)]), None);
    assert_eq!(cache.variable_instance_count(&id), 0);
    assert_eq!(FcCreateVariableInstance(&font, 0, &[(WGHT, 650.0)]), None);

    // Font with only fvar table cannot be instanced
    let font = rebuild(
        INSTRUMENT_SERIF,
        &[],
        vec![(*b"fvar", fvar_table(&[(WGHT, 300.0, 400.0, 700.0, 0)]))],
    );
    let id = add_font(&cache, &font, "Only fvar");
    assert!(cache.variation(&id).is_some());
    assert_eq!(cache.variable_instance(&id, &[(WGHT, 650.0)]), None);
}

#[test]
fn an_instance_is_the_font_the_free_function_makes() {
    let (cache, id) = cache_with(RED_HAT_MONO, "Red Hat Mono");
    for coordinates in [
        vec![(WGHT, 650.0)],
        vec![(WGHT, 300.0)],
        vec![(WGHT, 350.0)],
        vec![],
    ] {
        let made = cache.variable_instance(&id, &coordinates).unwrap();
        assert_eq!(
            FcCreateVariableInstance(RED_HAT_MONO, 0, &coordinates),
            Some(bytes_of(&cache, &made)),
            "{coordinates:?}"
        );
    }

    // Coordinate clamping and NaN handling
    let at =
        |coordinates: &[(FcAxisTag, f32)]| FcCreateVariableInstance(RED_HAT_MONO, 0, coordinates);
    assert!(at(&[(WGHT, 650.0)]).is_some());
    assert_ne!(at(&[(WGHT, 650.0)]), at(&[(WGHT, 300.0)]));
    assert_eq!(at(&[(WGHT, 9999.0)]), at(&[(WGHT, 700.0)]));
    assert_eq!(at(&[(WGHT, -9999.0)]), at(&[(WGHT, 300.0)]));
    assert_eq!(at(&[(WGHT, f32::NAN)]), at(&[]));
    assert_eq!(at(&[(WGHT, 400.0)]), at(&[]));
    assert_eq!(at(&[(*b"wdth", 80.0)]), at(&[]));
    assert_eq!(at(&[(WGHT, 300.0), (WGHT, 650.0)]), at(&[(WGHT, 650.0)]));

    // Negative test cases
    assert_eq!(
        FcCreateVariableInstance(INSTRUMENT_SERIF, 0, &[(WGHT, 650.0)]),
        None
    );
    assert_eq!(
        FcCreateVariableInstance(RED_HAT_MONO, 3, &[(WGHT, 650.0)]),
        None
    );
    assert_eq!(
        FcCreateVariableInstance(b"this is not a font", 0, &[]),
        None
    );
}

#[test]
fn a_face_has_at_most_32_instances_and_a_further_request_gets_the_nearest() {
    assert_eq!(FC_MAX_INSTANCES_PER_FACE, 32);
    assert_eq!(FC_MAX_INSTANCE_BYTES_PER_FACE, 128 << 20);

    let (cache, id) = cache_with(RED_HAT_MONO, "Red Hat Mono");
    let at = |k: u32| 300.0 + 12.0 * k as f32;
    let made: Vec<FontId> = (0..32)
        .map(|k| {
            cache
                .variable_instance(&id, &[(WGHT, at(k))])
                .expect("below the bound")
        })
        .collect();
    let mut distinct = made.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), 32);
    assert_eq!(cache.variable_instance_count(&id), 32);
    let listed = cache.list();

    // Cache limit: returns nearest instance
    assert_eq!(
        cache.variable_instance(&id, &[(WGHT, 690.0)]),
        Some(made[31])
    );
    assert_eq!(
        cache.variable_instance(&id, &[(WGHT, 700.0)]),
        Some(made[31])
    );
    assert_eq!(
        cache.variable_instance(&id, &[(WGHT, 301.0)]),
        Some(made[0])
    );
    assert_eq!(
        cache.variable_instance(&id, &[(WGHT, 400.0)]),
        Some(made[8])
    );
    // Tie breaker: selects earlier instance
    assert_eq!(
        cache.variable_instance(&id, &[(WGHT, 306.0)]),
        Some(made[0])
    );
    assert_eq!(
        cache.variable_instance(&id, &[(WGHT, at(5))]),
        Some(made[5])
    );
    assert_eq!(cache.variable_instance_count(&id), 32);

    // Verifies instances are not exposed in lists
    for face in &made {
        assert!(cache.get_font_bytes(face).is_some());
    }
    assert_eq!(cache.list(), listed);
}

#[test]
fn synthetic_faces_are_reachable_by_id_only() {
    let (cache, id) = cache_with(RED_HAT_MONO, "Red Hat Mono");
    let listed = cache.list();
    let source = cache
        .get_metadata_by_id(&id)
        .expect("the source has a pattern");

    let synthetic = cache
        .variable_instance(&id, &[(WGHT, 650.0)])
        .expect("an instance");

    // Synthetic faces are not listed in queries or fallback chains
    assert_eq!(cache.list(), listed);
    assert_eq!(cache.len(), listed.len());
    assert!(!cache.is_empty());
    let mut visited = 0;
    cache.for_each_pattern(|_, found| {
        assert_ne!(*found, synthetic);
        visited += 1;
    });
    assert_eq!(visited, listed.len());

    let label = "Red Hat Mono @ wght=650".to_string();
    let by = |name: Option<&str>, family: Option<&str>| {
        cache.query(
            &FcPattern {
                name: name.map(str::to_string),
                family: family.map(str::to_string),
                ..Default::default()
            },
            &mut Vec::new(),
        )
    };
    assert_eq!(by(None, Some(&label)), None);
    assert_eq!(by(Some(&label), None), None);
    assert_eq!(by(None, Some("Red Hat Mono")).map(|m| m.id), Some(id));
    assert_eq!(by(Some("Red Hat Mono"), None).map(|m| m.id), Some(id));

    let chain = cache.resolve_font_chain(
        &["Red Hat Mono".to_string(), "monospace".to_string()],
        FcWeight::Bold,
        PatternMatch::DontCare,
        PatternMatch::DontCare,
        &mut Vec::new(),
    );
    let mut in_chain: Vec<FontId> = Vec::new();
    for group in &chain.css_fallbacks {
        in_chain.extend(group.fonts.iter().map(|m| m.id));
        for script in &group.script_fonts {
            in_chain.extend(script.fonts.iter().map(|m| m.id));
        }
    }
    for script in &chain.unicode_fallbacks {
        in_chain.extend(script.fonts.iter().map(|m| m.id));
    }
    in_chain.extend(chain.last_resort.iter().map(|m| m.id));
    assert!(
        in_chain.contains(&id),
        "the source is in the chain: {chain:?}"
    );
    assert!(!in_chain.contains(&synthetic), "{chain:?}");
    assert_eq!(
        chain.resolve_char(&cache, 'A').map(|(found, _)| found),
        Some(id)
    );

    // Synthetic faces are retrievable by ID
    let bytes = bytes_of(&cache, &synthetic);
    assert_eq!(FcReadVariation(&bytes, 0), None);
    assert_ne!(bytes, RED_HAT_MONO);
    let Some(OwnedFontSource::Memory(font)) = cache.get_font_by_id(&synthetic) else {
        panic!("a synthetic face is a memory font");
    };
    assert_eq!(&*font.bytes, &bytes[..]);
    assert_eq!(font.font_index, 0);
    assert!(cache.is_memory_font(&synthetic));
    assert_eq!(cache.get_memory_font(&synthetic), Some(font));
    assert_eq!(cache.variation(&synthetic), None);
    assert_eq!(cache.variable_instance(&synthetic, &[(WGHT, 300.0)]), None);

    // Pattern properties match instance coordinates
    let pattern = cache.get_metadata_by_id(&synthetic).expect("a pattern");
    assert_eq!(pattern.family.as_deref(), Some(label.as_str()));
    assert_eq!(pattern.name.as_deref(), Some(label.as_str()));
    assert_eq!(
        (pattern.weight, pattern.bold),
        (FcWeight::Bold, PatternMatch::True)
    );
    assert_eq!(pattern.unicode_ranges, source.unicode_ranges);
    assert_eq!(pattern.monospace, source.monospace);
    assert_eq!(pattern.stretch, source.stretch);
    assert_eq!(pattern.italic, source.italic);
    assert_eq!(pattern.metadata, source.metadata);
}

#[test]
fn the_pattern_of_a_synthetic_face_agrees_with_the_tables_of_the_font() {
    let (cache, id) = cache_with(RED_HAT_MONO, "Red Hat Mono");
    for weight in [300.0, 350.0, 400.0, 450.0, 550.0, 650.0, 700.0] {
        let made = cache.variable_instance(&id, &[(WGHT, weight)]).unwrap();
        let pattern = cache.get_metadata_by_id(&made).unwrap();
        let parsed = FcParseFontBytes(&bytes_of(&cache, &made), "instance").unwrap();
        assert_eq!(pattern.weight, parsed[0].0.weight, "weight {weight}");
        assert_eq!(pattern.bold, parsed[0].0.bold, "bold at weight {weight}");
        assert_eq!(
            pattern.unicode_ranges,
            FcFontCache::normalize_unicode_ranges(parsed[0].0.unicode_ranges.clone()),
            "weight {weight}"
        );
    }
    let default = cache.variable_instance(&id, &[]).unwrap();
    let pattern = cache.get_metadata_by_id(&default).unwrap();
    assert_eq!(pattern.family.as_deref(), Some("Red Hat Mono @ default"));
    assert_eq!(
        (pattern.weight, pattern.bold),
        (FcWeight::Normal, PatternMatch::False)
    );
}

#[test]
fn clones_share_their_instances_and_a_new_cache_does_not() {
    let (cache, id) = cache_with(RED_HAT_MONO, "Red Hat Mono");
    let clone = cache.clone();
    let made = clone
        .variable_instance(&id, &[(WGHT, 650.0)])
        .expect("an instance");
    assert_eq!(cache.variable_instance_count(&id), 1);
    assert_eq!(cache.variable_instance(&id, &[(WGHT, 650.0)]), Some(made));
    assert!(cache.get_font_bytes(&made).is_some());
    assert!(Arc::ptr_eq(
        &cache.variation(&id).unwrap(),
        &clone.variation(&id).unwrap()
    ));

    // Independent caches do not share instances
    let other = FcFontCache::default();
    let pattern = cache.get_metadata_by_id(&id).unwrap();
    let font = cache.get_memory_font(&id).unwrap();
    other.with_memory_font_with_id(id, pattern, font);
    assert_eq!(other.variable_instance_count(&id), 0);
    assert!(other.get_font_bytes(&made).is_none());
    assert!(!Arc::ptr_eq(
        &cache.variation(&id).unwrap(),
        &other.variation(&id).unwrap()
    ));
    let theirs = other
        .variable_instance(&id, &[(WGHT, 650.0)])
        .expect("an instance");
    assert_ne!(theirs, made);
    assert_eq!(other.variable_instance_count(&id), 1);
    assert_eq!(cache.variable_instance_count(&id), 1);
}

#[test]
fn one_font_registered_under_three_names_shares_its_instances() {
    let cache = FcFontCache::default();
    let ids: Vec<FontId> = ["Red Hat Mono", "\"Red Hat Mono\"", "'Red Hat Mono'"]
        .iter()
        .map(|family| add_font(&cache, RED_HAT_MONO, family))
        .collect();
    assert_eq!(cache.len(), 3);

    let first = cache
        .variable_instance(&ids[0], &[(WGHT, 650.0)])
        .expect("an instance");
    for id in &ids {
        assert_eq!(cache.variable_instance(id, &[(WGHT, 650.0)]), Some(first));
        assert_eq!(cache.variable_instance_count(id), 1);
    }
    let second = cache.variable_instance(&ids[2], &[(WGHT, 350.0)]).unwrap();
    assert_ne!(second, first);
    for id in &ids {
        assert_eq!(cache.variable_instance_count(id), 2);
    }
    assert_eq!(cache.len(), 3);
}

#[test]
fn two_fonts_of_one_size_are_two_font_faces() {
    // Distinct fonts with identical sizes generate separate instances
    let copyright: Vec<u8> = "Copyright"
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect();
    let mut other = RED_HAT_MONO.to_vec();
    let at = other
        .windows(copyright.len())
        .position(|window| window == copyright)
        .expect("the copyright text in the name table");
    other[at + 1] = b'X';
    assert_eq!(other.len(), RED_HAT_MONO.len());

    let cache = FcFontCache::default();
    let first = add_font(&cache, RED_HAT_MONO, "Red Hat Mono");
    let second = add_font(&cache, &other, "Red Hat Mono Copy");
    let made_first = cache
        .variable_instance(&first, &[(WGHT, 650.0)])
        .expect("an instance");
    let made_second = cache
        .variable_instance(&second, &[(WGHT, 650.0)])
        .expect("an instance");
    assert_ne!(made_first, made_second);
    assert_ne!(
        bytes_of(&cache, &made_first),
        bytes_of(&cache, &made_second)
    );
    assert_eq!(cache.variable_instance_count(&first), 1);
    assert_eq!(cache.variable_instance_count(&second), 1);
}

#[test]
fn a_font_on_disk_gives_instances_too() {
    let dir = TempDir::new();
    let path = dir.0.join("RedHatMono.ttf");
    std::fs::write(&path, RED_HAT_MONO).expect("write the font");
    let path = path.to_string_lossy().into_owned();

    let cache = FcFontCache::default();
    let pattern = FcPattern {
        name: Some("Red Hat Mono".to_string()),
        family: Some("Red Hat Mono".to_string()),
        ..Default::default()
    };
    cache.insert_builder_font(
        pattern,
        FcFontPath {
            path: path.clone(),
            font_index: 0,
            bytes_hash: 0,
        },
    );
    let id = cache.lookup_paths_cached(&path).expect("registered")[0];

    let variation = cache.variation(&id).expect("a variable face on disk");
    assert_eq!(
        variation.axis(WGHT).map(|a| (a.min, a.max)),
        Some((300.0, 700.0))
    );
    let made = cache
        .variable_instance(&id, &[(WGHT, 650.0)])
        .expect("an instance");
    assert_eq!(
        FcCreateVariableInstance(RED_HAT_MONO, 0, &[(WGHT, 650.0)]),
        Some(bytes_of(&cache, &made))
    );
    assert_eq!(cache.variable_instance_count(&id), 1);
}

#[test]
fn the_faces_of_a_collection_have_their_own_axes_and_instances() {
    // Multi-face collection test
    let font = collection(&[RED_HAT_MONO, CANTARELL]);
    let range = |index| {
        FcReadVariation(&font, index)
            .and_then(|variation| variation.axis(WGHT).map(|axis| (axis.min, axis.max)))
    };
    assert_eq!(range(0), Some((300.0, 700.0)));
    assert_eq!(range(1), Some((100.0, 800.0)));
    assert_eq!(range(2), None);

    // Multi-face registration test
    let cache = FcFontCache::default();
    let ids: Vec<FontId> = (0..2)
        .map(|index| {
            let id = FontId::new();
            let pattern = FcPattern {
                name: Some(format!("Face {index}")),
                family: Some("Collection".to_string()),
                ..Default::default()
            };
            let face = FcFont {
                bytes: font.clone().into(),
                font_index: index,
                id: format!("face {index}"),
            };
            cache.with_memory_font_with_id(id, pattern, face);
            id
        })
        .collect();
    assert_eq!(
        cache.variation(&ids[0]).unwrap().axis(WGHT).unwrap().max,
        700.0
    );
    assert_eq!(
        cache.variation(&ids[1]).unwrap().axis(WGHT).unwrap().max,
        800.0
    );

    // Each collection face generates instances from its own outlines
    let made: Vec<FontId> = ids
        .iter()
        .map(|id| {
            cache
                .variable_instance(id, &[(WGHT, 700.0)])
                .expect("an instance")
        })
        .collect();
    assert_ne!(made[0], made[1]);
    for (index, id) in made.iter().enumerate() {
        assert_eq!(
            FcCreateVariableInstance(&font, index, &[(WGHT, 700.0)]),
            Some(bytes_of(&cache, id)),
            "face {index}"
        );
    }
    assert_ne!(bytes_of(&cache, &made[0]), bytes_of(&cache, &made[1]));
    assert_eq!(cache.variable_instance_count(&ids[0]), 1);
    assert_eq!(cache.variable_instance_count(&ids[1]), 1);
}

#[test]
fn a_font_file_that_is_missing_is_not_remembered_as_static() {
    let dir = TempDir::new();
    let path = dir.0.join("Later.ttf").to_string_lossy().into_owned();
    let cache = FcFontCache::default();
    cache.insert_builder_font(
        FcPattern {
            name: Some("Later".to_string()),
            family: Some("Later".to_string()),
            ..Default::default()
        },
        FcFontPath {
            path: path.clone(),
            font_index: 0,
            bytes_hash: 0,
        },
    );
    let id = cache.lookup_paths_cached(&path).expect("registered")[0];

    // Missing files return None without caching negative result
    assert_eq!(cache.variation(&id), None);
    assert_eq!(cache.variable_instance(&id, &[(WGHT, 650.0)]), None);

    // Newly written file is detected on next call
    std::fs::write(&path, RED_HAT_MONO).expect("write the font");
    assert!(cache.variation(&id).is_some());
    assert!(cache.variable_instance(&id, &[(WGHT, 650.0)]).is_some());
}

#[test]
fn a_cff2_font_gives_a_static_face_that_keeps_its_cff2_outlines() {
    let (cache, id) = cache_with(CANTARELL, "Cantarell");
    let variation = cache.variation(&id).expect("Cantarell is variable");
    let wght = variation.axis(WGHT).expect("a weight axis");
    assert_eq!((wght.min, wght.default, wght.max), (100.0, 400.0, 800.0));

    let bold = cache
        .variable_instance(&id, &[(WGHT, 700.0)])
        .expect("an instance");
    let thin = cache
        .variable_instance(&id, &[(WGHT, 100.0)])
        .expect("an instance");
    assert_ne!(bold, thin);
    let bytes = bytes_of(&cache, &bold);
    let tags = table_tags(&bytes);
    assert!(tags.iter().any(|t| t == "CFF2"), "{tags:?}");
    assert!(!tags.iter().any(|t| t == "fvar"), "{tags:?}");
    assert_eq!(FcReadVariation(&bytes, 0), None);
    assert!(FcParseFontBytes(&bytes, "cantarell").is_some());
    assert_ne!(bytes, bytes_of(&cache, &thin));
    assert_eq!(
        FcCreateVariableInstance(CANTARELL, 0, &[(WGHT, 700.0)]),
        Some(bytes)
    );
}

#[test]
#[cfg(feature = "async-registry")]
fn the_cache_of_a_registry_shares_its_instances() {
    use rust_fontconfig::config::FcScanConfig;
    use rust_fontconfig::registry::FcFontRegistry;

    let registry = FcFontRegistry::new_with_config(FcScanConfig::empty());
    registry.register_memory_fonts(vec![NamedFont::new("Red Hat Mono", RED_HAT_MONO.to_vec())]);
    let first = registry.shared_cache();
    let id = first.list()[0].1;
    let made = first
        .variable_instance(&id, &[(WGHT, 650.0)])
        .expect("an instance");

    // Cloned caches share instance state
    let second = registry.shared_cache();
    assert_eq!(second.variable_instance_count(&id), 1);
    assert_eq!(second.variable_instance(&id, &[(WGHT, 650.0)]), Some(made));
    assert!(registry.cache.get_font_bytes(&made).is_some());
}

#[test]
#[cfg(not(feature = "single-thread-unsafe-locks"))]
fn racing_requests_for_one_instance_make_one() {
    let (cache, id) = cache_with(RED_HAT_MONO, "Red Hat Mono");
    let threads = 6;
    let start = Arc::new(std::sync::Barrier::new(threads));
    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let cache = cache.clone();
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                cache.variable_instance(&id, &[(WGHT, 650.0)])
            })
        })
        .collect();
    let results: Vec<Option<FontId>> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(results[0].is_some());
    assert!(
        results.iter().all(|found| *found == results[0]),
        "{results:?}"
    );
    assert_eq!(cache.variable_instance_count(&id), 1);
    assert!(cache.get_font_bytes(&results[0].unwrap()).is_some());
}

/// A store in memory that counts what the cache asks of it.
#[derive(Default)]
struct FakeStore {
    kept: std::sync::Mutex<std::collections::HashMap<FcInstanceKey, Vec<u8>>>,
    loads: std::sync::atomic::AtomicUsize,
    stores: std::sync::atomic::AtomicUsize,
}

impl FakeStore {
    fn loads(&self) -> usize {
        self.loads.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn stores(&self) -> usize {
        self.stores.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn keys(&self) -> Vec<FcInstanceKey> {
        self.kept.lock().unwrap().keys().cloned().collect()
    }

    /// Replaces what is kept for `key`.
    fn keep(&self, key: &FcInstanceKey, bytes: &[u8]) {
        self.kept
            .lock()
            .unwrap()
            .insert(key.clone(), bytes.to_vec());
    }
}

impl FcInstanceStore for FakeStore {
    fn load(&self, key: &FcInstanceKey) -> Option<Vec<u8>> {
        self.loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.kept.lock().unwrap().get(key).cloned()
    }

    fn store(&self, key: &FcInstanceKey, bytes: &[u8]) {
        self.stores
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.keep(key, bytes);
    }
}

/// A cache of the Red Hat Mono fixture whose instances go through `store`.
fn stored_cache(store: &Arc<FakeStore>) -> (FcFontCache, FontId) {
    let (cache, id) = cache_with(RED_HAT_MONO, "Red Hat Mono");
    cache.set_instance_store(store.clone());
    (cache, id)
}

#[test]
fn an_instance_is_made_once_kept_and_asked_for_by_the_font_the_face_and_the_axis_values() {
    let store = Arc::new(FakeStore::default());
    let (cache, id) = stored_cache(&store);
    let instance = cache.variable_instance(&id, &[(WGHT, 650.0)]).unwrap();
    assert_eq!(
        (store.loads(), store.stores()),
        (1, 1),
        "asked once, kept once"
    );
    let keys = store.keys();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].font_len, RED_HAT_MONO.len());
    assert_eq!(keys[0].font_index, 0);
    assert_eq!(keys[0].cells, vec![650 * 64], "the wght axis in 1/64 units");
    let kept = store.kept.lock().unwrap().get(&keys[0]).cloned().unwrap();
    assert_eq!(
        kept,
        bytes_of(&cache, &instance),
        "the store holds the bytes of the instance"
    );

    // The same request again is the cache's own: the store is not asked.
    assert_eq!(
        cache.variable_instance(&id, &[(WGHT, 650.0)]),
        Some(instance)
    );
    assert_eq!((store.loads(), store.stores()), (1, 1));
}

#[test]
fn a_second_run_takes_the_instance_from_the_store_and_makes_nothing() {
    let store = Arc::new(FakeStore::default());
    let (first, id) = stored_cache(&store);
    first.variable_instance(&id, &[(WGHT, 650.0)]).unwrap();
    let key = store.keys().remove(0);
    // What the store keeps for the key is a font that is NOT what the cache would make: if the
    // second cache returns it, it did not make the instance.
    store.keep(&key, INSTRUMENT_SERIF);

    let (second, id) = stored_cache(&store);
    let instance = second.variable_instance(&id, &[(WGHT, 650.0)]).unwrap();
    assert_eq!(bytes_of(&second, &instance), INSTRUMENT_SERIF);
    assert_eq!(
        store.stores(),
        1,
        "nothing was made, so nothing was kept again"
    );
}

#[test]
fn what_the_store_holds_that_is_not_a_static_font_is_made_again_and_kept_over_it() {
    let store = Arc::new(FakeStore::default());
    let (first, id) = stored_cache(&store);
    let made = first.variable_instance(&id, &[(WGHT, 650.0)]).unwrap();
    let expected = bytes_of(&first, &made);
    let key = store.keys().remove(0);

    for bad in [
        &b"not a font"[..],
        &[][..],
        RED_HAT_MONO,
        &expected[..expected.len() / 2],
    ] {
        store.keep(&key, bad);
        let stores_before = store.stores();
        let (cache, id) = stored_cache(&store);
        let instance = cache.variable_instance(&id, &[(WGHT, 650.0)]).unwrap();
        assert_eq!(
            bytes_of(&cache, &instance),
            expected,
            "made again, whatever the store held"
        );
        assert_eq!(
            store.stores(),
            stores_before + 1,
            "and kept over the bad entry"
        );
    }
}

#[test]
fn a_font_that_cannot_be_instanced_is_not_kept() {
    let store = Arc::new(FakeStore::default());
    let (cache, id) = cache_with(INSTRUMENT_SERIF, "Instrument Serif");
    cache.set_instance_store(store.clone());
    assert_eq!(cache.variable_instance(&id, &[(WGHT, 400.0)]), None);
    assert_eq!(
        (store.loads(), store.stores()),
        (0, 0),
        "a static face never reaches the store"
    );
}
