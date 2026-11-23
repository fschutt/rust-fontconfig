//! Variable fonts: the axes of a face, and static faces made from it at chosen coordinates.
//!
//! `FcReadVariation` reads the axes of a face and `FcCreateVariableInstance` makes a static
//! font at chosen values of them. `FcFontCache::variable_instance` does the same inside a
//! cache and keeps the result as a synthetic face.
//!
//! A synthetic face is reachable by id only: `get_font_bytes`, `get_font_by_id`,
//! `get_metadata_by_id`, `get_memory_font` and `is_memory_font` return it for its id, while
//! `list`, `len`, `for_each_pattern`, `query` and the font chains do not include it.

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

/// Four-byte tag of a variation axis, such as `*b"wght"`.
pub type FcAxisTag = [u8; 4];

/// One axis of a variable font, as its `fvar` table declares it.
///
/// Values are in the unit of the axis: weight units for `wght`, percent for
/// `wdth`, points for `opsz`, degrees for `slnt`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FcAxis {
    pub tag: FcAxisTag,
    /// The lowest value.
    pub min: f32,
    /// The value of the default instance.
    pub default: f32,
    /// The highest value.
    pub max: f32,
    /// The `HIDDEN_AXIS` flag: the axis is not meant to be shown to a user.
    pub hidden: bool,
}

impl FcAxis {
    /// `value` limited to `min..=max`; NaN gives `default`.
    pub fn clamp(&self, value: f32) -> f32 {
        if value.is_nan() {
            self.default
        } else {
            value.max(self.min).min(self.max)
        }
    }
}

/// The axes of a variable font face, in the order of its `fvar` table.
#[derive(Debug, Clone, PartialEq)]
pub struct FcVariation {
    pub axes: Vec<FcAxis>,
}

impl FcVariation {
    /// The first axis with `tag`.
    pub fn axis(&self, tag: FcAxisTag) -> Option<&FcAxis> {
        self.axes.iter().find(|axis| axis.tag == tag)
    }
}

/// Most synthetic faces the cache makes from one font face; a further request
/// gets the nearest one that exists.
pub const FC_MAX_INSTANCES_PER_FACE: usize = 32;

/// Most bytes of synthetic faces the cache keeps for one font face. The first
/// one is kept whatever its size.
pub const FC_MAX_INSTANCE_BYTES_PER_FACE: usize = 128 << 20;

/// `fvar` axis flag: the axis is not meant to be shown to a user.
#[cfg(all(feature = "std", feature = "parsing"))]
const HIDDEN_AXIS: u16 = 0x0001;

/// A synthetic face: its bytes as a memory font, and its pattern.
#[derive(Debug)]
#[cfg_attr(not(all(feature = "std", feature = "parsing")), allow(dead_code))]
pub(crate) struct SyntheticFace {
    pub(crate) font: FcFont,
    pub(crate) pattern: FcPattern,
}

/// What the cache keeps about variable fonts: the lock `FcFontCacheShared::variation`.
#[cfg(all(feature = "std", feature = "parsing"))]
pub(crate) struct VariationState {
    /// The axes of each source face looked up so far.
    faces: HashMap<FontId, SourceFace>,
    /// The synthetic faces made from each font face, by content.
    sets: HashMap<ContentKey, InstanceSet>,
    /// `FC_MAX_INSTANCES_PER_FACE` and `FC_MAX_INSTANCE_BYTES_PER_FACE`; unit tests lower them.
    max_instances: usize,
    max_bytes: usize,
}

#[cfg(all(feature = "std", feature = "parsing"))]
impl Default for VariationState {
    fn default() -> Self {
        Self {
            faces: HashMap::new(),
            sets: HashMap::new(),
            max_instances: FC_MAX_INSTANCES_PER_FACE,
            max_bytes: FC_MAX_INSTANCE_BYTES_PER_FACE,
        }
    }
}

#[cfg(all(feature = "std", feature = "parsing"))]
impl VariationState {
    /// Whether `set` takes no further instance. Its first instance is always kept.
    fn is_full(&self, set: &InstanceSet) -> bool {
        !set.made.is_empty()
            && (set.closed || set.made.len() >= self.max_instances || set.bytes >= self.max_bytes)
    }
}

/// What was read from one source face.
#[cfg(all(feature = "std", feature = "parsing"))]
struct SourceFace {
    /// `None`: a static face.
    variation: Option<Arc<FcVariation>>,
    /// Set when the first instance of the face is wanted.
    content: Option<ContentKey>,
}

