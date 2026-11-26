//! Variable font axes inspection and static instance generation.

#[cfg(all(feature = "std", feature = "parsing"))]
use alloc::string::String;
use alloc::vec::Vec;
#[cfg(all(feature = "std", feature = "parsing"))]
use core::fmt::Write;
#[cfg(all(feature = "std", feature = "parsing"))]
use std::collections::HashMap;
#[cfg(all(feature = "std", feature = "parsing"))]
use std::sync::Arc;

#[cfg(all(feature = "std", feature = "parsing"))]
use allsorts::binary::read::ReadScope;
#[cfg(all(feature = "std", feature = "parsing"))]
use allsorts::font_data::{DynamicFontTableProvider, FontData};
#[cfg(all(feature = "std", feature = "parsing"))]
use allsorts::tables::variable_fonts::fvar::FvarTable;
#[cfg(all(feature = "std", feature = "parsing"))]
use allsorts::tables::{Fixed, FontTableProvider, OpenTypeData};
#[cfg(all(feature = "std", feature = "parsing"))]
use allsorts::tag;

use crate::{FcFont, FcPattern};
#[cfg(all(feature = "std", feature = "parsing"))]
use crate::{
    FcFontCache, FcStretch, FcWeight, FontBytes, FontId, OwnedFontSource, PatternMatch,
    StReadGuard, StWriteGuard,
};

/// Four-byte variation axis tag (e.g. `*b"wght"`).
pub type FcAxisTag = [u8; 4];

/// A variable font axis defined in the `fvar` table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FcAxis {
    pub tag: FcAxisTag,
    /// Minimum axis value.
    pub min: f32,
    /// Default axis value.
    pub default: f32,
    /// Maximum axis value.
    pub max: f32,
    /// Whether the axis is hidden from UI.
    pub hidden: bool,
}

impl FcAxis {
    /// Clamps `value` to `min..=max` (returns `default` if NaN).
    pub fn clamp(&self, value: f32) -> f32 {
        if value.is_nan() {
            self.default
        } else {
            value.max(self.min).min(self.max)
        }
    }
}

/// Axes of a variable font face.
#[derive(Debug, Clone, PartialEq)]
pub struct FcVariation {
    pub axes: Vec<FcAxis>,
}

impl FcVariation {
    /// Returns the axis matching `tag`.
    pub fn axis(&self, tag: FcAxisTag) -> Option<&FcAxis> {
        self.axes.iter().find(|axis| axis.tag == tag)
    }
}

/// Maximum number of synthetic instances cached per font face.
pub const FC_MAX_INSTANCES_PER_FACE: usize = 32;

/// Maximum bytes of synthetic instances cached per font face.
pub const FC_MAX_INSTANCE_BYTES_PER_FACE: usize = 128 << 20;

/// What identifies an instance: the font file, the face in it and the axis values.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FcInstanceKey {
    /// Length of the font file in bytes.
    pub font_len: usize,
    /// Content hash of the font file.
    pub font_hash: u64,
    /// Index of the face in the file.
    pub font_index: usize,
    /// Axis values in 1/64 units, one per axis in `fvar` order.
    pub cells: Vec<i32>,
}

/// Keeps instances between runs. `FcFontCache::variable_instance` asks `load` before it makes an
/// instance and hands `store` the bytes it made; both run outside the cache's locks. What `load`
/// returns must be a static font: the store validates what it keeps.
pub trait FcInstanceStore: Send + Sync {
    /// The bytes kept for `key`, if any.
    fn load(&self, key: &FcInstanceKey) -> Option<Vec<u8>>;
    /// Keeps `bytes` for `key`.
    fn store(&self, key: &FcInstanceKey, bytes: &[u8]);
}

/// Flag indicating a hidden axis in `fvar`.
#[cfg(all(feature = "std", feature = "parsing"))]
const HIDDEN_AXIS: u16 = 0x0001;

