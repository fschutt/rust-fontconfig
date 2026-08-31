//! # rust-fontconfig
//!
//! Pure-Rust rewrite of the Linux fontconfig library (no system dependencies). Enable the `parsing` feature to parse `.woff`, `.woff2`, `.ttc`, `.otf` and `.ttf` with allsorts.
//!
//! **NOTE**: Also works on Windows, macOS and WASM - without external dependencies!
//!
//! ## Usage
//!
//! ### Basic Font Query
//!
//! ```rust,no_run
//! use rust_fontconfig::{FcFontCache, FcPattern};
//!
//! fn main() {
//!     // Build the font cache
//!     let cache = FcFontCache::build();
//!
//!     // Query a font by name
//!     let results = cache.query(
//!         &FcPattern {
//!             name: Some(String::from("Arial")),
//!             ..Default::default()
//!         },
//!         &mut Vec::new() // Trace messages container
//!     );
//!
//!     if let Some(font_match) = results {
//!         println!("Font match ID: {:?}", font_match.id);
//!         println!("Font unicode ranges: {:?}", font_match.unicode_ranges);
//!     } else {
//!         println!("No matching font found");
//!     }
//! }
//! ```
//!
//! ### Resolve Font Chain and Query for Text
//!
//! ```rust,no_run
//! use rust_fontconfig::{FcFontCache, FcWeight, PatternMatch};
//!
//! fn main() {
//!     # #[cfg(feature = "std")]
//!     # {
//!     let cache = FcFontCache::build();
//!
//!     // Build font fallback chain (without text parameter)
//!     let font_chain = cache.resolve_font_chain(
//!         &["Arial".to_string(), "sans-serif".to_string()],
//!         FcWeight::Normal,
//!         PatternMatch::DontCare,
//!         PatternMatch::DontCare,
//!         &mut Vec::new(),
//!     );
//!
//!     // Query which fonts to use for specific text
//!     let text = "Hello 你好 Здравствуйте";
//!     let font_runs = font_chain.query_for_text(&cache, text);
//!
//!     println!("Text split into {} font runs:", font_runs.len());
//!     for run in font_runs {
//!         println!("  '{}' -> font {:?}", run.text, run.font_id);
//!     }
//!     # }
//! }
//! ```

#![allow(non_snake_case)]

// As of v4.1 this crate is std-only. The v4.0 `no_std` path is gone —
// it never supported the registry / multi-thread parsing anyway, and
// the shared-state `FcFontCache` refactor depends on `std::sync::RwLock`
// which is unavailable without std. Keeping the `alloc::` import paths
// means the existing call sites in this file and submodules keep
// compiling — in std builds `alloc` is just `core::alloc`'s companion
// crate already linked by the standard library.
extern crate alloc;

use alloc::collections::btree_map::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
#[cfg(all(feature = "std", feature = "parsing"))]
use allsorts::binary::read::ReadScope;
#[cfg(all(feature = "std", feature = "parsing"))]
use allsorts::get_name::fontcode_get_name;
#[cfg(all(feature = "std", feature = "parsing"))]
use allsorts::tables::os2::Os2;
#[cfg(all(feature = "std", feature = "parsing"))]
use allsorts::tables::{FontTableProvider, HheaTable, HmtxTable, MaxpTable};
#[cfg(all(feature = "std", feature = "parsing"))]
use allsorts::tag;
#[cfg(feature = "std")]
use std::path::PathBuf;

#[cfg(feature = "std")]
pub mod config;
#[cfg(feature = "std")]
pub mod fallback;
pub mod utils;
#[cfg(feature = "std")]
pub use config::{FcFallbackConfig, FcScriptFallback, GenericFamily};
#[cfg(feature = "std")]
use fallback::FontChainCacheKey;
#[cfg(feature = "std")]
pub use fallback::{CssFallbackGroup, FontFallbackChain, ScriptFallbackGroup};

#[cfg(feature = "ffi")]
pub mod ffi;

#[cfg(feature = "cache")]
pub mod disk_cache;
#[cfg(feature = "async-registry")]
pub mod multithread;
#[cfg(feature = "async-registry")]
pub mod registry;
#[cfg(feature = "async-registry")]
pub mod scoring;

#[cfg(all(target_os = "ios", feature = "std", feature = "parsing"))]
mod mobile_ios;

/// Operating system type for generic font family resolution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperatingSystem {
    Windows,
    Linux,
    MacOS,
    IOS,
    Android,
    Wasm,
}

impl OperatingSystem {
    /// Detect the current operating system at compile time
    pub fn current() -> Self {
        #[cfg(target_os = "windows")]
        return OperatingSystem::Windows;

        #[cfg(target_os = "linux")]
        return OperatingSystem::Linux;

        #[cfg(target_os = "macos")]
        return OperatingSystem::MacOS;

        #[cfg(target_os = "ios")]
        return OperatingSystem::IOS;

        #[cfg(target_os = "android")]
        return OperatingSystem::Android;

        #[cfg(target_family = "wasm")]
        return OperatingSystem::Wasm;

        #[cfg(not(any(
            target_os = "windows",
            target_os = "linux",
            target_os = "macos",
            target_os = "ios",
            target_os = "android",
            target_family = "wasm"
        )))]
        return OperatingSystem::Linux; // Default fallback
    }

    /// Built-in `serif` candidates for this OS, script-specific entries for
    /// `unicode_ranges` first. The data lives in [`FcFallbackConfig::os_defaults`].
    #[cfg(feature = "std")]
    #[deprecated(
        since = "5.0.0",
        note = "use `FcFallbackConfig::os_defaults(os).expand_generic(GenericFamily::Serif, ranges)`"
    )]
    pub fn get_serif_fonts(&self, unicode_ranges: &[UnicodeRange]) -> Vec<String> {
        FcFallbackConfig::os_defaults(*self).expand_generic(GenericFamily::Serif, unicode_ranges)
    }

    /// Built-in `sans-serif` candidates for this OS, script-specific entries
    /// for `unicode_ranges` first. The data lives in [`FcFallbackConfig::os_defaults`].
    #[cfg(feature = "std")]
    #[deprecated(
        since = "5.0.0",
        note = "use `FcFallbackConfig::os_defaults(os).expand_generic(GenericFamily::SansSerif, ranges)`"
    )]
    pub fn get_sans_serif_fonts(&self, unicode_ranges: &[UnicodeRange]) -> Vec<String> {
        FcFallbackConfig::os_defaults(*self)
            .expand_generic(GenericFamily::SansSerif, unicode_ranges)
    }

    /// Built-in `monospace` candidates for this OS, script-specific entries
    /// for `unicode_ranges` first. The data lives in [`FcFallbackConfig::os_defaults`].
    #[cfg(feature = "std")]
    #[deprecated(
        since = "5.0.0",
        note = "use `FcFallbackConfig::os_defaults(os).expand_generic(GenericFamily::Monospace, ranges)`"
    )]
    pub fn get_monospace_fonts(&self, unicode_ranges: &[UnicodeRange]) -> Vec<String> {
        FcFallbackConfig::os_defaults(*self)
            .expand_generic(GenericFamily::Monospace, unicode_ranges)
    }

    /// Expand one CSS family entry against the built-in tables for this OS.
    /// A named family expands to itself.
    #[cfg(feature = "std")]
    #[deprecated(
        since = "5.0.0",
        note = "use `FcFallbackConfig::os_defaults(os).expand_family(family, ranges)`"
    )]
    pub fn expand_generic_family(
        &self,
        family: &str,
        unicode_ranges: &[UnicodeRange],
    ) -> Vec<String> {
        FcFallbackConfig::os_defaults(*self).expand_family(family, unicode_ranges)
    }
}

/// Expand a CSS font stack against the built-in per-OS tables.
///
/// Kept for 4.x callers. Resolution no longer goes through this: the cache
/// resolves with its injected [`FcFallbackConfig`], and
/// [`FcFallbackConfig::os_defaults`] is the explicit opt-in to these tables.
#[cfg(feature = "std")]
#[deprecated(
    since = "5.0.0",
    note = "use `FcFallbackConfig::os_defaults(os).candidate_families(families, ranges)`"
)]
pub fn expand_font_families(
    families: &[String],
    os: OperatingSystem,
    unicode_ranges: &[UnicodeRange],
) -> Vec<String> {
    FcFallbackConfig::os_defaults(os).candidate_families(families, unicode_ranges)
}

/// UUID to identify a font (collections are broken up into separate fonts)
#[derive(Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
pub struct FontId(pub u128);

impl core::fmt::Debug for FontId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(self, f)
    }
}

impl core::fmt::Display for FontId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let id = self.0;
        write!(
            f,
            "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
            (id >> 96) & 0xFFFFFFFF,
            (id >> 80) & 0xFFFF,
            (id >> 64) & 0xFFFF,
            (id >> 48) & 0xFFFF,
            id & 0xFFFFFFFFFFFF
        )
    }
}

impl FontId {
    /// Generate a new unique FontId using an atomic counter
    pub fn new() -> Self {
        use core::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
        FontId(id)
    }
}

/// Whether a field is required to match (yes / no / don't care)
///
/// The discriminants are ABI: they are the values of `FcPatternMatch` in
/// `ffi/rust_fontconfig.h` (`FC_MATCH_TRUE = 0`, `FC_MATCH_FALSE = 1`,
/// `FC_MATCH_DONT_CARE = 2`) and cross the C boundary by value. They are
/// pinned explicitly because reordering the variants once silently swapped
/// them, so every C caller asking for `FC_MATCH_FALSE` got `True`. The
/// declaration order (which is what `serde` encodes) is unchanged, so
/// persisted manifests are unaffected. `tests/tests.rs` parses the header
/// and asserts the values agree.
#[derive(Debug, Default, Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub enum PatternMatch {
    /// Default: don't particularly care whether the requirement matches
    #[default]
    DontCare = 2,
    /// Requirement has to be true for the selected font
    True = 0,
    /// Requirement has to be false for the selected font
    False = 1,
}

impl PatternMatch {
    fn needs_to_match(&self) -> bool {
        matches!(self, PatternMatch::True | PatternMatch::False)
    }

    fn matches(&self, other: &PatternMatch) -> bool {
        match (self, other) {
            (PatternMatch::DontCare, _) => true,
            (_, PatternMatch::DontCare) => true,
            (a, b) => a == b,
        }
    }
}

/// Font weight values as defined in CSS specification
#[derive(Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub enum FcWeight {
    Thin = 100,
    ExtraLight = 200,
    Light = 300,
    Normal = 400,
    Medium = 500,
    SemiBold = 600,
    Bold = 700,
    ExtraBold = 800,
    Black = 900,
}

impl FcWeight {
    pub fn from_u16(weight: u16) -> Self {
        match weight {
            0..=149 => FcWeight::Thin,
            150..=249 => FcWeight::ExtraLight,
            250..=349 => FcWeight::Light,
            350..=449 => FcWeight::Normal,
            450..=549 => FcWeight::Medium,
            550..=649 => FcWeight::SemiBold,
            650..=749 => FcWeight::Bold,
            750..=849 => FcWeight::ExtraBold,
            _ => FcWeight::Black,
        }
    }

    pub fn find_best_match(&self, available: &[FcWeight]) -> Option<FcWeight> {
        if available.is_empty() {
            return None;
        }

        // Exact match
        if available.contains(self) {
            return Some(*self);
        }

        // Get numeric value
        let self_value = *self as u16;

        match *self {
            FcWeight::Normal => {
                // For Normal (400), try Medium (500) first
                if available.contains(&FcWeight::Medium) {
                    return Some(FcWeight::Medium);
                }
                // Then try lighter weights
                for weight in &[FcWeight::Light, FcWeight::ExtraLight, FcWeight::Thin] {
                    if available.contains(weight) {
                        return Some(*weight);
                    }
                }
                // Last, try heavier weights
                for weight in &[
                    FcWeight::SemiBold,
                    FcWeight::Bold,
                    FcWeight::ExtraBold,
                    FcWeight::Black,
                ] {
                    if available.contains(weight) {
                        return Some(*weight);
                    }
                }
            }
            FcWeight::Medium => {
                // For Medium (500), try Normal (400) first
                if available.contains(&FcWeight::Normal) {
                    return Some(FcWeight::Normal);
                }
                // Then try lighter weights
                for weight in &[FcWeight::Light, FcWeight::ExtraLight, FcWeight::Thin] {
                    if available.contains(weight) {
                        return Some(*weight);
                    }
                }
                // Last, try heavier weights
                for weight in &[
                    FcWeight::SemiBold,
                    FcWeight::Bold,
                    FcWeight::ExtraBold,
                    FcWeight::Black,
                ] {
                    if available.contains(weight) {
                        return Some(*weight);
                    }
                }
            }
            FcWeight::Thin | FcWeight::ExtraLight | FcWeight::Light => {
                // For lightweight fonts (<400), first try lighter or equal weights
                let mut best_match = None;
                let mut smallest_diff = u16::MAX;

                // Find the closest lighter weight
                for weight in available {
                    let weight_value = *weight as u16;
                    // Only consider weights <= self (per test expectation)
                    if weight_value <= self_value {
                        let diff = self_value - weight_value;
                        if diff < smallest_diff {
                            smallest_diff = diff;
                            best_match = Some(*weight);
                        }
                    }
                }

                if best_match.is_some() {
                    return best_match;
                }

                // If no lighter weight, find the closest heavier weight
                best_match = None;
                smallest_diff = u16::MAX;

                for weight in available {
                    let weight_value = *weight as u16;
                    if weight_value > self_value {
                        let diff = weight_value - self_value;
                        if diff < smallest_diff {
                            smallest_diff = diff;
                            best_match = Some(*weight);
                        }
                    }
                }

                return best_match;
            }
            FcWeight::SemiBold | FcWeight::Bold | FcWeight::ExtraBold | FcWeight::Black => {
                // For heavyweight fonts (>500), first try heavier or equal weights
                let mut best_match = None;
                let mut smallest_diff = u16::MAX;

                // Find the closest heavier weight
                for weight in available {
                    let weight_value = *weight as u16;
                    // Only consider weights >= self
                    if weight_value >= self_value {
                        let diff = weight_value - self_value;
                        if diff < smallest_diff {
                            smallest_diff = diff;
                            best_match = Some(*weight);
                        }
                    }
                }

                if best_match.is_some() {
                    return best_match;
                }

                // If no heavier weight, find the closest lighter weight
                best_match = None;
                smallest_diff = u16::MAX;

                for weight in available {
                    let weight_value = *weight as u16;
                    if weight_value < self_value {
                        let diff = self_value - weight_value;
                        if diff < smallest_diff {
                            smallest_diff = diff;
                            best_match = Some(*weight);
                        }
                    }
                }

                return best_match;
            }
        }

        // If nothing matches by now, return the first available weight
        Some(available[0])
    }
}

impl Default for FcWeight {
    fn default() -> Self {
        FcWeight::Normal
    }
}

/// CSS font-stretch values
#[derive(Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub enum FcStretch {
    UltraCondensed = 1,
    ExtraCondensed = 2,
    Condensed = 3,
    SemiCondensed = 4,
    Normal = 5,
    SemiExpanded = 6,
    Expanded = 7,
    ExtraExpanded = 8,
    UltraExpanded = 9,
}

impl FcStretch {
    pub fn is_condensed(&self) -> bool {
        use self::FcStretch::*;
        match self {
            UltraCondensed => true,
            ExtraCondensed => true,
            Condensed => true,
            SemiCondensed => true,
            Normal => false,
            SemiExpanded => false,
            Expanded => false,
            ExtraExpanded => false,
            UltraExpanded => false,
        }
    }
    pub fn from_u16(width_class: u16) -> Self {
        match width_class {
            1 => FcStretch::UltraCondensed,
            2 => FcStretch::ExtraCondensed,
            3 => FcStretch::Condensed,
            4 => FcStretch::SemiCondensed,
            5 => FcStretch::Normal,
            6 => FcStretch::SemiExpanded,
            7 => FcStretch::Expanded,
            8 => FcStretch::ExtraExpanded,
            9 => FcStretch::UltraExpanded,
            _ => FcStretch::Normal,
        }
    }

    /// Follows CSS spec for stretch matching
    pub fn find_best_match(&self, available: &[FcStretch]) -> Option<FcStretch> {
        if available.is_empty() {
            return None;
        }

        if available.contains(self) {
            return Some(*self);
        }

        // For 'normal' or condensed values, narrower widths are checked first, then wider values
        if *self <= FcStretch::Normal {
            // Find narrower values first
            let mut closest_narrower = None;
            for stretch in available.iter() {
                if *stretch < *self
                    && (closest_narrower.is_none() || *stretch > closest_narrower.unwrap())
                {
                    closest_narrower = Some(*stretch);
                }
            }

            if closest_narrower.is_some() {
                return closest_narrower;
            }

            // Otherwise, find wider values
            let mut closest_wider = None;
            for stretch in available.iter() {
                if *stretch > *self
                    && (closest_wider.is_none() || *stretch < closest_wider.unwrap())
                {
                    closest_wider = Some(*stretch);
                }
            }

            return closest_wider;
        } else {
            // For expanded values, wider values are checked first, then narrower values
            let mut closest_wider = None;
            for stretch in available.iter() {
                if *stretch > *self
                    && (closest_wider.is_none() || *stretch < closest_wider.unwrap())
                {
                    closest_wider = Some(*stretch);
                }
            }

            if closest_wider.is_some() {
                return closest_wider;
            }

            // Otherwise, find narrower values
            let mut closest_narrower = None;
            for stretch in available.iter() {
                if *stretch < *self
                    && (closest_narrower.is_none() || *stretch > closest_narrower.unwrap())
                {
                    closest_narrower = Some(*stretch);
                }
            }

            return closest_narrower;
        }
    }
}

impl Default for FcStretch {
    fn default() -> Self {
        FcStretch::Normal
    }
}

/// Unicode range representation for font matching
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
pub struct UnicodeRange {
    pub start: u32,
    pub end: u32,
}

/// The default set of Unicode-block fallback scripts that
/// [`FcFontCache::resolve_font_chain`] pulls in when no explicit
/// `scripts_hint` is supplied.
///
/// Keeping this exposed lets callers that *do* want the default
/// behaviour build the set explicitly — typically by union-ing it
/// with a detected-from-document set before calling
/// [`FcFontCache::resolve_font_chain_with_scripts`].
pub const DEFAULT_UNICODE_FALLBACK_SCRIPTS: &[UnicodeRange] = &[
    UnicodeRange {
        start: 0x0400,
        end: 0x04FF,
    }, // Cyrillic
    UnicodeRange {
        start: 0x0600,
        end: 0x06FF,
    }, // Arabic
    UnicodeRange {
        start: 0x0900,
        end: 0x097F,
    }, // Devanagari
    UnicodeRange {
        start: 0x3040,
        end: 0x309F,
    }, // Hiragana
    UnicodeRange {
        start: 0x30A0,
        end: 0x30FF,
    }, // Katakana
    UnicodeRange {
        start: 0x4E00,
        end: 0x9FFF,
    }, // CJK Unified Ideographs
    UnicodeRange {
        start: 0xAC00,
        end: 0xD7A3,
    }, // Hangul Syllables
];

impl UnicodeRange {
    pub fn contains(&self, c: char) -> bool {
        let c = c as u32;
        c >= self.start && c <= self.end
    }