/// A font face by content, so that one font registered under several ids shares its instances.
/// The hash is of the whole file: a collision would give a wrong face.
#[cfg(all(feature = "std", feature = "parsing"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ContentKey {
    len: usize,
    hash: u64,
    index: usize,
}

/// The synthetic faces made from one font face, in the order they were made.
#[cfg(all(feature = "std", feature = "parsing"))]
#[derive(Default)]
struct InstanceSet {
    made: Vec<Instance>,
    /// The bytes of all of `made`.
    bytes: usize,
    /// No more instances are made: the next one would have gone over the byte bound.
    closed: bool,
    /// The font cannot be made into an instance.
    unsupported: bool,
}

#[cfg(all(feature = "std", feature = "parsing"))]
struct Instance {
    /// One value per axis in `fvar` order, in 1/64 units.
    cells: Vec<i32>,
    id: FontId,
}

/// The result of a look in the instances of a font face.
#[cfg(all(feature = "std", feature = "parsing"))]
enum Lookup {
    Found(FontId),
    Unsupported,
    Make,
}

/// The result of making an instance.
#[cfg(all(feature = "std", feature = "parsing"))]
enum Made {
    Font(Vec<u8>),
    /// The font cannot be made into an instance.
    Cannot,
    /// The bytes of the source face could not be read.
    NoSource,
}

/// An axis as the `fvar` table holds it: values are 16.16 fixed point numbers.
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

    /// A 16.16 number limited to the range of the axis.
    fn fixed(&self, raw: i64) -> Fixed {
        let (min, max) = (
            i64::from(self.min.raw_value()),
            i64::from(self.max.raw_value()),
        );
        Fixed::from_raw(raw.clamp(min, max) as i32)
    }

    /// The value for a cell of 1/64; the cell of the default is exactly the default.
    fn at_cell(&self, cell: i32) -> Fixed {
        if cell == to_cell(f32::from(self.default)) {
            self.default
        } else {
            self.fixed(i64::from(cell) * 1024)
        }
    }
}

/// The face `index` of a font file. A file with one face has no other index.
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

/// The axes of the `fvar` table. `None` without the table, without an axis, or when an axis has
/// a range that is not in order (minimum, default, maximum).
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

/// The axes of face `font_index` of `font_bytes`, in the order of its `fvar` table.
///
/// `None` for a static face, a font that does not parse, a file without that face, and an
/// `fvar` table without an axis or with an axis whose range is not in order.
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

/// A static font made from face `font_index` of the variable font `font_bytes`, with the axes
/// at `coordinates` (axis tag and value).
///
/// An axis that is not named stays at its default. A value outside the range of its axis is
/// clamped and NaN gives the default. A tag the face does not have is ignored, and a tag given
/// twice takes its last value. Values are not rounded here; the cache rounds them to 1/64.
///
/// `None` for a static face, and for a font whose outlines are neither TrueType with `gvar`
/// nor CFF2. A CFF2 font keeps its CFF2 table in the result.
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

/// One number of the font per axis for `coordinates`. The default value gives exactly the
/// default of the font, so asking for it is the same as not naming the axis.
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

/// One number of the font per axis for `cells`.
#[cfg(all(feature = "std", feature = "parsing"))]
fn tuple_for_cells(axes: &[Axis], cells: &[i32]) -> Vec<Fixed> {
    axes.iter()
        .zip(cells)
        .map(|(axis, &cell)| axis.at_cell(cell))
        .collect()
}

/// The value of the last coordinate with `tag`.
#[cfg(all(feature = "std", feature = "parsing"))]
fn last_value(coordinates: &[(FcAxisTag, f32)], tag: FcAxisTag) -> Option<f32> {
    coordinates
        .iter()
        .rev()
        .find(|(candidate, _)| *candidate == tag)
        .map(|(_, value)| *value)
}

/// The font at `tuple`: one value per axis in `fvar` order.
#[cfg(all(feature = "std", feature = "parsing"))]
fn make_static(provider: &DynamicFontTableProvider<'_>, tuple: &[Fixed]) -> Option<Vec<u8>> {
    allsorts::variations::instance(provider, tuple)
        .ok()
        .map(|(font, _)| font)
}

/// The font of face `font_index` at `cells` (see `Instance::cells`).
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

/// A value in 1/64 units.
#[cfg(all(feature = "std", feature = "parsing"))]
fn to_cell(value: f32) -> i32 {
    (value * 64.0).round() as i32
}

/// The coordinates of one instance: one cell of 1/64 per axis in `fvar` order. An axis that is
/// not named is at its default; a value is clamped first, and NaN is the default.
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

