//! Library for getting and matching system fonts with
//! minimal dependencies

#![allow(non_snake_case)]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

#[cfg(test)]
mod tests;

use alloc::collections::btree_map::BTreeMap;
use alloc::string::{ToString, String};
use alloc::vec::Vec;
use alloc::borrow::ToOwned;
use alloc::{format, vec};
use allsorts_subset_browser::binary::read::ReadScope;
use allsorts_subset_browser::get_name::fontcode_get_name;
use allsorts_subset_browser::tables::os2::Os2;
use allsorts_subset_browser::tables::{FontTableProvider, HheaTable, HmtxTable, MaxpTable};
use allsorts_subset_browser::tag;
#[cfg(feature = "std")]
use std::path::PathBuf;

/// Simple UUID generator for font match IDs
/// Doesn't use external crates for simplicity
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
        write!(f, "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
            (id >> 96) & 0xFFFFFFFF,
            (id >> 80) & 0xFFFF,
            (id >> 64) & 0xFFFF,
            (id >> 48) & 0xFFFF,
            id & 0xFFFFFFFFFFFF)
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

#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq)]
#[repr(C)]
pub enum PatternMatch {
    True,
    False,
    DontCare,
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

impl Default for PatternMatch {
    fn default() -> Self {
        PatternMatch::DontCare
    }
}

/// Font weight values as defined in CSS specification
#[derive(Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq)]
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
    
    /// Follows CSS spec for weight matching
    pub fn find_best_match(&self, available: &[FcWeight]) -> Option<FcWeight> {
        if available.is_empty() {
            return None;
        }
        
        if available.contains(self) {
            return Some(*self);
        }
        
        match *self {
            FcWeight::Normal => {
                // Try 500 first, then below, then above
                if available.contains(&FcWeight::Medium) {
                    return Some(FcWeight::Medium);
                }
                // Fall through to Light case
                return FcWeight::Light.find_best_match(available);
            }
            FcWeight::Medium => {
                // Try 400 first, then below, then above
                if available.contains(&FcWeight::Normal) {
                    return Some(FcWeight::Normal);
                }
                // Fall through to Light case
                return FcWeight::Light.find_best_match(available);
            }
            FcWeight::Thin | FcWeight::ExtraLight | FcWeight::Light => {
                // Try weights below in descending order, then above in ascending
                for weight in [FcWeight::Light, FcWeight::ExtraLight, FcWeight::Thin].iter() {
                    if available.contains(weight) && *weight <= *self {
                        return Some(*weight);
                    }
                }
                
                for weight in [FcWeight::Normal, FcWeight::Medium, FcWeight::SemiBold, 
                               FcWeight::Bold, FcWeight::ExtraBold, FcWeight::Black].iter() {
                    if available.contains(weight) {
                        return Some(*weight);
                    }
                }
            }
            FcWeight::SemiBold | FcWeight::Bold | FcWeight::ExtraBold | FcWeight::Black => {
                // Try weights above in ascending order, then below in descending
                for weight in [FcWeight::SemiBold, FcWeight::Bold, FcWeight::ExtraBold, FcWeight::Black].iter() {
                    if available.contains(weight) && *weight >= *self {
                        return Some(*weight);
                    }
                }
                
                for weight in [FcWeight::Medium, FcWeight::Normal, FcWeight::Light, 
                               FcWeight::ExtraLight, FcWeight::Thin].iter() {
                    if available.contains(weight) {
                        return Some(*weight);
                    }
                }
            }
        }
        
        // If nothing matches, return the first available weight
        Some(available[0])
    }
}

impl Default for FcWeight {
    fn default() -> Self {
        FcWeight::Normal
    }
}