    pub fn overlaps(&self, other: &UnicodeRange) -> bool {
        self.start <= other.end && other.start <= self.end
    }

    pub fn is_subset_of(&self, other: &UnicodeRange) -> bool {
        self.start >= other.start && self.end <= other.end
    }
}

/// Check if any range covers CJK Unified Ideographs, Hiragana, Katakana, or Hangul
pub fn has_cjk_ranges(ranges: &[UnicodeRange]) -> bool {
    const BLOCKS: [UnicodeRange; 4] = [
        UnicodeRange {
            start: 0x3040,
            end: 0x309F,
        }, // Hiragana
        UnicodeRange {
            start: 0x30A0,
            end: 0x30FF,
        }, // Katakana
        UnicodeRange {
            start: 0x4E00,
            end: 0x9FFF,
        }, // CJK Unified Ideographs
        UnicodeRange {
            start: 0xAC00,
            end: 0xD7AF,
        }, // Hangul Syllables
    ];
    ranges.iter().any(|r| BLOCKS.iter().any(|b| r.overlaps(b)))
}

/// Check if any range covers the Arabic block
pub fn has_arabic_ranges(ranges: &[UnicodeRange]) -> bool {
    ranges.iter().any(|r| {
        r.overlaps(&UnicodeRange {
            start: 0x0600,
            end: 0x06FF,
        })
    })
}

/// Check if any range covers the Cyrillic block
pub fn has_cyrillic_ranges(ranges: &[UnicodeRange]) -> bool {
    ranges.iter().any(|r| {
        r.overlaps(&UnicodeRange {
            start: 0x0400,
            end: 0x04FF,
        })
    })
}

/// Check if any range covers the Hebrew block
pub fn has_hebrew_ranges(ranges: &[UnicodeRange]) -> bool {
    ranges.iter().any(|r| {
        r.overlaps(&UnicodeRange {
            start: 0x0590,
            end: 0x05FF,
        })
    })
}

/// Check if any range covers the Thai block
pub fn has_thai_ranges(ranges: &[UnicodeRange]) -> bool {
    ranges.iter().any(|r| {
        r.overlaps(&UnicodeRange {
            start: 0x0E00,
            end: 0x0E7F,
        })
    })
}

/// Log levels for trace messages
#[derive(Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub enum TraceLevel {
    Debug,
    Info,
    Warning,
    Error,
}

/// Reason for font matching failure or success
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MatchReason {
    NameMismatch {
        requested: Option<String>,
        found: Option<String>,
    },
    FamilyMismatch {
        requested: Option<String>,
        found: Option<String>,
    },
    StyleMismatch {
        property: &'static str,
        requested: String,
        found: String,
    },
    WeightMismatch {
        requested: FcWeight,
        found: FcWeight,
    },
    StretchMismatch {
        requested: FcStretch,
        found: FcStretch,
    },
    UnicodeRangeMismatch {
        character: char,
        ranges: Vec<UnicodeRange>,
    },
    Success,
}

/// Trace message for debugging font matching
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceMsg {
    pub level: TraceLevel,
    pub path: String,
    pub reason: MatchReason,
}

/// Hinting style for font rendering.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
pub enum FcHintStyle {
    #[default]
    None = 0,
    Slight = 1,
    Medium = 2,
    Full = 3,
}

/// Subpixel rendering order.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
pub enum FcRgba {
    #[default]
    Unknown = 0,
    Rgb = 1,
    Bgr = 2,
    Vrgb = 3,
    Vbgr = 4,
    None = 5,
}

/// LCD filter mode for subpixel rendering.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
pub enum FcLcdFilter {
    #[default]
    None = 0,
    Default = 1,
    Light = 2,
    Legacy = 3,
}

/// Per-font rendering configuration from system font config (Linux fonts.conf).
///
/// All fields are `Option<T>` -- `None` means "use system default".
/// On non-Linux platforms, this is always all-None (no per-font overrides).
#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
pub struct FcFontRenderConfig {
    pub antialias: Option<bool>,
    pub hinting: Option<bool>,
    pub hintstyle: Option<FcHintStyle>,
    pub autohint: Option<bool>,
    pub rgba: Option<FcRgba>,
    pub lcdfilter: Option<FcLcdFilter>,
    pub embeddedbitmap: Option<bool>,
    pub embolden: Option<bool>,
    pub dpi: Option<f64>,
    pub scale: Option<f64>,
    pub minspace: Option<bool>,
}

/// Helper newtype to provide Eq/Ord for Option<f64> via total-order bit comparison.
/// This allows FcFontRenderConfig to be used inside FcPattern which derives Eq + Ord.
impl Eq for FcFontRenderConfig {}

// Equality and ordering all go through `Ord::cmp`, which compares the `f64`
// fields by bit pattern. One definition keeps the three consistent (a derived
// `PartialEq`/`PartialOrd` next to a hand-written `Ord` disagreed on NaN and
// tripped clippy's `derive_ord_xor_partial_ord`).
impl PartialEq for FcFontRenderConfig {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == core::cmp::Ordering::Equal
    }
}

impl PartialOrd for FcFontRenderConfig {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FcFontRenderConfig {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // Compare all non-f64 fields first
        let ord = self
            .antialias
            .cmp(&other.antialias)
            .then_with(|| self.hinting.cmp(&other.hinting))
            .then_with(|| self.hintstyle.cmp(&other.hintstyle))
            .then_with(|| self.autohint.cmp(&other.autohint))
            .then_with(|| self.rgba.cmp(&other.rgba))
            .then_with(|| self.lcdfilter.cmp(&other.lcdfilter))
            .then_with(|| self.embeddedbitmap.cmp(&other.embeddedbitmap))
            .then_with(|| self.embolden.cmp(&other.embolden))
            .then_with(|| self.minspace.cmp(&other.minspace));

        // For f64 fields, use to_bits() for total ordering
        let ord = ord.then_with(|| {
            let a = self.dpi.map(|v| v.to_bits());
            let b = other.dpi.map(|v| v.to_bits());
            a.cmp(&b)
        });
        ord.then_with(|| {
            let a = self.scale.map(|v| v.to_bits());
            let b = other.scale.map(|v| v.to_bits());
            a.cmp(&b)
        })
    }
}

/// Font pattern for matching
#[derive(Default, Clone, PartialOrd, Ord, PartialEq, Eq)]
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct FcPattern {
    // font name
    pub name: Option<String>,
    // family name
    pub family: Option<String>,
    // "italic" property
    pub italic: PatternMatch,
    // "oblique" property
    pub oblique: PatternMatch,
    // "bold" property
    pub bold: PatternMatch,
    // "monospace" property
    pub monospace: PatternMatch,
    // "condensed" property
    pub condensed: PatternMatch,
    // font weight
    pub weight: FcWeight,
    // font stretch
    pub stretch: FcStretch,
    // unicode ranges to match
    pub unicode_ranges: Vec<UnicodeRange>,
    // extended font metadata
    pub metadata: FcFontMetadata,
    // per-font rendering configuration (from system fonts.conf on Linux)
    pub render_config: FcFontRenderConfig,
}

impl core::fmt::Debug for FcPattern {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut d = f.debug_struct("FcPattern");

        if let Some(name) = &self.name {
            d.field("name", name);
        }

        if let Some(family) = &self.family {
            d.field("family", family);
        }

        if self.italic != PatternMatch::DontCare {
            d.field("italic", &self.italic);
        }

        if self.oblique != PatternMatch::DontCare {
            d.field("oblique", &self.oblique);
        }

        if self.bold != PatternMatch::DontCare {
            d.field("bold", &self.bold);
        }

        if self.monospace != PatternMatch::DontCare {
            d.field("monospace", &self.monospace);
        }

        if self.condensed != PatternMatch::DontCare {
            d.field("condensed", &self.condensed);
        }

        if self.weight != FcWeight::Normal {
            d.field("weight", &self.weight);
        }

        if self.stretch != FcStretch::Normal {
            d.field("stretch", &self.stretch);
        }

        if !self.unicode_ranges.is_empty() {
            d.field("unicode_ranges", &self.unicode_ranges);
        }

        // Only show non-empty metadata fields
        let empty_metadata = FcFontMetadata::default();
        if self.metadata != empty_metadata {
            d.field("metadata", &self.metadata);
        }

        // Only show render_config when it differs from default
        let empty_render_config = FcFontRenderConfig::default();
        if self.render_config != empty_render_config {
            d.field("render_config", &self.render_config);
        }

        d.finish()
    }
}

/// Font metadata from the OS/2 table
#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
pub struct FcFontMetadata {
    pub copyright: Option<String>,
    pub designer: Option<String>,
    pub designer_url: Option<String>,
    pub font_family: Option<String>,
    pub font_subfamily: Option<String>,
    pub full_name: Option<String>,
    pub id_description: Option<String>,
    pub license: Option<String>,
    pub license_url: Option<String>,
    pub manufacturer: Option<String>,
    pub manufacturer_url: Option<String>,
    pub postscript_name: Option<String>,
    pub preferred_family: Option<String>,
    pub preferred_subfamily: Option<String>,
    pub trademark: Option<String>,
    pub unique_id: Option<String>,
    pub version: Option<String>,
}

impl FcPattern {
    /// Check if this pattern would match the given character
    pub fn contains_char(&self, c: char) -> bool {
        if self.unicode_ranges.is_empty() {
            return true; // No ranges specified means match all characters
        }

        for range in &self.unicode_ranges {
            if range.contains(c) {
                return true;
            }
        }

        false
    }
}

/// Font match result with UUID
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontMatch {
    pub id: FontId,
    pub unicode_ranges: Vec<UnicodeRange>,
    pub fallbacks: Vec<FontMatchNoFallback>,
}

/// Font match result with UUID (without fallback)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontMatchNoFallback {
    pub id: FontId,
    pub unicode_ranges: Vec<UnicodeRange>,
}

/// A run of text that uses the same font
/// Returned by FontFallbackChain::query_for_text()
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedFontRun {
    /// The text content of this run
    pub text: String,
    /// Start byte index in the original text
    pub start_byte: usize,
    /// End byte index in the original text (exclusive)
    pub end_byte: usize,
    /// The font to use for this run (None if no font found)
    pub font_id: Option<FontId>,
    /// Which CSS font-family this came from
    pub css_source: String,
}

/// Path to a font file
///
/// `bytes_hash` is a deterministic 64-bit hash of the file's full
/// byte contents (see [`crate::utils::content_hash_u64`]). All faces
/// of a given `.ttc` file share the same `bytes_hash`, and two
/// different paths pointing at the same file contents also do —
/// so the cache can share a single `Arc<[u8]>` across them via
/// [`FcFontCache::get_font_bytes`]. A value of `0` means "hash
/// not computed" (e.g. built from a filename-only scan, or loaded
/// from a legacy v1 disk cache); callers must treat `0` as opaque
/// and fall back to unshared reads.
#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq)]
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct FcFontPath {
    pub path: String,
    pub font_index: usize,
    /// 64-bit content hash of the file's bytes. 0 = not computed.
    #[cfg_attr(feature = "cache", serde(default))]
    pub bytes_hash: u64,
}

/// In-memory font data
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct FcFont {
    pub bytes: Vec<u8>,
    pub font_index: usize,
    pub id: String, // For identification in tests
}

/// Owned font-source descriptor, returned by
/// [`FcFontCache::get_font_by_id`].
///
/// In v4.0 this was a borrowed enum (`FontSource<'a>` with refs into
/// the pattern map). With v4.1's shared-state cache, the map lives
/// behind an `RwLock`, so returning a reference would require the
/// caller to hold a read guard for the full lifetime of the result —
/// which bleeds the locking strategy into every call site. The owned
/// variant clones the small `FcFont` / `FcFontPath` struct and
/// releases the lock immediately. Bytes/mmap are not cloned — those
/// go through `get_font_bytes` which hands out `Arc<FontBytes>`.
#[derive(Debug, Clone)]
pub enum OwnedFontSource {
    /// Font loaded from memory (small metadata + owned `Vec<u8>`).
    Memory(FcFont),
    /// Font loaded from disk.
    Disk(FcFontPath),
}

/// A handle to font bytes returned by [`FcFontCache::get_font_bytes`].
///
/// On disk, an `Mmap` is used so untouched pages don't count toward
/// process RSS. In-memory fonts (`FcFont`) come back as `Owned` since
/// they're already on the heap.
///
/// `FontBytes` derefs to `[u8]` and implements `AsRef<[u8]>`, so any
/// existing API that wants `&[u8]` (allsorts, ttf-parser, …) can
/// accept it without code changes.
///
/// Both variants are `Send + Sync` (mmaps and `Arc<[u8]>` are both
/// safe to share across threads).
#[cfg(feature = "std")]
pub enum FontBytes {
    /// Heap-owned bytes. Used for `FontSource::Memory` and as a
    /// fallback when mmap is unavailable.
    Owned(std::sync::Arc<[u8]>),
    /// File-backed mmap. Read-only; pages are demand-loaded by the
    /// kernel. Absent on wasm targets, where `mmapio` is unavailable
    /// (the optional dep is gated to `cfg(not(target_family="wasm"))`).
    #[cfg(not(target_family = "wasm"))]
    Mmapped(mmapio::Mmap),
}

#[cfg(feature = "std")]
impl FontBytes {
    /// Borrow the underlying byte slice.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        match self {
            FontBytes::Owned(arc) => arc,
            #[cfg(not(target_family = "wasm"))]
            FontBytes::Mmapped(m) => &m[..],
        }
    }
}

#[cfg(feature = "std")]
impl core::ops::Deref for FontBytes {
    type Target = [u8];
    #[inline]
    fn deref(&self) -> &[u8] {
        self.as_slice()
    }
}

#[cfg(feature = "std")]
impl AsRef<[u8]> for FontBytes {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

#[cfg(feature = "std")]
impl core::fmt::Debug for FontBytes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let kind = match self {
            FontBytes::Owned(_) => "Owned",
            #[cfg(not(target_family = "wasm"))]
            FontBytes::Mmapped(_) => "Mmapped",
        };
        write!(f, "FontBytes::{}({} bytes)", kind, self.as_slice().len())
    }
}

/// Open a font file as an mmap-backed [`FontBytes`]. Falls back to a
/// heap read if mmap fails (e.g. the file is on a network share that
/// doesn't support mmap, or we're on a target without `std`-mmap).
#[cfg(feature = "std")]
fn open_font_bytes_mmap(path: &str) -> Option<std::sync::Arc<FontBytes>> {
    use std::fs::File;
    use std::sync::Arc;

    #[cfg(not(target_family = "wasm"))]
    {
        if let Ok(file) = File::open(path) {
            // Safety: `Mmap::map` requires that the file is not
            // mutated while mapped. For system fonts that's the
            // overwhelming common case; if a user replaces the file
            // we accept reading the snapshot we mapped earlier.
            if let Ok(mmap) = unsafe { mmapio::MmapOptions::new().map(&file) } {
                return Some(Arc::new(FontBytes::Mmapped(mmap)));
            }
        }
    }
    let bytes = std::fs::read(path).ok()?;
    Some(Arc::new(FontBytes::Owned(Arc::from(bytes))))
}

/// A named font to be added to the font cache from memory.
/// This is the primary way to supply custom fonts to the application.
#[derive(Debug, Clone)]
pub struct NamedFont {
    /// Human-readable name for this font (e.g., "My Custom Font")
    pub name: String,
    /// The raw font file bytes (TTF, OTF, WOFF, WOFF2, TTC)
    pub bytes: Vec<u8>,
}

impl NamedFont {
    /// Create a new named font from bytes
    pub fn new(name: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            bytes,
        }
    }
}

/// Font cache, initialized at startup.
///
/// Thread-safe, shared font cache.
///
/// As of v4.1 the cache internally owns its state via
/// `Arc<RwLock<FcFontCacheInner>>`: cloning an `FcFontCache` returns
/// a handle that shares the same underlying data. Writes by one holder
/// (typically the background builder inside `FcFontRegistry`) become
/// immediately visible to every other holder (layout engines,
/// shape-time resolvers, etc.).
///
/// Before 4.1 the clone deep-copied every map, so external holders
/// were frozen at the moment they took the snapshot — the mismatch
/// between "live registry cache" and "frozen font manager cache"
/// was the root of the silent-text regression when lazy scout mode
/// was enabled. The shared-state design eliminates that entire class
/// of staleness bugs by construction.
pub struct FcFontCache {
    pub(crate) shared: std::sync::Arc<FcFontCacheShared>,
}

/// Shared interior of `FcFontCache`. Always accessed through an
/// `Arc` — never referenced directly by external callers.
// Internal lock wrapper for the cache state. Two implementations selected by feature:
//
// DEFAULT (general builds): backed by std `RwLock`. `read`/`write`/`lock` return
// `Result<_, Infallible>` for a uniform call site (a poisoned lock is recovered via
// `into_inner` — a memoisation cache is still valid to read after a panic).
//
// `single-thread-unsafe-locks` feature: a bare `UnsafeCell` with NO atomics; `read`/`write`/
// `lock` hand out a guard immediately. UNSOUND in a multi-threaded program — enable ONLY for a
// known single-threaded environment. Exists for the azul remill-lifted web backend
// (single-threaded wasm), where std's queue-based RwLock `lock_contended` path spins forever
// (no other thread ever unparks it) and hangs the layout solver.

#[cfg(not(feature = "single-thread-unsafe-locks"))]
pub struct StLock<T> {
    lock: std::sync::RwLock<T>,
}
#[cfg(not(feature = "single-thread-unsafe-locks"))]
impl<T> core::fmt::Debug for StLock<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("StLock(..)")
    }
}
#[cfg(not(feature = "single-thread-unsafe-locks"))]
impl<T> StLock<T> {
    pub fn new(v: T) -> Self {
        Self {
            lock: std::sync::RwLock::new(v),
        }
    }
    pub fn read(&self) -> Result<StReadGuard<'_, T>, core::convert::Infallible> {
        Ok(StReadGuard {
            g: self.lock.read().unwrap_or_else(|e| e.into_inner()),
        })
    }
    pub fn write(&self) -> Result<StWriteGuard<'_, T>, core::convert::Infallible> {
        Ok(StWriteGuard {
            g: self.lock.write().unwrap_or_else(|e| e.into_inner()),
        })
    }
    pub fn lock(&self) -> Result<StWriteGuard<'_, T>, core::convert::Infallible> {
        self.write()
    }
}
#[cfg(not(feature = "single-thread-unsafe-locks"))]
pub struct StReadGuard<'a, T> {
    g: std::sync::RwLockReadGuard<'a, T>,
}
#[cfg(not(feature = "single-thread-unsafe-locks"))]
impl<'a, T> core::ops::Deref for StReadGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.g
    }
}
#[cfg(not(feature = "single-thread-unsafe-locks"))]
pub struct StWriteGuard<'a, T> {
    g: std::sync::RwLockWriteGuard<'a, T>,
}
#[cfg(not(feature = "single-thread-unsafe-locks"))]
impl<'a, T> core::ops::Deref for StWriteGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.g
    }
}
#[cfg(not(feature = "single-thread-unsafe-locks"))]
impl<'a, T> core::ops::DerefMut for StWriteGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.g
    }
}