/// Cached synthetic instance.
#[derive(Debug)]
#[cfg_attr(not(all(feature = "std", feature = "parsing")), allow(dead_code))]
pub(crate) struct SyntheticFace {
    pub(crate) font: FcFont,
    pub(crate) pattern: FcPattern,
}

/// Internal cache for variable font axes and generated instances.
#[cfg(all(feature = "std", feature = "parsing"))]
pub(crate) struct VariationState {
    /// Cached axes per source font ID.
    faces: HashMap<FontId, SourceFace>,
    /// Generated instances keyed by font content.
    sets: HashMap<ContentKey, InstanceSet>,
    /// Instance count limit.
    max_instances: usize,
    /// Total byte size limit.
    max_bytes: usize,
    /// Where instances are kept between runs.
    store: Option<Arc<dyn FcInstanceStore>>,
}

#[cfg(all(feature = "std", feature = "parsing"))]
impl Default for VariationState {
    fn default() -> Self {
        Self {
            faces: HashMap::new(),
            sets: HashMap::new(),
            max_instances: FC_MAX_INSTANCES_PER_FACE,
            max_bytes: FC_MAX_INSTANCE_BYTES_PER_FACE,
            store: None,
        }
    }
}

#[cfg(all(feature = "std", feature = "parsing"))]
impl VariationState {
    /// Checks whether an instance set has reached capacity.
    fn is_full(&self, set: &InstanceSet) -> bool {
        !set.made.is_empty()
            && (set.closed || set.made.len() >= self.max_instances || set.bytes >= self.max_bytes)
    }
}

/// Cached metadata for a source font face.
#[cfg(all(feature = "std", feature = "parsing"))]
struct SourceFace {
    /// None for static faces.
    variation: Option<Arc<FcVariation>>,
    /// Content key for sharing instances across IDs.
    content: Option<ContentKey>,
}

/// Content-based identity of a font face.
#[cfg(all(feature = "std", feature = "parsing"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ContentKey {
    len: usize,
    hash: u64,
    index: usize,
}

/// Instances generated for a font face.
#[cfg(all(feature = "std", feature = "parsing"))]
#[derive(Default)]
struct InstanceSet {
    made: Vec<Instance>,
    /// Total size of generated instances in bytes.
    bytes: usize,
    /// Whether the size limit has been reached.
    closed: bool,
    /// True if instance generation failed for this font.
    unsupported: bool,
}

#[cfg(all(feature = "std", feature = "parsing"))]
struct Instance {
    /// Coordinates per axis in 1/64th units.
    cells: Vec<i32>,
    id: FontId,
}

/// Instance lookup result.
#[cfg(all(feature = "std", feature = "parsing"))]
enum Lookup {
    Found(FontId),
    Unsupported,
    Make,
}

/// Instance creation result.
#[cfg(all(feature = "std", feature = "parsing"))]
enum Made {
    Font(Vec<u8>),
    /// Unsupported font format.
    Cannot,
    /// Source font bytes could not be read.
    NoSource,
}

/// Internal 16.16 fixed-point representation of an `fvar` axis.
#[cfg(all(feature = "std", feature = "parsing"))]
struct Axis {
    tag: FcAxisTag,
    min: Fixed,
    default: Fixed,
    max: Fixed,
    hidden: bool,
}

#[cfg(all(feature = "std", feature = "parsing"))]
impl Axis {
    fn to_public(&self) -> FcAxis {
        FcAxis {
            tag: self.tag,
            min: f32::from(self.min),
            default: f32::from(self.default),
            max: f32::from(self.max),
            hidden: self.hidden,
        }
    }

    /// Clamps a 16.16 value to the axis range.
    fn fixed(&self, raw: i64) -> Fixed {
        let (min, max) = (
            i64::from(self.min.raw_value()),
            i64::from(self.max.raw_value()),
        );
        Fixed::from_raw(raw.clamp(min, max) as i32)
    }

