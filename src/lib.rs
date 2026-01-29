//! # rust-fontconfig
//!
//! Pure-Rust rewrite of the Linux fontconfig library (no system dependencies) - using allsorts as a font parser to support `.woff`, `.woff2`, `.ttc`, `.otf` and `.ttf`
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
//! }
//! ```

#![allow(non_snake_case)]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

#[cfg(all(feature = "std", feature = "parsing"))]
use alloc::borrow::ToOwned;
use alloc::collections::btree_map::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::{format, vec};
#[cfg(feature = "parsing")]
use allsorts::binary::read::ReadScope;
#[cfg(all(feature = "std", feature = "parsing"))]
use allsorts::get_name::fontcode_get_name;
#[cfg(feature = "parsing")]
use allsorts::tables::os2::Os2;
#[cfg(feature = "parsing")]
use allsorts::tables::{FontTableProvider, HheaTable, HmtxTable, MaxpTable};
#[cfg(feature = "parsing")]
use allsorts::tag;
#[cfg(all(feature = "std", feature = "parsing"))]
use std::path::PathBuf;

#[cfg(feature = "ffi")]
pub mod ffi;

/// Operating system type for generic font family resolution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperatingSystem {
    Windows,
    Linux,
    MacOS,
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
        
        #[cfg(target_family = "wasm")]
        return OperatingSystem::Wasm;
        
        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos", target_family = "wasm")))]
        return OperatingSystem::Linux; // Default fallback
    }
    
    /// Get system-specific fonts for the "serif" generic family
    /// Prioritizes fonts based on Unicode range coverage
    pub fn get_serif_fonts(&self, unicode_ranges: &[UnicodeRange]) -> Vec<String> {
        let has_cjk = unicode_ranges.iter().any(|r| {
            (r.start >= 0x4E00 && r.start <= 0x9FFF) || // CJK Unified Ideographs
            (r.start >= 0x3040 && r.start <= 0x309F) || // Hiragana
            (r.start >= 0x30A0 && r.start <= 0x30FF) || // Katakana
            (r.start >= 0xAC00 && r.start <= 0xD7AF)    // Hangul
        });
        
        let has_arabic = unicode_ranges.iter().any(|r| r.start >= 0x0600 && r.start <= 0x06FF);
        let _has_cyrillic = unicode_ranges.iter().any(|r| r.start >= 0x0400 && r.start <= 0x04FF);
        
        match self {
            OperatingSystem::Windows => {
                let mut fonts = Vec::new();
                if has_cjk {
                    fonts.extend_from_slice(&["MS Mincho", "SimSun", "MingLiU"]);
                }
                if has_arabic {
                    fonts.push("Traditional Arabic");
                }
                fonts.push("Times New Roman");
                fonts.iter().map(|s| s.to_string()).collect()
            }
            OperatingSystem::Linux => {
                let mut fonts = Vec::new();
                if has_cjk {
                    fonts.extend_from_slice(&["Noto Serif CJK SC", "Noto Serif CJK JP", "Noto Serif CJK KR"]);
                }
                if has_arabic {
                    fonts.push("Noto Serif Arabic");
                }
                fonts.extend_from_slice(&[
                    "Times", "Times New Roman", "DejaVu Serif", "Free Serif", 
                    "Noto Serif", "Bitstream Vera Serif", "Roman", "Regular"
                ]);
                fonts.iter().map(|s| s.to_string()).collect()
            }
            OperatingSystem::MacOS => {
                let mut fonts = Vec::new();
                if has_cjk {
                    fonts.extend_from_slice(&["Hiragino Mincho ProN", "STSong", "AppleMyungjo"]);
                }
                if has_arabic {
                    fonts.push("Geeza Pro");
                }
                fonts.extend_from_slice(&["Times", "New York", "Palatino"]);
                fonts.iter().map(|s| s.to_string()).collect()
            }
            OperatingSystem::Wasm => Vec::new(),
        }
    }
    
    /// Get system-specific fonts for the "sans-serif" generic family
    /// Prioritizes fonts based on Unicode range coverage
    pub fn get_sans_serif_fonts(&self, unicode_ranges: &[UnicodeRange]) -> Vec<String> {
        let has_cjk = unicode_ranges.iter().any(|r| {
            (r.start >= 0x4E00 && r.start <= 0x9FFF) || // CJK Unified Ideographs
            (r.start >= 0x3040 && r.start <= 0x309F) || // Hiragana
            (r.start >= 0x30A0 && r.start <= 0x30FF) || // Katakana
            (r.start >= 0xAC00 && r.start <= 0xD7AF)    // Hangul
        });
        
        let has_arabic = unicode_ranges.iter().any(|r| r.start >= 0x0600 && r.start <= 0x06FF);
        let _has_cyrillic = unicode_ranges.iter().any(|r| r.start >= 0x0400 && r.start <= 0x04FF);
        let has_hebrew = unicode_ranges.iter().any(|r| r.start >= 0x0590 && r.start <= 0x05FF);
        let has_thai = unicode_ranges.iter().any(|r| r.start >= 0x0E00 && r.start <= 0x0E7F);
        
        match self {
            OperatingSystem::Windows => {
                let mut fonts = Vec::new();
                if has_cjk {
                    fonts.extend_from_slice(&["Microsoft YaHei", "MS Gothic", "Malgun Gothic", "SimHei"]);
                }
                if has_arabic {
                    fonts.push("Segoe UI Arabic");
                }
                if has_hebrew {
                    fonts.push("Segoe UI Hebrew");
                }
                if has_thai {
                    fonts.push("Leelawadee UI");
                }
                fonts.extend_from_slice(&["Segoe UI", "Tahoma", "Microsoft Sans Serif", "MS Sans Serif", "Helv"]);
                fonts.iter().map(|s| s.to_string()).collect()
            }
            OperatingSystem::Linux => {
                let mut fonts = Vec::new();
                if has_cjk {
                    fonts.extend_from_slice(&[
                        "Noto Sans CJK SC", "Noto Sans CJK JP", "Noto Sans CJK KR",
                        "WenQuanYi Micro Hei", "Droid Sans Fallback"
                    ]);
                }
                if has_arabic {
                    fonts.push("Noto Sans Arabic");
                }
                if has_hebrew {
                    fonts.push("Noto Sans Hebrew");
                }
                if has_thai {
                    fonts.push("Noto Sans Thai");
                }
                fonts.extend_from_slice(&["Ubuntu", "Arial", "DejaVu Sans", "Noto Sans", "Liberation Sans"]);
                fonts.iter().map(|s| s.to_string()).collect()
            }
            OperatingSystem::MacOS => {
                let mut fonts = Vec::new();
                if has_cjk {
                    fonts.extend_from_slice(&[
                        "Hiragino Sans", "Hiragino Kaku Gothic ProN", 
                        "PingFang SC", "PingFang TC", "Apple SD Gothic Neo"
                    ]);
                }
                if has_arabic {
                    fonts.push("Geeza Pro");
                }
                if has_hebrew {
                    fonts.push("Arial Hebrew");
                }
                if has_thai {
                    fonts.push("Thonburi");
                }
                fonts.extend_from_slice(&["San Francisco", "Helvetica Neue", "Lucida Grande"]);
                fonts.iter().map(|s| s.to_string()).collect()
            }
            OperatingSystem::Wasm => Vec::new(),
        }
    }
    
    /// Get system-specific fonts for the "monospace" generic family
    /// Prioritizes fonts based on Unicode range coverage
    pub fn get_monospace_fonts(&self, unicode_ranges: &[UnicodeRange]) -> Vec<String> {
        let has_cjk = unicode_ranges.iter().any(|r| {
            (r.start >= 0x4E00 && r.start <= 0x9FFF) || // CJK Unified Ideographs
            (r.start >= 0x3040 && r.start <= 0x309F) || // Hiragana
            (r.start >= 0x30A0 && r.start <= 0x30FF) || // Katakana
            (r.start >= 0xAC00 && r.start <= 0xD7AF)    // Hangul
        });
        
        match self {
            OperatingSystem::Windows => {
                let mut fonts = Vec::new();
                if has_cjk {
                    fonts.extend_from_slice(&["MS Gothic", "SimHei"]);
                }
                fonts.extend_from_slice(&["Segoe UI Mono", "Courier New", "Cascadia Code", "Cascadia Mono", "Consolas"]);
                fonts.iter().map(|s| s.to_string()).collect()
            }
            OperatingSystem::Linux => {
                let mut fonts = Vec::new();
                if has_cjk {
                    fonts.extend_from_slice(&["Noto Sans Mono CJK SC", "Noto Sans Mono CJK JP", "WenQuanYi Zen Hei Mono"]);
                }
                fonts.extend_from_slice(&[
                    "Source Code Pro", "Cantarell", "DejaVu Sans Mono", 
                    "Roboto Mono", "Ubuntu Monospace", "Droid Sans Mono"
                ]);
                fonts.iter().map(|s| s.to_string()).collect()
            }
            OperatingSystem::MacOS => {
                let mut fonts = Vec::new();
                if has_cjk {
                    fonts.extend_from_slice(&["Hiragino Sans", "PingFang SC"]);
                }
                fonts.extend_from_slice(&["SF Mono", "Menlo", "Monaco", "Courier", "Oxygen Mono", "Source Code Pro", "Fira Mono"]);
                fonts.iter().map(|s| s.to_string()).collect()
            }
            OperatingSystem::Wasm => Vec::new(),
        }
    }
    
    /// Expand a generic CSS font family to system-specific font names
    /// Returns the original name if not a generic family
    /// Prioritizes fonts based on Unicode range coverage
    pub fn expand_generic_family(&self, family: &str, unicode_ranges: &[UnicodeRange]) -> Vec<String> {
        match family.to_lowercase().as_str() {
            "serif" => self.get_serif_fonts(unicode_ranges),
            "sans-serif" => self.get_sans_serif_fonts(unicode_ranges),
            "monospace" => self.get_monospace_fonts(unicode_ranges),
            "cursive" | "fantasy" | "system-ui" => {
                // Use sans-serif as fallback for these
                self.get_sans_serif_fonts(unicode_ranges)
            }
            _ => vec![family.to_string()],
        }
    }
}

/// Expand a CSS font-family stack with generic families resolved to OS-specific fonts
/// Prioritizes fonts based on Unicode range coverage
/// Example: ["Arial", "sans-serif"] on macOS with CJK ranges -> ["Arial", "PingFang SC", "Hiragino Sans", ...]
pub fn expand_font_families(families: &[String], os: OperatingSystem, unicode_ranges: &[UnicodeRange]) -> Vec<String> {
    let mut expanded = Vec::new();
    
    for family in families {
        expanded.extend(os.expand_generic_family(family, unicode_ranges));
    }
    
    expanded
}

/// UUID to identify a font (collections are broken up into separate fonts)
#[derive(Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Hash)]
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
    /// Generate a new pseudo-UUID without external dependencies
    pub fn new() -> Self {
        #[cfg(feature = "std")]
        {
            use std::time::{SystemTime, UNIX_EPOCH};
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default();

            let time_part = now.as_nanos();
            let random_part = {
                // Simple PRNG based on time
                let seed = now.as_secs() as u64;
                let a = 6364136223846793005u64;
                let c = 1442695040888963407u64;
                let r = a.wrapping_mul(seed).wrapping_add(c);
                r as u64
            };

            // Combine time and random parts
            let id = (time_part & 0xFFFFFFFFFFFFFFFFu128) | ((random_part as u128) << 64);
            FontId(id)
        }

        #[cfg(not(feature = "std"))]
        {
            // For no_std contexts, just use a counter
            static mut COUNTER: u128 = 0;
            let id = unsafe {
                COUNTER += 1;
                COUNTER
            };
            FontId(id)
        }
    }
}

/// Whether a field is required to match (yes / no / don't care)
#[derive(Debug, Default, Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Hash)]
#[repr(C)]
pub enum PatternMatch {
    /// Default: don't particularly care whether the requirement matches
    #[default]
    DontCare,
    /// Requirement has to be true for the selected font
    True,
    /// Requirement has to be false for the selected font
    False,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnicodeRange {
    pub start: u32,
    pub end: u32,
}

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

/// Font pattern for matching
#[derive(Default, Clone, PartialOrd, Ord, PartialEq, Eq)]
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