#[cfg(feature = "single-thread-unsafe-locks")]
pub struct StLock<T> {
    cell: std::cell::UnsafeCell<T>,
}
#[cfg(feature = "single-thread-unsafe-locks")]
unsafe impl<T> Sync for StLock<T> {}
#[cfg(feature = "single-thread-unsafe-locks")]
unsafe impl<T> Send for StLock<T> {}
#[cfg(feature = "single-thread-unsafe-locks")]
impl<T> core::fmt::Debug for StLock<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("StLock(..)")
    }
}
#[cfg(feature = "single-thread-unsafe-locks")]
impl<T> StLock<T> {
    pub fn new(v: T) -> Self {
        Self {
            cell: std::cell::UnsafeCell::new(v),
        }
    }
    pub fn read(&self) -> Result<StReadGuard<'_, T>, core::convert::Infallible> {
        Ok(StReadGuard {
            r: unsafe { &*self.cell.get() },
        })
    }
    pub fn write(&self) -> Result<StWriteGuard<'_, T>, core::convert::Infallible> {
        Ok(StWriteGuard {
            r: unsafe { &mut *self.cell.get() },
        })
    }
    pub fn lock(&self) -> Result<StWriteGuard<'_, T>, core::convert::Infallible> {
        Ok(StWriteGuard {
            r: unsafe { &mut *self.cell.get() },
        })
    }
}
#[cfg(feature = "single-thread-unsafe-locks")]
pub struct StReadGuard<'a, T> {
    r: &'a T,
}
#[cfg(feature = "single-thread-unsafe-locks")]
impl<'a, T> core::ops::Deref for StReadGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.r
    }
}
#[cfg(feature = "single-thread-unsafe-locks")]
pub struct StWriteGuard<'a, T> {
    r: &'a mut T,
}
#[cfg(feature = "single-thread-unsafe-locks")]
impl<'a, T> core::ops::Deref for StWriteGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.r
    }
}
#[cfg(feature = "single-thread-unsafe-locks")]
impl<'a, T> core::ops::DerefMut for StWriteGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.r
    }
}

pub(crate) struct FcFontCacheShared {
    /// Main pattern/metadata state, guarded by a reader-writer lock.
    /// Builder threads take the write lock to insert a parsed font;
    /// all query paths take the read lock.
    pub(crate) state: StLock<FcFontCacheInner>,
    /// Font fallback chain cache. Not part of the RwLock-guarded
    /// state because cache insertions happen under `&self` on read
    /// paths (they're a memoisation, not observable state).
    pub(crate) chain_cache: StLock<std::collections::HashMap<FontChainCacheKey, FontFallbackChain>>,
    /// Shared file-bytes cache: content-hash → weak [`FontBytes`].
    ///
    /// [`FcFontCache::get_font_bytes`] populates this so that multiple
    /// FontIds backed by the same file (e.g. every face of a `.ttc`)
    /// return the same `Arc<FontBytes>` — and therefore the same mmap
    /// — instead of each allocating their own buffer. We hold `Weak`
    /// references so the mmap unmap as soon as no parsed font holds
    /// it alive.
    pub(crate) shared_bytes: StLock<std::collections::HashMap<u64, std::sync::Weak<FontBytes>>>,
}

/// The actual font-pattern state, held behind the RwLock in
/// `FcFontCacheShared`. Private — all access goes through
/// `FcFontCache` methods which lock transparently.
#[derive(Default, Debug)]
pub(crate) struct FcFontCacheInner {
    /// Disk font path -> the ids of its faces. Duplicate detection on
    /// insert and `lookup_paths_cached`.
    pub(crate) by_path: BTreeMap<String, Vec<FontId>>,
    /// On-disk font paths
    pub(crate) disk_fonts: BTreeMap<FontId, FcFontPath>,
    /// In-memory fonts
    pub(crate) memory_fonts: BTreeMap<FontId, FcFont>,
    /// Metadata cache (patterns stored by ID for quick lookup)
    pub(crate) metadata: BTreeMap<FontId, FcPattern>,
    /// Normalized family/name -> the fonts that carry it.
    ///
    /// The one way a specific family name is looked up (see
    /// `fallback::faces_for_family`). Built at insertion, so a lookup is a
    /// single map probe and a miss costs nothing; the linear scan it
    /// replaced measured ~0.52 ms per lookup from azul, ~150 lookups per
    /// CSS stack.
    pub(crate) family_index: BTreeMap<String, alloc::vec::Vec<FontId>>,
    /// What generic families, missing named families, script blocks and
    /// uncovered characters resolve to. Injected; see [`FcFallbackConfig`].
    pub(crate) fallback_config: FcFallbackConfig,
}

impl FcFontCacheInner {
    /// Record `id` under the normalized spellings of its family and name.
    /// Called by the `insert_*` paths under the write lock. Only
    /// `normalize_family_name` runs here — no Unicode tables — so it is
    /// safe on every target, including the azul web lift.
    pub(crate) fn index_pattern_family(&mut self, pattern: &FcPattern, id: FontId) {
        for key in [pattern.family.as_deref(), pattern.name.as_deref()]
            .into_iter()
            .flatten()
            .map(crate::utils::normalize_family_name)
            .filter(|k| !k.is_empty())
        {
            let slot = self.family_index.entry(key).or_default();
            if !slot.contains(&id) {
                slot.push(id);
            }
        }
    }

    /// Register a font backed by a file. The one place a pattern enters the
    /// state: `patterns`, `metadata`, the family index and the file map
    /// stay consistent by construction.
    ///
    /// Coverage is normalized (sorted, disjoint) on the way in; everything
    /// that reads it — `fallback::covers`, `fallback::overlap_size` — relies
    /// on that to binary-search.
    ///
    /// Returns the id the font is registered under: `id` when it was
    /// inserted, or the existing id when the same face of the same file with
    /// the same pattern is already registered — a directory scanned twice, a
    /// manifest loaded on top of a scan — so a font is one record no matter
    /// how many roads lead to it, and two different files that happen to
    /// carry identical name tables stay two records.
    pub(crate) fn insert_disk_font(
        &mut self,
        mut pattern: FcPattern,
        id: FontId,
        path: FcFontPath,
    ) -> FontId {
        pattern.unicode_ranges =
            FcFontCache::normalize_unicode_ranges(core::mem::take(&mut pattern.unicode_ranges));
        if let Some(existing) = self.by_path.get(&path.path).and_then(|ids| {
            ids.iter().copied().find(|existing| {
                self.disk_fonts
                    .get(existing)
                    .is_some_and(|p| p.font_index == path.font_index)
                    && self.metadata.get(existing) == Some(&pattern)
            })
        }) {
            return existing;
        }
        self.index_pattern_family(&pattern, id);
        self.by_path.entry(path.path.clone()).or_default().push(id);
        self.disk_fonts.insert(id, path);
        self.metadata.insert(id, pattern);
        id
    }

    /// Register a font held in memory. See [`insert_disk_font`](Self::insert_disk_font).
    /// Returns the id the font is registered under: `id`, or the existing id
    /// when a memory font with the same pattern, face index and bytes (by
    /// content hash) is already registered.
    pub(crate) fn insert_memory_font(
        &mut self,
        mut pattern: FcPattern,
        id: FontId,
        font: FcFont,
    ) -> FontId {
        pattern.unicode_ranges =
            FcFontCache::normalize_unicode_ranges(core::mem::take(&mut pattern.unicode_ranges));
        let hash = crate::utils::content_dedup_hash_u64(&font.bytes);
        if let Some(existing) = self.memory_fonts.iter().find_map(|(existing, f)| {
            (f.font_index == font.font_index
                && self.metadata.get(existing) == Some(&pattern)
                && crate::utils::content_dedup_hash_u64(&f.bytes) == hash)
                .then_some(*existing)
        }) {
            return existing;
        }
        self.index_pattern_family(&pattern, id);
        self.memory_fonts.insert(id, font);
        self.metadata.insert(id, pattern);
        id
    }
}

impl Clone for FcFontCache {
    /// Shallow clone — the returned handle shares the same underlying
    /// state as `self`. Writes through either are visible to both.
    /// This is the whole point of the v4.1 redesign; callers that need
    /// an isolated frozen copy must explicitly request one (e.g. via
    /// `snapshot_state`, which is intentionally not provided because
    /// we no longer have a use case for it).
    fn clone(&self) -> Self {
        Self {
            shared: std::sync::Arc::clone(&self.shared),
        }
    }
}

impl core::fmt::Debug for FcFontCache {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let state = self.state_read();
        f.debug_struct("FcFontCache")
            .field("fonts", &state.metadata.len())
            .field("metadata_len", &state.metadata.len())
            .field("disk_fonts_len", &state.disk_fonts.len())
            .field("memory_fonts_len", &state.memory_fonts.len())
            .finish()
    }
}

impl Default for FcFontCache {
    fn default() -> Self {
        Self {
            shared: std::sync::Arc::new(FcFontCacheShared {
                state: StLock::new(FcFontCacheInner::default()),
                chain_cache: StLock::new(std::collections::HashMap::new()),
                shared_bytes: StLock::new(std::collections::HashMap::new()),
            }),
        }
    }
}

impl FcFontCache {
    /// The fallback configuration this cache resolves with (a copy).
    pub fn fallback_config(&self) -> FcFallbackConfig {
        self.state_read().fallback_config.clone()
    }

    /// Replace the fallback configuration. Every memoized chain is dropped,
    /// so the next `resolve_font_chain` reflects it.
    pub fn set_fallback_config(&self, config: FcFallbackConfig) -> &Self {
        self.state_write().fallback_config = config;
        self.clear_chain_cache();
        self
    }

    /// Builder-style [`set_fallback_config`](Self::set_fallback_config).
    pub fn with_fallback_config(self, config: FcFallbackConfig) -> Self {
        self.set_fallback_config(config);
        self
    }

    /// Drop every memoized chain. Called on every insert and config change.
    pub(crate) fn clear_chain_cache(&self) {
        match self.shared.chain_cache.lock() {
            Ok(mut memo) => memo.clear(),
            Err(e) => match e {},
        }
    }