    /// Converts a 1/64th cell coordinate to a 16.16 fixed point value.
    fn at_cell(&self, cell: i32) -> Fixed {
        if cell == to_cell(f32::from(self.default)) {
            self.default
        } else {
            self.fixed(i64::from(cell) * 1024)
        }
    }
}

/// Returns a table provider for face `index`.
#[cfg(all(feature = "std", feature = "parsing"))]
fn face_provider<'a>(
    font_file: &FontData<'a>,
    index: usize,
) -> Option<DynamicFontTableProvider<'a>> {
    let one_face = match font_file {
        FontData::OpenType(font) => matches!(font.data, OpenTypeData::Single(_)),
        FontData::Woff(_) => true,
        FontData::Woff2(font) => font.collection_directory.is_none(),
    };
    if one_face && index != 0 {
        return None;
    }
    font_file.table_provider(index).ok()
}

/// Reads axes from the `fvar` table.
#[cfg(all(feature = "std", feature = "parsing"))]
fn read_axes(provider: &impl FontTableProvider) -> Option<Vec<Axis>> {
    let data = provider.table_data(tag::FVAR).ok()??;
    let fvar = ReadScope::new(&data).read::<FvarTable<'_>>().ok()?;
    let axes: Vec<Axis> = fvar
        .axes()
        .map(|axis| Axis {
            tag: axis.axis_tag.to_be_bytes(),
            min: axis.min_value,
            default: axis.default_value,
            max: axis.max_value,
            hidden: axis.flags & HIDDEN_AXIS != 0,
        })
        .collect();
    let in_order = axes
        .iter()
        .all(|axis| axis.min <= axis.default && axis.default <= axis.max);
    (!axes.is_empty() && in_order).then_some(axes)
}

/// Reads the variation axes for `font_index` from font bytes.
/// Returns `None` if the font is static or invalid.
#[cfg(all(feature = "std", feature = "parsing"))]
#[allow(non_snake_case)]
pub fn FcReadVariation(font_bytes: &[u8], font_index: usize) -> Option<FcVariation> {
    let font_file = ReadScope::new(font_bytes).read::<FontData<'_>>().ok()?;
    let provider = face_provider(&font_file, font_index)?;
    let axes = read_axes(&provider)?;
    Some(FcVariation {
        axes: axes.iter().map(Axis::to_public).collect(),
    })
}

/// Generates static font bytes at the given axis coordinates.
///
/// Unspecified axes remain at default. Out-of-range values are clamped.
/// Returns `None` if the font is static or unsupported.
#[cfg(all(feature = "std", feature = "parsing"))]
#[allow(non_snake_case)]
pub fn FcCreateVariableInstance(
    font_bytes: &[u8],
    font_index: usize,
    coordinates: &[(FcAxisTag, f32)],
) -> Option<Vec<u8>> {
    let font_file = ReadScope::new(font_bytes).read::<FontData<'_>>().ok()?;
    let provider = face_provider(&font_file, font_index)?;
    let axes = read_axes(&provider)?;
    make_static(&provider, &tuple_for_values(&axes, coordinates))
}

/// Maps coordinates to fixed-point axis tuples.
#[cfg(all(feature = "std", feature = "parsing"))]
fn tuple_for_values(axes: &[Axis], coordinates: &[(FcAxisTag, f32)]) -> Vec<Fixed> {
    axes.iter()
        .map(|axis| match last_value(coordinates, axis.tag) {
            Some(value) if !value.is_nan() && value != f32::from(axis.default) => {
                axis.fixed((f64::from(value) * 65536.0).round() as i64)
            }
            _ => axis.default,
        })
        .collect()
}

/// Maps 1/64th cell coordinates to fixed-point axis tuples.
#[cfg(all(feature = "std", feature = "parsing"))]
fn tuple_for_cells(axes: &[Axis], cells: &[i32]) -> Vec<Fixed> {
    axes.iter()
        .zip(cells)
        .map(|(axis, &cell)| axis.at_cell(cell))
        .collect()
}