        d.finish()
    }
}

/// Font metadata from the OS/2 table
#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord)]
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

/// Resolved font fallback chain for a CSS font-family stack
/// This represents the complete chain of fonts to use for rendering text
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFallbackChain {
    /// CSS-based fallbacks: Each CSS font expanded to its system fallbacks
    /// Example: ["NotoSansJP" -> [Hiragino Sans, PingFang SC], "sans-serif" -> [Helvetica]]
    pub css_fallbacks: Vec<CssFallbackGroup>,
    
    /// Unicode-based fallbacks: Fonts added to cover missing Unicode ranges
    /// Only populated if css_fallbacks don't cover all requested characters
    pub unicode_fallbacks: Vec<FontMatch>,
    
    /// The original CSS font-family stack that was requested
    pub original_stack: Vec<String>,
}

impl FontFallbackChain {
    /// Resolve which font should be used for a specific character
    /// Returns (FontId, css_source_name) where css_source_name indicates which CSS font matched
    /// Returns None if no font in the chain can render this character
    pub fn resolve_char(&self, cache: &FcFontCache, ch: char) -> Option<(FontId, String)> {
        let codepoint = ch as u32;
        
        // First check CSS fallbacks in order
        for group in &self.css_fallbacks {
            for font in &group.fonts {
                if let Some(meta) = cache.get_metadata_by_id(&font.id) {
                    // Check if this font's unicode ranges cover the character
                    if meta.unicode_ranges.is_empty() {
                        // Font has no unicode range info - skip it, don't assume it covers everything
                        // This is important because fonts that don't properly declare their ranges
                        // should not be used as a catch-all
                        continue;
                    } else {
                        // Check if character is in any of the font's ranges
                        for range in &meta.unicode_ranges {
                            if codepoint >= range.start && codepoint <= range.end {
                                return Some((font.id, group.css_name.clone()));
                            }
                        }
                        // Character not in any range - continue to next font
                    }
                }
            }
        }
        
        // If not found in CSS fallbacks, check Unicode fallbacks
        for font in &self.unicode_fallbacks {
            if let Some(meta) = cache.get_metadata_by_id(&font.id) {
                // Check if this font's unicode ranges cover the character
                for range in &meta.unicode_ranges {
                    if codepoint >= range.start && codepoint <= range.end {
                        return Some((font.id, "(unicode-fallback)".to_string()));
                    }
                }
            }
        }
        
        None
    }
    
    /// Resolve all characters in a text string to their fonts
    /// Returns a vector of (character, FontId, css_source) tuples
    pub fn resolve_text(&self, cache: &FcFontCache, text: &str) -> Vec<(char, Option<(FontId, String)>)> {
        text.chars()
            .map(|ch| (ch, self.resolve_char(cache, ch)))
            .collect()
    }
    
    /// Query which fonts should be used for a text string, grouped by font
    /// Returns runs of consecutive characters that use the same font
    /// This is the main API for text shaping - call this to get font runs, then shape each run
    pub fn query_for_text(&self, cache: &FcFontCache, text: &str) -> Vec<ResolvedFontRun> {
        if text.is_empty() {
            return Vec::new();
        }
        
        let mut runs: Vec<ResolvedFontRun> = Vec::new();
        let mut current_font: Option<FontId> = None;
        let mut current_css_source: Option<String> = None;
        let mut current_start_byte: usize = 0;
        
        for (byte_idx, ch) in text.char_indices() {
            let resolved = self.resolve_char(cache, ch);
            let (font_id, css_source) = match &resolved {
                Some((id, source)) => (Some(*id), Some(source.clone())),
                None => (None, None),
            };
            
            // Check if we need to start a new run
            let font_changed = font_id != current_font;
            
            if font_changed && byte_idx > 0 {
                // Finalize the current run
                let run_text = &text[current_start_byte..byte_idx];
                runs.push(ResolvedFontRun {
                    text: run_text.to_string(),
                    start_byte: current_start_byte,
                    end_byte: byte_idx,
                    font_id: current_font,
                    css_source: current_css_source.clone().unwrap_or_default(),
                });
                current_start_byte = byte_idx;
            }
            
            current_font = font_id;
            current_css_source = css_source;
        }
        
        // Finalize the last run
        if current_start_byte < text.len() {
            let run_text = &text[current_start_byte..];
            runs.push(ResolvedFontRun {
                text: run_text.to_string(),
                start_byte: current_start_byte,
                end_byte: text.len(),
                font_id: current_font,
                css_source: current_css_source.unwrap_or_default(),
            });
        }
        
        runs
    }
}

/// A group of fonts that are fallbacks for a single CSS font-family name
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssFallbackGroup {
    /// The CSS font name (e.g., "NotoSansJP", "sans-serif")
    pub css_name: String,
    
    /// System fonts that match this CSS name
    /// First font in list is the best match
    pub fonts: Vec<FontMatch>,
}

/// Cache key for font fallback chain queries
/// 
/// IMPORTANT: This key intentionally does NOT include unicode_ranges.
/// Font chains should be cached by CSS properties only, not by text content.
/// Different texts with the same CSS font-stack should share the same chain.
#[cfg(feature = "std")]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FontChainCacheKey {
    /// CSS font stack (expanded to OS-specific fonts)
    font_families: Vec<String>,
    /// Font weight
    weight: FcWeight,
    /// Font style flags
    italic: PatternMatch,
    oblique: PatternMatch,
}

/// Path to a font file
#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq)]
#[repr(C)]
pub struct FcFontPath {
    pub path: String,
    pub font_index: usize,
}

/// In-memory font data
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct FcFont {
    pub bytes: Vec<u8>,
    pub font_index: usize,
    pub id: String, // For identification in tests
}

