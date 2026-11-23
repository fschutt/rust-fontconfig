//! Variable fonts: the axes of a face, and static faces made from it at chosen coordinates.

use alloc::vec::Vec;

/// Four-byte tag of a variation axis, such as `*b"wght"`.
pub type FcAxisTag = [u8; 4];

/// One axis of a variable font, as its `fvar` table declares it.
///
/// Values are in the unit of the axis: weight units for `wght`, percent for
/// `wdth`, points for `opsz`, degrees for `slnt`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FcAxis {
    pub tag: FcAxisTag,
    pub min: f32,
    /// The value of the default instance.
    pub default: f32,
    pub max: f32,
    /// The `HIDDEN_AXIS` flag: the axis is not meant to be shown to a user.
    pub hidden: bool,
}

impl FcAxis {
    /// `value` limited to `min..=max`; NaN gives `default`.
    pub fn clamp(&self, value: f32) -> f32 {
        value
    }
}

/// The axes of a variable font face, in the order of its `fvar` table.
#[derive(Debug, Clone, PartialEq)]
pub struct FcVariation {
    pub axes: Vec<FcAxis>,
}

impl FcVariation {
    /// The first axis with `tag`.
    pub fn axis(&self, _tag: FcAxisTag) -> Option<&FcAxis> {
        None
    }
}

/// Most synthetic faces the cache makes from one font face; a further request
/// gets the nearest one that exists.
pub const FC_MAX_INSTANCES_PER_FACE: usize = 32;

/// Most bytes of synthetic faces the cache keeps for one font face. The first
/// one is kept whatever its size.
pub const FC_MAX_INSTANCE_BYTES_PER_FACE: usize = 128 << 20;

/// The axes of face `font_index` of `font_bytes`.
#[cfg(all(feature = "std", feature = "parsing"))]
#[allow(non_snake_case)]
pub fn FcReadVariation(_font_bytes: &[u8], _font_index: usize) -> Option<FcVariation> {
    None
}

/// A static font made from face `font_index` of the variable font `font_bytes`.
#[cfg(all(feature = "std", feature = "parsing"))]
#[allow(non_snake_case)]
pub fn FcCreateVariableInstance(
    _font_bytes: &[u8],
    _font_index: usize,
    _coordinates: &[(FcAxisTag, f32)],
) -> Option<Vec<u8>> {
    None
}

#[cfg(all(feature = "std", feature = "parsing"))]
impl crate::FcFontCache {
    /// The axes of face `id`.
    pub fn variation(&self, _id: &crate::FontId) -> Option<std::sync::Arc<FcVariation>> {
        None
    }

    /// The synthetic face of variable face `source` at `coordinates`.
    pub fn variable_instance(
        &self,
        _source: &crate::FontId,
        _coordinates: &[(FcAxisTag, f32)],
    ) -> Option<crate::FontId> {
        None
    }

    /// How many synthetic faces exist for the font face of `source`.
    pub fn variable_instance_count(&self, _source: &crate::FontId) -> usize {
        0
    }
}