    /// The configured candidates for `family`: a generic keyword's base
    /// candidates, or a named family's substitutions.
    #[deprecated(
        since = "5.0.0",
        note = "read `fallback_config().generic_candidates(..)` / `substitutions_for(..)`"
    )]
    pub fn system_alias_prefs(&self, family: &str) -> Vec<String> {
        let state = self.state_read();
        match GenericFamily::from_css(family) {
            Some(generic) => state.fallback_config.generic_candidates(generic).to_vec(),
            None => state.fallback_config.substitutions_for(family).to_vec(),
        }
    }

    /// Expand a CSS stack through this cache's configuration, filling gaps
    /// from the built-in tables for `os`.
    #[deprecated(
        since = "5.0.0",
        note = "use `fallback_config().candidate_families(families, ranges)`"
    )]
    pub fn expand_font_families_config_first(
        &self,
        families: &[String],
        os: OperatingSystem,
        unicode_ranges: &[UnicodeRange],
    ) -> Vec<String> {
        let mut config = self.fallback_config();
        config.merge_defaults(&FcFallbackConfig::os_defaults(os));
        config.candidate_families(families, unicode_ranges)
    }

    /// Acquire a read guard on the cache's state. Panics if the lock
    /// was poisoned by a panic inside the write guard — same
    /// contract as `RwLock::read().expect(..)`.
    #[inline]
    pub(crate) fn state_read(&self) -> StReadGuard<'_, FcFontCacheInner> {
        // [az-web-lift] StLock::read() is Infallible (never poisons/spins).
        match self.shared.state.read() {
            Ok(g) => g,
            Err(e) => match e {},
        }
    }

    /// Acquire a write guard on the cache's state. Panics on
    /// poisoning, same as `state_read`.
    #[inline]
    pub(crate) fn state_write(&self) -> StWriteGuard<'_, FcFontCacheInner> {
        // [az-web-lift] StLock::write() is Infallible (never poisons/spins).
        match self.shared.state.write() {
            Ok(g) => g,
            Err(e) => match e {},
        }
    }

    /// Adds in-memory font files.
    ///
    /// Note: takes `&self` — the shared cache handles interior
    /// mutability via the RwLock.
    pub fn with_memory_fonts(&self, fonts: Vec<(FcPattern, FcFont)>) -> &Self {
        // Auto-detect Unicode coverage for any naively-registered font
        // (empty `unicode_ranges`) BEFORE taking the write lock, so we don't
        // hold it across font parsing. See `populate_memory_font_ranges`.
        let fonts: Vec<(FcPattern, FcFont)> = fonts
            .into_iter()
            .map(|(pattern, font)| (Self::populate_memory_font_ranges(pattern, &font), font))
            .collect();
        let mut state = self.state_write();
        for (pattern, font) in fonts {
            let id = FontId::new();
            state.insert_memory_font(pattern, id, font);
        }
        self
    }

    /// Adds a memory font with a specific ID (for testing).
    pub fn with_memory_font_with_id(&self, id: FontId, pattern: FcPattern, font: FcFont) -> &Self {
        let pattern = Self::populate_memory_font_ranges(pattern, &font);
        let mut state = self.state_write();
        state.insert_memory_font(pattern, id, font);
        self
    }

    /// Fill in a memory font's `unicode_ranges` from its raw bytes when the
    /// caller left them empty.
    ///
    /// A normal caller of [`FcFontCache::with_memory_fonts`] just hands over
    /// a name and the font bytes — they don't hand-compute the cmap. But
    /// [`FontFallbackChain::resolve_char`] deliberately skips any font that
    /// reports *no* coverage (it refuses to assume a blank range list means
    /// "covers everything"). Without this step a naively-registered bundled
    /// font could never be selected for any character — the exact bug that
    /// bites headless / wasm / embedder-bundled-font setups.
    ///
    /// With the `parsing` feature we reuse the *same* OS/2 + cmap detection
    /// pipeline the on-disk builder uses (via [`FcParseFontBytes`] →
    /// `parse_font_faces`). Without `parsing` the pattern is returned
    /// unchanged and the caller must populate `unicode_ranges` themselves.
    #[cfg(all(feature = "std", feature = "parsing"))]
    fn populate_memory_font_ranges(mut pattern: FcPattern, font: &FcFont) -> FcPattern {
        if !pattern.unicode_ranges.is_empty() {
            return pattern;
        }
        if let Some(faces) = FcParseFontBytes(&font.bytes, &font.id) {
            // A `.ttc` yields several faces; pick the one matching this
            // font's index, else fall back to the first parsed face. All
            // patterns of a single face share the same `unicode_ranges`.
            let ranges = faces
                .iter()
                .find(|(_, f)| f.font_index == font.font_index)
                .or_else(|| faces.first())
                .map(|(p, _)| p.unicode_ranges.clone())
                .unwrap_or_default();
            if !ranges.is_empty() {
                pattern.unicode_ranges = ranges;
            }
        }
        pattern
    }

    /// Without the `parsing` feature there is no cmap/OS2 parser available,
    /// so the caller-provided pattern is stored verbatim.
    #[cfg(not(all(feature = "std", feature = "parsing")))]
    fn populate_memory_font_ranges(pattern: FcPattern, _font: &FcFont) -> FcPattern {
        pattern
    }

    /// Register a newly-parsed on-disk font. Called by the builder
    /// thread inside `FcFontRegistry`. Allocates a fresh `FontId`,
    /// inserts the pattern + path + metadata in one write lock, and
    /// invalidates the chain cache so subsequent resolutions pick
    /// up the new font.
    pub fn insert_builder_font(&self, pattern: FcPattern, path: FcFontPath) {
        let id = FontId::new();
        {
            let mut state = self.state_write();
            state.insert_disk_font(pattern, id, path);
        }
        // Invalidate chain cache so callers see the new font on the
        // next resolve. Scoped after the state write to keep lock
        // nesting shallow.
        self.clear_chain_cache();
    }

    #[cfg(feature = "std")]
    #[doc(hidden)]
    pub fn chain_cache_len(&self) -> usize {
        self.shared.chain_cache.lock().map(|c| c.len()).unwrap_or(0)
    }

    /// Insert a *fast-probed* pattern into the cache and return its
    /// fresh `FontId`. Used by [`FcFontRegistry::request_fonts_fast`]
    /// when a cmap probe discovers a font that covers some subset of
    /// the requested codepoints. The pattern's `family` is guessed from
    /// the filename, so that guess is what the family index carries.
    pub fn insert_fast_pattern(&self, pattern: FcPattern, path: FcFontPath) -> FontId {
        let id = {
            let mut state = self.state_write();
            state.insert_disk_font(pattern, FontId::new(), path)
        };
        self.clear_chain_cache();
        id
    }

    /// Every `FontId` registered from the file at `path` (one per face and
    /// name), or `None` if nothing is. A map probe; `request_fonts_fast` uses
    /// it to reuse fast-probed faces across layout passes.
    pub fn lookup_paths_cached(&self, path: &str) -> Option<Vec<FontId>> {
        self.state_read()
            .by_path
            .get(path)
            .cloned()
            .filter(|ids| !ids.is_empty())
    }

    /// Get font data for a given font ID.
    ///
    /// Returns owned values (not references) because the underlying
    /// maps live behind an RwLock — a reference could not outlive
    /// the read guard. In-memory fonts come back as cloned `FcFont`
    /// instances; disk fonts return their `FcFontPath`.
    pub fn get_font_by_id(&self, id: &FontId) -> Option<OwnedFontSource> {
        let state = self.state_read();
        if let Some(font) = state.memory_fonts.get(id) {
            return Some(OwnedFontSource::Memory(font.clone()));
        }
        if let Some(path) = state.disk_fonts.get(id) {
            return Some(OwnedFontSource::Disk(path.clone()));
        }
        None
    }

    /// Get metadata for a font ID. Returns an owned `FcPattern`
    /// (cloned out of the shared map) because we can't return a
    /// reference across the RwLock boundary.
    pub fn get_metadata_by_id(&self, id: &FontId) -> Option<FcPattern> {
        self.state_read().metadata.get(id).cloned()
    }

    /// Get the font bytes for `id` as a shared [`FontBytes`].
    ///
    /// On disk the returned `Arc<FontBytes>` wraps an mmap of the file
    /// (`FontBytes::Mmapped`). Untouched pages of the file never count
    /// toward the process's RSS — for a font where layout shapes only
    /// a handful of glyphs, this is the difference between paying for
    /// the whole 4 MiB `.ttc` and paying for the cmap + a few glyf
    /// pages.
    ///
    /// In-memory fonts (`FontSource::Memory`) come back as
    /// `FontBytes::Owned`, since the bytes are already on the heap.
    ///
    /// Multiple `FontId`s backed by the same file content (every face
    /// of a `.ttc`, or two paths with identical bytes) return the
    /// *same* `Arc<FontBytes>` thanks to a content-hash → `Weak`
    /// cache. Bytes get unmapped automatically when the last consumer
    /// drops the Arc.
    ///
    /// `FontBytes` derefs to `[u8]`, so callers that only need
    /// `&[u8]` (allsorts, ttf-parser, …) can pass it through without
    /// thinking about the backing.
    ///
    /// Failure modes: returns `None` if the path is unknown, or the
    /// file no longer exists / cannot be opened, or the mmap call
    /// fails. Callers may retry with a fresh `get_font_bytes` if they
    /// suspect the file was replaced underneath them; the next call
    /// re-opens cleanly.
    #[cfg(feature = "std")]
    pub fn get_font_bytes(&self, id: &FontId) -> Option<std::sync::Arc<FontBytes>> {
        use std::sync::Arc;
        match self.get_font_by_id(id)? {
            OwnedFontSource::Memory(font) => {
                Some(Arc::new(FontBytes::Owned(Arc::from(font.bytes.as_slice()))))
            }
            OwnedFontSource::Disk(path) => {
                let hash = path.bytes_hash;
                if hash != 0 {
                    if let Ok(guard) = self.shared.shared_bytes.lock() {
                        if let Some(weak) = guard.get(&hash) {
                            if let Some(arc) = weak.upgrade() {
                                return Some(arc);
                            }
                        }
                    }
                }

                let arc = open_font_bytes_mmap(&path.path)?;
                if hash != 0 {
                    if let Ok(mut guard) = self.shared.shared_bytes.lock() {
                        // Overwrite any stale weak ref that failed to upgrade.
                        guard.insert(hash, Arc::downgrade(&arc));
                    }
                }
                Some(arc)
            }
        }
    }

    /// Returns an empty font cache (no_std / no filesystem).
    #[cfg(not(feature = "std"))]
    pub fn build() -> Self {
        Self::default()
    }

    /// Scans system font directories using filename heuristics (no allsorts).
    #[cfg(all(feature = "std", not(feature = "parsing")))]
    pub fn build() -> Self {
        Self::build_from_filenames()
    }

    /// Scans and parses all system fonts via allsorts for full metadata.
    #[cfg(all(feature = "std", feature = "parsing"))]
    pub fn build() -> Self {
        Self::build_inner(None)
    }

    /// Filename-only scan: discovers fonts on disk, guesses metadata from
    /// the filename using [`config::tokenize_font_stem`].
    #[cfg(all(feature = "std", not(feature = "parsing")))]
    fn build_from_filenames() -> Self {
        let cache = Self::default();
        {
            let mut state = cache.state_write();
            state.fallback_config = FcFallbackConfig::os_defaults(OperatingSystem::current());
            for dir in crate::config::font_directories(OperatingSystem::current()) {
                for path in FcCollectFontFilesRecursive(dir) {
                    let pattern = match pattern_from_filename(&path) {
                        Some(p) => p,
                        None => continue,
                    };
                    state.insert_disk_font(
                        pattern,
                        FontId::new(),
                        FcFontPath {
                            path: path.to_string_lossy().to_string(),
                            font_index: 0,
                            // Filename-only scan — we never read the bytes,
                            // so there's no dedup key. Leave as 0.
                            bytes_hash: 0,
                        },
                    );
                }
            }
        }
        cache
    }

    /// Builds a font cache with only specific font families (and their fallbacks).
    ///
    /// This is a performance optimization for applications that know ahead of time
    /// which fonts they need. Instead of scanning all system fonts (which can be slow
    /// on systems with many fonts), only fonts matching the specified families are loaded.
    ///
    /// Generic family names like "sans-serif", "serif", "monospace" are expanded
    /// to OS-specific font names (e.g., "sans-serif" on macOS becomes "Helvetica Neue",
    /// "San Francisco", etc.).
    ///
    /// **Note**: This will NOT automatically load fallback fonts for scripts not covered
    /// by the requested families. If you need Arabic, CJK, or emoji support, either:
    /// - Add those families explicitly to the filter
    /// - Use `with_memory_fonts()` to add bundled fonts
    /// - Use `build()` to load all system fonts
    ///
    /// # Arguments
    /// * `families` - Font family names to load (e.g., ["Arial", "sans-serif"])
    ///
    /// # Example
    /// ```ignore
    /// // Only load Arial and sans-serif fallback fonts
    /// let cache = FcFontCache::build_with_families(&["Arial", "sans-serif"]);
    /// ```
    #[cfg(all(feature = "std", feature = "parsing"))]
    pub fn build_with_families(families: &[impl AsRef<str>]) -> Self {
        // Expand generic families to OS-specific names. This runs BEFORE the
        // cache exists, so only the built-in lists are available here — the
        // filter is a superset selector (which files to parse), not the
        // final resolution, which goes config-first at query time.
        let os = OperatingSystem::current();
        let mut target_families: Vec<String> = Vec::new();

        for family in families {
            let family_str = family.as_ref();
            let expanded = FcFallbackConfig::os_defaults(os)
                .expand_family(family_str, DEFAULT_UNICODE_FALLBACK_SCRIPTS);
            if expanded.is_empty() || (expanded.len() == 1 && expanded[0] == family_str) {
                target_families.push(family_str.to_string());
            } else {
                target_families.extend(expanded);
            }
        }

        Self::build_inner(Some(&target_families))
    }

    /// Inner build function that handles both filtered and unfiltered font loading.
    ///
    /// # Arguments
    /// * `family_filter` - If Some, only load fonts matching these family names.
    ///                     If None, load all fonts.
    #[cfg(all(feature = "std", feature = "parsing"))]
    fn build_inner(family_filter: Option<&[String]>) -> Self {
        let cache = FcFontCache::default();

        // Normalize filter families for matching
        let filter_normalized: Option<Vec<String>> = family_filter.map(|families| {
            families
                .iter()
                .map(|f| crate::utils::normalize_family_name(f))
                .collect()
        });

        // Helper closure to check if a pattern matches the filter
        let matches_filter = |pattern: &FcPattern| -> bool {
            match &filter_normalized {
                None => true, // No filter = accept all
                Some(targets) => {
                    pattern.name.as_ref().map_or(false, |name| {
                        let name_norm = crate::utils::normalize_family_name(name);
                        targets.iter().any(|target| name_norm.contains(target))
                    }) || pattern.family.as_ref().map_or(false, |family| {
                        let family_norm = crate::utils::normalize_family_name(family);
                        targets.iter().any(|target| family_norm.contains(target))
                    })
                }
            }
        };

        let mut state = cache.state_write();
        state.fallback_config = FcFallbackConfig::os_defaults(OperatingSystem::current());

        #[cfg(target_os = "linux")]
        {
            if let Some((font_entries, render_configs, system_aliases)) = FcScanDirectories() {
                // The platform configuration is the authority; the built-in
                // tables only fill what it leaves unsaid.
                let mut config = FcFallbackConfig::default();
                config.absorb_system_aliases(system_aliases);
                config.merge_defaults(&state.fallback_config);
                state.fallback_config = config;
                for (mut pattern, path) in font_entries {
                    if matches_filter(&pattern) {
                        // Apply per-font render config if a matching family rule exists
                        if let Some(family) = pattern.name.as_ref().or(pattern.family.as_ref()) {
                            if let Some(rc) = render_configs.get(family) {
                                pattern.render_config = rc.clone();
                            }
                        }
                        let id = FontId::new();
                        state.insert_disk_font(pattern, id, path);
                    }
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            let system_root = std::env::var("SystemRoot")
                .or_else(|_| std::env::var("WINDIR"))
                .unwrap_or_else(|_| "C:\\Windows".to_string());

            let user_profile =
                std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\Default".to_string());

            let font_dirs = vec![
                (None, format!("{}\\Fonts\\", system_root)),
                (
                    None,
                    format!(
                        "{}\\AppData\\Local\\Microsoft\\Windows\\Fonts\\",
                        user_profile
                    ),
                ),
            ];

            let font_entries = FcScanDirectoriesInner(&font_dirs);
            for (pattern, path) in font_entries {
                if matches_filter(&pattern) {
                    let id = FontId::new();
                    state.insert_disk_font(pattern, id, path);
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            let font_dirs = vec![
                (None, "~/Library/Fonts".to_owned()),
                (None, "/System/Library/Fonts".to_owned()),
                (None, "/Library/Fonts".to_owned()),
                (None, "/System/Library/AssetsV2".to_owned()),
            ];

            let font_entries = FcScanDirectoriesInner(&font_dirs);
            for (pattern, path) in font_entries {
                if matches_filter(&pattern) {
                    let id = FontId::new();
                    state.insert_disk_font(pattern, id, path);
                }
            }
        }

        // iOS: the app sandbox denies a plain `read_dir` on `/System/Library/...`,
        // but `CTFontManagerCopyAvailableFontURLs` returns sandbox-mediated
        // `CFURL`s that *are* openable. We enumerate via CoreText, then feed
        // each URL into the same `FcParseFont` path the desktop arms use.
        #[cfg(target_os = "ios")]
        {
            let font_files = crate::mobile_ios::copy_available_font_urls();
            let font_entries = FcParseFontFiles(&font_files);
            for (pattern, path) in font_entries {
                if matches_filter(&pattern) {
                    let id = FontId::new();
                    state.insert_disk_font(pattern, id, path);
                }
            }
        }

        // Android: system fonts live at world-readable paths. Vendor partitions
        // (`/product/fonts`, `/system_ext/fonts`) carry OEM-specific families
        // on Samsung One UI / MIUI / EMUI; `/data/fonts` is the per-user font
        // dir on recent ROMs.
        #[cfg(target_os = "android")]
        {
            let font_dirs = vec![
                (None, "/system/fonts".to_owned()),
                (None, "/product/fonts".to_owned()),
                (None, "/system_ext/fonts".to_owned()),
                (None, "/data/fonts".to_owned()),
            ];

            let font_entries = FcScanDirectoriesInner(&font_dirs);
            for (pattern, path) in font_entries {
                if matches_filter(&pattern) {
                    let id = FontId::new();
                    state.insert_disk_font(pattern, id, path);
                }
            }
        }

        drop(state);
        cache
    }

    /// Check if a font ID is a memory font (preferred over disk fonts)
    pub fn is_memory_font(&self, id: &FontId) -> bool {
        self.state_read().memory_fonts.contains_key(id)
    }

    /// Every registered font with its pattern, in registration order.
    pub fn list(&self) -> Vec<(FcPattern, FontId)> {
        self.state_read()
            .metadata
            .iter()
            .map(|(id, pattern)| (pattern.clone(), *id))
            .collect()
    }

    /// Visit every registered font without cloning. The read lock is held
    /// for the duration, so `f` must not call back into this cache.
    pub fn for_each_pattern<F: FnMut(&FcPattern, &FontId)>(&self, mut f: F) {
        let state = self.state_read();
        for (id, pattern) in &state.metadata {
            f(pattern, id);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.state_read().metadata.is_empty()
    }

    /// Number of registered fonts (one per face and name record).
    pub fn len(&self) -> usize {
        self.state_read().metadata.len()
    }

    /// Like [`FcFontCache::query`], but **total**: it returns `None` only when the
    /// cache holds no fonts at all.
    ///
    /// This is the `fc-match` contract. `fc-match` never fails — fontconfig
    /// substitutes through its config chain, which is why `fc-match Cantarell`
    /// answers with e.g. `NotoSans-Regular.ttf` on a machine that has no
    /// Cantarell. [`FcFontCache::query`] deliberately does NOT do that: it is the
    /// honest "was this exact request satisfiable?" answer, and a caller that
    /// wants to report an unresolved family needs it.
    ///
    /// A *rendering* caller must use this one instead. Handing a renderer `None`
    /// means one of two things, and both are bugs the caller usually discovers
    /// far from here: text silently vanishes, or the caller invents its own
    /// fallback whose font is not registered where the renderer later looks it
    /// up by hash — so layout succeeds and rendering cannot resolve what layout
    /// produced.
    ///
    /// Resolution order, mirroring fontconfig's own relaxation:
    ///   1. the pattern exactly as given;
    ///   2. the same pattern with `name`/`family` cleared — keeps weight, slant,
    ///      monospace and the requested unicode coverage, so a Bold request does
    ///      not silently become Regular;
    ///   3. coverage only — the last-resort "any font that can draw this text".
    ///
    /// Each step is a strictly wider query than the last, so this never returns a
    /// *worse* match than `query` would have.
    pub fn query_with_fallback(
        &self,
        pattern: &FcPattern,
        trace: &mut Vec<TraceMsg>,
    ) -> Option<FontMatch> {
        if let Some(m) = self.query(pattern, trace) {
            return Some(m);
        }

        // 2. Drop the family/name constraint, keep how it should LOOK.
        if pattern.name.is_some() || pattern.family.is_some() {
            let relaxed = FcPattern {
                name: None,
                family: None,
                ..pattern.clone()
            };
            if let Some(m) = self.query(&relaxed, trace) {
                return Some(m);
            }
        }

        // 3. Coverage only. Anything that can render the requested ranges.
        let bare = FcPattern {
            unicode_ranges: pattern.unicode_ranges.clone(),
            ..FcPattern::default()
        };
        self.query(&bare, trace)
    }

    /// Queries a font from the in-memory cache, returns the first found font (early return)
    /// Memory fonts are always preferred over disk fonts with the same match quality.
    ///
    /// This is FALLIBLE by design — see [`FcFontCache::query_with_fallback`] for the
    /// `fc-match`-style total variant that a renderer should use.
    pub fn query(&self, pattern: &FcPattern, trace: &mut Vec<TraceMsg>) -> Option<FontMatch> {
        let state = self.state_read();

        // Memory fonts first, then the one ranking every path shares
        // (`fallback::RankKey`): style closeness, then how much of the
        // requested coverage the font misses, narrower before wider, name.
        // Breadth of coverage is never a bonus.
        let mut matches: Vec<(bool, fallback::RankKey, FontId, &FcPattern)> = Vec::new();

        for (id, metadata) in &state.metadata {
            if Self::query_matches_internal(metadata, pattern, trace) {
                let is_disk = !state.memory_fonts.contains_key(id);
                matches.push((
                    is_disk,
                    fallback::RankKey::for_request(pattern, metadata, &pattern.unicode_ranges),
                    *id,
                    metadata,
                ));
            }
        }

        matches.sort();

        matches.first().map(|(_, _, id, metadata)| FontMatch {
            id: *id,
            unicode_ranges: metadata.unicode_ranges.clone(),
            fallbacks: Vec::new(),
        })
    }

    /// Get in-memory font data (cloned out of the shared state).
    pub fn get_memory_font(&self, id: &FontId) -> Option<FcFont> {
        self.state_read().memory_fonts.get(id).cloned()
    }

    /// Check if a pattern matches the query, with detailed tracing
    fn trace_path(k: &FcPattern) -> String {
        k.name
            .as_ref()
            .cloned()
            .unwrap_or_else(|| "<unknown>".to_string())
    }

    pub fn query_matches_internal(
        k: &FcPattern,
        pattern: &FcPattern,
        trace: &mut Vec<TraceMsg>,
    ) -> bool {
        // Check name - substring match
        if let Some(ref name) = pattern.name {
            if !k.name.as_ref().map_or(false, |kn| kn.contains(name)) {
                trace.push(TraceMsg {
                    level: TraceLevel::Info,
                    path: Self::trace_path(k),
                    reason: MatchReason::NameMismatch {
                        requested: pattern.name.clone(),
                        found: k.name.clone(),
                    },
                });
                return false;
            }
        }

        // Check family - substring match
        if let Some(ref family) = pattern.family {
            if !k.family.as_ref().map_or(false, |kf| kf.contains(family)) {
                trace.push(TraceMsg {
                    level: TraceLevel::Info,
                    path: Self::trace_path(k),
                    reason: MatchReason::FamilyMismatch {
                        requested: pattern.family.clone(),
                        found: k.family.clone(),
                    },
                });
                return false;
            }
        }

        // Check style properties
        let style_properties = [
            (
                "italic",
                pattern.italic.needs_to_match(),
                pattern.italic.matches(&k.italic),
            ),
            (
                "oblique",
                pattern.oblique.needs_to_match(),
                pattern.oblique.matches(&k.oblique),
            ),
            (
                "bold",
                pattern.bold.needs_to_match(),
                pattern.bold.matches(&k.bold),
            ),
            (
                "monospace",
                pattern.monospace.needs_to_match(),
                pattern.monospace.matches(&k.monospace),
            ),
            (
                "condensed",
                pattern.condensed.needs_to_match(),
                pattern.condensed.matches(&k.condensed),
            ),
        ];

        for (property_name, needs_to_match, matches) in style_properties {
            if needs_to_match && !matches {
                let (requested, found) = match property_name {
                    "italic" => (format!("{:?}", pattern.italic), format!("{:?}", k.italic)),
                    "oblique" => (format!("{:?}", pattern.oblique), format!("{:?}", k.oblique)),
                    "bold" => (format!("{:?}", pattern.bold), format!("{:?}", k.bold)),
                    "monospace" => (
                        format!("{:?}", pattern.monospace),
                        format!("{:?}", k.monospace),
                    ),
                    "condensed" => (
                        format!("{:?}", pattern.condensed),
                        format!("{:?}", k.condensed),
                    ),
                    _ => (String::new(), String::new()),
                };

                trace.push(TraceMsg {
                    level: TraceLevel::Info,
                    path: Self::trace_path(k),
                    reason: MatchReason::StyleMismatch {
                        property: property_name,
                        requested,
                        found,
                    },
                });
                return false;
            }
        }

        // Check weight - hard filter if non-normal weight is requested
        if pattern.weight != FcWeight::Normal && pattern.weight != k.weight {
            trace.push(TraceMsg {
                level: TraceLevel::Info,
                path: Self::trace_path(k),
                reason: MatchReason::WeightMismatch {
                    requested: pattern.weight,
                    found: k.weight,
                },
            });
            return false;
        }

        // Check stretch - hard filter if non-normal stretch is requested
        if pattern.stretch != FcStretch::Normal && pattern.stretch != k.stretch {
            trace.push(TraceMsg {
                level: TraceLevel::Info,
                path: Self::trace_path(k),
                reason: MatchReason::StretchMismatch {
                    requested: pattern.stretch,
                    found: k.stretch,
                },
            });
            return false;
        }

        // Check unicode ranges if specified
        if !pattern.unicode_ranges.is_empty() {
            let mut has_overlap = false;

            for p_range in &pattern.unicode_ranges {
                for k_range in &k.unicode_ranges {
                    if p_range.overlaps(k_range) {
                        has_overlap = true;
                        break;
                    }
                }
                if has_overlap {
                    break;
                }
            }

            if !has_overlap {
                trace.push(TraceMsg {
                    level: TraceLevel::Info,
                    path: Self::trace_path(k),
                    reason: MatchReason::UnicodeRangeMismatch {
                        character: '\0', // No specific character to report
                        ranges: k.unicode_ranges.clone(),
                    },
                });
                return false;
            }
        }

        true
    }

    /// Extract tokens from a font name
    /// E.g., "NotoSansJP" -> ["Noto", "Sans", "JP"]
    /// E.g., "Noto Sans CJK JP" -> ["Noto", "Sans", "CJK", "JP"]
    pub fn extract_font_name_tokens(name: &str) -> Vec<String> {
        let mut tokens = Vec::new();
        let mut current_token = String::new();
        let mut last_was_lower = false;

        for c in name.chars() {
            if c.is_whitespace() || c == '-' || c == '_' {
                // Word separator
                if !current_token.is_empty() {
                    tokens.push(current_token.clone());
                    current_token.clear();
                }
                last_was_lower = false;
            } else if c.is_uppercase() && last_was_lower && !current_token.is_empty() {
                // CamelCase boundary (e.g., "Noto" | "Sans")
                tokens.push(current_token.clone());
                current_token.clear();
                current_token.push(c);
                last_was_lower = false;
            } else {
                current_token.push(c);
                last_was_lower = c.is_lowercase();
            }
        }

        if !current_token.is_empty() {
            tokens.push(current_token);
        }

        tokens
    }

    /// Total coverage of `ranges` in codepoints (widths summed; callers
    /// pass a normalized, disjoint set).
    /// Find fallback fonts for a given pattern
    // Helper to calculate total unicode coverage
    pub fn calculate_unicode_coverage(ranges: &[UnicodeRange]) -> u64 {
        ranges
            .iter()
            .map(|range| (range.end - range.start + 1) as u64)
            .sum()
    }

    /// Coalesce ranges into a sorted, **disjoint** set.
    ///
    /// [`FcFontCache::calculate_unicode_coverage`] sums `end - start + 1` with no
    /// overlap handling, and that sum ranks fallback candidates. A font's coverage
    /// is built from two sources whose block boundaries do not align — the OS/2
    /// `ulUnicodeRange` bit mappings and the cmap block probe — so merging them
    /// naively double-counts the overlap and inflates the score. That is exactly
    /// how a CJK megafont wins a Latin run it has no business winning.
    ///
    /// Touching ranges (`prev.end + 1 == next.start`) are merged as well: they
    /// describe the same contiguous coverage, and leaving them split would make
    /// one set compare unequal to another purely by which source produced it.
    pub fn normalize_unicode_ranges(mut ranges: Vec<UnicodeRange>) -> Vec<UnicodeRange> {
        if ranges.len() < 2 {
            return ranges;
        }

        ranges.sort_unstable();

        let mut out: Vec<UnicodeRange> = Vec::with_capacity(ranges.len());
        for range in ranges {
            match out.last_mut() {
                // Overlapping or touching: extend. `saturating_add` so an `end` of
                // u32::MAX cannot wrap around into a bogus failure-to-merge.
                Some(prev) if range.start <= prev.end.saturating_add(1) => {
                    prev.end = prev.end.max(range.end);
                }
                _ => out.push(range),
            }
        }
        out
    }

    /// Calculate how well a font's Unicode ranges cover the requested ranges
    /// Returns a compatibility score (higher is better, 0 means no overlap)
    pub fn calculate_unicode_compatibility(
        requested: &[UnicodeRange],
        available: &[UnicodeRange],
    ) -> i32 {
        if requested.is_empty() {
            // No specific requirements, return total coverage
            return Self::calculate_unicode_coverage(available) as i32;
        }

        let mut total_coverage = 0u32;

        for req_range in requested {
            for avail_range in available {
                // Calculate overlap between requested and available ranges
                let overlap_start = req_range.start.max(avail_range.start);
                let overlap_end = req_range.end.min(avail_range.end);

                if overlap_start <= overlap_end {
                    // There is overlap
                    let overlap_size = overlap_end - overlap_start + 1;
                    total_coverage += overlap_size;
                }
            }
        }

        total_coverage as i32
    }

    pub fn calculate_style_score(original: &FcPattern, candidate: &FcPattern) -> i32 {
        let mut score = 0_i32;

        // Weight calculation with special handling for bold property
        if (original.bold == PatternMatch::True && candidate.weight == FcWeight::Bold)
            || (original.bold == PatternMatch::False && candidate.weight != FcWeight::Bold)
        {
            // No weight penalty when bold is requested and font has Bold weight
            // No weight penalty when non-bold is requested and font has non-Bold weight
        } else {
            // Apply normal weight difference penalty
            let weight_diff = (original.weight as i32 - candidate.weight as i32).abs();
            score += weight_diff as i32;
        }

        // Exact weight match bonus: reward fonts whose weight matches the request exactly,
        // with an extra bonus when both are Normal (the most common case for body text)
        if original.weight == candidate.weight {
            score -= 15;
            if original.weight == FcWeight::Normal {
                score -= 10; // Extra bonus for Normal-Normal match
            }
        }

        // Stretch calculation with special handling for condensed property
        if (original.condensed == PatternMatch::True && candidate.stretch.is_condensed())
            || (original.condensed == PatternMatch::False && !candidate.stretch.is_condensed())
        {
            // No stretch penalty when condensed is requested and font has condensed stretch
            // No stretch penalty when non-condensed is requested and font has non-condensed stretch
        } else {
            // Apply normal stretch difference penalty
            let stretch_diff = (original.stretch as i32 - candidate.stretch as i32).abs();
            score += (stretch_diff * 100) as i32;
        }

        // Handle style properties with standard penalties and bonuses
        let style_props = [
            (original.italic, candidate.italic, 300, 150),
            (original.oblique, candidate.oblique, 200, 100),
            (original.bold, candidate.bold, 300, 150),
            (original.monospace, candidate.monospace, 100, 50),
            (original.condensed, candidate.condensed, 100, 50),
        ];

        for (orig, cand, mismatch_penalty, dontcare_penalty) in style_props {
            if orig.needs_to_match() {
                if orig == PatternMatch::False && cand == PatternMatch::DontCare {
                    // Requesting non-italic but font doesn't declare: small penalty
                    // (less than a full mismatch but more than a perfect match)
                    score += dontcare_penalty / 2;
                } else if !orig.matches(&cand) {
                    if cand == PatternMatch::DontCare {
                        score += dontcare_penalty;
                    } else {
                        score += mismatch_penalty;
                    }
                } else if orig == PatternMatch::True && cand == PatternMatch::True {
                    // Give bonus for exact True match
                    score -= 20;
                } else if orig == PatternMatch::False && cand == PatternMatch::False {
                    // Give bonus for exact False match (prefer explicitly non-italic
                    // over fonts with unknown/DontCare italic status)
                    score -= 20;
                }
            } else {
                // orig == DontCare: prefer "normal" fonts over styled ones.
                // When the caller doesn't specify italic/bold/etc., a font
                // that IS italic/bold should score slightly worse than one
                // that isn't, so Regular is chosen over Italic by default.
                if cand == PatternMatch::True {
                    score += dontcare_penalty / 3;
                }
            }
        }

        // ── Name-based "base font" detection ──
        // The shorter the font name relative to its family, the more "basic" the
        // variant.  E.g. "System Font" (the base) should score better than
        // "System Font Regular Italic" (a variant) when the user hasn't
        // explicitly requested italic.
        if let (Some(name), Some(family)) = (&candidate.name, &candidate.family) {
            let name_lower = name.to_ascii_lowercase();
            let family_lower = family.to_ascii_lowercase();

            // Strip the family prefix from the name to get the "extra" part
            let extra = if name_lower.starts_with(&family_lower) {
                name_lower[family_lower.len()..].to_string()
            } else {
                String::new()
            };

            // Strip common neutral descriptors that don't indicate a style variant
            let stripped = extra
                .replace("regular", "")
                .replace("normal", "")
                .replace("book", "")
                .replace("roman", "");
            let stripped = stripped.trim();

            if stripped.is_empty() {
                // This is a "base font" – name is just the family (± "Regular")
                score -= 50;
            } else {
                // Name has extra style descriptors – add a penalty per extra word
                let extra_words = stripped.split_whitespace().count();
                score += (extra_words as i32) * 25;
            }
        }

        // ── Subfamily "Regular" bonus ──
        // Fonts whose OpenType subfamily is exactly "Regular" are the canonical
        // base variant and should be strongly preferred.
        if let Some(ref subfamily) = candidate.metadata.font_subfamily {
            let sf_lower = subfamily.to_ascii_lowercase();
            if sf_lower == "regular" {
                score -= 30;
            }
        }

        score
    }
}

#[cfg(all(feature = "std", feature = "parsing"))]
#[allow(non_snake_case, dead_code)]
fn FcScanDirectories() -> Option<(
    Vec<(FcPattern, FcFontPath)>,
    BTreeMap<String, FcFontRenderConfig>,
    BTreeMap<String, Vec<String>>,
)> {
    let config = FcSystemConfig::from_system()?;
    if config.font_dirs.is_empty() {
        return None;
    }
    let dirs: Vec<(Option<String>, String)> = config
        .font_dirs
        .iter()
        .map(|dir| (None, dir.to_string_lossy().into_owned()))
        .collect();
    Some((
        FcScanDirectoriesInner(&dirs),
        config.render_configs,
        config.aliases,
    ))
}

/// Deepest chain of `<include>`s [`FcSystemConfig::parse_tree`] follows.
/// Cycles are caught by the visited set; this bounds pathological trees.
#[cfg(all(feature = "std", feature = "parsing"))]
const MAX_INCLUDE_DEPTH: usize = 64;

/// What a fontconfig configuration tree says, as far as this crate reads
/// it: where fonts live, per-family rendering settings, and `<alias>`
/// preferences. Produced by [`FcSystemConfig::parse_tree`] from a root
/// file and everything it includes.
///
/// The parser has no OS dependency and is compiled and tested everywhere;
/// only [`FcSystemConfig::from_system`]'s default location is a Linux
/// convention. `FcFontCache::build` and `FcFontRegistry::new` consult it
/// where it exists and fill the gaps from the built-in tables.
#[cfg(all(feature = "std", feature = "parsing"))]
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FcSystemConfig {
    /// `<dir>` entries, resolved (prefixes and `~` expanded), in order,
    /// each once.
    pub font_dirs: Vec<PathBuf>,
    /// `<match target="font">` rendering settings keyed by family.
    pub render_configs: BTreeMap<String, FcFontRenderConfig>,
    /// `<alias><family>X</family><prefer>…</prefer></alias>` entries keyed by
    /// the normalized family name (`"sansserif"`, `"arial"`), preferred
    /// families in configured order, appended across files in include
    /// order and deduplicated.
    pub aliases: BTreeMap<String, Vec<String>>,
    /// Every configuration file that was read, in the order it was read.
    pub files: Vec<PathBuf>,
}

#[cfg(all(feature = "std", feature = "parsing"))]
impl FcSystemConfig {
    /// The platform configuration: `$FONTCONFIG_FILE` when set and
    /// non-empty, else `/etc/fonts/fonts.conf`. `None` when that file does
    /// not exist — the normal case on every OS but Linux.
    pub fn from_system() -> Option<Self> {
        let root = std::env::var("FONTCONFIG_FILE")
            .ok()
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| "/etc/fonts/fonts.conf".to_string());
        let root = PathBuf::from(root);
        if !root.is_file() {
            return None;
        }
        Self::parse_tree(&root)
    }

    /// Parse `root` and everything it includes, the way fontconfig does:
    /// includes are followed in document order, depth first; an included
    /// directory contributes its `[0-9]*.conf` files in name order; every
    /// file is read at most once (a cycle is simply ignored); missing
    /// includes are skipped.
    ///
    /// Relative `<include>` paths resolve like fontconfig's
    /// `FcConfigGetFilename`: against each directory of `$FONTCONFIG_PATH`,
    /// then against the directory of `root` — so the stock
    /// `<include ignore_missing="yes">conf.d</include>` finds
    /// `/etc/fonts/conf.d` no matter where the process runs. `prefix="xdg"`
    /// resolves against `$XDG_CONFIG_HOME` (includes) or `$XDG_DATA_HOME`
    /// (dirs), `prefix="relative"` against the including file's directory,
    /// `prefix="cwd"` / `"default"` against the working directory.
    ///
    /// `None` if `root` cannot be read.
    pub fn parse_tree(root: &std::path::Path) -> Option<Self> {
        use std::collections::VecDeque;

        let root_dir = root.parent().map(|d| d.to_path_buf()).unwrap_or_default();
        let search_dirs: Vec<PathBuf> = std::env::var_os("FONTCONFIG_PATH")
            .map(|v| std::env::split_paths(&v).collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .chain(core::iter::once(root_dir))
            .collect();

        let mut config = Self::default();
        let mut visited: alloc::collections::BTreeSet<PathBuf> =
            alloc::collections::BTreeSet::new();
        let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
        queue.push_back((root.to_path_buf(), 0));

        while let Some((path, depth)) = queue.pop_front() {
            if depth > MAX_INCLUDE_DEPTH {
                continue;
            }
            let identity = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if !visited.insert(identity) {
                continue;
            }
            let Ok(metadata) = std::fs::metadata(&path) else {
                continue;
            };

            if metadata.is_dir() {
                // The directory's files come next, before anything queued
                // after the include that named the directory.
                let mut entries: Vec<PathBuf> = std::fs::read_dir(&path)
                    .ok()?
                    .filter_map(|entry| entry.ok().map(|e| e.path()))
                    .filter(|p| std::fs::metadata(p).map(|m| m.is_file()).unwrap_or(false))
                    .filter(|p| {
                        p.file_name().map(|n| n.to_string_lossy()).is_some_and(|n| {
                            n.starts_with(|c: char| c.is_ascii_digit()) && n.ends_with(".conf")
                        })
                    })
                    .collect();
                entries.sort();
                for (i, entry) in entries.into_iter().enumerate() {
                    queue.insert(i, (entry, depth));
                }
                continue;
            }
            if !metadata.is_file() {
                continue;
            }

            let Ok(xml) = std::fs::read_to_string(&path) else {
                continue;
            };
            let mut includes: Vec<(Option<String>, PathBuf)> = Vec::new();
            let mut dirs: Vec<(Option<String>, String)> = Vec::new();
            if ParseFontsConf(&xml, &mut includes, &mut dirs).is_none() {
                continue;
            }
            ParseFontsConfRenderConfig(&xml, &mut config.render_configs);
            ParseFontsConfAliases(&xml, &mut config.aliases);
            config.files.push(path.clone());

            let here = path.parent().map(|d| d.to_path_buf()).unwrap_or_default();
            for (prefix, dir) in dirs {
                let resolved = match prefix.as_deref() {
                    Some("relative") => Some(here.join(dir)),
                    _ => process_path(&prefix, PathBuf::from(dir), false),
                };
                if let Some(dir) = resolved {
                    if !config.font_dirs.contains(&dir) {
                        config.font_dirs.push(dir);
                    }
                }
            }

            // This file's includes come next, in document order, before
            // whatever was queued by the file that included this one.
            let mut position = 0;
            for (prefix, include) in includes {
                let resolved = match prefix.as_deref() {
                    Some("relative") => Some(here.join(include)),
                    Some(_) => process_path(&prefix, include, true),
                    None => process_path(&None, include, true).map(|expanded| {
                        if expanded.is_absolute() {
                            expanded
                        } else {
                            search_dirs
                                .iter()
                                .map(|dir| dir.join(&expanded))
                                .find(|candidate| candidate.exists())
                                .unwrap_or_else(|| {
                                    search_dirs
                                        .last()
                                        .map(|dir| dir.join(&expanded))
                                        .unwrap_or(expanded)
                                })
                        }
                    }),
                };
                if let Some(resolved) = resolved {
                    queue.insert(position, (resolved, depth + 1));
                    position += 1;
                }
            }
        }

        // A root that could not be read (or parsed) yields no files at all.
        if config.files.is_empty() {
            return None;
        }
        Some(config)
    }
}

/// Parse `<alias><family>NAME</family><prefer><family>...</family>...</prefer></alias>`
/// blocks from a fontconfig XML file into `aliases`.
///
/// Keys are normalized with [`crate::utils::normalize_family_name`];
/// preferred families keep their configured order, appended across files
/// in include order (fontconfig semantics), deduplicated.
#[cfg(all(feature = "std", feature = "parsing"))]
fn ParseFontsConfAliases(input: &str, aliases: &mut BTreeMap<String, Vec<String>>) {
    use xmlparser::Token::*;
    use xmlparser::Tokenizer;

    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Idle,
        InAlias,
        InAliasFamily,
        InPrefer,
        InPreferFamily,
    }

    let mut state = State::Idle;
    let mut alias_key: Option<String> = None;
    let mut preferred: Vec<String> = Vec::new();
    let mut text_buf = String::new();

    for token_result in Tokenizer::from(input) {
        let token = match token_result {
            Ok(token) => token,
            Err(_) => continue,
        };
        match token {
            ElementStart { local, .. } => match local.as_str() {
                "alias" => {
                    state = State::InAlias;
                    alias_key = None;
                    preferred.clear();
                }
                "family" if state == State::InAlias => {
                    state = State::InAliasFamily;
                    text_buf.clear();
                }
                "prefer" if state == State::InAlias => {
                    state = State::InPrefer;
                }
                "family" if state == State::InPrefer => {
                    state = State::InPreferFamily;
                    text_buf.clear();
                }
                _ => {}
            },
            Text { text } => {
                if state == State::InAliasFamily || state == State::InPreferFamily {
                    text_buf.push_str(text.as_str());
                }
            }
            ElementEnd { end, .. } => {
                use xmlparser::ElementEnd;
                let closed = match end {
                    ElementEnd::Close(_, local) => Some(local.as_str().to_owned()),
                    _ => None,
                };
                let Some(closed) = closed else { continue };
                match closed.as_str() {
                    "family" => match state {
                        State::InAliasFamily => {
                            let t = text_buf.trim();
                            if !t.is_empty() && alias_key.is_none() {
                                alias_key = Some(t.to_owned());
                            }
                            state = State::InAlias;
                        }
                        State::InPreferFamily => {
                            let t = text_buf.trim();
                            if !t.is_empty() {
                                preferred.push(t.to_owned());
                            }
                            state = State::InPrefer;
                        }
                        _ => {}
                    },
                    "prefer" if state == State::InPrefer => {
                        state = State::InAlias;
                    }
                    "alias" => {
                        if let Some(key) = alias_key.take() {
                            if !preferred.is_empty() {
                                let norm = crate::utils::normalize_family_name(&key);
                                let entry = aliases.entry(norm).or_default();
                                for fam in preferred.drain(..) {
                                    if !entry.iter().any(|e| e == &fam) {
                                        entry.push(fam);
                                    }
                                }
                            }
                        }
                        state = State::Idle;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

// Parses the fonts.conf file
#[cfg(all(feature = "std", feature = "parsing"))]
fn ParseFontsConf(
    input: &str,
    paths_to_visit: &mut Vec<(Option<String>, PathBuf)>,
    font_paths: &mut Vec<(Option<String>, String)>,
) -> Option<()> {
    use xmlparser::Token::*;
    use xmlparser::Tokenizer;

    const TAG_INCLUDE: &str = "include";
    const TAG_DIR: &str = "dir";
    const ATTRIBUTE_PREFIX: &str = "prefix";

    let mut current_prefix: Option<&str> = None;
    let mut current_path: Option<&str> = None;
    let mut is_in_include = false;
    let mut is_in_dir = false;

    for token_result in Tokenizer::from(input) {
        let token = match token_result {
            Ok(token) => token,
            Err(_) => return None,
        };

        match token {
            ElementStart { local, .. } => {
                if is_in_include || is_in_dir {
                    return None; /* error: nested tags */
                }

                match local.as_str() {
                    TAG_INCLUDE => {
                        is_in_include = true;
                    }
                    TAG_DIR => {
                        is_in_dir = true;
                    }
                    _ => continue,
                }

                current_path = None;
            }
            Text { text, .. } => {
                let text = text.as_str().trim();
                if text.is_empty() {
                    continue;
                }
                if is_in_include || is_in_dir {
                    current_path = Some(text);
                }
            }
            Attribute { local, value, .. } => {
                if !is_in_include && !is_in_dir {
                    continue;
                }
                // attribute on <include> or <dir> node
                if local.as_str() == ATTRIBUTE_PREFIX {
                    current_prefix = Some(value.as_str());
                }
            }
            ElementEnd { end, .. } => {
                let end_tag = match end {
                    xmlparser::ElementEnd::Close(_, a) => a,
                    _ => continue,
                };

                match end_tag.as_str() {
                    TAG_INCLUDE => {
                        if !is_in_include {
                            continue;
                        }

                        if let Some(current_path) = current_path.as_ref() {
                            paths_to_visit.push((
                                current_prefix.map(ToOwned::to_owned),
                                PathBuf::from(*current_path),
                            ));
                        }
                    }
                    TAG_DIR => {
                        if !is_in_dir {
                            continue;
                        }

                        if let Some(current_path) = current_path.as_ref() {
                            font_paths.push((
                                current_prefix.map(ToOwned::to_owned),
                                (*current_path).to_owned(),
                            ));
                        }
                    }
                    _ => continue,
                }

                is_in_include = false;
                is_in_dir = false;
                current_path = None;
                current_prefix = None;
            }
            _ => {}
        }
    }

    Some(())
}

/// Parses `<match target="font">` blocks from fonts.conf XML and returns
/// a map from family name to per-font rendering configuration.
///
/// Example fonts.conf snippet that this handles:
/// ```xml
/// <match target="font">
///   <test name="family"><string>Inconsolata</string></test>
///   <edit name="antialias" mode="assign"><bool>true</bool></edit>
///   <edit name="hintstyle" mode="assign"><const>hintslight</const></edit>
/// </match>
/// ```
#[cfg(all(feature = "std", feature = "parsing"))]
fn ParseFontsConfRenderConfig(input: &str, configs: &mut BTreeMap<String, FcFontRenderConfig>) {
    use xmlparser::Token::*;
    use xmlparser::Tokenizer;

    // Parser state machine
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        /// Outside any relevant block
        Idle,
        /// Inside <match target="font">
        InMatchFont,
        /// Inside <test name="family"> within a match block
        InTestFamily,
        /// Inside <edit name="..."> within a match block
        InEdit,
    }

    let mut state = State::Idle;
    let mut match_is_font_target = false;
    let mut current_family: Option<String> = None;
    let mut current_edit_name: Option<String> = None;
    let mut current_value: Option<String> = None;
    let mut value_tag: Option<String> = None;
    let mut config = FcFontRenderConfig::default();
    let mut in_test = false;
    let mut test_name: Option<String> = None;

    for token_result in Tokenizer::from(input) {
        let token = match token_result {
            Ok(token) => token,
            Err(_) => continue,
        };

        match token {
            ElementStart { local, .. } => {
                let tag = local.as_str();
                match tag {
                    "match" => {
                        // Reset state for a new match block
                        match_is_font_target = false;
                        current_family = None;
                        config = FcFontRenderConfig::default();
                    }
                    "test" if state == State::InMatchFont => {
                        in_test = true;
                        test_name = None;
                    }
                    "edit" if state == State::InMatchFont => {
                        current_edit_name = None;
                    }
                    "bool" | "double" | "const" | "string" | "int" => {
                        if state == State::InTestFamily || state == State::InEdit {
                            value_tag = Some(tag.to_owned());
                            current_value = None;
                        }
                    }
                    _ => {}
                }
            }
            Attribute { local, value, .. } => {
                let attr_name = local.as_str();
                let attr_value = value.as_str();

                match attr_name {
                    "target" => {
                        if attr_value == "font" {
                            match_is_font_target = true;
                        }
                    }
                    "name" => {
                        if in_test && state == State::InMatchFont {
                            test_name = Some(attr_value.to_owned());
                        } else if state == State::InMatchFont {
                            current_edit_name = Some(attr_value.to_owned());
                        }
                    }
                    _ => {}
                }
            }
            Text { text, .. } => {
                let text = text.as_str().trim();
                if !text.is_empty() && (state == State::InTestFamily || state == State::InEdit) {
                    current_value = Some(text.to_owned());
                }
            }
            ElementEnd { end, .. } => {
                match end {
                    xmlparser::ElementEnd::Open => {
                        // Tag just opened (after attributes processed)
                        if match_is_font_target && state == State::Idle {
                            state = State::InMatchFont;
                            match_is_font_target = false;
                        } else if in_test {
                            if test_name.as_deref() == Some("family") {
                                state = State::InTestFamily;
                            }
                            in_test = false;
                        } else if current_edit_name.is_some() && state == State::InMatchFont {
                            state = State::InEdit;
                        }
                    }
                    xmlparser::ElementEnd::Close(_, local) => {
                        let tag = local.as_str();
                        match tag {
                            "match" => {
                                // End of match block: store config if we have a family
                                if let Some(family) = current_family.take() {
                                    let empty = FcFontRenderConfig::default();
                                    if config != empty {
                                        configs.insert(family, config.clone());
                                    }
                                }
                                state = State::Idle;
                                config = FcFontRenderConfig::default();
                            }
                            "test" => {
                                if state == State::InTestFamily {
                                    // Extract the family name from the value we collected
                                    if let Some(ref val) = current_value {
                                        current_family = Some(val.clone());
                                    }
                                    state = State::InMatchFont;
                                }
                                current_value = None;
                                value_tag = None;
                            }
                            "edit" => {
                                if state == State::InEdit {
                                    // Apply the collected value to the config
                                    if let (Some(ref name), Some(ref val)) =
                                        (&current_edit_name, &current_value)
                                    {
                                        apply_edit_value(
                                            &mut config,
                                            name,
                                            val,
                                            value_tag.as_deref(),
                                        );
                                    }
                                    state = State::InMatchFont;
                                }
                                current_edit_name = None;
                                current_value = None;
                                value_tag = None;
                            }
                            "bool" | "double" | "const" | "string" | "int" => {
                                // value_tag and current_value already set by Text handler
                            }
                            _ => {}
                        }
                    }
                    xmlparser::ElementEnd::Empty => {
                        // Self-closing tags: nothing to do
                    }
                }
            }
            _ => {}
        }
    }
}

/// Apply a parsed edit value to the render config.
#[cfg(all(feature = "std", feature = "parsing"))]
fn apply_edit_value(
    config: &mut FcFontRenderConfig,
    edit_name: &str,
    value: &str,
    _value_tag: Option<&str>,
) {
    match edit_name {
        "antialias" => {
            config.antialias = parse_bool_value(value);
        }
        "hinting" => {
            config.hinting = parse_bool_value(value);
        }
        "autohint" => {
            config.autohint = parse_bool_value(value);
        }
        "embeddedbitmap" => {
            config.embeddedbitmap = parse_bool_value(value);
        }
        "embolden" => {
            config.embolden = parse_bool_value(value);
        }
        "minspace" => {
            config.minspace = parse_bool_value(value);
        }
        "hintstyle" => {
            config.hintstyle = parse_hintstyle_const(value);
        }
        "rgba" => {
            config.rgba = parse_rgba_const(value);
        }
        "lcdfilter" => {
            config.lcdfilter = parse_lcdfilter_const(value);
        }
        "dpi" => {
            if let Ok(v) = value.parse::<f64>() {
                config.dpi = Some(v);
            }
        }
        "scale" => {
            if let Ok(v) = value.parse::<f64>() {
                config.scale = Some(v);
            }
        }
        _ => {
            // Unknown edit property, ignore
        }
    }
}

#[cfg(all(feature = "std", feature = "parsing"))]
fn parse_bool_value(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

#[cfg(all(feature = "std", feature = "parsing"))]
fn parse_hintstyle_const(value: &str) -> Option<FcHintStyle> {
    match value {
        "hintnone" => Some(FcHintStyle::None),
        "hintslight" => Some(FcHintStyle::Slight),
        "hintmedium" => Some(FcHintStyle::Medium),
        "hintfull" => Some(FcHintStyle::Full),
        _ => None,
    }
}

#[cfg(all(feature = "std", feature = "parsing"))]
fn parse_rgba_const(value: &str) -> Option<FcRgba> {
    match value {
        "unknown" => Some(FcRgba::Unknown),
        "rgb" => Some(FcRgba::Rgb),
        "bgr" => Some(FcRgba::Bgr),
        "vrgb" => Some(FcRgba::Vrgb),
        "vbgr" => Some(FcRgba::Vbgr),
        "none" => Some(FcRgba::None),
        _ => None,
    }
}

#[cfg(all(feature = "std", feature = "parsing"))]
fn parse_lcdfilter_const(value: &str) -> Option<FcLcdFilter> {
    match value {
        "lcdnone" => Some(FcLcdFilter::None),
        "lcddefault" => Some(FcLcdFilter::Default),
        "lcdlight" => Some(FcLcdFilter::Light),
        "lcdlegacy" => Some(FcLcdFilter::Legacy),
        _ => None,
    }
}

/// Intermediate parsed data from a single font face within a font file.
/// Used to share parsing logic between `FcParseFont` and `FcParseFontBytesInner`.
#[cfg(all(feature = "std", feature = "parsing"))]
struct ParsedFontFace {
    pattern: FcPattern,
    font_index: usize,
}

/// Parse all font table data from a single font face and return the extracted patterns.
///
/// This is the shared core of `FcParseFont` and `FcParseFontBytesInner`:
/// TTC detection, font table parsing, OS/2/head/post reading, unicode range extraction,
/// CMAP verification, monospace detection, metadata extraction, and pattern creation.
#[cfg(all(feature = "std", feature = "parsing"))]
fn parse_font_faces(font_bytes: &[u8]) -> Option<Vec<ParsedFontFace>> {
    use allsorts::{
        binary::read::ReadScope,
        font_data::FontData,
        get_name::fontcode_get_name,
        post::PostTable,
        tables::{os2::Os2, HeadTable, NameTable},
        tag,
    };
    use std::collections::BTreeSet;

    const FONT_SPECIFIER_NAME_ID: u16 = 4;
    const FONT_SPECIFIER_FAMILY_ID: u16 = 1;

    let max_fonts = if font_bytes.len() >= 12 && &font_bytes[0..4] == b"ttcf" {
        // Read numFonts from TTC header (offset 8, 4 bytes)
        let num_fonts =
            u32::from_be_bytes([font_bytes[8], font_bytes[9], font_bytes[10], font_bytes[11]]);
        // Cap at a reasonable maximum as a safety measure
        std::cmp::min(num_fonts as usize, 100)
    } else {
        // Not a collection, just one font
        1
    };

    let scope = ReadScope::new(font_bytes);
    let font_file = scope.read::<FontData<'_>>().ok()?;

    // Handle collections properly by iterating through all fonts
    let mut results = Vec::new();

    for font_index in 0..max_fonts {
        let provider = font_file.table_provider(font_index).ok()?;
        let head_data = provider.table_data(tag::HEAD).ok()??.into_owned();
        let head_table = ReadScope::new(&head_data).read::<HeadTable>().ok()?;

        let is_bold = head_table.is_bold();
        let is_italic = head_table.is_italic();
        let mut detected_monospace = None;

        let post_data = provider.table_data(tag::POST).ok()??;
        if let Ok(post_table) = ReadScope::new(&post_data).read::<PostTable>() {
            // isFixedPitch here - https://learn.microsoft.com/en-us/typography/opentype/spec/post#header
            detected_monospace = Some(post_table.header.is_fixed_pitch != 0);
        }

        // Get font properties from OS/2 table.
        //
        // OS/2 is OPTIONAL in TrueType - only OpenType requires it - and plenty
        // of real fonts ship without one, including the base-14 PDF font subsets
        // printpdf embeds. This used to be `.ok()??`, which turned "no OS/2" into
        // "not a font" and made the whole face invisible to the cache even though
        // allsorts parses it perfectly well.
        //
        // Nothing below actually needs OS/2: `head.macStyle` already gave us bold
        // and italic, `post`/`hmtx` cover monospace, and coverage has been
        // cmap-authoritative since 4.4.8. So treat it as the hint it is.
        let os2_data = provider.table_data(tag::OS_2).ok().flatten();
        let os2_table = os2_data
            .as_deref()
            .and_then(|data| ReadScope::new(data).read_dep::<Os2>(data.len()).ok());

        // Extract additional style information
        let is_oblique = os2_table.as_ref().is_some_and(|os2| {
            os2.fs_selection
                .contains(allsorts::tables::os2::FsSelectionFlag::OBLIQUE)
        });
        // Without OS/2 the only weight signal is the `head.macStyle` bold bit, so
        // the face lands on Bold or Normal rather than a precise class.
        let weight = os2_table.as_ref().map_or(
            if is_bold {
                FcWeight::Bold
            } else {
                FcWeight::Normal
            },
            |os2| FcWeight::from_u16(os2.us_weight_class),
        );
        let stretch = os2_table.as_ref().map_or(FcStretch::Normal, |os2| {
            FcStretch::from_u16(os2.us_width_class)
        });

        // Coverage comes from the cmap and nothing else: every codepoint the
        // face maps to a real glyph, exactly. See `cmap_coverage`.
        let unicode_ranges = cmap_coverage(&provider).unwrap_or_default();

        // Use the shared detect_monospace helper for PANOSE + hmtx fallback
        let is_monospace =
            detect_monospace(&provider, os2_table.as_ref(), detected_monospace).unwrap_or(false);

        let name_data = provider.table_data(tag::NAME).ok()??.into_owned();
        let name_table = ReadScope::new(&name_data).read::<NameTable>().ok()?;

        // Extract metadata from name table
        let mut metadata = FcFontMetadata::default();

        const NAME_ID_COPYRIGHT: u16 = 0;
        const NAME_ID_FAMILY: u16 = 1;
        const NAME_ID_SUBFAMILY: u16 = 2;
        const NAME_ID_UNIQUE_ID: u16 = 3;
        const NAME_ID_FULL_NAME: u16 = 4;
        const NAME_ID_VERSION: u16 = 5;
        const NAME_ID_POSTSCRIPT_NAME: u16 = 6;
        const NAME_ID_TRADEMARK: u16 = 7;
        const NAME_ID_MANUFACTURER: u16 = 8;
        const NAME_ID_DESIGNER: u16 = 9;
        const NAME_ID_DESCRIPTION: u16 = 10;
        const NAME_ID_VENDOR_URL: u16 = 11;
        const NAME_ID_DESIGNER_URL: u16 = 12;
        const NAME_ID_LICENSE: u16 = 13;
        const NAME_ID_LICENSE_URL: u16 = 14;
        const NAME_ID_PREFERRED_FAMILY: u16 = 16;
        const NAME_ID_PREFERRED_SUBFAMILY: u16 = 17;

        metadata.copyright = get_name_string(&name_data, NAME_ID_COPYRIGHT);
        metadata.font_family = get_name_string(&name_data, NAME_ID_FAMILY);
        metadata.font_subfamily = get_name_string(&name_data, NAME_ID_SUBFAMILY);
        metadata.full_name = get_name_string(&name_data, NAME_ID_FULL_NAME);
        metadata.unique_id = get_name_string(&name_data, NAME_ID_UNIQUE_ID);
        metadata.version = get_name_string(&name_data, NAME_ID_VERSION);
        metadata.postscript_name = get_name_string(&name_data, NAME_ID_POSTSCRIPT_NAME);
        metadata.trademark = get_name_string(&name_data, NAME_ID_TRADEMARK);
        metadata.manufacturer = get_name_string(&name_data, NAME_ID_MANUFACTURER);
        metadata.designer = get_name_string(&name_data, NAME_ID_DESIGNER);
        metadata.id_description = get_name_string(&name_data, NAME_ID_DESCRIPTION);
        metadata.designer_url = get_name_string(&name_data, NAME_ID_DESIGNER_URL);
        metadata.manufacturer_url = get_name_string(&name_data, NAME_ID_VENDOR_URL);
        metadata.license = get_name_string(&name_data, NAME_ID_LICENSE);
        metadata.license_url = get_name_string(&name_data, NAME_ID_LICENSE_URL);
        metadata.preferred_family = get_name_string(&name_data, NAME_ID_PREFERRED_FAMILY);
        metadata.preferred_subfamily = get_name_string(&name_data, NAME_ID_PREFERRED_SUBFAMILY);

        // One font can support multiple patterns
        let mut f_family = None;

        let patterns = name_table
            .name_records
            .iter()
            .filter_map(|name_record| {
                let name_id = name_record.name_id;
                if name_id == FONT_SPECIFIER_FAMILY_ID {
                    if let Ok(Some(family)) =
                        fontcode_get_name(&name_data, FONT_SPECIFIER_FAMILY_ID)
                    {
                        f_family = Some(family);
                    }
                    None
                } else if name_id == FONT_SPECIFIER_NAME_ID {
                    let family = f_family.as_ref()?;
                    let name = fontcode_get_name(&name_data, FONT_SPECIFIER_NAME_ID).ok()??;
                    if name.to_bytes().is_empty() {
                        None
                    } else {
                        let mut name_str = String::from_utf8_lossy(name.to_bytes()).to_string();
                        let mut family_str = String::from_utf8_lossy(family.as_bytes()).to_string();
                        if name_str.starts_with('.') {
                            name_str = name_str[1..].to_string();
                        }
                        if family_str.starts_with('.') {
                            family_str = family_str[1..].to_string();
                        }
                        Some((
                            FcPattern {
                                name: Some(name_str),
                                family: Some(family_str),
                                bold: if is_bold {
                                    PatternMatch::True
                                } else {
                                    PatternMatch::False
                                },
                                italic: if is_italic {
                                    PatternMatch::True
                                } else {
                                    PatternMatch::False
                                },
                                oblique: if is_oblique {
                                    PatternMatch::True
                                } else {
                                    PatternMatch::False
                                },
                                monospace: if is_monospace {
                                    PatternMatch::True
                                } else {
                                    PatternMatch::False
                                },
                                condensed: if stretch <= FcStretch::Condensed {
                                    PatternMatch::True
                                } else {
                                    PatternMatch::False
                                },
                                weight,
                                stretch,
                                unicode_ranges: unicode_ranges.clone(),
                                metadata: metadata.clone(),
                                render_config: FcFontRenderConfig::default(),
                            },
                            font_index,
                        ))
                    }
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();

        results.extend(patterns.into_iter().map(|(pat, idx)| ParsedFontFace {
            pattern: pat,
            font_index: idx,
        }));
    }

    if results.is_empty() {
        None
    } else {
        Some(results)
    }
}

// Remaining implementation for font scanning, parsing, etc.
#[cfg(all(feature = "std", feature = "parsing"))]
pub(crate) fn FcParseFont(filepath: &PathBuf) -> Option<Vec<(FcPattern, FcFontPath)>> {
    #[cfg(all(not(target_family = "wasm"), feature = "std"))]
    use mmapio::MmapOptions;
    use std::fs::File;

    // Try parsing the font file and see if the postscript name matches
    let file = File::open(filepath).ok()?;

    #[cfg(all(not(target_family = "wasm"), feature = "std"))]
    let font_bytes = unsafe { MmapOptions::new().map(&file).ok()? };

    #[cfg(not(all(not(target_family = "wasm"), feature = "std")))]
    let font_bytes = std::fs::read(filepath).ok()?;

    let faces = parse_font_faces(&font_bytes[..])?;
    let path_str = filepath.to_string_lossy().to_string();
    // Hash once per file — every face of a .ttc shares this value,
    // so the shared-bytes cache can return the same Arc<[u8]> for
    // all of them. Use the cheap sampled variant so the scout doesn't
    // page-fault the full file into RSS just to produce a dedup key.
    let bytes_hash = crate::utils::content_dedup_hash_u64(&font_bytes[..]);

    Some(
        faces
            .into_iter()
            .map(|face| {
                (
                    face.pattern,
                    FcFontPath {
                        path: path_str.clone(),
                        font_index: face.font_index,
                        bytes_hash,
                    },
                )
            })
            .collect(),
    )
}

/// Coverage info returned by a fast-probe parse.
///
/// Produced by [`FcParseFontFaceFast`] / [`FcProbeCoverage`] — the
/// v4.2 "cheap cmap-only" entry point. Unlike `parse_font_faces`,
/// this path does **not** read NAME, OS/2, POST, HHEA, HMTX, HEAD's
/// style metadata, or anything else. It only reads the table
/// directory, `head.macStyle` (2 bytes), and the cmap subtable that
/// matches the codepoints we care about. ~1 ms/face on warm FS
/// cache vs ~13 ms for the full parse.
///
/// The `pattern.unicode_ranges` is populated from the *actual* cmap
/// contents (one `UnicodeRange` per covered codepoint in the input
/// set) rather than the OS/2 `ulUnicodeRange` bitfield. That's more
/// precise (OS/2 bits lie on many fonts — they're hints, not ground
/// truth) and means `FontFallbackChain::resolve_char`'s coverage
/// check matches what the shaper can actually render.
#[cfg(all(feature = "std", feature = "parsing"))]
#[derive(Debug, Clone)]
pub struct FastCoverage {
    /// Metadata pattern with `unicode_ranges` populated from the
    /// codepoints this face covered from the request set. `name` /
    /// `family` fields are left empty — callers already have the
    /// filename-guessed family in [`FcFontRegistry.known_paths`];
    /// we avoid the NAME table read entirely.
    pub pattern: FcPattern,
    /// Subset of the input codepoints that this face covers (maps
    /// to a non-zero gid via the best cmap subtable). May be empty
    /// if the face covers none, in which case callers should fall
    /// through to the next candidate path.
    pub covered: alloc::collections::BTreeSet<char>,
    /// `head.macStyle.bold` (bit 0).
    pub is_bold: bool,
    /// `head.macStyle.italic` (bit 1).
    pub is_italic: bool,
}

/// Fast per-face coverage probe.
///
/// Opens the provided font bytes as a `FontData` (detects TTC
/// collections), walks the given face, reads `head.macStyle` for
/// bold/italic flags, picks the best cmap subtable, and records
/// which of the requested codepoints have a non-zero gid.
///
/// Cost: table-dir parse + head (54 bytes) + cmap (5-100 KiB,
/// faulted in from mmap). No heap allocation besides the
/// covered-codepoints set and the returned `FcPattern`.
///
/// Returns `None` only if the font bytes are structurally bad or
/// the face index is out of range — empty coverage returns
/// `Some` with `covered.is_empty()`, so the caller can distinguish
/// "this face doesn't have the char we want" (try next face) from
/// "this file is corrupt" (give up on the whole file).
#[cfg(all(feature = "std", feature = "parsing"))]
#[allow(non_snake_case)]
pub fn FcParseFontFaceFast(
    font_bytes: &[u8],
    font_index: usize,
    codepoints: &alloc::collections::BTreeSet<char>,
) -> Option<FastCoverage> {
    use allsorts::{
        binary::read::ReadScope,
        font_data::FontData,
        tables::{
            cmap::{Cmap, CmapSubtable},
            FontTableProvider, HeadTable,
        },
        tag,
    };

    let scope = ReadScope::new(font_bytes);
    let font_file = scope.read::<FontData<'_>>().ok()?;
    let provider = font_file.table_provider(font_index).ok()?;

    // head — 54 bytes, macStyle at offset 44. Cheap.
    let head_data = provider.table_data(tag::HEAD).ok()??;
    let head_table = ReadScope::new(&head_data).read::<HeadTable>().ok()?;
    let is_bold = head_table.is_bold();
    let is_italic = head_table.is_italic();

    // cmap — find the best Unicode subtable, probe each codepoint.
    // The mmap page-cache only faults in the bytes we touch.
    let cmap_data = provider.table_data(tag::CMAP).ok()??;
    let cmap = ReadScope::new(&cmap_data).read::<Cmap<'_>>().ok()?;
    let encoding_record = find_best_cmap_subtable(&cmap)?;
    let cmap_subtable = ReadScope::new(&cmap_data)
        .offset(encoding_record.offset as usize)
        .read::<CmapSubtable<'_>>()
        .ok()?;

    let mut covered: alloc::collections::BTreeSet<char> = alloc::collections::BTreeSet::new();
    for ch in codepoints {
        if matches!(cmap_subtable.map_glyph(*ch as u32), Ok(Some(gid)) if gid != 0) {
            covered.insert(*ch);
        }
    }
    // The face's full coverage from the subtable's segments — the same exact
    // set the scan path stores — so a fast-probed face is not left claiming
    // only the characters that happened to be asked for so far.
    let covered_ranges =
        coverage_from_subtable(&cmap_subtable, &cmap_data, encoding_record.offset as usize)
            .unwrap_or_default();

    let weight = if is_bold {
        FcWeight::Bold
    } else {
        FcWeight::Normal
    };
    let italic_match = if is_italic {
        PatternMatch::True
    } else {
        PatternMatch::False
    };

    let pattern = FcPattern {
        name: None,
        family: None,
        weight,
        italic: italic_match,
        oblique: PatternMatch::DontCare,
        monospace: PatternMatch::DontCare,
        unicode_ranges: covered_ranges,
        ..Default::default()
    };

    Some(FastCoverage {
        pattern,
        covered,
        is_bold,
        is_italic,
    })
}

/// Count the number of faces inside a TTC, or `1` for a single-face
/// font file. Used by [`FcFontRegistry::request_fonts_fast`] to
/// iterate every face in a `.ttc` without paying the full-parse
/// cost (the TTC header is 12 bytes).
#[cfg(all(feature = "std", feature = "parsing"))]
#[allow(non_snake_case)]
pub fn FcCountFontFaces(font_bytes: &[u8]) -> usize {
    if font_bytes.len() >= 12 && &font_bytes[0..4] == b"ttcf" {
        let num_fonts =
            u32::from_be_bytes([font_bytes[8], font_bytes[9], font_bytes[10], font_bytes[11]]);
        // Same cap as parse_font_faces, for safety.
        std::cmp::min(num_fonts as usize, 100).max(1)
    } else {
        1
    }
}

/// Parse font bytes and extract font patterns for in-memory fonts.
///
/// This is the public API for parsing in-memory font data to create
/// `(FcPattern, FcFont)` tuples that can be added to an `FcFontCache`
/// via `with_memory_fonts()`.
///
/// # Arguments
/// * `font_bytes` - The raw bytes of a TrueType/OpenType font file
/// * `font_id` - An identifier string for this font (used internally)
///
/// # Returns
/// A vector of `(FcPattern, FcFont)` tuples, one for each font face in the file.
/// Returns `None` if the font could not be parsed.
///
/// # Example
/// ```ignore
/// use rust_fontconfig::{FcFontCache, FcParseFontBytes};
///
/// let font_bytes = include_bytes!("path/to/font.ttf");
/// let mut cache = FcFontCache::default();
///
/// if let Some(fonts) = FcParseFontBytes(font_bytes, "MyFont") {
///     cache.with_memory_fonts(fonts);
/// }
/// ```
#[cfg(all(feature = "std", feature = "parsing"))]
#[allow(non_snake_case)]
pub fn FcParseFontBytes(font_bytes: &[u8], font_id: &str) -> Option<Vec<(FcPattern, FcFont)>> {
    FcParseFontBytesInner(font_bytes, font_id)
}

/// Internal implementation for parsing font bytes.
/// Delegates to `parse_font_faces` for shared parsing logic and wraps results as `FcFont`.
#[cfg(all(feature = "std", feature = "parsing"))]
fn FcParseFontBytesInner(font_bytes: &[u8], font_id: &str) -> Option<Vec<(FcPattern, FcFont)>> {
    let faces = parse_font_faces(font_bytes)?;
    let id = font_id.to_string();
    let bytes = font_bytes.to_vec();

    Some(
        faces
            .into_iter()
            .map(|face| {
                (
                    face.pattern,
                    FcFont {
                        bytes: bytes.clone(),
                        font_index: face.font_index,
                        id: id.clone(),
                    },
                )
            })
            .collect(),
    )
}

#[cfg(all(feature = "std", feature = "parsing"))]
fn FcScanDirectoriesInner(paths: &[(Option<String>, String)]) -> Vec<(FcPattern, FcFontPath)> {
    #[cfg(all(feature = "multithreading", not(target_family = "wasm")))]
    {
        use rayon::prelude::*;

        // scan directories in parallel
        paths
            .par_iter()
            .filter_map(|(prefix, p)| {
                process_path(prefix, PathBuf::from(p), false).map(FcScanSingleDirectoryRecursive)
            })
            .flatten()
            .collect()
    }
    // wasm has no rayon (it's target-gated off), so even with `multithreading`
    // enabled wasm falls back to the sequential path.
    #[cfg(not(all(feature = "multithreading", not(target_family = "wasm"))))]
    {
        paths
            .iter()
            .filter_map(|(prefix, p)| {
                process_path(prefix, PathBuf::from(p), false).map(FcScanSingleDirectoryRecursive)
            })
            .flatten()
            .collect()
    }
}

/// Font files under `dir`: see [`crate::utils::collect_font_files`] (cycle-safe,
/// extension-filtered). The scan used to open and mmap every regular file
/// under its roots — on macOS that is all of `/System/Library/AssetsV2`.
#[cfg(feature = "std")]
#[allow(non_snake_case)]
fn FcCollectFontFilesRecursive(dir: PathBuf) -> Vec<PathBuf> {
    crate::utils::collect_font_files(&dir)
}

#[cfg(all(feature = "std", feature = "parsing"))]
fn FcScanSingleDirectoryRecursive(dir: PathBuf) -> Vec<(FcPattern, FcFontPath)> {
    let files = FcCollectFontFilesRecursive(dir);
    FcParseFontFiles(&files)
}

#[cfg(all(feature = "std", feature = "parsing"))]
fn FcParseFontFiles(files_to_parse: &[PathBuf]) -> Vec<(FcPattern, FcFontPath)> {
    let result = {
        #[cfg(all(feature = "multithreading", not(target_family = "wasm")))]
        {
            use rayon::prelude::*;

            files_to_parse
                .par_iter()
                .filter_map(|file| FcParseFont(file))
                .collect::<Vec<Vec<_>>>()
        }
        #[cfg(not(all(feature = "multithreading", not(target_family = "wasm"))))]
        {
            files_to_parse
                .iter()
                .filter_map(|file| FcParseFont(file))
                .collect::<Vec<Vec<_>>>()
        }
    };

    result.into_iter().flat_map(|f| f.into_iter()).collect()
}

#[cfg(all(feature = "std", feature = "parsing"))]
/// Takes a path & prefix and resolves them to a usable path, or `None` if they're unsupported/unavailable.
///
/// Behaviour is based on: https://www.freedesktop.org/software/fontconfig/fontconfig-user.html
fn process_path(
    prefix: &Option<String>,
    mut path: PathBuf,
    is_include_path: bool,
) -> Option<PathBuf> {
    use std::env::var;

    const HOME_SHORTCUT: &str = "~";
    const CWD_PATH: &str = ".";

    const HOME_ENV_VAR: &str = "HOME";
    const XDG_CONFIG_HOME_ENV_VAR: &str = "XDG_CONFIG_HOME";
    const XDG_CONFIG_HOME_DEFAULT_PATH_SUFFIX: &str = ".config";
    const XDG_DATA_HOME_ENV_VAR: &str = "XDG_DATA_HOME";
    const XDG_DATA_HOME_DEFAULT_PATH_SUFFIX: &str = ".local/share";

    const PREFIX_CWD: &str = "cwd";
    const PREFIX_DEFAULT: &str = "default";
    const PREFIX_XDG: &str = "xdg";

    // These three could, in theory, be cached, but the work required to do so outweighs the minor benefits
    fn get_home_value() -> Option<PathBuf> {
        var(HOME_ENV_VAR).ok().map(PathBuf::from)
    }
    fn get_xdg_config_home_value() -> Option<PathBuf> {
        var(XDG_CONFIG_HOME_ENV_VAR)
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                get_home_value()
                    .map(|home_path| home_path.join(XDG_CONFIG_HOME_DEFAULT_PATH_SUFFIX))
            })
    }
    fn get_xdg_data_home_value() -> Option<PathBuf> {
        var(XDG_DATA_HOME_ENV_VAR)
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                get_home_value().map(|home_path| home_path.join(XDG_DATA_HOME_DEFAULT_PATH_SUFFIX))
            })
    }

    // Resolve the tilde character in the path, if present
    if path.starts_with(HOME_SHORTCUT) {
        if let Some(home_path) = get_home_value() {
            path = home_path.join(
                path.strip_prefix(HOME_SHORTCUT)
                    .expect("already checked that it starts with the prefix"),
            );
        } else {
            return None;
        }
    }

    // Resolve prefix values
    match prefix {
        Some(prefix) => match prefix.as_str() {
            PREFIX_CWD | PREFIX_DEFAULT => {
                let mut new_path = PathBuf::from(CWD_PATH);
                new_path.push(path);

                Some(new_path)
            }
            PREFIX_XDG => {
                if is_include_path {
                    get_xdg_config_home_value()
                        .map(|xdg_config_home_path| xdg_config_home_path.join(path))
                } else {
                    get_xdg_data_home_value()
                        .map(|xdg_data_home_path| xdg_data_home_path.join(path))
                }
            }
            _ => None, // Unsupported prefix
        },
        None => Some(path),
    }
}

// Helper function to extract a string from the name table
#[cfg(all(feature = "std", feature = "parsing"))]
fn get_name_string(name_data: &[u8], name_id: u16) -> Option<String> {
    fontcode_get_name(name_data, name_id)
        .ok()
        .flatten()
        .map(|name| String::from_utf8_lossy(name.to_bytes()).to_string())
}

/// Find the best Unicode CMAP subtable from a font provider.
/// Tries multiple platform/encoding combinations in priority order.
#[cfg(all(feature = "std", feature = "parsing"))]
fn find_best_cmap_subtable<'a>(
    cmap: &allsorts::tables::cmap::Cmap<'a>,
) -> Option<allsorts::tables::cmap::EncodingRecord> {
    use allsorts::tables::cmap::{EncodingId, PlatformId};

    // Full-repertoire subtables first (they carry the astral planes: emoji,
    // CJK extensions), BMP-only ones after — the order FreeType and
    // fontconfig use.
    cmap.find_subtable(PlatformId::UNICODE, EncodingId(4))
        .or_else(|| cmap.find_subtable(PlatformId::WINDOWS, EncodingId(10)))
        .or_else(|| cmap.find_subtable(PlatformId::UNICODE, EncodingId(3)))
        .or_else(|| cmap.find_subtable(PlatformId::WINDOWS, EncodingId(1)))
        .or_else(|| cmap.find_subtable(PlatformId::UNICODE, EncodingId(0)))
        .or_else(|| cmap.find_subtable(PlatformId::UNICODE, EncodingId(1)))
}

/// Exact coverage of a font face: every codepoint its best Unicode cmap
/// subtable maps to a real glyph, as a normalized (sorted, disjoint) range
/// list. This is the same source fontconfig builds its `FcCharSet` from.
/// OS/2's `ulUnicodeRange` bits are not consulted: fonts get them wrong in
/// both directions, and a block-level hint cannot say which characters of a
/// block are missing.
///
/// Cost is proportional to the subtable's segment count (format 4) or group
/// count (format 12) — hundreds to a few thousand entries — with no
/// per-codepoint lookups except inside format-4 segments that index the
/// glyphIdArray, which can contain holes.
#[cfg(all(feature = "std", feature = "parsing"))]
fn cmap_coverage(provider: &impl FontTableProvider) -> Option<Vec<UnicodeRange>> {
    use allsorts::binary::read::ReadScope;
    use allsorts::tables::cmap::{Cmap, CmapSubtable};

    let cmap_data = provider.table_data(tag::CMAP).ok()??;
    let cmap = ReadScope::new(&cmap_data).read::<Cmap<'_>>().ok()?;
    let record = find_best_cmap_subtable(&cmap)?;
    let subtable = ReadScope::new(&cmap_data)
        .offset(record.offset as usize)
        .read::<CmapSubtable<'_>>()
        .ok()?;
    coverage_from_subtable(&subtable, &cmap_data, record.offset as usize)
}

/// See [`cmap_coverage`]. `cmap_data` and `offset` locate the raw subtable,
/// needed for format 12 whose groups allsorts does not expose.
#[cfg(all(feature = "std", feature = "parsing"))]
fn coverage_from_subtable(
    subtable: &allsorts::tables::cmap::CmapSubtable<'_>,
    cmap_data: &[u8],
    offset: usize,
) -> Option<Vec<UnicodeRange>> {
    use allsorts::tables::cmap::CmapSubtable;

    let mut ranges: Vec<UnicodeRange> = Vec::new();
    let mut push = |start: u32, end: u32| {
        if start > end {
            return;
        }
        match ranges.last_mut() {
            Some(last) if start <= last.end.saturating_add(1) => last.end = last.end.max(end),
            _ => ranges.push(UnicodeRange { start, end }),
        }
    };

    match subtable {
        CmapSubtable::Format4(f4) => {
            let seg_count = f4.start_codes.len();
            let glyph_ids: Vec<u16> = f4.glyph_id_array.iter().collect();
            let segments = f4
                .start_codes
                .iter()
                .zip(f4.end_codes.iter())
                .zip(f4.id_deltas.iter())
                .zip(f4.id_range_offsets.iter())
                .enumerate();
            for (i, (((start, end), delta), range_offset)) in segments {
                if start == 0xFFFF {
                    continue; // the mandatory terminal segment
                }
                let (start, end) = (start as u32, end as u32);
                if range_offset == 0 {
                    // gid = code + delta (mod 2^16): exactly one code of the
                    // segment can land on glyph 0.
                    let zero_code = (delta as u16).wrapping_neg() as u32;
                    if zero_code >= start && zero_code <= end {
                        if zero_code > start {
                            push(start, zero_code - 1);
                        }
                        if zero_code < end {
                            push(zero_code + 1, end);
                        }
                    } else {
                        push(start, end);
                    }
                } else {
                    // glyphIdArray-indexed segment (OpenType cmap §format 4):
                    // the value for `code` sits at
                    // idRangeOffset/2 + (code - start) - (segCount - i) in
                    // glyphIdArray; 0 there means missing, otherwise idDelta
                    // is added. Indexed directly: a CJK BMP subtable has
                    // thousands of these codes, and a per-code lookup through
                    // the subtable is a linear scan over its segments.
                    let base = (range_offset as usize / 2).wrapping_sub(seg_count - i);
                    for code in start..=end {
                        let index = base.wrapping_add((code - start) as usize);
                        let Some(&value) = glyph_ids.get(index) else {
                            continue;
                        };
                        if value != 0 && value.wrapping_add(delta as u16) != 0 {
                            push(code, code);
                        }
                    }
                }
            }
        }
        CmapSubtable::Format12 { .. } => {
            for (start, end, start_gid) in format12_groups(cmap_data, offset)? {
                // gid = start_gid + (code - start): only the first code of a
                // group that starts at glyph 0 maps to .notdef.
                let first = if start_gid == 0 {
                    start.saturating_add(1)
                } else {
                    start
                };
                push(first, end.min(0x10FFFF));
            }
        }
        CmapSubtable::Format0 { glyph_id_array, .. } => {
            for (code, gid) in glyph_id_array.iter().enumerate() {
                if gid != 0 {
                    push(code as u32, code as u32);
                }
            }
        }
        CmapSubtable::Format6 {
            first_code,
            glyph_id_array,
            ..
        } => {
            for (i, gid) in glyph_id_array.iter().enumerate() {
                if gid != 0 {
                    let code = *first_code as u32 + i as u32;
                    push(code, code);
                }
            }
        }
        CmapSubtable::Format10 {
            start_char_code,
            glyph_id_array,
            ..
        } => {
            for (i, gid) in glyph_id_array.iter().enumerate() {
                if gid != 0 {
                    let code = *start_char_code + i as u32;
                    push(code, code);
                }
            }
        }
        CmapSubtable::Format2 { .. } => {
            // Legacy mixed 8/16-bit CJK encodings — never a Unicode subtable,
            // but if it is all the font has, enumerate it.
            let mut codes: Vec<u32> = Vec::new();
            subtable
                .mappings_fn(|code, gid| {
                    if gid != 0 {
                        codes.push(code);
                    }
                })
                .ok()?;
            codes.sort_unstable();
            for code in codes {
                push(code, code);
            }
        }
    }

    if ranges.is_empty() {
        None
    } else {
        Some(FcFontCache::normalize_unicode_ranges(ranges))
    }
}

/// The `(startCharCode, endCharCode, startGlyphID)` groups of the format-12
/// subtable at `offset` in the raw cmap table. Layout: format u16, reserved
/// u16, length u32, language u32, numGroups u32, then the groups.
#[cfg(all(feature = "std", feature = "parsing"))]
fn format12_groups(cmap_data: &[u8], offset: usize) -> Option<Vec<(u32, u32, u32)>> {
    let table = cmap_data.get(offset..)?;
    let u16_at = |at: usize| {
        table
            .get(at..at + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
    };
    let u32_at = |at: usize| {
        table
            .get(at..at + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    if u16_at(0)? != 12 {
        return None;
    }
    let num_groups = u32_at(12)? as usize;
    let mut groups = Vec::with_capacity(num_groups.min(1 << 16));
    for i in 0..num_groups {
        let at = 16 + i * 12;
        groups.push((u32_at(at)?, u32_at(at + 4)?, u32_at(at + 8)?));
    }
    Some(groups)
}

// Helper function to detect if a font is monospace
#[cfg(all(feature = "std", feature = "parsing"))]
fn detect_monospace(
    provider: &impl FontTableProvider,
    os2_table: Option<&Os2>,
    detected_monospace: Option<bool>,
) -> Option<bool> {
    if let Some(is_monospace) = detected_monospace {
        return Some(is_monospace);
    }

    // Try using PANOSE classification, when there is an OS/2 table to read it
    // from; otherwise fall straight through to the hmtx width check.
    if let Some(os2_table) = os2_table {
        if os2_table.panose[0] == 2 {
            // 2 = Latin Text
            return Some(os2_table.panose[3] == 9); // 9 = Monospaced
        }
    }

    // Check glyph widths in hmtx table
    let hhea_data = provider.table_data(tag::HHEA).ok()??;
    let hhea_table = ReadScope::new(&hhea_data).read::<HheaTable>().ok()?;
    let maxp_data = provider.table_data(tag::MAXP).ok()??;
    let maxp_table = ReadScope::new(&maxp_data).read::<MaxpTable>().ok()?;
    let hmtx_data = provider.table_data(tag::HMTX).ok()??;
    let hmtx_table = ReadScope::new(&hmtx_data)
        .read_dep::<HmtxTable<'_>>((
            usize::from(maxp_table.num_glyphs),
            usize::from(hhea_table.num_h_metrics),
        ))
        .ok()?;

    let mut monospace = true;
    let mut last_advance = 0;

    // Check if all advance widths are the same
    for i in 0..hhea_table.num_h_metrics as usize {
        let advance = hmtx_table.h_metrics.read_item(i).ok()?.advance_width;
        if i > 0 && advance != last_advance {
            monospace = false;
            break;
        }
        last_advance = advance;
    }

    Some(monospace)
}

/// Guess font metadata from a filename using the existing tokenizer.
///
/// Uses [`config::tokenize_font_stem`] and [`config::FONT_STYLE_TOKENS`]
/// to extract the family name and detect style hints from the filename.
///
/// Only compiled for the filename-only (`not(parsing)`) scan path — its
/// sole caller is [`FcFontCache::build_from_filenames`]. With `parsing`
/// on, allsorts reads real metadata and this fallback is unused.
#[cfg(all(feature = "std", not(feature = "parsing")))]
fn pattern_from_filename(path: &std::path::Path) -> Option<FcPattern> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "ttf" | "otf" | "ttc" | "woff" | "woff2" => {}
        _ => return None,
    }

    let stem = path.file_stem()?.to_str()?;
    let all_tokens = crate::config::tokenize_lowercase(stem);

    // Style detection: check if any token matches a known style keyword
    let has_token = |kw: &str| all_tokens.iter().any(|t| t == kw);
    let is_bold = has_token("bold") || has_token("heavy");
    let is_italic = has_token("italic");
    let is_oblique = has_token("oblique");
    let is_mono = has_token("mono") || has_token("monospace");
    let is_condensed = has_token("condensed");

    // Family = non-style tokens joined
    let family_tokens = crate::config::tokenize_font_stem(stem);
    if family_tokens.is_empty() {
        return None;
    }
    let family = family_tokens.join(" ");

    Some(FcPattern {
        name: Some(stem.to_string()),
        family: Some(family),
        bold: if is_bold {
            PatternMatch::True
        } else {
            PatternMatch::False
        },
        italic: if is_italic {
            PatternMatch::True
        } else {
            PatternMatch::False
        },
        oblique: if is_oblique {
            PatternMatch::True
        } else {
            PatternMatch::DontCare
        },
        monospace: if is_mono {
            PatternMatch::True
        } else {
            PatternMatch::DontCare
        },
        condensed: if is_condensed {
            PatternMatch::True
        } else {
            PatternMatch::DontCare
        },
        weight: if is_bold {
            FcWeight::Bold
        } else {
            FcWeight::Normal
        },
        stretch: if is_condensed {
            FcStretch::Condensed
        } else {
            FcStretch::Normal
        },
        unicode_ranges: Vec::new(),
        metadata: FcFontMetadata::default(),
        render_config: FcFontRenderConfig::default(),
    })
}

#[cfg(all(test, feature = "std", feature = "parsing"))]
mod system_alias_tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0"?>
<fontconfig>
  <alias>
    <family>sans-serif</family>
    <prefer>
      <family>Noto Sans</family>
      <family>DejaVu Sans</family>
    </prefer>
  </alias>
  <alias>
    <family>Arial</family>
    <prefer><family>Liberation Sans</family></prefer>
  </alias>
  <alias binding="same">
    <family>monospace</family>
    <prefer><family>Noto Sans Mono</family></prefer>
  </alias>
</fontconfig>"#;

    const SECOND_FILE: &str = r#"<fontconfig>
  <alias>
    <family>sans-serif</family>
    <prefer>
      <family>Ubuntu</family>
      <family>Noto Sans</family>
    </prefer>
  </alias>
</fontconfig>"#;

    #[test]
    fn alias_blocks_parse_with_order_and_dedup_across_files() {
        let mut aliases = BTreeMap::new();
        ParseFontsConfAliases(SAMPLE, &mut aliases);
        ParseFontsConfAliases(SECOND_FILE, &mut aliases);
        let key = crate::utils::normalize_family_name("sans-serif");
        assert_eq!(
            aliases.get(&key).map(Vec::as_slice),
            Some(
                &[
                    "Noto Sans".to_string(),
                    "DejaVu Sans".to_string(),
                    "Ubuntu".to_string()
                ][..]
            ),
            "prefer entries append across files in include order, deduplicated"
        );
        assert_eq!(
            aliases.get("arial").map(Vec::as_slice),
            Some(&["Liberation Sans".to_string()][..]),
            "named-family aliases parse too (key normalized)"
        );
        assert_eq!(
            aliases.get("monospace").map(Vec::as_slice),
            Some(&["Noto Sans Mono".to_string()][..]),
            "alias attributes (binding=...) do not confuse the parser"
        );
    }

    #[test]
    fn config_first_expansion_beats_the_builtin_lists() {
        // What `build_inner` does on Linux: the parsed aliases are the
        // authority, the built-in tables only fill what they leave unsaid.
        let mut aliases = BTreeMap::new();
        ParseFontsConfAliases(SAMPLE, &mut aliases);
        let mut config = FcFallbackConfig::default();
        config.absorb_system_aliases(aliases);
        config.merge_defaults(&FcFallbackConfig::os_defaults(OperatingSystem::Linux));

        let cache = FcFontCache::default().with_fallback_config(config);
        let out = cache
            .fallback_config()
            .candidate_families(&["Arial".to_string(), "sans-serif".to_string()], &[]);
        assert_eq!(
            out,
            vec![
                "Arial".to_string(),           // named family keeps itself first
                "Liberation Sans".to_string(), // its configured substitution
                "Noto Sans".to_string(),       // sans-serif configured prefer list
                "DejaVu Sans".to_string(),
            ],
            "configured preferences resolve the stack; no built-in list entries leak in"
        );
    }

    #[test]
    fn generic_family_without_config_falls_back_to_builtin_lists() {
        let mut config = FcFallbackConfig::default();
        config.merge_defaults(&FcFallbackConfig::os_defaults(OperatingSystem::Linux));
        let out = config.candidate_families(&["sans-serif".to_string()], &[]);
        assert!(
            !out.is_empty() && out.iter().any(|f| f == "DejaVu Sans"),
            "no configuration parsed -> the built-in candidates are the last resort: {out:?}"
        );
    }
}

#[cfg(all(test, feature = "std", feature = "parsing"))]
mod coverage_tests {
    use super::*;
    use allsorts::binary::read::ReadScope;
    use allsorts::font_data::FontData;
    use allsorts::tables::cmap::{Cmap, CmapSubtable};
    use allsorts::tables::FontTableProvider;

    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/InstrumentSerif-Regular.ttf");

    /// Every codepoint up to `max` the face's best subtable maps to a real
    /// glyph, found the slow way: one `map_glyph` per codepoint.
    fn brute_force(bytes: &[u8], face: usize, max: u32) -> Vec<UnicodeRange> {
        let font = ReadScope::new(bytes)
            .read::<FontData<'_>>()
            .expect("font data");
        let provider = font.table_provider(face).expect("face");
        let cmap_data = provider
            .table_data(tag::CMAP)
            .expect("cmap")
            .expect("cmap present");
        let cmap = ReadScope::new(&cmap_data)
            .read::<Cmap<'_>>()
            .expect("cmap header");
        let record = find_best_cmap_subtable(&cmap).expect("a Unicode subtable");
        let subtable = ReadScope::new(&cmap_data)
            .offset(record.offset as usize)
            .read::<CmapSubtable<'_>>()
            .expect("subtable");
        // allsorts' format-12 `map_glyph` is a linear scan over the groups,
        // so enumerate those subtables once instead of probing per codepoint.
        let mut codes: Vec<u32> = Vec::new();
        if matches!(subtable, CmapSubtable::Format12 { .. }) {
            subtable
                .mappings_fn(|cp, gid| {
                    if gid != 0 && cp <= max {
                        codes.push(cp);
                    }
                })
                .expect("format-12 mappings");
            codes.sort_unstable();
            codes.dedup();
        } else {
            for cp in 0..=max {
                if (0xD800..=0xDFFF).contains(&cp) {
                    continue;
                }
                if matches!(subtable.map_glyph(cp), Ok(Some(gid)) if gid != 0) {
                    codes.push(cp);
                }
            }
        }
        let mut out: Vec<UnicodeRange> = Vec::new();
        for cp in codes {
            match out.last_mut() {
                Some(last) if last.end + 1 == cp => last.end = cp,
                _ => out.push(UnicodeRange { start: cp, end: cp }),
            }
        }
        out
    }

    fn clipped(ranges: &[UnicodeRange], max: u32) -> Vec<UnicodeRange> {
        ranges
            .iter()
            .filter(|r| r.start <= max)
            .map(|r| UnicodeRange {
                start: r.start,
                end: r.end.min(max),
            })
            .collect()
    }

    #[test]
    fn format12_groups_are_read_from_the_raw_table() {
        let mut table = Vec::new();
        table.extend_from_slice(&12u16.to_be_bytes()); // format
        table.extend_from_slice(&0u16.to_be_bytes()); // reserved
        table.extend_from_slice(&(16u32 + 2 * 12).to_be_bytes()); // length
        table.extend_from_slice(&0u32.to_be_bytes()); // language
        table.extend_from_slice(&2u32.to_be_bytes()); // numGroups
        for (start, end, gid) in [(0x20u32, 0x7Eu32, 3u32), (0x1F600, 0x1F64F, 200)] {
            table.extend_from_slice(&start.to_be_bytes());
            table.extend_from_slice(&end.to_be_bytes());
            table.extend_from_slice(&gid.to_be_bytes());
        }
        // Embedded at an offset, as inside a real cmap table.
        let mut cmap = vec![0u8; 40];
        cmap.extend_from_slice(&table);

        assert_eq!(
            format12_groups(&cmap, 40),
            Some(vec![(0x20, 0x7E, 3), (0x1F600, 0x1F64F, 200)])
        );
        assert_eq!(
            format12_groups(&cmap, 0),
            None,
            "offset 0 is not a format-12 subtable"
        );
        assert_eq!(
            format12_groups(&cmap[..50], 40),
            None,
            "a truncated table is rejected"
        );
    }

    /// The parsed coverage of the bundled fixture is exactly the set of
    /// codepoints its cmap maps — no block rounding in either direction.
    #[test]
    fn fixture_coverage_equals_the_cmap_exactly() {
        let faces = FcParseFontBytes(FIXTURE, "fixture").expect("the fixture parses");
        let parsed = &faces[0].0.unicode_ranges;
        assert!(!parsed.is_empty());
        assert_eq!(
            *parsed,
            FcFontCache::normalize_unicode_ranges(parsed.clone()),
            "stored coverage is normalized"
        );

        let exact = brute_force(FIXTURE, 0, 0x10FFFF);
        assert_eq!(
            *parsed, exact,
            "segment walk and per-codepoint lookup disagree"
        );

        // Sanity on the shape: a Latin text face, not a block-rounded one.
        assert!(crate::fallback::covers(parsed, 'A' as u32));
        assert!(!crate::fallback::covers(parsed, 0x4E00));
        let latin_ext_a = UnicodeRange {
            start: 0x0100,
            end: 0x017F,
        };
        let overlap = crate::fallback::overlap_size(parsed, &latin_ext_a);
        assert!(
            overlap > 0 && overlap < 128,
            "the fixture covers part of Latin Extended-A ({overlap} of 128); a block-rounded \
             coverage would report all or nothing"
        );
    }

    fn with_best_subtable<R>(
        bytes: &[u8],
        face: usize,
        f: impl FnOnce(&CmapSubtable<'_>, &[u8], usize) -> R,
    ) -> Option<R> {
        let font = ReadScope::new(bytes).read::<FontData<'_>>().ok()?;
        let provider = font.table_provider(face).ok()?;
        let cmap_data = provider.table_data(tag::CMAP).ok()??;
        let cmap = ReadScope::new(&cmap_data).read::<Cmap<'_>>().ok()?;
        let record = find_best_cmap_subtable(&cmap)?;
        let subtable = ReadScope::new(&cmap_data)
            .offset(record.offset as usize)
            .read::<CmapSubtable<'_>>()
            .ok()?;
        Some(f(&subtable, &cmap_data, record.offset as usize))
    }

    /// Every installed face: each parsed range starts and ends on a mapped
    /// codepoint and the codepoints just outside it are unmapped. O(ranges)
    /// per face, so a whole system takes seconds; the fixture test above is
    /// the full codepoint-by-codepoint reference. Run on demand:
    /// `cargo test --features parsing --lib every_installed -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn every_installed_font_coverage_matches_its_cmap_at_every_boundary() {
        fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if crate::utils::is_font_file(&path) {
                    out.push(path);
                }
            }
        }
        let mut files = Vec::new();
        for dir in crate::config::font_directories(OperatingSystem::current()) {
            walk(&dir, &mut files);
        }
        // A bounded sample keeps this to seconds on machines with thousands
        // of downloadable fonts (macOS AssetsV2); set RFC_COVERAGE_CHECK_ALL
        // to check every file.
        if std::env::var_os("RFC_COVERAGE_CHECK_ALL").is_none() {
            files.truncate(400);
        }
        let surrogate = |cp: u32| (0xD800..=0xDFFF).contains(&cp);
        let (mut faces_checked, mut skipped) = (0usize, 0usize);
        for path in &files {
            let Ok(bytes) = std::fs::read(path) else {
                continue;
            };
            let Some(faces) = FcParseFontBytes(&bytes, &path.to_string_lossy()) else {
                skipped += 1;
                continue;
            };
            let mut seen_faces = alloc::collections::BTreeSet::new();
            for (pattern, font) in &faces {
                if !seen_faces.insert(font.font_index) {
                    continue;
                }
                let where_ = format!("{}#{}", path.display(), font.font_index);
                let checked =
                    with_best_subtable(&bytes, font.font_index, |subtable, cmap_data, offset| {
                        // allsorts' format-12 `map_glyph` scans the groups linearly;
                        // a CJK face has thousands, so look those up by binary search.
                        let groups = match subtable {
                            CmapSubtable::Format12 { .. } => format12_groups(cmap_data, offset),
                            _ => None,
                        };
                        let mapped = |cp: u32| match &groups {
                            Some(groups) => {
                                let i = groups.partition_point(|g| g.1 < cp);
                                groups.get(i).is_some_and(|&(start, end, gid)| {
                                    start <= cp && cp <= end && (gid != 0 || cp != start)
                                })
                            }
                            None => matches!(subtable.map_glyph(cp), Ok(Some(gid)) if gid != 0),
                        };
                        for r in &pattern.unicode_ranges {
                            assert!(
                                mapped(r.start) && mapped(r.end),
                                "{where_}: {r:?} does not end on mapped codepoints"
                            );
                            if r.start > 0 && !surrogate(r.start - 1) {
                                assert!(!mapped(r.start - 1), "{where_}: {r:?} starts late");
                            }
                            if r.end < 0x10FFFF && !surrogate(r.end + 1) {
                                assert!(!mapped(r.end + 1), "{where_}: {r:?} ends early");
                            }
                        }
                    });
                if checked.is_some() {
                    faces_checked += 1;
                }
            }
        }
        println!(
            "checked {faces_checked} faces in {} files ({skipped} unparsable)",
            files.len()
        );
        assert!(faces_checked > 0, "no fonts found to check");
    }
}