/// CSS font-stretch values
#[derive(Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq)]
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
                if *stretch < *self && (closest_narrower.is_none() || *stretch > closest_narrower.unwrap()) {
                    closest_narrower = Some(*stretch);
                }
            }
            
            if closest_narrower.is_some() {
                return closest_narrower;
            }
            
            // Otherwise, find wider values
            let mut closest_wider = None;
            for stretch in available.iter() {
                if *stretch > *self && (closest_wider.is_none() || *stretch < closest_wider.unwrap()) {
                    closest_wider = Some(*stretch);
                }
            }
            
            return closest_wider;
        } else {
            // For expanded values, wider values are checked first, then narrower values
            let mut closest_wider = None;
            for stretch in available.iter() {
                if *stretch > *self && (closest_wider.is_none() || *stretch < closest_wider.unwrap()) {
                    closest_wider = Some(*stretch);
                }
            }
            
            if closest_wider.is_some() {
                return closest_wider;
            }
            
            // Otherwise, find narrower values
            let mut closest_narrower = None;
            for stretch in available.iter() {
                if *stretch < *self && (closest_narrower.is_none() || *stretch > closest_narrower.unwrap()) {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
#[derive(Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq)]
pub enum TraceLevel {
    Debug,
    Info,
    Warning,
    Error,
}

/// Reason for font matching failure or success
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchReason {
    NameMismatch { requested: Option<String>, found: Option<String> },
    FamilyMismatch { requested: Option<String>, found: Option<String> },
    StyleMismatch { property: &'static str, requested: String, found: String },
    WeightMismatch { requested: FcWeight, found: FcWeight },
    StretchMismatch { requested: FcStretch, found: FcStretch },
    UnicodeRangeMismatch { character: char, ranges: Vec<UnicodeRange> },
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
#[derive(Debug, Default, Clone, PartialOrd, Ord, PartialEq, Eq)]
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

#[derive(Debug, Default, Clone)]
pub struct FcFontCache {
    // Pattern to path mapping
    map: BTreeMap<FcPattern, FcFontPath>,
    // Memory fonts (id to font bytes)
    memory_fonts: BTreeMap<String, FcFont>,
    // UUID to font path mapping
    id_map: BTreeMap<FontId, FcFontPath>,
}

impl FcFontCache {
    /// Adds in-memory font files
    pub fn with_memory_fonts(&mut self, fonts: Vec<(FcPattern, FcFont)>) -> &mut Self {
        for (pattern, font) in fonts {
            let font_id = font.id.clone();
            let font_path = FcFontPath {
                path: format!("memory:{}", font_id),
                font_index: font.font_index,
            };
            
            self.memory_fonts.insert(font_id, font);
            self.map.insert(pattern, font_path);
        }
        
        self
    }
    
    /// Get font data for a given font ID
    pub fn get_font_by_id(&self, id: &FontId) -> Option<&FcFontPath> {
        self.id_map.get(id)
    }
    
    /// Get in-memory font data
    pub fn get_memory_font(&self, id: &str) -> Option<&FcFont> {
        self.memory_fonts.get(id)
    }

    /// Builds a new font cache
    #[cfg(not(all(feature = "std", feature = "parsing")))]
    pub fn build() -> Self {
        Self::default()
    }

    /// Builds a new font cache from all fonts discovered on the system
    #[cfg(all(feature = "std", feature = "parsing"))]
    pub fn build() -> Self {
        #[cfg(target_os = "linux")]
        {
            let mut cache = FcFontCache {
                map: FcScanDirectories()
                    .unwrap_or_default()
                    .into_iter()
                    .collect(),
                memory_fonts: BTreeMap::new(),
                id_map: BTreeMap::new(),
            };
            cache.build_id_map();
            cache
        }

        #[cfg(target_os = "windows")]
        {
            // `~` isn't actually valid on Windows, but it will be converted by `process_path`
            let font_dirs = vec![
                (None, "C:\\Windows\\Fonts\\".to_owned()),
                (None, "~\\AppData\\Local\\Microsoft\\Windows\\Fonts\\".to_owned()),
            ];
            
            let mut cache = FcFontCache {
                map: FcScanDirectoriesInner(&font_dirs)
                    .into_iter()
                    .collect(),
                memory_fonts: BTreeMap::new(),
                id_map: BTreeMap::new(),
            };
            cache.build_id_map();
            cache
        }

        #[cfg(target_os = "macos")]
        {
            let font_dirs = vec![
                (None, "~/Library/Fonts".to_owned()),
                (None, "/System/Library/Fonts".to_owned()),
                (None, "/Library/Fonts".to_owned()),
            ];
            
            let mut cache = FcFontCache {
                map: FcScanDirectoriesInner(&font_dirs)
                .into_iter()
                .collect(),
                memory_fonts: BTreeMap::new(),
                id_map: BTreeMap::new(),
            };
            cache.build_id_map();
            cache
        }

        #[cfg(target_family = "wasm")]
        {
            Self::default()
        }
    }
    
    /// Build the ID map for quick lookups
    fn build_id_map(&mut self) {
        let mut id_map = BTreeMap::new();
        
        for (_, path) in &self.map {
            let id = FontId::new();
            id_map.insert(id, path.clone());
        }
        
        self.id_map = id_map;
    }

    /// Returns the list of fonts and font patterns
    pub fn list(&self) -> &BTreeMap<FcPattern, FcFontPath> {
        &self.map
    }
    
    /// Check if a pattern matches the query, with detailed tracing
    fn query_matches_internal(
        k: &FcPattern, 
        pattern: &FcPattern, 
        trace: &mut Vec<TraceMsg>
    ) -> bool {
            // Check name - substring match
            if let Some(ref name) = pattern.name {
                let matches = k.name.as_ref()
                    .map_or(false, |k_name| k_name.contains(name));
                    
                if !matches {
                    trace.push(TraceMsg {
                        level: TraceLevel::Info,
                        path: k.name.as_ref().map_or_else(|| "<unknown>".to_string(), Clone::clone),
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
                let matches = k.family.as_ref()
                    .map_or(false, |k_family| k_family.contains(family));
                    
                if !matches {
                    trace.push(TraceMsg {
                        level: TraceLevel::Info,
                        path: k.name.as_ref().map_or_else(|| "<unknown>".to_string(), Clone::clone),
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
            ("italic", pattern.italic.needs_to_match(), pattern.italic.matches(&k.italic)),
            ("oblique", pattern.oblique.needs_to_match(), pattern.oblique.matches(&k.oblique)),
            ("bold", pattern.bold.needs_to_match(), pattern.bold.matches(&k.bold)),
            ("monospace", pattern.monospace.needs_to_match(), pattern.monospace.matches(&k.monospace)),
            ("condensed", pattern.condensed.needs_to_match(), pattern.condensed.matches(&k.condensed)),
        ];
        
        for (property_name, needs_to_match, matches) in style_properties {
            if needs_to_match && !matches {
                trace.push(TraceMsg {
                    level: TraceLevel::Info,
                    path: k.name.as_ref().map_or_else(|| "<unknown>".to_string(), |s| s.clone()),
                    reason: MatchReason::StyleMismatch {
                        property: property_name,
                        requested: format!("{:?}", pattern.italic),
                        found: format!("{:?}", k.italic),
                    },
                });
                return false;
            }
        }
        
        // Check weight
        if pattern.weight != FcWeight::Normal && pattern.weight != k.weight {
            trace.push(TraceMsg {
                level: TraceLevel::Info,
                path: k.name.as_ref().map_or_else(|| "<unknown>".to_string(), |s| s.clone()),
                reason: MatchReason::WeightMismatch {
                    requested: pattern.weight,
                    found: k.weight,
                },
            });
            return false;
        }
        
        // Check stretch
        if pattern.stretch != FcStretch::Normal && pattern.stretch != k.stretch {
            trace.push(TraceMsg {
                level: TraceLevel::Info,
                path: k.name.as_ref().map_or_else(|| "<unknown>".to_string(), |s| s.clone()),
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
                    path: k.name.as_ref().map_or_else(|| "<unknown>".to_string(), |s| s.clone()),
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

    /// Find fallback fonts for a given pattern
    // Helper to calculate total unicode coverage
    fn calculate_unicode_coverage(ranges: &[UnicodeRange]) -> u64 {
        ranges.iter()
            .map(|range| (range.end - range.start + 1) as u64)
            .sum()
    }
    
    // Calculate CSS style matching score (lower is better)
    fn calculate_style_score(original: &FcPattern, candidate: &FcPattern) -> u32 {
        let mut score = 0;
        
        // Weight difference (0-800)
        let weight_diff = (original.weight as i32 - candidate.weight as i32).abs();
        score += weight_diff as u32;
        
        // Stretch difference (1-9)
        let stretch_diff = (original.stretch as i32 - candidate.stretch as i32).abs();
        score += (stretch_diff * 100) as u32;
        
        // Style properties
        if original.italic != candidate.italic { score += 300; }
        if original.bold != candidate.bold { score += 300; }
        if original.oblique != candidate.oblique { score += 200; }
        if original.monospace != candidate.monospace { score += 100; }
        
        score
    }
    
    fn find_fallbacks(&self, pattern: &FcPattern, trace: &mut Vec<TraceMsg>) -> Vec<FontMatchNoFallback> {
        let mut candidates = Vec::new();
        
        // Collect all potential fallbacks (excluding original pattern)
        for (k, v) in &self.map {
            if k == pattern { continue; }
            
            if let Some(id) = self.id_map.iter()
                .find(|(_, path)| *path == v)
                .map(|(id, _)| *id) 
            {
                // Check if this font supports any of the unicode ranges
                if !k.unicode_ranges.is_empty() {
                    let supports_ranges = pattern.unicode_ranges.iter().any(|p_range| 
                        k.unicode_ranges.iter().any(|k_range| p_range.overlaps(k_range))
                    );
                    
                    if supports_ranges {
                        let coverage = Self::calculate_unicode_coverage(&k.unicode_ranges);
                        let style_score = Self::calculate_style_score(pattern, k);
                        candidates.push((FontMatchNoFallback {
                            id: id,
                            unicode_ranges: k.unicode_ranges.clone(),
                        }, coverage, style_score, k.clone()));
                    }
                }
            }
        }
        
        // Sort by coverage (highest first), then by style score (lowest first)
        candidates.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2)));
        
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

    /// Queries a font from the in-memory `font -> file` mapping, returns the first found font (early return)
    pub fn query(&self, pattern: &FcPattern, trace: &mut Vec<TraceMsg>) -> Option<FontMatch> {
        let mut matches = Vec::new();
        
        for (k, v) in &self.map {
            if Self::query_matches_internal(k, pattern, trace) {
                let id = *self.id_map.iter()
                    .find(|(_, path)| *path == v)
                    .map(|(id, _)| id)
                    .unwrap_or(&FontId(0));
                    
                let coverage = Self::calculate_unicode_coverage(&k.unicode_ranges);
                matches.push((id, coverage, k.clone()));
            }
        }
        
        // Sort by unicode coverage (highest first)
        matches.sort_by(|a, b| b.1.cmp(&a.1));
        
        matches.first().map(|(id, _, k)| {
            // Find fallbacks for this font
            let fallbacks = self.find_fallbacks(k, trace);
            
            FontMatch {
                id: *id,
                unicode_ranges: k.unicode_ranges.clone(),
                fallbacks,
            }
        })
    }
    
    /// Queries a font from the in-memory `font -> file` mapping, returns all matching fonts
    pub fn query_all(&self, pattern: &FcPattern, trace: &mut Vec<TraceMsg>) -> Vec<FontMatch> {
        let mut matches = Vec::new();
        
        for (k, v) in &self.map {
            if Self::query_matches_internal(k, pattern, trace) {
                let id = *self.id_map.iter()
                    .find(|(_, path)| *path == v)
                    .map(|(id, _)| id)
                    .unwrap_or(&FontId(0));
                
                let coverage = Self::calculate_unicode_coverage(&k.unicode_ranges);
                matches.push((id, coverage, k.clone()));
            }
        }
        
        // Sort by unicode coverage (highest first)
        matches.sort_by(|a, b| b.1.cmp(&a.1));
        
        matches.into_iter()
            .map(|(id, _, k)| {
                let fallbacks = self.find_fallbacks(&k, trace);
                
                FontMatch {
                    id,
                    unicode_ranges: k.unicode_ranges.clone(),
                    fallbacks,
                }
            })
            .collect()
    }

    /// Find fonts that can render the given text, considering Unicode ranges
    pub fn query_for_text(
        &self, 
        pattern: &FcPattern, 
        text: &str,
        trace: &mut Vec<TraceMsg>
    ) -> Vec<FontMatch> {
        let base_matches = self.query_all(pattern, trace);
        
        // Early return if no matches or text is empty
        if base_matches.is_empty() || text.is_empty() {
            return base_matches;
        }
        
        let chars: Vec<char> = text.chars().collect();
        let mut required_fonts = Vec::new();
        let mut covered_chars = vec![false; chars.len()];
        
        // First try with the matches we already have
        for font_match in &base_matches {
            let font_path = match self.id_map.get(&font_match.id) {
                Some(path) => path,
                None => continue,
            };
            
            let pattern = self.map.iter()
                .find(|(_, path)| path == &font_path)
                .map(|(pattern, _)| pattern);
            
            if let Some(pattern) = pattern {
                for (i, &c) in chars.iter().enumerate() {
                    if !covered_chars[i] && pattern.contains_char(c) {
                        covered_chars[i] = true;
                    }
                }
            }
            
            // Check if this font covers any characters
            let covers_some = covered_chars.iter().any(|&covered| covered);
            if covers_some {
                required_fonts.push(font_match.clone());
            }
        }
        
        // If there are still uncovered characters, look for additional fonts
        let all_covered = covered_chars.iter().all(|&covered| covered);
        if !all_covered {
            // Create a pattern that matches any font that supports the uncovered characters
            let mut fallback_pattern = FcPattern::default();
            
            // Add the uncovered characters as Unicode ranges
            for (i, &c) in chars.iter().enumerate() {
                if !covered_chars[i] {
                    let c_value = c as u32;
                    fallback_pattern.unicode_ranges.push(UnicodeRange {
                        start: c_value,
                        end: c_value,
                    });
                    
                    trace.push(TraceMsg {
                        level: TraceLevel::Warning,
                        path: "<fallback search>".to_string(),
                        reason: MatchReason::UnicodeRangeMismatch {
                            character: c,
                            ranges: Vec::new(), // We don't know which fonts to report here
                        },
                    });
                }
            }
            
            // Find fonts that support these uncovered characters
            let fallback_matches = self.query_all(&fallback_pattern, trace);
            
            for font_match in fallback_matches {
                if !required_fonts.iter().any(|m| m.id == font_match.id) {
                    required_fonts.push(font_match);
                }
            }
        }
        
        required_fonts
    }
}

// Remaining implementation for font scanning, parsing, etc.
#[cfg(all(feature = "std", feature = "parsing"))]
fn FcParseFont(filepath: &PathBuf) -> Option<Vec<(FcPattern, FcFontPath)>> {
    use allsorts_subset_browser::{
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
        let num_fonts = u32::from_be_bytes([
            font_bytes[8], font_bytes[9], font_bytes[10], font_bytes[11]
        ]);
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
        let is_oblique = os2_table.fs_selection.contains(allsorts_subset_browser::tables::os2::FsSelection::OBLIQUE);
        let weight = FcWeight::from_u16(os2_table.us_weight_class);
        let stretch = FcStretch::from_u16(os2_table.us_width_class);
        
        // Extract unicode ranges
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
            // Range 1 (Basic Latin through General Punctuation)
            (0, 0x0000, 0x007F), // Basic Latin
            (1, 0x0080, 0x00FF), // Latin-1 Supplement
            (2, 0x0100, 0x017F), // Latin Extended-A
            // ... add more range mappings
            
            // A simplified example - in practice, you'd include all ranges from the OpenType spec
            (7, 0x0370, 0x03FF), // Greek and Coptic
            (9, 0x0400, 0x04FF), // Cyrillic
            (29, 0x2000, 0x206F), // General Punctuation
            (57, 0x4E00, 0x9FFF), // CJK Unified Ideographs
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
        
        // If no monospace detection yet, check using hmtx
        if detected_monospace.is_none() {

            // Try using PANOSE classification
            if os2_table.panose[0] == 2 { // 2 = Latin Text
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

        results.extend(
            patterns
                .into_iter()
                .map(|(pat, index)| {
                    (
                        pat,
                        FcFontPath {
                            path: filepath.to_string_lossy().to_string(),
                            font_index: index,
                        },
                    )
                })
        );
    }
    
    if results.is_empty() {
        None
    } else {
        Some(results)
    }
}

#[cfg(all(feature = "std", feature = "parsing"))]
fn FcScanDirectoriesInner(paths: &[(Option<String>, String)]) -> Vec<(FcPattern, FcFontPath)> {

    #[cfg(feature = "multithreading")] {
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
    #[cfg(not(feature = "multithreading"))] {
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
        #[cfg(feature = "multithreading")] {
            use rayon::prelude::*;

            files_to_parse
            .par_iter()
            .filter_map(|file| FcParseFont(file))
            .collect::<Vec<Vec<_>>>()
        }
        #[cfg(not(feature = "multithreading"))] {
            files_to_parse
            .iter()
            .filter_map(|file| FcParseFont(file))
            .collect::<Vec<Vec<_>>>()
        }
    };

    result
    .into_iter()
    .flat_map(|f| f.into_iter())
    .collect()
}

#[cfg(feature = "std")]
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
        var(HOME_ENV_VAR)
            .ok()
            .map(PathBuf::from)
    }
    fn get_xdg_config_home_value() -> Option<PathBuf> {
        var(XDG_CONFIG_HOME_ENV_VAR)
            .ok()
            .map(PathBuf::from)
            .or_else(|| get_home_value()
                .map(|home_path|
                    home_path.join(XDG_CONFIG_HOME_DEFAULT_PATH_SUFFIX))
            )
    }
    fn get_xdg_data_home_value() -> Option<PathBuf> {
        var(XDG_DATA_HOME_ENV_VAR)
            .ok()
            .map(PathBuf::from)
            .or_else(|| get_home_value()
                .map(|home_path|
                    home_path.join(XDG_DATA_HOME_DEFAULT_PATH_SUFFIX))
            )
    }

    // Resolve the tilde character in the path, if present
    if path.starts_with(HOME_SHORTCUT) {
        if let Some(home_path) = get_home_value() {
            path = home_path.join(path.strip_prefix(HOME_SHORTCUT).expect("already checked that it starts with the prefix"));
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
            _ => None // Unsupported prefix
        }
        None => Some(path),
    }
}

// Helper function to extract a string from the name table
fn get_name_string(name_data: &[u8], name_id: u16) -> Option<String> {
    fontcode_get_name(name_data, name_id)
        .ok()
        .flatten()
        .map(|name| String::from_utf8_lossy(name.to_bytes()).to_string())
}

// Helper function to extract unicode ranges
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
        (0, 0x0000, 0x007F),   // Basic Latin
        (1, 0x0080, 0x00FF),   // Latin-1 Supplement
        (2, 0x0100, 0x017F),   // Latin Extended-A
        (7, 0x0370, 0x03FF),   // Greek and Coptic
        (9, 0x0400, 0x04FF),   // Cyrillic
        (29, 0x2000, 0x206F),  // General Punctuation
        (57, 0x4E00, 0x9FFF),  // CJK Unified Ideographs
        // Add more ranges as needed
    ];
    
    for (bit, start, end) in &range_mappings {
        let range_idx = bit / 32;
        let bit_pos = bit % 32;
        
        if range_idx < 4 && (ranges[range_idx] & (1 << bit_pos)) != 0 {
            unicode_ranges.push(UnicodeRange { start: *start, end: *end });
        }
    }
    
    unicode_ranges
}

// Helper function to detect if a font is monospace
fn detect_monospace(
    provider: &impl FontTableProvider,
    os2_table: &Os2,
    detected_monospace: Option<bool>
) -> Option<bool> {
    if let Some(is_monospace) = detected_monospace {
        return Some(is_monospace);
    }

    // Try using PANOSE classification
    if os2_table.panose[0] == 2 { // 2 = Latin Text
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