/// The text of a tag.
#[cfg(all(feature = "std", feature = "parsing"))]
fn tag_text(tag: FcAxisTag) -> String {
    String::from_utf8_lossy(&tag).into_owned()
}

/// The label of a synthetic face, such as `Red Hat Mono @ wght=650 wdth=87.5`: the source's
/// family, then the axes that are not at their default.
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

/// The width class nearest to a `wdth` value in percent, as an OS/2 table has it; a tie goes to
/// the narrower class.
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

/// The pattern of a synthetic face: the source's, with `family` and `name` set to the label,
/// and weight, stretch, italic and oblique set from the axes the face has.
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

/// The instance of `made` nearest to `wanted`: the sum over the axes of the difference as a
/// part of the range of the axis. A tie goes to the earlier id.
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

    /// The bytes and the face index of the face `id`.
    fn source_bytes(&self, id: &FontId) -> Option<(Arc<FontBytes>, usize)> {
        let index = match self.get_font_by_id(id)? {
            OwnedFontSource::Memory(font) => font.font_index,
            OwnedFontSource::Disk(path) => path.font_index,
        };
        Some((self.get_font_bytes(id)?, index))
    }

    /// The identity of the font face `source` by content. Reads the bytes the first time.
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

    /// The axes of face `id`; `None` for a static face, an unknown id and a font that does not
    /// parse.
    ///
    /// They are read from the bytes once and kept, the answer `None` of a static face too. An id
    /// the cache does not hold, and bytes that cannot be read, are not kept.
    pub fn variation(&self, id: &FontId) -> Option<Arc<FcVariation>> {
        let known = self
            .variation_read()
            .faces
            .get(id)
            .map(|face| face.variation.clone());
        if let Some(variation) = known {
            return variation;
        }
        // A synthetic face is static.
        if self.state_read().derived.contains_key(id) {
            return None;
        }
        let (bytes, index) = self.source_bytes(id)?;
        let variation = FcReadVariation(bytes.as_slice(), index).map(Arc::new);
        // If another call stored the answer first, that one is used.
        let mut state = self.variation_write();
        let face = state.faces.entry(*id).or_insert(SourceFace {
            variation,
            content: None,
        });
        face.variation.clone()
    }

    /// The synthetic face of the variable face `source` with its axes at `coordinates` (tag and
    /// value).
    ///
    /// A synthetic face is a static font made from the variable one. It is made on the first
    /// request and kept, and it is reachable by its id only: see the `variation` module.
    ///
    /// An axis that is not named stays at its default. A value outside the range of its axis is
    /// clamped and NaN gives the default. A tag the face does not have is ignored, and a tag
    /// given twice takes its last value. Values are rounded to 1/64, so 350.001 and 350 are one
    /// instance. The default coordinates give the default instance, a static face too.
    ///
    /// At most `FC_MAX_INSTANCES_PER_FACE` faces and `FC_MAX_INSTANCE_BYTES_PER_FACE` bytes are
    /// made from one font face; beyond that the nearest existing one is returned and none is
    /// removed. A font face is its bytes and its index, so a font registered under several ids
    /// shares its instances.
    ///
    /// The pattern of the face is the source's with `family` and `name` set to a label such as
    /// `Red Hat Mono @ wght=650`, which no family query finds. Weight, stretch, italic and
    /// oblique follow the axes `wght`, `wdth`, `ital` and `slnt` the face has.
    ///
    /// `None` if `source` is not a variable face, or if the font cannot be made into an instance
    /// (its outlines are neither TrueType with `gvar` nor CFF2). The second case is kept: the
    /// font is not tried again.
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
    /// // The same coordinates give the same id.
    /// assert_eq!(cache.variable_instance(&face, &[(*b"wght", 650.0)]), Some(semibold));
    /// // The face is a font file without variation tables.
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
        // No lock is held while the font is made: a large font takes a while.
        let made = match self.source_bytes(source) {
            Some((bytes, index)) => match make_from_cells(bytes.as_slice(), index, &wanted) {
                Some(font) => Made::Font(font),
                None => Made::Cannot,
            },
            None => Made::NoSource,
        };
        self.add_instance(source, key, &variation, wanted, made)
    }

    /// The number of synthetic faces made from the font face of `source`. They are shared by
    /// every id of the same bytes.
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

    /// The instance with the coordinates `wanted`, or the nearest one if the set is full.
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

    /// Register a font that was just made as a synthetic face. If another call made the same
    /// instance or filled the set in the meantime, its result is used and this font is dropped.
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