/// Returns the last coordinate value matching `tag`.
#[cfg(all(feature = "std", feature = "parsing"))]
fn last_value(coordinates: &[(FcAxisTag, f32)], tag: FcAxisTag) -> Option<f32> {
    coordinates
        .iter()
        .rev()
        .find(|(candidate, _)| *candidate == tag)
        .map(|(_, value)| *value)
}

/// Instantiates a static font at the given coordinate tuple.
#[cfg(all(feature = "std", feature = "parsing"))]
fn make_static(provider: &DynamicFontTableProvider<'_>, tuple: &[Fixed]) -> Option<Vec<u8>> {
    allsorts::variations::instance(provider, tuple)
        .ok()
        .map(|(font, _)| font)
}

/// Instantiates a static font from quantized cell coordinates.
#[cfg(all(feature = "std", feature = "parsing"))]
fn make_from_cells(font_bytes: &[u8], font_index: usize, cells: &[i32]) -> Option<Vec<u8>> {
    let font_file = ReadScope::new(font_bytes).read::<FontData<'_>>().ok()?;
    let provider = face_provider(&font_file, font_index)?;
    let axes = read_axes(&provider)?;
    if axes.len() != cells.len() {
        return None;
    }
    make_static(&provider, &tuple_for_cells(&axes, cells))
}

/// Whether every table the directory of the sfnt `font` lists lies within its bytes (a cut file does not).
#[cfg(all(feature = "std", feature = "parsing"))]
fn tables_within(font: &[u8]) -> bool {
    if font.len() < 12 {
        return false;
    }
    let tables = usize::from(u16::from_be_bytes([font[4], font[5]]));
    (0..tables).all(|i| {
        font.get(12 + i * 16 + 8..12 + i * 16 + 16)
            .is_some_and(|record| {
                let offset =
                    u32::from_be_bytes([record[0], record[1], record[2], record[3]]) as usize;
                let length =
                    u32::from_be_bytes([record[4], record[5], record[6], record[7]]) as usize;
                offset
                    .checked_add(length)
                    .is_some_and(|end| end <= font.len())
            })
    })
}

/// The bytes of the instance at `wanted`: the store's when it has them as a font that parses, else
/// made and handed to the store.
#[cfg(all(feature = "std", feature = "parsing"))]
fn instance_bytes(
    store: Option<&dyn FcInstanceStore>,
    key: &ContentKey,
    font_bytes: &[u8],
    font_index: usize,
    wanted: &[i32],
) -> Option<Vec<u8>> {
    let Some(store) = store else {
        return make_from_cells(font_bytes, font_index, wanted);
    };
    let store_key = FcInstanceKey {
        font_len: key.len,
        font_hash: key.hash,
        font_index,
        cells: wanted.to_vec(),
    };
    if let Some(kept) = store.load(&store_key) {
        if tables_within(&kept)
            && ReadScope::new(&kept).read::<FontData<'_>>().is_ok()
            && FcReadVariation(&kept, 0).is_none()
        {
            return Some(kept);
        }
    }
    let font = make_from_cells(font_bytes, font_index, wanted)?;
    store.store(&store_key, &font);
    Some(font)
}

/// Quantizes a float value to 1/64th units.
#[cfg(all(feature = "std", feature = "parsing"))]
fn to_cell(value: f32) -> i32 {
    (value * 64.0).round() as i32
}

/// Converts coordinates to canonical 1/64th cell values per axis.
#[cfg(all(feature = "std", feature = "parsing"))]
pub(crate) fn canonical_cells(
    variation: &FcVariation,
    coordinates: &[(FcAxisTag, f32)],
) -> Vec<i32> {
    variation
        .axes
        .iter()
        .map(|axis| {
            let value = last_value(coordinates, axis.tag).map_or(axis.default, |v| axis.clamp(v));
            to_cell(value)
        })
        .collect()
}