/// Font source enum to represent either disk or memory fonts
#[derive(Debug, Clone)]
pub enum FontSource<'a> {
    /// Font loaded from memory
    Memory(&'a FcFont),
    /// Font loaded from disk
    Disk(&'a FcFontPath),
}

/// Font cache, initialized at startup
#[derive(Debug)]
pub struct FcFontCache {
    // Pattern to FontId mapping (query index)
    patterns: BTreeMap<FcPattern, FontId>,
    // On-disk font paths
    disk_fonts: BTreeMap<FontId, FcFontPath>,
    // In-memory fonts
    memory_fonts: BTreeMap<FontId, FcFont>,
    // Metadata cache (patterns stored by ID for quick lookup)
    metadata: BTreeMap<FontId, FcPattern>,
    // Token index: maps lowercase tokens ("noto", "sans", "jp") to sets of FontIds
    // This enables fast fuzzy search by intersecting token sets
    token_index: BTreeMap<String, alloc::collections::BTreeSet<FontId>>,
    // Pre-tokenized font names (lowercase): FontId -> Vec<lowercase tokens>
    // Avoids re-tokenization during fuzzy search
    font_tokens: BTreeMap<FontId, Vec<String>>,
    // Font fallback chain cache (CSS stack + unicode -> resolved chain)
    #[cfg(feature = "std")]
    chain_cache: std::sync::Mutex<std::collections::HashMap<FontChainCacheKey, FontFallbackChain>>,
}

impl Clone for FcFontCache {
    fn clone(&self) -> Self {
        Self {
            patterns: self.patterns.clone(),
            disk_fonts: self.disk_fonts.clone(),
            memory_fonts: self.memory_fonts.clone(),
            metadata: self.metadata.clone(),
            token_index: self.token_index.clone(),
            font_tokens: self.font_tokens.clone(),
            #[cfg(feature = "std")]
            chain_cache: std::sync::Mutex::new(std::collections::HashMap::new()), // Empty cache for cloned instance
        }
    }
}

impl Default for FcFontCache {
    fn default() -> Self {
        Self {
            patterns: BTreeMap::new(),
            disk_fonts: BTreeMap::new(),
            memory_fonts: BTreeMap::new(),
            metadata: BTreeMap::new(),
            token_index: BTreeMap::new(),
            font_tokens: BTreeMap::new(),
            #[cfg(feature = "std")]
            chain_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

impl FcFontCache {
    /// Helper method to add a font pattern to the token index
    fn index_pattern_tokens(&mut self, pattern: &FcPattern, id: FontId) {
        // Extract tokens from both name and family
        let mut all_tokens = Vec::new();
        
        if let Some(name) = &pattern.name {
            all_tokens.extend(Self::extract_font_name_tokens(name));
        }
        
        if let Some(family) = &pattern.family {
            all_tokens.extend(Self::extract_font_name_tokens(family));
        }
        
        // Convert tokens to lowercase and store them
        let tokens_lower: Vec<String> = all_tokens.iter().map(|t| t.to_lowercase()).collect();
        
        // Add each token (lowercase) to the index
        for token_lower in &tokens_lower {
            self.token_index
                .entry(token_lower.clone())
                .or_insert_with(alloc::collections::BTreeSet::new)
                .insert(id);
        }
        
        // Store pre-tokenized font name for fast lookup (no re-tokenization needed)
        self.font_tokens.insert(id, tokens_lower);
    }

    /// Adds in-memory font files
    pub fn with_memory_fonts(&mut self, fonts: Vec<(FcPattern, FcFont)>) -> &mut Self {
        for (pattern, font) in fonts {
            let id = FontId::new();
            self.patterns.insert(pattern.clone(), id);
            self.metadata.insert(id, pattern.clone());
            self.memory_fonts.insert(id, font);
            self.index_pattern_tokens(&pattern, id);
        }
        self
    }

    /// Adds a memory font with a specific ID (for testing)
    pub fn with_memory_font_with_id(
        &mut self,
        id: FontId,
        pattern: FcPattern,
        font: FcFont,
    ) -> &mut Self {
        self.patterns.insert(pattern.clone(), id);
        self.metadata.insert(id, pattern.clone());
        self.memory_fonts.insert(id, font);
        self.index_pattern_tokens(&pattern, id);
        self
    }

    /// Get font data for a given font ID
    pub fn get_font_by_id<'a>(&'a self, id: &FontId) -> Option<FontSource<'a>> {
        // Check memory fonts first
        if let Some(font) = self.memory_fonts.get(id) {
            return Some(FontSource::Memory(font));
        }
        // Then check disk fonts
        if let Some(path) = self.disk_fonts.get(id) {
            return Some(FontSource::Disk(path));
        }
        None
    }

    /// Get metadata directly from an ID
    pub fn get_metadata_by_id(&self, id: &FontId) -> Option<&FcPattern> {
        self.metadata.get(id)
    }

    /// Get font bytes (either from disk or memory)
    #[cfg(feature = "std")]
    pub fn get_font_bytes(&self, id: &FontId) -> Option<Vec<u8>> {
        match self.get_font_by_id(id)? {
            FontSource::Memory(font) => {
                Some(font.bytes.clone())
            }
            FontSource::Disk(path) => {
                std::fs::read(&path.path).ok()
            }
        }
    }

    /// Builds a new font cache
    #[cfg(not(all(feature = "std", feature = "parsing")))]
    pub fn build() -> Self {
        Self::default()
    }

    /// Builds a new font cache from all fonts discovered on the system
    #[cfg(all(feature = "std", feature = "parsing"))]
    pub fn build() -> Self {
        let mut cache = FcFontCache::default();

        #[cfg(target_os = "linux")]
        {
            if let Some(font_entries) = FcScanDirectories() {
                for (pattern, path) in font_entries {
                    let id = FontId::new();
                    cache.patterns.insert(pattern.clone(), id);
                    cache.metadata.insert(id, pattern.clone());
                    cache.disk_fonts.insert(id, path);
                    cache.index_pattern_tokens(&pattern, id);
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            // Get the Windows system root directory from environment variable
            // Falls back to C:\Windows if not found
            let system_root = std::env::var("SystemRoot")
                .or_else(|_| std::env::var("WINDIR"))
                .unwrap_or_else(|_| "C:\\Windows".to_string());
            
            // Get user profile directory for user-installed fonts
            let user_profile = std::env::var("USERPROFILE")
                .unwrap_or_else(|_| "C:\\Users\\Default".to_string());
            
            let font_dirs = vec![
                (None, format!("{}\\Fonts\\", system_root)),
                (
                    None,
                    format!("{}\\AppData\\Local\\Microsoft\\Windows\\Fonts\\", user_profile),
                ),
            ];

            let font_entries = FcScanDirectoriesInner(&font_dirs);
            for (pattern, path) in font_entries {
                let id = FontId::new();
                cache.patterns.insert(pattern.clone(), id);
                cache.metadata.insert(id, pattern.clone());
                cache.disk_fonts.insert(id, path);
                cache.index_pattern_tokens(&pattern, id);
            }
        }

        #[cfg(target_os = "macos")]
        {
            let font_dirs = vec![
                (None, "~/Library/Fonts".to_owned()),
                (None, "/System/Library/Fonts".to_owned()),
                (None, "/Library/Fonts".to_owned()),
                // Scan AssetsV2 for dynamic system fonts (PingFang, SF Pro, etc.)
                (None, "/System/Library/AssetsV2".to_owned()),
            ];

            let font_entries = FcScanDirectoriesInner(&font_dirs);
            for (pattern, path) in font_entries {
                let id = FontId::new();
                cache.patterns.insert(pattern.clone(), id);
                cache.metadata.insert(id, pattern.clone());
                cache.disk_fonts.insert(id, path);
                cache.index_pattern_tokens(&pattern, id);
            }
        }

        cache
    }

    /// Returns the list of fonts and font patterns
    pub fn list(&self) -> Vec<(&FcPattern, FontId)> {
        self.patterns
            .iter()
            .map(|(pattern, id)| (pattern, *id))
            .collect()
    }

    /// Queries a font from the in-memory cache, returns the first found font (early return)
    pub fn query(&self, pattern: &FcPattern, trace: &mut Vec<TraceMsg>) -> Option<FontMatch> {
        let mut matches = Vec::new();

        for (stored_pattern, id) in &self.patterns {
            if Self::query_matches_internal(stored_pattern, pattern, trace) {
                let metadata = self.metadata.get(id).unwrap_or(stored_pattern);
                
                // Calculate Unicode compatibility score
                let unicode_compatibility = if pattern.unicode_ranges.is_empty() {
                    // No specific Unicode requirements, use general coverage
                    Self::calculate_unicode_coverage(&metadata.unicode_ranges) as i32
                } else {
                    // Calculate how well this font covers the requested Unicode ranges
                    Self::calculate_unicode_compatibility(&pattern.unicode_ranges, &metadata.unicode_ranges)
                };
                
                let style_score = Self::calculate_style_score(pattern, metadata);
                matches.push((*id, unicode_compatibility, style_score, metadata.clone()));
            }
        }

        // Sort by Unicode compatibility (highest first), THEN by style score (lowest first)
        // This ensures legibility is supreme priority
        matches.sort_by(|a, b| {
            b.1.cmp(&a.1) // Unicode compatibility (higher is better)
                .then_with(|| a.2.cmp(&b.2)) // Style score (lower is better)
        });

        matches.first().map(|(id, _, _, metadata)| {
            FontMatch {
                id: *id,
                unicode_ranges: metadata.unicode_ranges.clone(),
                fallbacks: Vec::new(), // Fallbacks computed lazily via compute_fallbacks()
            }
        })
    }

    /// Queries all fonts matching a pattern (internal use only)
    /// 
    /// Note: This function is now private. Use resolve_font_chain() to build a font fallback chain,
    /// then call FontFallbackChain::query_for_text() to resolve fonts for specific text.
    fn query_internal(&self, pattern: &FcPattern, trace: &mut Vec<TraceMsg>) -> Vec<FontMatch> {
        let mut matches = Vec::new();

        for (stored_pattern, id) in &self.patterns {
            if Self::query_matches_internal(stored_pattern, pattern, trace) {
                let metadata = self.metadata.get(id).unwrap_or(stored_pattern);
                
                // Calculate Unicode compatibility score
                let unicode_compatibility = if pattern.unicode_ranges.is_empty() {
                    Self::calculate_unicode_coverage(&metadata.unicode_ranges) as i32
                } else {
                    Self::calculate_unicode_compatibility(&pattern.unicode_ranges, &metadata.unicode_ranges)
                };
                
                let style_score = Self::calculate_style_score(pattern, metadata);
                matches.push((*id, unicode_compatibility, style_score, metadata.clone()));
            }
        }

        // Sort by style score (lowest first), THEN by Unicode compatibility (highest first)
        // Style matching (weight, italic, etc.) is now the primary criterion
        matches.sort_by(|a, b| {
            a.2.cmp(&b.2) // Style score (lower is better)
                .then_with(|| b.1.cmp(&a.1)) // Unicode compatibility (higher is better)
        });

        matches
            .into_iter()
            .map(|(id, _, _, metadata)| {
                FontMatch {
                    id,
                    unicode_ranges: metadata.unicode_ranges.clone(),
                    fallbacks: Vec::new(), // Fallbacks computed lazily via compute_fallbacks()
                }
            })
            .collect()
    }

    /// Compute fallback fonts for a given font
    /// This is a lazy operation that can be expensive - only call when actually needed
    /// (e.g., for FFI or debugging, not needed for resolve_char)
    pub fn compute_fallbacks(
        &self,
        font_id: &FontId,
        trace: &mut Vec<TraceMsg>,
    ) -> Vec<FontMatchNoFallback> {
        // Get the pattern for this font
        let pattern = match self.metadata.get(font_id) {
            Some(p) => p,
            None => return Vec::new(),
        };
        
        self.compute_fallbacks_for_pattern(pattern, Some(font_id), trace)
    }
    
    fn compute_fallbacks_for_pattern(
        &self,
        pattern: &FcPattern,
        exclude_id: Option<&FontId>,
        _trace: &mut Vec<TraceMsg>,
    ) -> Vec<FontMatchNoFallback> {
        let mut candidates = Vec::new();

        // Collect all potential fallbacks (excluding original pattern)
        for (stored_pattern, id) in &self.patterns {
            // Skip if this is the original font
            if exclude_id.is_some() && exclude_id.unwrap() == id {
                continue;
            }

            // Check if this font supports any of the unicode ranges
            if !stored_pattern.unicode_ranges.is_empty() && !pattern.unicode_ranges.is_empty() {
                // Calculate Unicode compatibility
                let unicode_compatibility = Self::calculate_unicode_compatibility(
                    &pattern.unicode_ranges,
                    &stored_pattern.unicode_ranges
                );
                
                // Only include if there's actual overlap
                if unicode_compatibility > 0 {
                    let style_score = Self::calculate_style_score(pattern, stored_pattern);
                    candidates.push((
                        FontMatchNoFallback {
                            id: *id,
                            unicode_ranges: stored_pattern.unicode_ranges.clone(),
                        },
                        unicode_compatibility,
                        style_score,
                        stored_pattern.clone(),
                    ));
                }
            } else if pattern.unicode_ranges.is_empty() && !stored_pattern.unicode_ranges.is_empty() {
                // No specific Unicode requirements, use general coverage
                let coverage = Self::calculate_unicode_coverage(&stored_pattern.unicode_ranges) as i32;
                let style_score = Self::calculate_style_score(pattern, stored_pattern);
                candidates.push((
                    FontMatchNoFallback {
                        id: *id,
                        unicode_ranges: stored_pattern.unicode_ranges.clone(),
                    },
                    coverage,
                    style_score,
                    stored_pattern.clone(),
                ));
            }
        }

        // Sort by Unicode compatibility (highest first), THEN by style score (lowest first)
        candidates.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| a.2.cmp(&b.2))
        });

        // Deduplicate by keeping only the best match per unique unicode range
        let mut seen_ranges = Vec::new();
        let mut deduplicated = Vec::new();

        for (id, _, _, pattern) in candidates {
            let mut is_new_range = false;

            for range in &pattern.unicode_ranges {
                if !seen_ranges.iter().any(|r: &UnicodeRange| r.overlaps(range)) {
                    seen_ranges.push(*range);
                    is_new_range = true;
                }
            }

            if is_new_range {
                deduplicated.push(id);
            }
        }

        deduplicated
    }

    /// Get in-memory font data
    pub fn get_memory_font(&self, id: &FontId) -> Option<&FcFont> {
        self.memory_fonts.get(id)
    }

    /// Check if a pattern matches the query, with detailed tracing
    fn query_matches_internal(
        k: &FcPattern,
        pattern: &FcPattern,
        trace: &mut Vec<TraceMsg>,
    ) -> bool {
        // Check name - substring match
        if let Some(ref name) = pattern.name {
            let matches = k
                .name
                .as_ref()
                .map_or(false, |k_name| k_name.contains(name));

            if !matches {
                trace.push(TraceMsg {
                    level: TraceLevel::Info,
                    path: k
                        .name
                        .as_ref()
                        .map_or_else(|| "<unknown>".to_string(), Clone::clone),
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
            let matches = k
                .family
                .as_ref()
                .map_or(false, |k_family| k_family.contains(family));

            if !matches {
                trace.push(TraceMsg {
                    level: TraceLevel::Info,
                    path: k
                        .name
                        .as_ref()
                        .map_or_else(|| "<unknown>".to_string(), Clone::clone),
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
                    path: k
                        .name
                        .as_ref()
                        .map_or_else(|| "<unknown>".to_string(), |s| s.clone()),
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
                path: k
                    .name
                    .as_ref()
                    .map_or_else(|| "<unknown>".to_string(), |s| s.clone()),
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
                path: k
                    .name
                    .as_ref()
                    .map_or_else(|| "<unknown>".to_string(), |s| s.clone()),
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
                    path: k
                        .name
                        .as_ref()
                        .map_or_else(|| "<unknown>".to_string(), |s| s.clone()),
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
    
    /// Resolve a complete font fallback chain for a CSS font-family stack
    /// This is the main entry point for font resolution with caching
    /// Automatically expands generic CSS families (serif, sans-serif, monospace) to OS-specific fonts
    /// 
    /// # Arguments
    /// * `font_families` - CSS font-family stack (e.g., ["Arial", "sans-serif"])
    /// * `text` - The text to render (used to extract Unicode ranges)
    /// * `weight` - Font weight
    /// * `italic` - Italic style requirement
    /// * `oblique` - Oblique style requirement
    /// * `trace` - Debug trace messages
    /// 
    /// # Returns
    /// A complete font fallback chain with CSS fallbacks and Unicode fallbacks
    /// 
    /// # Example
    /// ```no_run
    /// # use rust_fontconfig::{FcFontCache, FcWeight, PatternMatch};
    /// let cache = FcFontCache::build();
    /// let families = vec!["Arial".to_string(), "sans-serif".to_string()];
    /// let chain = cache.resolve_font_chain(&families, FcWeight::Normal, 
    ///                                       PatternMatch::DontCare, PatternMatch::DontCare, 
    ///                                       &mut Vec::new());
    /// // On macOS: families expanded to ["Arial", "San Francisco", "Helvetica Neue", "Lucida Grande"]
    /// ```
    #[cfg(feature = "std")]
    pub fn resolve_font_chain(
        &self,
        font_families: &[String],
        weight: FcWeight,
        italic: PatternMatch,
        oblique: PatternMatch,
        trace: &mut Vec<TraceMsg>,
    ) -> FontFallbackChain {
        self.resolve_font_chain_with_os(font_families, weight, italic, oblique, trace, OperatingSystem::current())
    }
    
    /// Resolve font chain with explicit OS specification (useful for testing)
    #[cfg(feature = "std")]
    pub fn resolve_font_chain_with_os(
        &self,
        font_families: &[String],
        weight: FcWeight,
        italic: PatternMatch,
        oblique: PatternMatch,
        trace: &mut Vec<TraceMsg>,
        os: OperatingSystem,
    ) -> FontFallbackChain {
        // Check cache FIRST - key uses original (unexpanded) families
        // This ensures all text nodes with same CSS properties share one chain
        let cache_key = FontChainCacheKey {
            font_families: font_families.to_vec(),  // Use ORIGINAL families, not expanded
            weight,
            italic,
            oblique,
        };
        
        if let Ok(cache) = self.chain_cache.lock() {
            if let Some(cached) = cache.get(&cache_key) {
                return cached.clone();
            }
        }
        
        // Expand generic CSS families to OS-specific fonts (no unicode ranges needed anymore)
        let expanded_families = expand_font_families(font_families, os, &[]);
        
        // Build the chain
        let chain = self.resolve_font_chain_uncached(
            &expanded_families,
            weight,
            italic,
            oblique,
            trace,
        );
        
        // Cache the result
        if let Ok(mut cache) = self.chain_cache.lock() {
            cache.insert(cache_key, chain.clone());
        }
        
        chain
    }
    
    /// Internal implementation without caching
    /// 
    /// Note: This function no longer takes text/unicode_ranges as input.
    /// Instead, the returned FontFallbackChain has a query_for_text() method
    /// that can be called to resolve which fonts to use for specific text.
    #[cfg(feature = "std")]
    fn resolve_font_chain_uncached(
        &self,
        font_families: &[String],
        weight: FcWeight,
        italic: PatternMatch,
        oblique: PatternMatch,
        trace: &mut Vec<TraceMsg>,
    ) -> FontFallbackChain {
        let mut css_fallbacks = Vec::new();
        
        // Resolve each CSS font-family to its system fallbacks
        for (_i, family) in font_families.iter().enumerate() {
            // Check if this is a generic font family
            let (pattern, is_generic) = if Self::is_generic_family(family) {
                // For generic families, don't filter by name, use font properties instead
                let pattern = match family.as_str() {
                    "sans-serif" => FcPattern {
                        name: None,
                        weight,
                        italic,
                        oblique,
                        monospace: PatternMatch::False,
                        unicode_ranges: Vec::new(),
                        ..Default::default()
                    },
                    "serif" => FcPattern {
                        name: None,
                        weight,
                        italic,
                        oblique,
                        monospace: PatternMatch::False,
                        unicode_ranges: Vec::new(),
                        ..Default::default()
                    },
                    "monospace" => FcPattern {
                        name: None,
                        weight,
                        italic,
                        oblique,
                        monospace: PatternMatch::True,
                        unicode_ranges: Vec::new(),
                        ..Default::default()
                    },
                    _ => FcPattern {
                        name: None,
                        weight,
                        italic,
                        oblique,
                        unicode_ranges: Vec::new(),
                        ..Default::default()
                    },
                };
                (pattern, true)
            } else {
                // Specific font family name
                let pattern = FcPattern {
                    name: Some(family.clone()),
                    weight,
                    italic,
                    oblique,
                    unicode_ranges: Vec::new(),
                    ..Default::default()
                };
                (pattern, false)
            };
            
            // Use fuzzy matching for specific fonts (fast token-based lookup)
            // For generic families, use query (slower but necessary for property matching)
            let mut matches = if is_generic {
                // Generic families need full pattern matching
                self.query_internal(&pattern, trace)
            } else {
                // Specific font names: use fast token-based fuzzy matching
                self.fuzzy_query_by_name(family, weight, italic, oblique, &[], trace)
            };
            
            // For generic families, limit to top 5 fonts to avoid too many matches
            if is_generic && matches.len() > 5 {
                matches.truncate(5);
            }
            
            // Always add the CSS fallback group to preserve CSS ordering
            // even if no fonts were found for this family
            css_fallbacks.push(CssFallbackGroup {
                css_name: family.clone(),
                fonts: matches,
            });
        }
        
        // Unicode fallbacks are now resolved lazily in query_for_text()
        // This avoids the expensive unicode coverage check during chain building
        FontFallbackChain {
            css_fallbacks,
            unicode_fallbacks: Vec::new(), // Will be populated on-demand
            original_stack: font_families.to_vec(),
        }
    }
    
    /// Extract Unicode ranges from text
    #[allow(dead_code)]
    fn extract_unicode_ranges(text: &str) -> Vec<UnicodeRange> {
        let mut chars: Vec<char> = text.chars().collect();
        chars.sort_unstable();
        chars.dedup();
        
        if chars.is_empty() {
            return Vec::new();
        }
        
        let mut ranges = Vec::new();
        let mut range_start = chars[0] as u32;
        let mut range_end = range_start;
        
        for &c in &chars[1..] {
            let codepoint = c as u32;
            if codepoint == range_end + 1 {
                range_end = codepoint;
            } else {
                ranges.push(UnicodeRange { start: range_start, end: range_end });
                range_start = codepoint;
                range_end = codepoint;
            }
        }
        
        ranges.push(UnicodeRange { start: range_start, end: range_end });
        ranges
    }
    
    /// Check if a font family name is a generic CSS family
    #[cfg(feature = "std")]
    fn is_generic_family(family: &str) -> bool {
        matches!(
            family.to_lowercase().as_str(),
            "serif" | "sans-serif" | "monospace" | "cursive" | "fantasy" | "system-ui"
        )
    }
    
    /// Fuzzy query for fonts by name when exact match fails
    /// Uses intelligent token-based matching with inverted index for speed:
    /// 1. Break name into tokens (e.g., "NotoSansJP" -> ["noto", "sans", "jp"])
    /// 2. Use token_index to find candidate fonts via BTreeSet intersection
    /// 3. Score only the candidate fonts (instead of all 800+ patterns)
    /// 4. Prioritize fonts matching more tokens + Unicode coverage
    #[cfg(feature = "std")]
    fn fuzzy_query_by_name(
        &self,
        requested_name: &str,
        weight: FcWeight,
        italic: PatternMatch,
        oblique: PatternMatch,
        unicode_ranges: &[UnicodeRange],
        _trace: &mut Vec<TraceMsg>,
    ) -> Vec<FontMatch> {
        // Extract tokens from the requested name (e.g., "NotoSansJP" -> ["noto", "sans", "jp"])
        let tokens = Self::extract_font_name_tokens(requested_name);
        
        if tokens.is_empty() {
            return Vec::new();
        }
        
        // Convert tokens to lowercase for case-insensitive lookup
        let tokens_lower: Vec<String> = tokens.iter().map(|t| t.to_lowercase()).collect();
        
        // Progressive token matching strategy:
        // Start with first token, then progressively narrow down with each additional token
        // If adding a token results in 0 matches, use the previous (broader) set
        // Example: ["Noto"] -> 10 fonts, ["Noto","Sans"] -> 2 fonts, ["Noto","Sans","JP"] -> 0 fonts => use 2 fonts
        
        // Start with the first token
        let first_token = &tokens_lower[0];
        let mut candidate_ids = match self.token_index.get(first_token) {
            Some(ids) if !ids.is_empty() => ids.clone(),
            _ => {
                // First token not found - no fonts match, quit immediately
                return Vec::new();
            }
        };
        
        // Progressively narrow down with each additional token
        for token in &tokens_lower[1..] {
            if let Some(token_ids) = self.token_index.get(token) {
                // Calculate intersection
                let intersection: alloc::collections::BTreeSet<FontId> = 
                    candidate_ids.intersection(token_ids).copied().collect();
                
                if intersection.is_empty() {
                    // Adding this token results in 0 matches - keep previous set and stop
                    break;
                } else {
                    // Successfully narrowed down - use intersection
                    candidate_ids = intersection;
                }
            } else {
                // Token not in index - keep current set and stop
                break;
            }
        }
        
        // Now score only the candidate fonts (HUGE speedup!)
        let mut candidates = Vec::new();
        
        for id in candidate_ids {
            let pattern = match self.metadata.get(&id) {
                Some(p) => p,
                None => continue,
            };
            
            // Get pre-tokenized font name (already lowercase)
            let font_tokens_lower = match self.font_tokens.get(&id) {
                Some(tokens) => tokens,
                None => continue,
            };
            
            if font_tokens_lower.is_empty() {
                continue;
            }
            
            // Calculate token match score (how many requested tokens appear in font name)
            // Both tokens_lower and font_tokens_lower are already lowercase, so direct comparison
            let token_matches = tokens_lower.iter()
                .filter(|req_token| {
                    font_tokens_lower.iter().any(|font_token| {
                        // Both already lowercase - just check if font token contains request token
                        font_token.contains(req_token.as_str())
                    })
                })
                .count();
            
            // Skip if no tokens match (shouldn't happen due to index, but safety check)
            if token_matches == 0 {
                continue;
            }
            
            // Calculate token similarity score (0-100)
            let token_similarity = (token_matches * 100 / tokens.len()) as i32;
            
            // Calculate Unicode range similarity
            let unicode_similarity = if !unicode_ranges.is_empty() && !pattern.unicode_ranges.is_empty() {
                Self::calculate_unicode_compatibility(unicode_ranges, &pattern.unicode_ranges)
            } else {
                0
            };
            
            // CRITICAL: If we have Unicode requirements, ONLY accept fonts that cover them
            // A font with great name match but no Unicode coverage is useless
            if !unicode_ranges.is_empty() && unicode_similarity == 0 {
                continue;
            }
            
            let style_score = Self::calculate_style_score(&FcPattern {
                weight,
                italic,
                oblique,
                ..Default::default()
            }, pattern);
            
            candidates.push((
                id,
                token_similarity,
                unicode_similarity,
                style_score,
                pattern.clone(),
            ));
        }
        
        // Sort by:
        // 1. Token matches (more matches = better)
        // 2. Unicode compatibility (if ranges provided)
        // 3. Style score (lower is better)
        candidates.sort_by(|a, b| {
            if !unicode_ranges.is_empty() {
                // When we have Unicode requirements, prioritize coverage
                b.1.cmp(&a.1) // Token similarity (higher is better) - PRIMARY
                    .then_with(|| b.2.cmp(&a.2)) // Unicode similarity (higher is better) - SECONDARY
                    .then_with(|| a.3.cmp(&b.3)) // Style score (lower is better) - TERTIARY
            } else {
                // No Unicode requirements, token similarity is primary
                b.1.cmp(&a.1) // Token similarity (higher is better)
                    .then_with(|| a.3.cmp(&b.3)) // Style score (lower is better)
            }
        });
        
        // Take top 5 matches
        candidates.truncate(5);
        
        // Convert to FontMatch
        candidates
            .into_iter()
            .map(|(id, _token_sim, _unicode_sim, _style, pattern)| {
                FontMatch {
                    id,
                    unicode_ranges: pattern.unicode_ranges.clone(),
                    fallbacks: Vec::new(), // Fallbacks computed lazily via compute_fallbacks()
                }
            })
            .collect()
    }
    
    /// Extract tokens from a font name
    /// E.g., "NotoSansJP" -> ["Noto", "Sans", "JP"]
    /// E.g., "Noto Sans CJK JP" -> ["Noto", "Sans", "CJK", "JP"]
    fn extract_font_name_tokens(name: &str) -> Vec<String> {
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
    
    /// Normalize font name for comparison (remove spaces, lowercase, keep only ASCII alphanumeric)
    /// This ensures we only compare Latin-script names, ignoring localized names
    #[allow(dead_code)]
    fn normalize_font_name(name: &str) -> String {
        name.chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect()
    }
    
    /// Calculate Levenshtein distance between two strings
    #[allow(dead_code)]
    fn levenshtein_distance(s1: &str, s2: &str) -> usize {
        let len1 = s1.chars().count();
        let len2 = s2.chars().count();
        
        if len1 == 0 {
            return len2;
        }
        if len2 == 0 {
            return len1;
        }
        
        let mut prev_row: Vec<usize> = (0..=len2).collect();
        let mut curr_row = vec![0; len2 + 1];
        
        for (i, c1) in s1.chars().enumerate() {
            curr_row[0] = i + 1;
            
            for (j, c2) in s2.chars().enumerate() {
                let cost = if c1 == c2 { 0 } else { 1 };
                curr_row[j + 1] = (curr_row[j] + 1)
                    .min(prev_row[j + 1] + 1)
                    .min(prev_row[j] + cost);
            }
            
            core::mem::swap(&mut prev_row, &mut curr_row);
        }
        
        prev_row[len2]
    }
    
    /// Find fonts to cover missing Unicode ranges
    /// Uses intelligent matching: prefers fonts with similar names to existing ones
    /// Early quits once all Unicode ranges are covered for performance
    #[allow(dead_code)]
    fn find_unicode_fallbacks(
        &self,
        unicode_ranges: &[UnicodeRange],
        covered_chars: &[bool],
        existing_groups: &[CssFallbackGroup],
        weight: FcWeight,
        italic: PatternMatch,
        oblique: PatternMatch,
        trace: &mut Vec<TraceMsg>,
    ) -> Vec<FontMatch> {
        // Extract uncovered ranges
        let mut uncovered_ranges = Vec::new();
        for (i, &covered) in covered_chars.iter().enumerate() {
            if !covered && i < unicode_ranges.len() {
                uncovered_ranges.push(unicode_ranges[i].clone());
            }
        }
        
        if uncovered_ranges.is_empty() {
            return Vec::new();
        }
        
        // Query for fonts that cover these ranges
        let pattern = FcPattern {
            name: None, // Wildcard - match any font
            weight,
            italic,
            oblique,
            unicode_ranges: uncovered_ranges.clone(),
            ..Default::default()
        };
        
        let mut candidates = self.query_internal(&pattern, trace);
        
        // Intelligent sorting: prefer fonts with similar names to existing ones
        // Extract font family prefixes from existing fonts (e.g., "Noto Sans" from "Noto Sans JP")
        let existing_prefixes: Vec<String> = existing_groups
            .iter()
            .flat_map(|group| {
                group.fonts.iter().filter_map(|font| {
                    self.get_metadata_by_id(&font.id)
                        .and_then(|meta| meta.family.clone())
                        .and_then(|family| {
                            // Extract prefix (e.g., "Noto Sans" from "Noto Sans JP")
                            family.split_whitespace()
                                .take(2)
                                .collect::<Vec<_>>()
                                .join(" ")
                                .into()
                        })
                })
            })
            .collect();
        
        // Sort candidates by:
        // 1. Name similarity to existing fonts (highest priority)
        // 2. Unicode coverage (secondary)
        candidates.sort_by(|a, b| {
            let a_meta = self.get_metadata_by_id(&a.id);
            let b_meta = self.get_metadata_by_id(&b.id);
            
            let a_score = Self::calculate_font_similarity_score(a_meta, &existing_prefixes);
            let b_score = Self::calculate_font_similarity_score(b_meta, &existing_prefixes);
            
            b_score.cmp(&a_score) // Higher score = better match
                .then_with(|| {
                    let a_coverage = Self::calculate_unicode_compatibility(&uncovered_ranges, &a.unicode_ranges);
                    let b_coverage = Self::calculate_unicode_compatibility(&uncovered_ranges, &b.unicode_ranges);
                    b_coverage.cmp(&a_coverage)
                })
        });
        
        // Early quit optimization: only take fonts until all ranges are covered
        let mut result = Vec::new();
        let mut remaining_uncovered: Vec<bool> = vec![true; uncovered_ranges.len()];
        
        for candidate in candidates {
            // Check which ranges this font covers
            let mut covers_new_range = false;
            
            for (i, range) in uncovered_ranges.iter().enumerate() {
                if remaining_uncovered[i] {
                    // Check if this font covers this range
                    for font_range in &candidate.unicode_ranges {
                        if font_range.overlaps(range) {
                            remaining_uncovered[i] = false;
                            covers_new_range = true;
                            break;
                        }
                    }
                }
            }
            
            // Only add fonts that cover at least one new range
            if covers_new_range {
                result.push(candidate);
                
                // Early quit: if all ranges are covered, stop
                if remaining_uncovered.iter().all(|&uncovered| !uncovered) {
                    break;
                }
            }
        }
        
        result
    }
    
    /// Calculate similarity score between a font and existing font prefixes
    /// Higher score = more similar
    #[allow(dead_code)]
    fn calculate_font_similarity_score(
        font_meta: Option<&FcPattern>,
        existing_prefixes: &[String],
    ) -> i32 {
        let Some(meta) = font_meta else { return 0; };
        let Some(family) = &meta.family else { return 0; };
        
        // Check if this font's family matches any existing prefix
        for prefix in existing_prefixes {
            if family.starts_with(prefix) {
                return 100; // Strong match
            }
            if family.contains(prefix) {
                return 50; // Partial match
            }
        }
        
        0 // No match
    }
    
    /// Find fallback fonts for a given pattern
    // Helper to calculate total unicode coverage
    fn calculate_unicode_coverage(ranges: &[UnicodeRange]) -> u64 {
        ranges
            .iter()
            .map(|range| (range.end - range.start + 1) as u64)
            .sum()
    }

    /// Calculate how well a font's Unicode ranges cover the requested ranges
    /// Returns a compatibility score (higher is better, 0 means no overlap)
    fn calculate_unicode_compatibility(
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

    fn calculate_style_score(original: &FcPattern, candidate: &FcPattern) -> i32 {

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
                if !orig.matches(&cand) {
                    if cand == PatternMatch::DontCare {
                        score += dontcare_penalty;
                    } else {
                        score += mismatch_penalty;
                    }
                } else if orig == PatternMatch::True && cand == PatternMatch::True {
                    // Give bonus for exact True match to solve the test case
                    score -= 20;
                }
            }
        }

        score
    }
}

#[cfg(all(feature = "std", feature = "parsing", target_os = "linux"))]
fn FcScanDirectories() -> Option<Vec<(FcPattern, FcFontPath)>> {
    use std::fs;
    use std::path::Path;

    const BASE_FONTCONFIG_PATH: &str = "/etc/fonts/fonts.conf";

    if !Path::new(BASE_FONTCONFIG_PATH).exists() {
        return None;
    }

    let mut font_paths = Vec::with_capacity(32);
    let mut paths_to_visit = vec![(None, PathBuf::from(BASE_FONTCONFIG_PATH))];

    while let Some((prefix, path_to_visit)) = paths_to_visit.pop() {
        let path = match process_path(&prefix, path_to_visit, true) {
            Some(path) => path,
            None => continue,
        };

        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };

        if metadata.is_file() {
            let xml_utf8 = match fs::read_to_string(&path) {
                Ok(xml_utf8) => xml_utf8,
                Err(_) => continue,
            };

            if ParseFontsConf(&xml_utf8, &mut paths_to_visit, &mut font_paths).is_none() {
                continue;
            }
        } else if metadata.is_dir() {
            let dir_entries = match fs::read_dir(&path) {
                Ok(dir_entries) => dir_entries,
                Err(_) => continue,
            };

            for entry_result in dir_entries {
                let entry = match entry_result {
                    Ok(entry) => entry,
                    Err(_) => continue,
                };

                let entry_path = entry.path();

                // `fs::metadata` traverses symbolic links
                let entry_metadata = match fs::metadata(&entry_path) {
                    Ok(metadata) => metadata,
                    Err(_) => continue,
                };

                if !entry_metadata.is_file() {
                    continue;
                }

                let file_name = match entry_path.file_name() {
                    Some(name) => name,
                    None => continue,
                };

                let file_name_str = file_name.to_string_lossy();
                if file_name_str.starts_with(|c: char| c.is_ascii_digit())
                    && file_name_str.ends_with(".conf")
                {
                    paths_to_visit.push((None, entry_path));
                }
            }
        }
    }

    if font_paths.is_empty() {
        return None;
    }

    Some(FcScanDirectoriesInner(&font_paths))
}

// Parses the fonts.conf file
#[cfg(all(feature = "std", feature = "parsing", target_os = "linux"))]
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

// Remaining implementation for font scanning, parsing, etc.
#[cfg(all(feature = "std", feature = "parsing"))]
fn FcParseFont(filepath: &PathBuf) -> Option<Vec<(FcPattern, FcFontPath)>> {
    use allsorts::{
        binary::read::ReadScope,
        font_data::FontData,
        get_name::fontcode_get_name,
        post::PostTable,
        tables::{
            os2::Os2, FontTableProvider, HeadTable, HheaTable, HmtxTable, MaxpTable, NameTable,
        },
        tag,
    };
    #[cfg(all(not(target_family = "wasm"), feature = "std"))]
    use mmapio::MmapOptions;
    use std::collections::BTreeSet;
    use std::fs::File;

    const FONT_SPECIFIER_NAME_ID: u16 = 4;
    const FONT_SPECIFIER_FAMILY_ID: u16 = 1;

    // Try parsing the font file and see if the postscript name matches
    let file = File::open(filepath).ok()?;

    #[cfg(all(not(target_family = "wasm"), feature = "std"))]
    let font_bytes = unsafe { MmapOptions::new().map(&file).ok()? };

    #[cfg(not(all(not(target_family = "wasm"), feature = "std")))]
    let font_bytes = std::fs::read(filepath).ok()?;

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

    let scope = ReadScope::new(&font_bytes[..]);
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

        // Get font properties from OS/2 table
        let os2_data = provider.table_data(tag::OS_2).ok()??;
        let os2_table = ReadScope::new(&os2_data)
            .read_dep::<Os2>(os2_data.len())
            .ok()?;

        // Extract additional style information
        let is_oblique = os2_table
            .fs_selection
            .contains(allsorts::tables::os2::FsSelection::OBLIQUE);
        let weight = FcWeight::from_u16(os2_table.us_weight_class);
        let stretch = FcStretch::from_u16(os2_table.us_width_class);

        // Extract unicode ranges from OS/2 table (fast, but may be inaccurate)
        // These are hints about what the font *should* support
        // For actual glyph coverage verification, query the font file directly
        let mut unicode_ranges = Vec::new();

        // Process the 4 Unicode range bitfields from OS/2 table
        let ranges = [
            os2_table.ul_unicode_range1,
            os2_table.ul_unicode_range2,
            os2_table.ul_unicode_range3,
            os2_table.ul_unicode_range4,
        ];

        // Unicode range bit positions to actual ranges
        // Based on OpenType spec: https://learn.microsoft.com/en-us/typography/opentype/spec/os2#ur
        let range_mappings = [
            // ulUnicodeRange1 (bits 0-31)
            (0, 0x0000, 0x007F), // Basic Latin
            (1, 0x0080, 0x00FF), // Latin-1 Supplement
            (2, 0x0100, 0x017F), // Latin Extended-A
            (3, 0x0180, 0x024F), // Latin Extended-B
            (4, 0x0250, 0x02AF), // IPA Extensions
            (5, 0x02B0, 0x02FF), // Spacing Modifier Letters
            (6, 0x0300, 0x036F), // Combining Diacritical Marks
            (7, 0x0370, 0x03FF), // Greek and Coptic
            (8, 0x2C80, 0x2CFF), // Coptic
            (9, 0x0400, 0x04FF), // Cyrillic
            (10, 0x0530, 0x058F), // Armenian
            (11, 0x0590, 0x05FF), // Hebrew
            (12, 0x0600, 0x06FF), // Arabic
            (13, 0x0700, 0x074F), // Syriac
            (14, 0x0780, 0x07BF), // Thaana
            (15, 0x0900, 0x097F), // Devanagari
            (16, 0x0980, 0x09FF), // Bengali
            (17, 0x0A00, 0x0A7F), // Gurmukhi
            (18, 0x0A80, 0x0AFF), // Gujarati
            (19, 0x0B00, 0x0B7F), // Oriya
            (20, 0x0B80, 0x0BFF), // Tamil
            (21, 0x0C00, 0x0C7F), // Telugu
            (22, 0x0C80, 0x0CFF), // Kannada
            (23, 0x0D00, 0x0D7F), // Malayalam
            (24, 0x0E00, 0x0E7F), // Thai
            (25, 0x0E80, 0x0EFF), // Lao
            (26, 0x10A0, 0x10FF), // Georgian
            (27, 0x1B00, 0x1B7F), // Balinese
            (28, 0x1100, 0x11FF), // Hangul Jamo
            (29, 0x1E00, 0x1EFF), // Latin Extended Additional
            (30, 0x1F00, 0x1FFF), // Greek Extended
            (31, 0x2000, 0x206F), // General Punctuation
            
            // ulUnicodeRange2 (bits 32-63)
            (32, 0x2070, 0x209F), // Superscripts And Subscripts
            (33, 0x20A0, 0x20CF), // Currency Symbols
            (34, 0x20D0, 0x20FF), // Combining Diacritical Marks For Symbols
            (35, 0x2100, 0x214F), // Letterlike Symbols
            (36, 0x2150, 0x218F), // Number Forms
            (37, 0x2190, 0x21FF), // Arrows
            (38, 0x2200, 0x22FF), // Mathematical Operators
            (39, 0x2300, 0x23FF), // Miscellaneous Technical
            (40, 0x2400, 0x243F), // Control Pictures
            (41, 0x2440, 0x245F), // Optical Character Recognition
            (42, 0x2460, 0x24FF), // Enclosed Alphanumerics
            (43, 0x2500, 0x257F), // Box Drawing
            (44, 0x2580, 0x259F), // Block Elements
            (45, 0x25A0, 0x25FF), // Geometric Shapes
            (46, 0x2600, 0x26FF), // Miscellaneous Symbols
            (47, 0x2700, 0x27BF), // Dingbats
            (48, 0x3000, 0x303F), // CJK Symbols And Punctuation
            (49, 0x3040, 0x309F), // Hiragana
            (50, 0x30A0, 0x30FF), // Katakana
            (51, 0x3100, 0x312F), // Bopomofo
            (52, 0x3130, 0x318F), // Hangul Compatibility Jamo
            (53, 0x3190, 0x319F), // Kanbun
            (54, 0x31A0, 0x31BF), // Bopomofo Extended
            (55, 0x31C0, 0x31EF), // CJK Strokes
            (56, 0x31F0, 0x31FF), // Katakana Phonetic Extensions
            (57, 0x3200, 0x32FF), // Enclosed CJK Letters And Months
            (58, 0x3300, 0x33FF), // CJK Compatibility
            (59, 0x4E00, 0x9FFF), // CJK Unified Ideographs
            (60, 0xA000, 0xA48F), // Yi Syllables
            (61, 0xA490, 0xA4CF), // Yi Radicals
            (62, 0xAC00, 0xD7AF), // Hangul Syllables
            (63, 0xD800, 0xDFFF), // Non-Plane 0 (note: surrogates, not directly usable)
            
            // ulUnicodeRange3 (bits 64-95)
            (64, 0x10000, 0x10FFFF), // Phoenician and other non-BMP (bit 64 indicates non-BMP support)
            (65, 0xF900, 0xFAFF), // CJK Compatibility Ideographs
            (66, 0xFB00, 0xFB4F), // Alphabetic Presentation Forms
            (67, 0xFB50, 0xFDFF), // Arabic Presentation Forms-A
            (68, 0xFE00, 0xFE0F), // Variation Selectors
            (69, 0xFE10, 0xFE1F), // Vertical Forms
            (70, 0xFE20, 0xFE2F), // Combining Half Marks
            (71, 0xFE30, 0xFE4F), // CJK Compatibility Forms
            (72, 0xFE50, 0xFE6F), // Small Form Variants
            (73, 0xFE70, 0xFEFF), // Arabic Presentation Forms-B
            (74, 0xFF00, 0xFFEF), // Halfwidth And Fullwidth Forms
            (75, 0xFFF0, 0xFFFF), // Specials
            (76, 0x0F00, 0x0FFF), // Tibetan
            (77, 0x0700, 0x074F), // Syriac
            (78, 0x0780, 0x07BF), // Thaana
            (79, 0x0D80, 0x0DFF), // Sinhala
            (80, 0x1000, 0x109F), // Myanmar
            (81, 0x1200, 0x137F), // Ethiopic
            (82, 0x13A0, 0x13FF), // Cherokee
            (83, 0x1400, 0x167F), // Unified Canadian Aboriginal Syllabics
            (84, 0x1680, 0x169F), // Ogham
            (85, 0x16A0, 0x16FF), // Runic
            (86, 0x1780, 0x17FF), // Khmer
            (87, 0x1800, 0x18AF), // Mongolian
            (88, 0x2800, 0x28FF), // Braille Patterns
            (89, 0xA000, 0xA48F), // Yi Syllables
            (90, 0x1680, 0x169F), // Ogham
            (91, 0x16A0, 0x16FF), // Runic
            (92, 0x1700, 0x171F), // Tagalog
            (93, 0x1720, 0x173F), // Hanunoo
            (94, 0x1740, 0x175F), // Buhid
            (95, 0x1760, 0x177F), // Tagbanwa
            
            // ulUnicodeRange4 (bits 96-127)
            (96, 0x1900, 0x194F), // Limbu
            (97, 0x1950, 0x197F), // Tai Le
            (98, 0x1980, 0x19DF), // New Tai Lue
            (99, 0x1A00, 0x1A1F), // Buginese
            (100, 0x2C00, 0x2C5F), // Glagolitic
            (101, 0x2D30, 0x2D7F), // Tifinagh
            (102, 0x4DC0, 0x4DFF), // Yijing Hexagram Symbols
            (103, 0xA800, 0xA82F), // Syloti Nagri
            (104, 0x10000, 0x1007F), // Linear B Syllabary
            (105, 0x10080, 0x100FF), // Linear B Ideograms
            (106, 0x10100, 0x1013F), // Aegean Numbers
            (107, 0x10140, 0x1018F), // Ancient Greek Numbers
            (108, 0x10300, 0x1032F), // Old Italic
            (109, 0x10330, 0x1034F), // Gothic
            (110, 0x10380, 0x1039F), // Ugaritic
            (111, 0x103A0, 0x103DF), // Old Persian
            (112, 0x10400, 0x1044F), // Deseret
            (113, 0x10450, 0x1047F), // Shavian
            (114, 0x10480, 0x104AF), // Osmanya
            (115, 0x10800, 0x1083F), // Cypriot Syllabary
            (116, 0x10A00, 0x10A5F), // Kharoshthi
            (117, 0x1D000, 0x1D0FF), // Byzantine Musical Symbols
            (118, 0x1D100, 0x1D1FF), // Musical Symbols
            (119, 0x1D200, 0x1D24F), // Ancient Greek Musical Notation
            (120, 0x1D300, 0x1D35F), // Tai Xuan Jing Symbols
            (121, 0x1D400, 0x1D7FF), // Mathematical Alphanumeric Symbols
            (122, 0x1F000, 0x1F02F), // Mahjong Tiles
            (123, 0x1F030, 0x1F09F), // Domino Tiles
            (124, 0x1F300, 0x1F9FF), // Miscellaneous Symbols And Pictographs (Emoji)
            (125, 0x1F680, 0x1F6FF), // Transport And Map Symbols
            (126, 0x1F700, 0x1F77F), // Alchemical Symbols
            (127, 0x1F900, 0x1F9FF), // Supplemental Symbols and Pictographs
        ];

        for (range_idx, bit_pos, start, end) in range_mappings.iter().map(|&(bit, start, end)| {
            let range_idx = bit / 32;
            let bit_pos = bit % 32;
            (range_idx, bit_pos, start, end)
        }) {
            if range_idx < 4 && (ranges[range_idx] & (1 << bit_pos)) != 0 {
                unicode_ranges.push(UnicodeRange { start, end });
            }
        }
        
        // Verify OS/2 reported ranges against actual CMAP support
        // OS/2 ulUnicodeRange bits can be unreliable - fonts may claim support
        // for ranges they don't actually have glyphs for
        unicode_ranges = verify_unicode_ranges_with_cmap(&provider, unicode_ranges);
        
        // If still empty (OS/2 had no ranges or all were invalid), do full CMAP analysis
        if unicode_ranges.is_empty() {
            if let Some(cmap_ranges) = analyze_cmap_coverage(&provider) {
                unicode_ranges = cmap_ranges;
            }
        }

        // If no monospace detection yet, check using hmtx
        if detected_monospace.is_none() {
            // Try using PANOSE classification
            if os2_table.panose[0] == 2 {
                // 2 = Latin Text
                detected_monospace = Some(os2_table.panose[3] == 9); // 9 = Monospaced
            } else {
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
                for i in 0..hhea_table.num_h_metrics as usize {
                    let advance = hmtx_table.h_metrics.read_item(i).ok()?.advance_width;
                    if i > 0 && advance != last_advance {
                        monospace = false;
                        break;
                    }
                    last_advance = advance;
                }

                detected_monospace = Some(monospace);
            }
        }

        let is_monospace = detected_monospace.unwrap_or(false);

        let name_data = provider.table_data(tag::NAME).ok()??.into_owned();
        let name_table = ReadScope::new(&name_data).read::<NameTable>().ok()?;

        // One font can support multiple patterns
        let mut f_family = None;

        let patterns = name_table
            .name_records
            .iter()
            .filter_map(|name_record| {
                let name_id = name_record.name_id;
                if name_id == FONT_SPECIFIER_FAMILY_ID {
                    let family = fontcode_get_name(&name_data, FONT_SPECIFIER_FAMILY_ID).ok()??;
                    f_family = Some(family);
                    None
                } else if name_id == FONT_SPECIFIER_NAME_ID {
                    let family = f_family.as_ref()?;
                    let name = fontcode_get_name(&name_data, FONT_SPECIFIER_NAME_ID).ok()??;
                    if name.to_bytes().is_empty() {
                        None
                    } else {
                        // Initialize metadata structure
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

                        // Extract metadata from name table
                        metadata.copyright = get_name_string(&name_data, NAME_ID_COPYRIGHT);
                        metadata.font_family = get_name_string(&name_data, NAME_ID_FAMILY);
                        metadata.font_subfamily = get_name_string(&name_data, NAME_ID_SUBFAMILY);
                        metadata.full_name = get_name_string(&name_data, NAME_ID_FULL_NAME);
                        metadata.unique_id = get_name_string(&name_data, NAME_ID_UNIQUE_ID);
                        metadata.version = get_name_string(&name_data, NAME_ID_VERSION);
                        metadata.postscript_name =
                            get_name_string(&name_data, NAME_ID_POSTSCRIPT_NAME);
                        metadata.trademark = get_name_string(&name_data, NAME_ID_TRADEMARK);
                        metadata.manufacturer = get_name_string(&name_data, NAME_ID_MANUFACTURER);
                        metadata.designer = get_name_string(&name_data, NAME_ID_DESIGNER);
                        metadata.id_description = get_name_string(&name_data, NAME_ID_DESCRIPTION);
                        metadata.designer_url = get_name_string(&name_data, NAME_ID_DESIGNER_URL);
                        metadata.manufacturer_url = get_name_string(&name_data, NAME_ID_VENDOR_URL);
                        metadata.license = get_name_string(&name_data, NAME_ID_LICENSE);
                        metadata.license_url = get_name_string(&name_data, NAME_ID_LICENSE_URL);
                        metadata.preferred_family =
                            get_name_string(&name_data, NAME_ID_PREFERRED_FAMILY);
                        metadata.preferred_subfamily =
                            get_name_string(&name_data, NAME_ID_PREFERRED_SUBFAMILY);

                        let mut name = String::from_utf8_lossy(name.to_bytes()).to_string();
                        let mut family = String::from_utf8_lossy(family.as_bytes()).to_string();
                        if name.starts_with(".") {
                            name = name[1..].to_string();
                        }
                        if family.starts_with(".") {
                            family = family[1..].to_string();
                        }
                        Some((
                            FcPattern {
                                name: Some(name),
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
                                metadata,
                            },
                            font_index,
                        ))
                    }
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();

        results.extend(patterns.into_iter().map(|(pat, index)| {
            (
                pat,
                FcFontPath {
                    path: filepath.to_string_lossy().to_string(),
                    font_index: index,
                },
            )
        }));
    }

    if results.is_empty() {
        None
    } else {
        Some(results)
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
/// Used by both FcParseFont (for disk fonts) and FcParseFontBytes (for memory fonts).
#[cfg(all(feature = "std", feature = "parsing"))]
fn FcParseFontBytesInner(font_bytes: &[u8], font_id: &str) -> Option<Vec<(FcPattern, FcFont)>> {
    use allsorts::{
        binary::read::ReadScope,
        font_data::FontData,
        get_name::fontcode_get_name,
        post::PostTable,
        tables::{
            os2::Os2, FontTableProvider, HeadTable, HheaTable, HmtxTable, MaxpTable, NameTable,
        },
        tag,
    };
    use std::collections::BTreeSet;

    const FONT_SPECIFIER_NAME_ID: u16 = 4;
    const FONT_SPECIFIER_FAMILY_ID: u16 = 1;

    let max_fonts = if font_bytes.len() >= 12 && &font_bytes[0..4] == b"ttcf" {
        let num_fonts =
            u32::from_be_bytes([font_bytes[8], font_bytes[9], font_bytes[10], font_bytes[11]]);
        std::cmp::min(num_fonts as usize, 100)
    } else {
        1
    };

    let scope = ReadScope::new(font_bytes);
    let font_file = scope.read::<FontData<'_>>().ok()?;

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
            detected_monospace = Some(post_table.header.is_fixed_pitch != 0);
        }

        let os2_data = provider.table_data(tag::OS_2).ok()??;
        let os2_table = ReadScope::new(&os2_data)
            .read_dep::<Os2>(os2_data.len())
            .ok()?;

        let is_oblique = os2_table
            .fs_selection
            .contains(allsorts::tables::os2::FsSelection::OBLIQUE);
        let weight = FcWeight::from_u16(os2_table.us_weight_class);
        let stretch = FcStretch::from_u16(os2_table.us_width_class);

        let mut unicode_ranges = Vec::new();
        let ranges = [
            os2_table.ul_unicode_range1,
            os2_table.ul_unicode_range2,
            os2_table.ul_unicode_range3,
            os2_table.ul_unicode_range4,
        ];

        // Full Unicode range bit mappings (same as FcParseFont)
        let range_mappings = [
            (0, 0x0000u32, 0x007Fu32),
            (1, 0x0080, 0x00FF),
            (2, 0x0100, 0x017F),
            (3, 0x0180, 0x024F),
            (4, 0x0250, 0x02AF),
            (5, 0x02B0, 0x02FF),
            (6, 0x0300, 0x036F),
            (7, 0x0370, 0x03FF),
            (8, 0x2C80, 0x2CFF),
            (9, 0x0400, 0x04FF),
            (10, 0x0530, 0x058F),
            (11, 0x0590, 0x05FF),
            (12, 0x0600, 0x06FF),
            (31, 0x2000, 0x206F),
            (48, 0x3000, 0x303F),
            (49, 0x3040, 0x309F),
            (50, 0x30A0, 0x30FF),
            (59, 0x4E00, 0x9FFF),
            (62, 0xAC00, 0xD7AF),
        ];

        for &(bit, start, end) in &range_mappings {
            let range_idx = (bit / 32) as usize;
            let bit_pos = bit % 32;
            if range_idx < 4 && (ranges[range_idx] & (1 << bit_pos)) != 0 {
                unicode_ranges.push(UnicodeRange { start, end });
            }
        }

        unicode_ranges = verify_unicode_ranges_with_cmap(&provider, unicode_ranges);

        if unicode_ranges.is_empty() {
            if let Some(cmap_ranges) = analyze_cmap_coverage(&provider) {
                unicode_ranges = cmap_ranges;
            }
        }

        if detected_monospace.is_none() {
            if os2_table.panose[0] == 2 {
                detected_monospace = Some(os2_table.panose[3] == 9);
            } else if let (Ok(Some(hhea_data)), Ok(Some(maxp_data)), Ok(Some(hmtx_data))) = (
                provider.table_data(tag::HHEA),
                provider.table_data(tag::MAXP),
                provider.table_data(tag::HMTX),
            ) {
                if let (Ok(hhea_table), Ok(maxp_table)) = (
                    ReadScope::new(&hhea_data).read::<HheaTable>(),
                    ReadScope::new(&maxp_data).read::<MaxpTable>(),
                ) {
                    if let Ok(hmtx_table) = ReadScope::new(&hmtx_data).read_dep::<HmtxTable<'_>>((
                        usize::from(maxp_table.num_glyphs),
                        usize::from(hhea_table.num_h_metrics),
                    )) {
                        let mut monospace = true;
                        let mut last_advance = 0;
                        for i in 0..hhea_table.num_h_metrics as usize {
                            if let Ok(metric) = hmtx_table.h_metrics.read_item(i) {
                                if i > 0 && metric.advance_width != last_advance {
                                    monospace = false;
                                    break;
                                }
                                last_advance = metric.advance_width;
                            }
                        }
                        detected_monospace = Some(monospace);
                    }
                }
            }
        }

        let is_monospace = detected_monospace.unwrap_or(false);

        let name_data = provider.table_data(tag::NAME).ok()??.into_owned();
        let name_table = ReadScope::new(&name_data).read::<NameTable>().ok()?;

        let mut f_family = None;

        let patterns: BTreeSet<_> = name_table
            .name_records
            .iter()
            .filter_map(|name_record| {
                let name_id = name_record.name_id;
                if name_id == FONT_SPECIFIER_FAMILY_ID {
                    if let Ok(Some(family)) = fontcode_get_name(&name_data, FONT_SPECIFIER_FAMILY_ID) {
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
                                bold: if is_bold { PatternMatch::True } else { PatternMatch::False },
                                italic: if is_italic { PatternMatch::True } else { PatternMatch::False },
                                oblique: if is_oblique { PatternMatch::True } else { PatternMatch::False },
                                monospace: if is_monospace { PatternMatch::True } else { PatternMatch::False },
                                condensed: if stretch <= FcStretch::Condensed { PatternMatch::True } else { PatternMatch::False },
                                weight,
                                stretch,
                                unicode_ranges: unicode_ranges.clone(),
                                metadata: FcFontMetadata::default(),
                            },
                            font_index,
                        ))
                    }
                } else {
                    None
                }
            })
            .collect();

        results.extend(patterns.into_iter().map(|(pat, idx)| {
            (
                pat,
                FcFont {
                    bytes: font_bytes.to_vec(),
                    font_index: idx,
                    id: font_id.to_string(),
                },
            )
        }));
    }

    if results.is_empty() {
        None
    } else {
        Some(results)
    }
}

#[cfg(all(feature = "std", feature = "parsing"))]
fn FcScanDirectoriesInner(paths: &[(Option<String>, String)]) -> Vec<(FcPattern, FcFontPath)> {
    #[cfg(feature = "multithreading")]
    {
        use rayon::prelude::*;

        // scan directories in parallel
        paths
            .par_iter()
            .filter_map(|(prefix, p)| {
                if let Some(path) = process_path(prefix, PathBuf::from(p), false) {
                    Some(FcScanSingleDirectoryRecursive(path))
                } else {
                    None
                }
            })
            .flatten()
            .collect()
    }
    #[cfg(not(feature = "multithreading"))]
    {
        paths
            .iter()
            .filter_map(|(prefix, p)| {
                if let Some(path) = process_path(prefix, PathBuf::from(p), false) {
                    Some(FcScanSingleDirectoryRecursive(path))
                } else {
                    None
                }
            })
            .flatten()
            .collect()
    }
}

#[cfg(all(feature = "std", feature = "parsing"))]
fn FcScanSingleDirectoryRecursive(dir: PathBuf) -> Vec<(FcPattern, FcFontPath)> {
    let mut files_to_parse = Vec::new();
    let mut dirs_to_parse = vec![dir];

    'outer: loop {
        let mut new_dirs_to_parse = Vec::new();

        'inner: for dir in dirs_to_parse.clone() {
            let dir = match std::fs::read_dir(dir) {
                Ok(o) => o,
                Err(_) => continue 'inner,
            };

            for (path, pathbuf) in dir.filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                let pathbuf = path.to_path_buf();
                Some((path, pathbuf))
            }) {
                if path.is_dir() {
                    new_dirs_to_parse.push(pathbuf);
                } else {
                    files_to_parse.push(pathbuf);
                }
            }
        }

        if new_dirs_to_parse.is_empty() {
            break 'outer;
        } else {
            dirs_to_parse = new_dirs_to_parse;
        }
    }

    FcParseFontFiles(&files_to_parse)
}

#[cfg(all(feature = "std", feature = "parsing"))]
fn FcParseFontFiles(files_to_parse: &[PathBuf]) -> Vec<(FcPattern, FcFontPath)> {
    let result = {
        #[cfg(feature = "multithreading")]
        {
            use rayon::prelude::*;

            files_to_parse
                .par_iter()
                .filter_map(|file| FcParseFont(file))
                .collect::<Vec<Vec<_>>>()
        }
        #[cfg(not(feature = "multithreading"))]
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

/// Representative test codepoints for each Unicode block.
/// These are carefully chosen to be actual script characters (not punctuation/symbols)
/// that a font claiming to support this script should definitely have.
#[cfg(all(feature = "std", feature = "parsing"))]
fn get_verification_codepoints(start: u32, end: u32) -> Vec<u32> {
    match start {
        // Basic Latin - test uppercase, lowercase, and digits
        0x0000 => vec!['A' as u32, 'M' as u32, 'Z' as u32, 'a' as u32, 'm' as u32, 'z' as u32],
        // Latin-1 Supplement - common accented letters
        0x0080 => vec![0x00C0, 0x00C9, 0x00D1, 0x00E0, 0x00E9, 0x00F1], // À É Ñ à é ñ
        // Latin Extended-A
        0x0100 => vec![0x0100, 0x0110, 0x0141, 0x0152, 0x0160], // Ā Đ Ł Œ Š
        // Latin Extended-B
        0x0180 => vec![0x0180, 0x01A0, 0x01B0, 0x01CD], // ƀ Ơ ư Ǎ
        // IPA Extensions
        0x0250 => vec![0x0250, 0x0259, 0x026A, 0x0279], // ɐ ə ɪ ɹ
        // Greek and Coptic
        0x0370 => vec![0x0391, 0x0392, 0x0393, 0x03B1, 0x03B2, 0x03C9], // Α Β Γ α β ω
        // Cyrillic
        0x0400 => vec![0x0410, 0x0411, 0x0412, 0x0430, 0x0431, 0x042F], // А Б В а б Я
        // Armenian
        0x0530 => vec![0x0531, 0x0532, 0x0533, 0x0561, 0x0562], // Ա Բ Գ ա բ
        // Hebrew
        0x0590 => vec![0x05D0, 0x05D1, 0x05D2, 0x05E9, 0x05EA], // א ב ג ש ת
        // Arabic
        0x0600 => vec![0x0627, 0x0628, 0x062A, 0x062C, 0x0645], // ا ب ت ج م
        // Syriac
        0x0700 => vec![0x0710, 0x0712, 0x0713, 0x0715], // ܐ ܒ ܓ ܕ
        // Devanagari
        0x0900 => vec![0x0905, 0x0906, 0x0915, 0x0916, 0x0939], // अ आ क ख ह
        // Bengali
        0x0980 => vec![0x0985, 0x0986, 0x0995, 0x0996], // অ আ ক খ
        // Gurmukhi
        0x0A00 => vec![0x0A05, 0x0A06, 0x0A15, 0x0A16], // ਅ ਆ ਕ ਖ
        // Gujarati
        0x0A80 => vec![0x0A85, 0x0A86, 0x0A95, 0x0A96], // અ આ ક ખ
        // Oriya
        0x0B00 => vec![0x0B05, 0x0B06, 0x0B15, 0x0B16], // ଅ ଆ କ ଖ
        // Tamil
        0x0B80 => vec![0x0B85, 0x0B86, 0x0B95, 0x0BA4], // அ ஆ க த
        // Telugu
        0x0C00 => vec![0x0C05, 0x0C06, 0x0C15, 0x0C16], // అ ఆ క ఖ
        // Kannada
        0x0C80 => vec![0x0C85, 0x0C86, 0x0C95, 0x0C96], // ಅ ಆ ಕ ಖ
        // Malayalam
        0x0D00 => vec![0x0D05, 0x0D06, 0x0D15, 0x0D16], // അ ആ ക ഖ
        // Thai
        0x0E00 => vec![0x0E01, 0x0E02, 0x0E04, 0x0E07, 0x0E40], // ก ข ค ง เ
        // Lao
        0x0E80 => vec![0x0E81, 0x0E82, 0x0E84, 0x0E87], // ກ ຂ ຄ ງ
        // Myanmar
        0x1000 => vec![0x1000, 0x1001, 0x1002, 0x1010, 0x1019], // က ခ ဂ တ မ
        // Georgian
        0x10A0 => vec![0x10D0, 0x10D1, 0x10D2, 0x10D3], // ა ბ გ დ
        // Hangul Jamo
        0x1100 => vec![0x1100, 0x1102, 0x1103, 0x1161, 0x1162], // ᄀ ᄂ ᄃ ᅡ ᅢ
        // Ethiopic
        0x1200 => vec![0x1200, 0x1208, 0x1210, 0x1218], // ሀ ለ ሐ መ
        // Cherokee
        0x13A0 => vec![0x13A0, 0x13A1, 0x13A2, 0x13A3], // Ꭰ Ꭱ Ꭲ Ꭳ
        // Khmer
        0x1780 => vec![0x1780, 0x1781, 0x1782, 0x1783], // ក ខ គ ឃ
        // Mongolian
        0x1800 => vec![0x1820, 0x1821, 0x1822, 0x1823], // ᠠ ᠡ ᠢ ᠣ
        // Hiragana
        0x3040 => vec![0x3042, 0x3044, 0x3046, 0x304B, 0x304D, 0x3093], // あ い う か き ん
        // Katakana
        0x30A0 => vec![0x30A2, 0x30A4, 0x30A6, 0x30AB, 0x30AD, 0x30F3], // ア イ ウ カ キ ン
        // Bopomofo
        0x3100 => vec![0x3105, 0x3106, 0x3107, 0x3108], // ㄅ ㄆ ㄇ ㄈ
        // CJK Unified Ideographs - common characters
        0x4E00 => vec![0x4E00, 0x4E2D, 0x4EBA, 0x5927, 0x65E5, 0x6708], // 一 中 人 大 日 月
        // Hangul Syllables
        0xAC00 => vec![0xAC00, 0xAC01, 0xAC04, 0xB098, 0xB2E4], // 가 각 간 나 다
        // CJK Compatibility Ideographs
        0xF900 => vec![0xF900, 0xF901, 0xF902], // 豈 更 車
        // Arabic Presentation Forms-A
        0xFB50 => vec![0xFB50, 0xFB51, 0xFB52, 0xFB56], // ﭐ ﭑ ﭒ ﭖ
        // Arabic Presentation Forms-B
        0xFE70 => vec![0xFE70, 0xFE72, 0xFE74, 0xFE76], // ﹰ ﹲ ﹴ ﹶ
        // Halfwidth and Fullwidth Forms
        0xFF00 => vec![0xFF01, 0xFF21, 0xFF41, 0xFF61], // ！ Ａ ａ ｡
        // Default: sample at regular intervals
        _ => {
            let range_size = end - start;
            if range_size > 20 {
                vec![
                    start + range_size / 5,
                    start + 2 * range_size / 5,
                    start + 3 * range_size / 5,
                    start + 4 * range_size / 5,
                ]
            } else {
                vec![start, start + range_size / 2]
            }
        }
    }
}

/// Verify OS/2 reported Unicode ranges against actual CMAP support.
/// Returns only ranges that are actually supported by the font's CMAP table.
#[cfg(all(feature = "std", feature = "parsing"))]
fn verify_unicode_ranges_with_cmap(
    provider: &impl FontTableProvider, 
    os2_ranges: Vec<UnicodeRange>
) -> Vec<UnicodeRange> {
    use allsorts::tables::cmap::{Cmap, CmapSubtable, PlatformId, EncodingId};
    
    if os2_ranges.is_empty() {
        return Vec::new();
    }
    
    // Try to get CMAP subtable
    let cmap_data = match provider.table_data(tag::CMAP) {
        Ok(Some(data)) => data,
        _ => return os2_ranges, // Can't verify, trust OS/2
    };
    
    let cmap = match ReadScope::new(&cmap_data).read::<Cmap<'_>>() {
        Ok(c) => c,
        Err(_) => return os2_ranges,
    };
    
    // Find the best Unicode subtable
    let encoding_record = cmap.find_subtable(PlatformId::UNICODE, EncodingId(3))
        .or_else(|| cmap.find_subtable(PlatformId::UNICODE, EncodingId(4)))
        .or_else(|| cmap.find_subtable(PlatformId::WINDOWS, EncodingId(1)))
        .or_else(|| cmap.find_subtable(PlatformId::WINDOWS, EncodingId(10)))
        .or_else(|| cmap.find_subtable(PlatformId::UNICODE, EncodingId(0)))
        .or_else(|| cmap.find_subtable(PlatformId::UNICODE, EncodingId(1)));
    
    let encoding_record = match encoding_record {
        Some(r) => r,
        None => return os2_ranges, // No suitable subtable, trust OS/2
    };
    
    let cmap_subtable = match ReadScope::new(&cmap_data)
        .offset(encoding_record.offset as usize)
        .read::<CmapSubtable<'_>>() 
    {
        Ok(st) => st,
        Err(_) => return os2_ranges,
    };
    
    // Verify each range
    let mut verified_ranges = Vec::new();
    
    for range in os2_ranges {
        let test_codepoints = get_verification_codepoints(range.start, range.end);
        
        // Require at least 50% of test codepoints to have valid glyphs
        // This is stricter than before to avoid false positives
        let required_hits = (test_codepoints.len() + 1) / 2; // ceil(len/2)
        let mut hits = 0;
        
        for cp in test_codepoints {
            if cp >= range.start && cp <= range.end {
                if let Ok(Some(gid)) = cmap_subtable.map_glyph(cp) {
                    if gid != 0 {
                        hits += 1;
                        if hits >= required_hits {
                            break;
                        }
                    }
                }
            }
        }
        
        if hits >= required_hits {
            verified_ranges.push(range);
        }
    }
    
    verified_ranges
}

/// Analyze CMAP table to discover font coverage when OS/2 provides no info.
/// This is the fallback when OS/2 ulUnicodeRange bits are all zero.
#[cfg(all(feature = "std", feature = "parsing"))]
fn analyze_cmap_coverage(provider: &impl FontTableProvider) -> Option<Vec<UnicodeRange>> {
    use allsorts::tables::cmap::{Cmap, CmapSubtable, PlatformId, EncodingId};
    
    let cmap_data = provider.table_data(tag::CMAP).ok()??;
    let cmap = ReadScope::new(&cmap_data).read::<Cmap<'_>>().ok()?;
    
    let encoding_record = cmap.find_subtable(PlatformId::UNICODE, EncodingId(3))
        .or_else(|| cmap.find_subtable(PlatformId::UNICODE, EncodingId(4)))
        .or_else(|| cmap.find_subtable(PlatformId::WINDOWS, EncodingId(1)))
        .or_else(|| cmap.find_subtable(PlatformId::WINDOWS, EncodingId(10)))
        .or_else(|| cmap.find_subtable(PlatformId::UNICODE, EncodingId(0)))
        .or_else(|| cmap.find_subtable(PlatformId::UNICODE, EncodingId(1)))?;
    
    let cmap_subtable = ReadScope::new(&cmap_data)
        .offset(encoding_record.offset as usize)
        .read::<CmapSubtable<'_>>()
        .ok()?;
    
    // Standard Unicode blocks to probe
    let blocks_to_check: &[(u32, u32)] = &[
        (0x0000, 0x007F), // Basic Latin
        (0x0080, 0x00FF), // Latin-1 Supplement
        (0x0100, 0x017F), // Latin Extended-A
        (0x0180, 0x024F), // Latin Extended-B
        (0x0250, 0x02AF), // IPA Extensions
        (0x0300, 0x036F), // Combining Diacritical Marks
        (0x0370, 0x03FF), // Greek and Coptic
        (0x0400, 0x04FF), // Cyrillic
        (0x0500, 0x052F), // Cyrillic Supplement
        (0x0530, 0x058F), // Armenian
        (0x0590, 0x05FF), // Hebrew
        (0x0600, 0x06FF), // Arabic
        (0x0700, 0x074F), // Syriac
        (0x0900, 0x097F), // Devanagari
        (0x0980, 0x09FF), // Bengali
        (0x0A00, 0x0A7F), // Gurmukhi
        (0x0A80, 0x0AFF), // Gujarati
        (0x0B00, 0x0B7F), // Oriya
        (0x0B80, 0x0BFF), // Tamil
        (0x0C00, 0x0C7F), // Telugu
        (0x0C80, 0x0CFF), // Kannada
        (0x0D00, 0x0D7F), // Malayalam
        (0x0E00, 0x0E7F), // Thai
        (0x0E80, 0x0EFF), // Lao
        (0x1000, 0x109F), // Myanmar
        (0x10A0, 0x10FF), // Georgian
        (0x1100, 0x11FF), // Hangul Jamo
        (0x1200, 0x137F), // Ethiopic
        (0x13A0, 0x13FF), // Cherokee
        (0x1780, 0x17FF), // Khmer
        (0x1800, 0x18AF), // Mongolian
        (0x2000, 0x206F), // General Punctuation
        (0x20A0, 0x20CF), // Currency Symbols
        (0x2100, 0x214F), // Letterlike Symbols
        (0x2190, 0x21FF), // Arrows
        (0x2200, 0x22FF), // Mathematical Operators
        (0x2500, 0x257F), // Box Drawing
        (0x25A0, 0x25FF), // Geometric Shapes
        (0x2600, 0x26FF), // Miscellaneous Symbols
        (0x3000, 0x303F), // CJK Symbols and Punctuation
        (0x3040, 0x309F), // Hiragana
        (0x30A0, 0x30FF), // Katakana
        (0x3100, 0x312F), // Bopomofo
        (0x3130, 0x318F), // Hangul Compatibility Jamo
        (0x4E00, 0x9FFF), // CJK Unified Ideographs
        (0xAC00, 0xD7AF), // Hangul Syllables
        (0xF900, 0xFAFF), // CJK Compatibility Ideographs
        (0xFB50, 0xFDFF), // Arabic Presentation Forms-A
        (0xFE70, 0xFEFF), // Arabic Presentation Forms-B
        (0xFF00, 0xFFEF), // Halfwidth and Fullwidth Forms
    ];
    
    let mut ranges = Vec::new();
    
    for &(start, end) in blocks_to_check {
        let test_codepoints = get_verification_codepoints(start, end);
        let required_hits = (test_codepoints.len() + 1) / 2;
        let mut hits = 0;
        
        for cp in test_codepoints {
            if let Ok(Some(gid)) = cmap_subtable.map_glyph(cp) {
                if gid != 0 {
                    hits += 1;
                    if hits >= required_hits {
                        break;
                    }
                }
            }
        }
        
        if hits >= required_hits {
            ranges.push(UnicodeRange { start, end });
        }
    }
    
    if ranges.is_empty() {
        None
    } else {
        Some(ranges)
    }
}

// Helper function to extract unicode ranges (unused, kept for reference)
#[cfg(feature = "parsing")]
#[allow(dead_code)]
fn extract_unicode_ranges(os2_table: &Os2) -> Vec<UnicodeRange> {
    let mut unicode_ranges = Vec::new();

    // Process the 4 Unicode range bitfields from OS/2 table
    let ranges = [
        os2_table.ul_unicode_range1,
        os2_table.ul_unicode_range2,
        os2_table.ul_unicode_range3,
        os2_table.ul_unicode_range4,
    ];

    // Unicode range bit positions to actual ranges
    // Based on OpenType spec
    let range_mappings = [
        (0, 0x0000, 0x007F),  // Basic Latin
        (1, 0x0080, 0x00FF),  // Latin-1 Supplement
        (2, 0x0100, 0x017F),  // Latin Extended-A
        (7, 0x0370, 0x03FF),  // Greek and Coptic
        (9, 0x0400, 0x04FF),  // Cyrillic
        (29, 0x2000, 0x206F), // General Punctuation
        (57, 0x4E00, 0x9FFF), // CJK Unified Ideographs
                              // Add more ranges as needed
    ];

    for (bit, start, end) in &range_mappings {
        let range_idx = bit / 32;
        let bit_pos = bit % 32;

        if range_idx < 4 && (ranges[range_idx] & (1 << bit_pos)) != 0 {
            unicode_ranges.push(UnicodeRange {
                start: *start,
                end: *end,
            });
        }
    }

    unicode_ranges
}

// Helper function to detect if a font is monospace
#[cfg(feature = "parsing")]
#[allow(dead_code)]
fn detect_monospace(
    provider: &impl FontTableProvider,
    os2_table: &Os2,
    detected_monospace: Option<bool>,
) -> Option<bool> {
    if let Some(is_monospace) = detected_monospace {
        return Some(is_monospace);
    }

    // Try using PANOSE classification
    if os2_table.panose[0] == 2 {
        // 2 = Latin Text
        return Some(os2_table.panose[3] == 9); // 9 = Monospaced
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