/// Converts tag bytes to a string.
#[cfg(all(feature = "std", feature = "parsing"))]
fn tag_text(tag: FcAxisTag) -> String {
    String::from_utf8_lossy(&tag).into_owned()
}

/// Formats a descriptive label for a synthetic face (e.g. `Family @ wght=650`).
#[cfg(all(feature = "std", feature = "parsing"))]
fn synthetic_label(source: &FcPattern, variation: &FcVariation, cells: &[i32]) -> String {
    let family = source
        .family
        .as_deref()
        .or(source.name.as_deref())
        .unwrap_or("font");
    let mut label = String::from(family);
    label.push_str(" @");
    let mut changed = false;
    for (axis, &cell) in variation.axes.iter().zip(cells) {
        if cell != to_cell(axis.default) {
            changed = true;
            let _ = write!(label, " {}={}", tag_text(axis.tag), cell as f32 / 64.0);
        }
    }
    if !changed {
        label.push_str(" default");
    }
    label
}

/// Maps a `wdth` percentage to `FcStretch`.
#[cfg(all(feature = "std", feature = "parsing"))]
fn stretch_of(percent: f32) -> FcStretch {
    const CLASSES: [(f32, FcStretch); 9] = [
        (50.0, FcStretch::UltraCondensed),
        (62.5, FcStretch::ExtraCondensed),
        (75.0, FcStretch::Condensed),
        (87.5, FcStretch::SemiCondensed),
        (100.0, FcStretch::Normal),
        (112.5, FcStretch::SemiExpanded),
        (125.0, FcStretch::Expanded),
        (150.0, FcStretch::ExtraExpanded),
        (200.0, FcStretch::UltraExpanded),
    ];
    let mut best = CLASSES[0];
    for class in CLASSES {
        if (percent - class.0).abs() < (percent - best.0).abs() {
            best = class;
        }
    }
    best.1
}

/// Builds an `FcPattern` for a synthetic instance.
#[cfg(all(feature = "std", feature = "parsing"))]
pub(crate) fn synthetic_pattern(
    source: &FcPattern,
    variation: &FcVariation,
    cells: &[i32],
) -> FcPattern {
    let label = synthetic_label(source, variation, cells);
    let mut pattern = source.clone();
    pattern.name = Some(label.clone());
    pattern.family = Some(label);
    let flag = |on: bool| {
        if on {
            PatternMatch::True
        } else {
            PatternMatch::False
        }
    };
    for (axis, &cell) in variation.axes.iter().zip(cells) {
        let value = cell as f32 / 64.0;
        match &axis.tag {
            b"wght" => {
                pattern.weight = FcWeight::from_u16(value.round() as u16);
                pattern.bold = flag(pattern.weight >= FcWeight::SemiBold);
            }
            b"wdth" => {
                pattern.stretch = stretch_of(value);
                pattern.condensed = flag(pattern.stretch <= FcStretch::Condensed);
            }
            b"ital" => pattern.italic = flag(value >= 0.5),
            b"slnt" => pattern.oblique = flag(cell != 0),
            _ => {}
        }
    }
    pattern
}

/// Finds the existing instance closest to `wanted`.
#[cfg(all(feature = "std", feature = "parsing"))]
fn nearest(made: &[Instance], variation: &FcVariation, wanted: &[i32]) -> Option<FontId> {
    let distance = |cells: &[i32]| -> f64 {
        variation
            .axes
            .iter()
            .zip(cells)
            .zip(wanted)
            .map(|((axis, &have), &want)| {
                let range = f64::from(axis.max) - f64::from(axis.min);
                if range > 0.0 {
                    f64::from((have - want).abs()) / 64.0 / range
                } else {
                    0.0
                }
            })
            .sum()
    };
    made.iter()
        .map(|instance| (distance(&instance.cells), instance.id))
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|(_, id)| id)
}

#[cfg(all(feature = "std", feature = "parsing"))]
impl FcFontCache {
    fn variation_read(&self) -> StReadGuard<'_, VariationState> {
        match self.shared.variation.read() {
            Ok(g) => g,
            Err(e) => match e {},
        }
    }

    fn variation_write(&self) -> StWriteGuard<'_, VariationState> {
        match self.shared.variation.write() {
            Ok(g) => g,
            Err(e) => match e {},
        }
    }

    /// Returns font bytes and face index for `id`.
    fn source_bytes(&self, id: &FontId) -> Option<(Arc<FontBytes>, usize)> {
        let index = match self.get_font_by_id(id)? {
            OwnedFontSource::Memory(font) => font.font_index,
            OwnedFontSource::Disk(path) => path.font_index,
        };
        Some((self.get_font_bytes(id)?, index))
    }

    /// Computes or retrieves the content key for `source`.
    fn content_key(&self, source: &FontId) -> Option<ContentKey> {
        let known = self
            .variation_read()
            .faces
            .get(source)
            .and_then(|face| face.content);
        if known.is_some() {
            return known;
        }
        let (bytes, index) = self.source_bytes(source)?;
        let key = ContentKey {
            len: bytes.len(),
            hash: crate::utils::content_hash_u64(&bytes),
            index,
        };
        if let Some(face) = self.variation_write().faces.get_mut(source) {
            face.content = Some(key);
        }
        Some(key)
    }

    /// Returns the variation axes of face `id`, or `None` if static or unparseable.
    /// Results are cached.
    pub fn variation(&self, id: &FontId) -> Option<Arc<FcVariation>> {
        let known = self
            .variation_read()
            .faces
            .get(id)
            .map(|face| face.variation.clone());
        if let Some(variation) = known {
            return variation;
        }
        // Synthetic faces do not have variation tables.
        if self.state_read().derived.contains_key(id) {
            return None;
        }
        let (bytes, index) = self.source_bytes(id)?;
        let variation = FcReadVariation(bytes.as_slice(), index).map(Arc::new);
        // Cache the parsed variation.
        let mut state = self.variation_write();
        let face = state.faces.entry(*id).or_insert(SourceFace {
            variation,
            content: None,
        });
        face.variation.clone()
    }

    /// Returns a synthetic static face for `source` at the chosen axis `coordinates`.
    ///
    /// Instances are created on demand, cached, and retrievable by the returned `FontId`.
    /// Unspecified axes use defaults, and coordinates are rounded to 1/64th units.
    /// If capacity limits are reached, the nearest existing instance is returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_fontconfig::{FcFont, FcFontCache, FcPattern, FcReadVariation};
    ///
    /// let path = concat!(
    ///     env!("CARGO_MANIFEST_DIR"),
    ///     "/tests/fixtures/RedHatMono-VariableFont_wght.ttf"
    /// );
    /// let cache = FcFontCache::default();
    /// cache.with_memory_fonts(vec![(
    ///     FcPattern {
    ///         family: Some("Red Hat Mono".to_string()),
    ///         ..Default::default()
    ///     },
    ///     FcFont {
    ///         bytes: std::fs::read(path).unwrap().into(),
    ///         font_index: 0,
    ///         id: "red-hat-mono".to_string(),
    ///     },
    /// )]);
    /// let face = cache.list()[0].1;
    ///
    /// let semibold = cache.variable_instance(&face, &[(*b"wght", 650.0)]).unwrap();
    /// assert_eq!(cache.variable_instance(&face, &[(*b"wght", 650.0)]), Some(semibold));
    /// let bytes = cache.get_font_bytes(&semibold).unwrap();
    /// assert!(FcReadVariation(&bytes, 0).is_none());
    /// ```
    pub fn variable_instance(
        &self,
        source: &FontId,
        coordinates: &[(FcAxisTag, f32)],
    ) -> Option<FontId> {
        let variation = self.variation(source)?;
        let wanted = canonical_cells(&variation, coordinates);
        let key = self.content_key(source)?;
        match self.lookup_instance(&key, &variation, &wanted) {
            Lookup::Found(id) => return Some(id),
            Lookup::Unsupported => return None,
            Lookup::Make => {}
        }
        // Generate instance outside the lock.
        let store = self.variation_read().store.clone();
        let made = match self.source_bytes(source) {
            Some((bytes, index)) => {
                match instance_bytes(store.as_deref(), &key, bytes.as_slice(), index, &wanted) {
                    Some(font) => Made::Font(font),
                    None => Made::Cannot,
                }
            }
            None => Made::NoSource,
        };
        self.add_instance(source, key, &variation, wanted, made)
    }

    /// Sets the store instances are loaded from before they are made and kept in once made.
    pub fn set_instance_store(&self, store: Arc<dyn FcInstanceStore>) -> &Self {
        self.variation_write().store = Some(store);
        self
    }

    /// Returns the number of synthetic instances generated for `source`.
    pub fn variable_instance_count(&self, source: &FontId) -> usize {
        if self.variation(source).is_none() {
            return 0;
        }
        let Some(key) = self.content_key(source) else {
            return 0;
        };
        let state = self.variation_read();
        state.sets.get(&key).map_or(0, |set| set.made.len())
    }

    /// Looks up an existing instance or determines if a new one should be created.
    fn lookup_instance(&self, key: &ContentKey, variation: &FcVariation, wanted: &[i32]) -> Lookup {
        let state = self.variation_read();
        let Some(set) = state.sets.get(key) else {
            return Lookup::Make;
        };
        if set.unsupported {
            return Lookup::Unsupported;
        }
        if let Some(found) = set.made.iter().find(|instance| instance.cells == wanted) {
            return Lookup::Found(found.id);
        }
        if state.is_full(set) {
            return match nearest(&set.made, variation, wanted) {
                Some(id) => Lookup::Found(id),
                None => Lookup::Make,
            };
        }
        Lookup::Make
    }

    /// Registers a newly created synthetic instance in the cache.
    fn add_instance(
        &self,
        source: &FontId,
        key: ContentKey,
        variation: &FcVariation,
        wanted: Vec<i32>,
        made: Made,
    ) -> Option<FontId> {
        let bytes = match made {
            Made::Font(bytes) => bytes,
            Made::Cannot => {
                self.variation_write()
                    .sets
                    .entry(key)
                    .or_default()
                    .unsupported = true;
                return None;
            }
            Made::NoSource => return None,
        };
        let pattern = synthetic_pattern(&self.get_metadata_by_id(source)?, variation, &wanted);
        let font = FcFont {
            bytes: bytes.into(),
            font_index: 0,
            id: pattern.name.clone().unwrap_or_default(),
        };

        let mut state = self.variation_write();
        let (max_instances, max_bytes) = (state.max_instances, state.max_bytes);
        let set = state.sets.entry(key).or_default();
        if let Some(found) = set.made.iter().find(|instance| instance.cells == wanted) {
            return Some(found.id);
        }
        if !set.made.is_empty() {
            let over_bytes = set.bytes.saturating_add(font.bytes.len()) > max_bytes;
            if over_bytes {
                set.closed = true;
            }
            if over_bytes || set.made.len() >= max_instances {
                return nearest(&set.made, variation, &wanted);
            }
        }
        let id = FontId::new();
        set.bytes = set.bytes.saturating_add(font.bytes.len());
        set.made.push(Instance { cells: wanted, id });
        self.state_write()
            .derived
            .insert(id, SyntheticFace { font, pattern });
        Some(id)
    }
}

#[cfg(all(test, feature = "std", feature = "parsing"))]
#[path = "variation_test.rs"]
mod variation_test;
