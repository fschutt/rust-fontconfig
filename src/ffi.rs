//! FFI bindings for rust-fontconfig
//!
//! This module provides C-compatible bindings for the rust-fontconfig library.

use crate::*;
use std::ffi::{c_char, c_uint, c_void, CStr, CString};
use std::fmt::Write;
use std::mem;
use std::ptr;
use std::slice;

/// C-compatible font ID representation
#[repr(C)]
pub struct FcFontIdC {
    high: u64,
    low: u64,
}

impl FcFontIdC {
    fn from_fontid(id: &FontId) -> Self {
        let id_value = id.0;
        Self {
            high: (id_value >> 64) as u64,
            low: id_value as u64,
        }
    }
}

impl FontId {
    fn from_fontid_c(id: &FcFontIdC) -> Self {
        let combined = ((id.high as u128) << 64) | (id.low as u128);
        FontId(combined)
    }
}

/// C-compatible representation of a font match without fallbacks
#[repr(C)]
pub struct FcFontMatchNoFallbackC {
    id: FcFontIdC,
    unicode_ranges: *mut UnicodeRange,
    unicode_ranges_count: usize,
}

/// C-compatible representation of a font match with fallbacks
#[repr(C)]
pub struct FcFontMatchC {
    id: FcFontIdC,
    unicode_ranges: *mut UnicodeRange,
    unicode_ranges_count: usize,
    fallbacks: *mut FcFontMatchNoFallbackC,
    fallbacks_count: usize,
}

/// C-compatible font path
#[repr(C)]
pub struct FcFontPathC {
    path: *mut c_char,
    font_index: usize,
}

/// C-compatible in-memory font data
#[repr(C)]
pub struct FcFontC {
    bytes: *mut u8,
    bytes_len: usize,
    font_index: usize,
    id: *mut c_char,
}

/// C-compatible font metadata
#[repr(C)]
pub struct FcFontMetadataC {
    copyright: *mut c_char,
    designer: *mut c_char,
    designer_url: *mut c_char,
    font_family: *mut c_char,
    font_subfamily: *mut c_char,
    full_name: *mut c_char,
    id_description: *mut c_char,
    license: *mut c_char,
    license_url: *mut c_char,
    manufacturer: *mut c_char,
    manufacturer_url: *mut c_char,
    postscript_name: *mut c_char,
    preferred_family: *mut c_char,
    preferred_subfamily: *mut c_char,
    trademark: *mut c_char,
    unique_id: *mut c_char,
    version: *mut c_char,
}

/// C-compatible pattern for matching
#[repr(C)]
pub struct FcPatternC {
    name: *mut c_char,
    family: *mut c_char,
    italic: PatternMatch,
    oblique: PatternMatch,
    bold: PatternMatch,
    monospace: PatternMatch,
    condensed: PatternMatch,
    weight: FcWeight,
    stretch: FcStretch,
    unicode_ranges: *mut UnicodeRange,
    unicode_ranges_count: usize,
    metadata: FcFontMetadataC,
}

/// Reason type for trace messages
#[repr(C)]
pub enum FcReasonTypeC {
    NameMismatch = 0,
    FamilyMismatch = 1,
    StyleMismatch = 2,
    WeightMismatch = 3,
    StretchMismatch = 4,
    UnicodeRangeMismatch = 5,
    Success = 6,
}

/// Trace message level
#[repr(C)]
pub enum FcTraceLevelC {
    Debug = 0,
    Info = 1,
    Warning = 2,
    Error = 3,
}

impl From<TraceLevel> for FcTraceLevelC {
    fn from(level: TraceLevel) -> Self {
        match level {
            TraceLevel::Debug => FcTraceLevelC::Debug,
            TraceLevel::Info => FcTraceLevelC::Info,
            TraceLevel::Warning => FcTraceLevelC::Warning,
            TraceLevel::Error => FcTraceLevelC::Error,
        }
    }
}

/// C-compatible trace message
#[repr(C)]
pub struct FcTraceMsgC {
    level: FcTraceLevelC,
    path: *mut c_char,
    reason: *mut c_void, // Opaque pointer to MatchReason
}

/// Helper to convert Rust Option<String> to C char pointer
fn option_string_to_c_char(s: Option<&String>) -> *mut c_char {
    match s {
        Some(s) => {
            let c_str = CString::new(s.as_str()).unwrap_or_default();
            let ptr = c_str.into_raw();
            ptr
        }
        None => ptr::null_mut(),
    }
}

/// Helper to free C string
unsafe fn free_c_string(s: *mut c_char) {
    if !s.is_null() {
        let _ = CString::from_raw(s);
    }
}

/// Helper to convert C string to Rust Option<String>
unsafe fn c_char_to_option_string(s: *const c_char) -> Option<String> {
    if s.is_null() {
        None
    } else {
        Some(CStr::from_ptr(s).to_string_lossy().into_owned())
    }
}

/// Convert Rust FcPattern to C FcPatternC
fn pattern_to_c(pattern: &FcPattern) -> FcPatternC {
    let name = option_string_to_c_char(pattern.name.as_ref());
    let family = option_string_to_c_char(pattern.family.as_ref());

    let unicode_ranges_count = pattern.unicode_ranges.len();
    let unicode_ranges = if unicode_ranges_count > 0 {
        let mut ranges = Vec::with_capacity(unicode_ranges_count);
        for range in &pattern.unicode_ranges {
            ranges.push(*range);
        }
        let ptr = ranges.as_mut_ptr();
        mem::forget(ranges);
        ptr
    } else {
        ptr::null_mut()
    };

    let metadata = FcFontMetadataC {
        copyright: option_string_to_c_char(pattern.metadata.copyright.as_ref()),
        designer: option_string_to_c_char(pattern.metadata.designer.as_ref()),
        designer_url: option_string_to_c_char(pattern.metadata.designer_url.as_ref()),
        font_family: option_string_to_c_char(pattern.metadata.font_family.as_ref()),
        font_subfamily: option_string_to_c_char(pattern.metadata.font_subfamily.as_ref()),
        full_name: option_string_to_c_char(pattern.metadata.full_name.as_ref()),
        id_description: option_string_to_c_char(pattern.metadata.id_description.as_ref()),
        license: option_string_to_c_char(pattern.metadata.license.as_ref()),
        license_url: option_string_to_c_char(pattern.metadata.license_url.as_ref()),
        manufacturer: option_string_to_c_char(pattern.metadata.manufacturer.as_ref()),
        manufacturer_url: option_string_to_c_char(pattern.metadata.manufacturer_url.as_ref()),
        postscript_name: option_string_to_c_char(pattern.metadata.postscript_name.as_ref()),
        preferred_family: option_string_to_c_char(pattern.metadata.preferred_family.as_ref()),
        preferred_subfamily: option_string_to_c_char(pattern.metadata.preferred_subfamily.as_ref()),
        trademark: option_string_to_c_char(pattern.metadata.trademark.as_ref()),
        unique_id: option_string_to_c_char(pattern.metadata.unique_id.as_ref()),
        version: option_string_to_c_char(pattern.metadata.version.as_ref()),
    };

    FcPatternC {
        name,
        family,
        italic: pattern.italic,
        oblique: pattern.oblique,
        bold: pattern.bold,
        monospace: pattern.monospace,
        condensed: pattern.condensed,
        weight: pattern.weight,
        stretch: pattern.stretch,
        unicode_ranges,
        unicode_ranges_count,
        metadata,
    }
}

/// Convert C FcPatternC to Rust FcPattern
unsafe fn c_to_pattern(pattern: *const FcPatternC) -> FcPattern {
    let pattern = &*pattern;

    let name = c_char_to_option_string(pattern.name);
    let family = c_char_to_option_string(pattern.family);

    let mut unicode_ranges = Vec::new();
    if !pattern.unicode_ranges.is_null() && pattern.unicode_ranges_count > 0 {
        unicode_ranges =
            slice::from_raw_parts(pattern.unicode_ranges, pattern.unicode_ranges_count).to_vec();
    }

    let metadata = FcFontMetadata {
        copyright: c_char_to_option_string(pattern.metadata.copyright),
        designer: c_char_to_option_string(pattern.metadata.designer),
        designer_url: c_char_to_option_string(pattern.metadata.designer_url),
        font_family: c_char_to_option_string(pattern.metadata.font_family),
        font_subfamily: c_char_to_option_string(pattern.metadata.font_subfamily),
        full_name: c_char_to_option_string(pattern.metadata.full_name),
        id_description: c_char_to_option_string(pattern.metadata.id_description),
        license: c_char_to_option_string(pattern.metadata.license),
        license_url: c_char_to_option_string(pattern.metadata.license_url),
        manufacturer: c_char_to_option_string(pattern.metadata.manufacturer),
        manufacturer_url: c_char_to_option_string(pattern.metadata.manufacturer_url),
        postscript_name: c_char_to_option_string(pattern.metadata.postscript_name),
        preferred_family: c_char_to_option_string(pattern.metadata.preferred_family),
        preferred_subfamily: c_char_to_option_string(pattern.metadata.preferred_subfamily),
        trademark: c_char_to_option_string(pattern.metadata.trademark),
        unique_id: c_char_to_option_string(pattern.metadata.unique_id),
        version: c_char_to_option_string(pattern.metadata.version),
    };

    FcPattern {
        name,
        family,
        italic: pattern.italic,
        oblique: pattern.oblique,
        bold: pattern.bold,
        monospace: pattern.monospace,
        condensed: pattern.condensed,
        weight: pattern.weight,
        stretch: pattern.stretch,
        unicode_ranges,
        metadata,
    }
}

/// Free a C pattern
unsafe fn free_pattern_c(pattern: *mut FcPatternC) {
    if pattern.is_null() {
        return;
    }

    let pattern = &mut *pattern;

    free_c_string(pattern.name);
    free_c_string(pattern.family);

    if !pattern.unicode_ranges.is_null() && pattern.unicode_ranges_count > 0 {
        let _ = Vec::from_raw_parts(
            pattern.unicode_ranges,
            pattern.unicode_ranges_count,
            pattern.unicode_ranges_count,
        );
    }

    // Free metadata strings
    free_c_string(pattern.metadata.copyright);
    free_c_string(pattern.metadata.designer);
    free_c_string(pattern.metadata.designer_url);
    free_c_string(pattern.metadata.font_family);
    free_c_string(pattern.metadata.font_subfamily);
    free_c_string(pattern.metadata.full_name);
    free_c_string(pattern.metadata.id_description);
    free_c_string(pattern.metadata.license);
    free_c_string(pattern.metadata.license_url);
    free_c_string(pattern.metadata.manufacturer);
    free_c_string(pattern.metadata.manufacturer_url);
    free_c_string(pattern.metadata.postscript_name);
    free_c_string(pattern.metadata.preferred_family);
    free_c_string(pattern.metadata.preferred_subfamily);
    free_c_string(pattern.metadata.trademark);
    free_c_string(pattern.metadata.unique_id);
    free_c_string(pattern.metadata.version);

    let _ = Box::from_raw(pattern);
}

/// Convert Rust font match to C representation
fn font_match_to_c(match_obj: &FontMatch) -> FcFontMatchC {
    let id = FcFontIdC::from_fontid(&match_obj.id);

    let unicode_ranges_count = match_obj.unicode_ranges.len();
    let unicode_ranges = if unicode_ranges_count > 0 {
        let mut ranges = Vec::with_capacity(unicode_ranges_count);
        for range in &match_obj.unicode_ranges {
            ranges.push(*range);
        }
        let ptr = ranges.as_mut_ptr();
        mem::forget(ranges);
        ptr
    } else {
        ptr::null_mut()
    };

    let fallbacks_count = match_obj.fallbacks.len();
    let fallbacks = if fallbacks_count > 0 {
        let mut fb = Vec::with_capacity(fallbacks_count);
        for fallback in &match_obj.fallbacks {
            let fallback_ranges_count = fallback.unicode_ranges.len();
            let fallback_ranges = if fallback_ranges_count > 0 {
                let mut ranges = Vec::with_capacity(fallback_ranges_count);
                for range in &fallback.unicode_ranges {
                    ranges.push(*range);
                }
                let ptr = ranges.as_mut_ptr();
                mem::forget(ranges);
                ptr
            } else {
                ptr::null_mut()
            };

            fb.push(FcFontMatchNoFallbackC {
                id: FcFontIdC::from_fontid(&fallback.id),
                unicode_ranges: fallback_ranges,
                unicode_ranges_count: fallback_ranges_count,
            });
        }
        let ptr = fb.as_mut_ptr();
        mem::forget(fb);
        ptr
    } else {
        ptr::null_mut()
    };

    FcFontMatchC {
        id,
        unicode_ranges,
        unicode_ranges_count,
        fallbacks,
        fallbacks_count,
    }
}

/// Free a C font match
unsafe fn free_font_match_c(match_obj: *mut FcFontMatchC) {
    if match_obj.is_null() {
        return;
    }

    let match_obj = &mut *match_obj;

    if !match_obj.unicode_ranges.is_null() && match_obj.unicode_ranges_count > 0 {
        let _ = Vec::from_raw_parts(
            match_obj.unicode_ranges,
            match_obj.unicode_ranges_count,
            match_obj.unicode_ranges_count,
        );
    }

    if !match_obj.fallbacks.is_null() && match_obj.fallbacks_count > 0 {
        let fallbacks = slice::from_raw_parts_mut(match_obj.fallbacks, match_obj.fallbacks_count);

        for fallback in fallbacks {
            if !fallback.unicode_ranges.is_null() && fallback.unicode_ranges_count > 0 {
                let _ = Vec::from_raw_parts(
                    fallback.unicode_ranges,
                    fallback.unicode_ranges_count,
                    fallback.unicode_ranges_count,
                );
            }
        }

        let _ = Vec::from_raw_parts(
            match_obj.fallbacks,
            match_obj.fallbacks_count,
            match_obj.fallbacks_count,
        );
    }

    let _ = Box::from_raw(match_obj);
}

/// Convert trace messages to C representation
fn trace_msgs_to_c(trace: &[TraceMsg]) -> (*mut FcTraceMsgC, usize) {
    if trace.is_empty() {
        return (ptr::null_mut(), 0);
    }

    let count = trace.len();
    let mut trace_c = Vec::with_capacity(count);

    for msg in trace {
        let path = CString::new(msg.path.clone())
            .unwrap_or_default()
            .into_raw();

        // Create a boxed MatchReason and convert to opaque pointer
        let reason = Box::new(msg.reason.clone());
        let reason_ptr = Box::into_raw(reason) as *mut c_void;

        trace_c.push(FcTraceMsgC {
            level: msg.level.into(),
            path,
            reason: reason_ptr,
        });
    }

    let ptr = trace_c.as_mut_ptr();
    let count = trace_c.len();
    mem::forget(trace_c);

    (ptr, count)
}

/// Create a new font ID
#[no_mangle]
pub extern "C" fn fc_font_id_new() -> FcFontIdC {
    FcFontIdC::from_fontid(&FontId::new())
}

/// Create a new font cache
#[no_mangle]
pub extern "C" fn fc_cache_build() -> *mut FcFontCache {
    let cache = FcFontCache::build();
    Box::into_raw(Box::new(cache))
}

/// Free the font cache
#[no_mangle]
pub extern "C" fn fc_cache_free(cache: *mut FcFontCache) {
    if !cache.is_null() {
        unsafe {
            let _ = Box::from_raw(cache);
        }
    }
}

/// Create a new default pattern
#[no_mangle]
pub extern "C" fn fc_pattern_new() -> *mut FcPatternC {
    let pattern = FcPattern::default();
    let pattern_c = pattern_to_c(&pattern);
    Box::into_raw(Box::new(pattern_c))
}

/// Free a pattern
#[no_mangle]
pub extern "C" fn fc_pattern_free(pattern: *mut FcPatternC) {
    if !pattern.is_null() {
        unsafe {
            free_pattern_c(pattern);
        }
    }
}

/// Set pattern name
#[no_mangle]
pub extern "C" fn fc_pattern_set_name(pattern: *mut FcPatternC, name: *const c_char) {
    if pattern.is_null() || name.is_null() {
        return;
    }

    unsafe {
        let pattern = &mut *pattern;

        // Free existing name if any
        free_c_string(pattern.name);

        // Set new name
        let name_str = CStr::from_ptr(name).to_string_lossy().into_owned();
        pattern.name = CString::new(name_str).unwrap_or_default().into_raw();
    }
}

/// Set pattern family
#[no_mangle]
pub extern "C" fn fc_pattern_set_family(pattern: *mut FcPatternC, family: *const c_char) {
    if pattern.is_null() || family.is_null() {
        return;
    }

    unsafe {
        let pattern = &mut *pattern;

        // Free existing family if any
        free_c_string(pattern.family);

        // Set new family
        let family_str = CStr::from_ptr(family).to_string_lossy().into_owned();
        pattern.family = CString::new(family_str).unwrap_or_default().into_raw();
    }
}

/// Set pattern italic
#[no_mangle]
pub extern "C" fn fc_pattern_set_italic(pattern: *mut FcPatternC, italic: PatternMatch) {
    if pattern.is_null() {
        return;
    }

    unsafe {
        let pattern = &mut *pattern;
        pattern.italic = italic;
    }
}

/// Set pattern bold
#[no_mangle]
pub extern "C" fn fc_pattern_set_bold(pattern: *mut FcPatternC, bold: PatternMatch) {
    if pattern.is_null() {
        return;
    }

    unsafe {
        let pattern = &mut *pattern;
        pattern.bold = bold;
    }
}

/// Set pattern monospace
#[no_mangle]
pub extern "C" fn fc_pattern_set_monospace(pattern: *mut FcPatternC, monospace: PatternMatch) {
    if pattern.is_null() {
        return;
    }

    unsafe {
        let pattern = &mut *pattern;
        pattern.monospace = monospace;
    }
}

/// Set pattern weight
#[no_mangle]
pub extern "C" fn fc_pattern_set_weight(pattern: *mut FcPatternC, weight: FcWeight) {
    if pattern.is_null() {
        return;
    }

    unsafe {
        let pattern = &mut *pattern;
        pattern.weight = weight;
    }
}

/// Set pattern stretch
#[no_mangle]
pub extern "C" fn fc_pattern_set_stretch(pattern: *mut FcPatternC, stretch: FcStretch) {
    if pattern.is_null() {
        return;
    }

    unsafe {
        let pattern = &mut *pattern;
        pattern.stretch = stretch;
    }
}

/// Add unicode range to pattern
#[no_mangle]
pub extern "C" fn fc_pattern_add_unicode_range(
    pattern: *mut FcPatternC,
    start: c_uint,
    end: c_uint,
) {
    if pattern.is_null() {
        return;
    }

    unsafe {
        let pattern = &mut *pattern;

        let new_range = UnicodeRange { start, end };

        // Create a new array with additional capacity
        let mut new_ranges = Vec::with_capacity(pattern.unicode_ranges_count + 1);

        // Copy existing ranges if any
        if !pattern.unicode_ranges.is_null() && pattern.unicode_ranges_count > 0 {
            new_ranges.extend_from_slice(slice::from_raw_parts(
                pattern.unicode_ranges,
                pattern.unicode_ranges_count,
            ));

            // Free the old array
            let _ = Vec::from_raw_parts(
                pattern.unicode_ranges,
                pattern.unicode_ranges_count,
                pattern.unicode_ranges_count,
            );
        }

        // Add the new range
        new_ranges.push(new_range);

        // Update the pattern
        pattern.unicode_ranges = new_ranges.as_mut_ptr();
        pattern.unicode_ranges_count = new_ranges.len();

        // Forget the vector to avoid double-free
        mem::forget(new_ranges);
    }
}

/// Free a font match
#[no_mangle]
pub extern "C" fn fc_font_match_free(match_obj: *mut FcFontMatchC) {
    if !match_obj.is_null() {
        unsafe {
            free_font_match_c(match_obj);
        }
    }
}

/// Free an array of font matches
#[no_mangle]
pub extern "C" fn fc_font_matches_free(matches: *mut *mut FcFontMatchC, count: usize) {
    if matches.is_null() || count == 0 {
        return;
    }

    unsafe {
        let matches_slice = slice::from_raw_parts_mut(matches, count);

        for match_ptr in matches_slice {
            if !match_ptr.is_null() {
                free_font_match_c(*match_ptr);
            }
        }

        let _ = Vec::from_raw_parts(matches, count, count);
    }
}

/// Free font path
#[no_mangle]
pub extern "C" fn fc_font_path_free(path: *mut FcFontPathC) {
    if path.is_null() {
        return;
    }

    unsafe {
        let path = &mut *path;
        free_c_string(path.path);
        let _ = Box::from_raw(path);
    }
}

/// Free an in-memory font
#[no_mangle]
pub extern "C" fn fc_font_free(font: *mut FcFontC) {
    if font.is_null() {
        return;
    }

    unsafe {
        let font = &mut *font;

        if !font.bytes.is_null() && font.bytes_len > 0 {
            let _ = Vec::from_raw_parts(font.bytes, font.bytes_len, font.bytes_len);
        }

        free_c_string(font.id);
        let _ = Box::from_raw(font);
    }
}

/// Get trace reason type
#[no_mangle]
pub extern "C" fn fc_trace_get_reason_type(trace: *const FcTraceMsgC) -> FcReasonTypeC {
    if trace.is_null() {
        return FcReasonTypeC::Success;
    }

    unsafe {
        let trace = &*trace;

        if trace.reason.is_null() {
            return FcReasonTypeC::Success;
        }

        let reason = &*(trace.reason as *const MatchReason);

        match reason {
            MatchReason::NameMismatch { .. } => FcReasonTypeC::NameMismatch,
            MatchReason::FamilyMismatch { .. } => FcReasonTypeC::FamilyMismatch,
            MatchReason::StyleMismatch { .. } => FcReasonTypeC::StyleMismatch,
            MatchReason::WeightMismatch { .. } => FcReasonTypeC::WeightMismatch,
            MatchReason::StretchMismatch { .. } => FcReasonTypeC::StretchMismatch,
            MatchReason::UnicodeRangeMismatch { .. } => FcReasonTypeC::UnicodeRangeMismatch,
            MatchReason::Success => FcReasonTypeC::Success,
        }
    }
}

/// Free trace messages
#[no_mangle]
pub extern "C" fn fc_trace_free(trace: *mut FcTraceMsgC, count: usize) {
    if trace.is_null() || count == 0 {
        return;
    }

    unsafe {
        let trace_slice = slice::from_raw_parts_mut(trace, count);

        for msg in trace_slice {
            free_c_string(msg.path);

            if !msg.reason.is_null() {
                let _ = Box::from_raw(msg.reason as *mut MatchReason);
            }
        }

        let _ = Vec::from_raw_parts(trace, count, count);
    }
}

/// Convert font ID to string
#[no_mangle]
pub extern "C" fn fc_font_id_to_string(
    id: *const FcFontIdC,
    buffer: *mut c_char,
    buffer_size: usize,
) -> bool {
    if id.is_null() || buffer.is_null() || buffer_size == 0 {
        return false;
    }

    unsafe {
        let id_rust = FontId::from_fontid_c(&*id);
        let mut id_str = String::new();

        if write!(id_str, "{}", id_rust).is_err() {
            return false;
        }

        if id_str.len() >= buffer_size {
            return false;
        }

        let c_str = CString::new(id_str).unwrap_or_default();
        let src = c_str.as_bytes_with_nul();
        let dest = slice::from_raw_parts_mut(buffer as *mut u8, buffer_size);

        for (i, &byte) in src.iter().enumerate() {
            if i < buffer_size {
                dest[i] = byte;
            } else {
                return false;
            }
        }

        true
    }
}

/// Font info for listing fonts
#[repr(C)]
pub struct FcFontInfoC {
    id: FcFontIdC,
    name: *mut c_char,
    family: *mut c_char,
}

/// Free array of font info
#[no_mangle]
pub extern "C" fn fc_font_info_free(info: *mut FcFontInfoC, count: usize) {
    if info.is_null() || count == 0 {
        return;
    }

    unsafe {
        let info_slice = slice::from_raw_parts_mut(info, count);

        for item in info_slice {
            free_c_string(item.name);
            free_c_string(item.family);
        }

        let _ = Vec::from_raw_parts(info, count, count);
    }
}

/// Free font metadata
#[no_mangle]
pub extern "C" fn fc_font_metadata_free(metadata: *mut FcFontMetadataC) {
    if metadata.is_null() {
        return;
    }

    unsafe {
        let metadata = &mut *metadata;

        free_c_string(metadata.copyright);
        free_c_string(metadata.designer);
        free_c_string(metadata.designer_url);
        free_c_string(metadata.font_family);
        free_c_string(metadata.font_subfamily);
        free_c_string(metadata.full_name);
        free_c_string(metadata.id_description);
        free_c_string(metadata.license);
        free_c_string(metadata.license_url);
        free_c_string(metadata.manufacturer);
        free_c_string(metadata.manufacturer_url);
        free_c_string(metadata.postscript_name);
        free_c_string(metadata.preferred_family);
        free_c_string(metadata.preferred_subfamily);
        free_c_string(metadata.trademark);
        free_c_string(metadata.unique_id);
        free_c_string(metadata.version);

        let _ = Box::from_raw(metadata);
    }
}

/// Get font path by ID
#[no_mangle]
pub extern "C" fn fc_cache_get_font_path(
    cache: *const FcFontCache,
    id: *const FcFontIdC,
) -> *mut FcFontPathC {
    if cache.is_null() || id.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let cache = &*cache;
        let id_rust = FontId::from_fontid_c(&*id);

        match cache.get_font_by_id(&id_rust) {
            Some(FontSource::Disk(path)) => {
                let path_c = FcFontPathC {
                    path: CString::new(path.path.clone())
                        .unwrap_or_default()
                        .into_raw(),
                    font_index: path.font_index,
                };

                Box::into_raw(Box::new(path_c))
            }
            Some(FontSource::Memory(font)) => {
                // For memory fonts, return a special path
                let path_c = FcFontPathC {
                    path: CString::new(format!("memory:{}", font.id))
                        .unwrap_or_default()
                        .into_raw(),
                    font_index: font.font_index,
                };

                Box::into_raw(Box::new(path_c))
            }
            None => ptr::null_mut(),
        }
    }
}

/// Query a font from the cache
#[no_mangle]
pub extern "C" fn fc_cache_query(
    cache: *const FcFontCache,
    pattern: *const FcPatternC,
    trace: *mut *mut FcTraceMsgC,
    trace_count: *mut usize,
) -> *mut FcFontMatchC {
    if cache.is_null() || pattern.is_null() || trace.is_null() || trace_count.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let cache = &*cache;
        let pattern_rust = c_to_pattern(pattern);

        let mut trace_msgs = Vec::new();
        let result = cache.query(&pattern_rust, &mut trace_msgs);

        // Convert trace messages
        let (trace_c, count) = trace_msgs_to_c(&trace_msgs);
        *trace = trace_c;
        *trace_count = count;

        match result {
            Some(match_obj) => {
                let match_c = font_match_to_c(&match_obj);
                Box::into_raw(Box::new(match_c))
            }
            None => ptr::null_mut(),
        }
    }
}

/// Get metadata by font ID
#[no_mangle]
pub extern "C" fn fc_cache_get_font_metadata(
    cache: *const FcFontCache,
    id: *const FcFontIdC,
) -> *mut FcFontMetadataC {
    if cache.is_null() || id.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let cache = &*cache;
        let id_rust = FontId::from_fontid_c(&*id);

        // Get metadata directly from ID
        let pattern = match cache.get_metadata_by_id(&id_rust) {
            Some(pattern) => pattern,
            None => return ptr::null_mut(),
        };

        // Create metadata from pattern
        let metadata = Box::new(FcFontMetadataC {
            copyright: option_string_to_c_char(pattern.metadata.copyright.as_ref()),
            designer: option_string_to_c_char(pattern.metadata.designer.as_ref()),
            designer_url: option_string_to_c_char(pattern.metadata.designer_url.as_ref()),
            font_family: option_string_to_c_char(pattern.metadata.font_family.as_ref()),
            font_subfamily: option_string_to_c_char(pattern.metadata.font_subfamily.as_ref()),
            full_name: option_string_to_c_char(pattern.metadata.full_name.as_ref()),
            id_description: option_string_to_c_char(pattern.metadata.id_description.as_ref()),
            license: option_string_to_c_char(pattern.metadata.license.as_ref()),
            license_url: option_string_to_c_char(pattern.metadata.license_url.as_ref()),
            manufacturer: option_string_to_c_char(pattern.metadata.manufacturer.as_ref()),
            manufacturer_url: option_string_to_c_char(pattern.metadata.manufacturer_url.as_ref()),
            postscript_name: option_string_to_c_char(pattern.metadata.postscript_name.as_ref()),
            preferred_family: option_string_to_c_char(pattern.metadata.preferred_family.as_ref()),
            preferred_subfamily: option_string_to_c_char(
                pattern.metadata.preferred_subfamily.as_ref(),
            ),
            trademark: option_string_to_c_char(pattern.metadata.trademark.as_ref()),
            unique_id: option_string_to_c_char(pattern.metadata.unique_id.as_ref()),
            version: option_string_to_c_char(pattern.metadata.version.as_ref()),
        });

        Box::into_raw(metadata)
    }
}

/// Create a new in-memory font
#[no_mangle]
pub extern "C" fn fc_font_new(
    bytes: *const u8,
    bytes_len: usize,
    font_index: usize,
    id: *const c_char,
) -> *mut FcFontC {
    if bytes.is_null() || bytes_len == 0 || id.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let id_rust = CStr::from_ptr(id).to_string_lossy().into_owned();
        let bytes_vec = slice::from_raw_parts(bytes, bytes_len).to_vec();

        let bytes_ptr = Box::into_raw(bytes_vec.into_boxed_slice()) as *mut u8;

        let font = FcFontC {
            bytes: bytes_ptr,
            bytes_len,
            font_index,
            id: CString::new(id_rust).unwrap_or_default().into_raw(),
        };

        Box::into_raw(Box::new(font))
    }
}

/// Get all available fonts in the cache
#[no_mangle]
pub extern "C" fn fc_cache_list_fonts(
    cache: *const FcFontCache,
    count: *mut usize,
) -> *mut FcFontInfoC {
    if cache.is_null() || count.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let cache = &*cache;
        let font_list = cache.list();

        if font_list.is_empty() {
            *count = 0;
            return ptr::null_mut();
        }

        let mut font_info = Vec::with_capacity(font_list.len());

        for (pattern, id) in font_list {
            let name = option_string_to_c_char(pattern.name.as_ref());
            let family = option_string_to_c_char(pattern.family.as_ref());

            font_info.push(FcFontInfoC {
                id: FcFontIdC::from_fontid(&id),
                name,
                family,
            });
        }

        *count = font_info.len();
        let ptr = font_info.as_mut_ptr();
        mem::forget(font_info);

        ptr
    }
}

/// Add in-memory fonts to the cache
#[no_mangle]
pub extern "C" fn fc_cache_add_memory_fonts(
    cache: *mut FcFontCache,
    patterns: *const FcPatternC,
    fonts: *const FcFontC,
    count: usize,
) {
    if cache.is_null() || patterns.is_null() || fonts.is_null() || count == 0 {
        return;
    }

    unsafe {
        let cache = &mut *cache;
        let patterns_slice = slice::from_raw_parts(patterns, count);
        let fonts_slice = slice::from_raw_parts(fonts, count);

        let mut memory_fonts = Vec::with_capacity(count);

        for i in 0..count {
            let pattern = c_to_pattern(&patterns_slice[i]);
            let font = &fonts_slice[i];

            let font_id = c_char_to_option_string(font.id).unwrap_or_default();
            let bytes = if font.bytes.is_null() || font.bytes_len == 0 {
                Vec::new()
            } else {
                slice::from_raw_parts(font.bytes, font.bytes_len).to_vec()
            };

            memory_fonts.push((
                pattern,
                FcFont {
                    bytes,
                    font_index: font.font_index,
                    id: font_id,
                },
            ));
        }

        cache.with_memory_fonts(memory_fonts);
    }
}

/// Query all fonts matching a pattern
#[no_mangle]
pub extern "C" fn fc_cache_query_all(
    cache: *const FcFontCache,
    pattern: *const FcPatternC,
    trace: *mut *mut FcTraceMsgC,
    trace_count: *mut usize,
    matches_count: *mut usize,
) -> *mut *mut FcFontMatchC {
    if cache.is_null()
        || pattern.is_null()
        || trace.is_null()
        || trace_count.is_null()
        || matches_count.is_null()
    {
        return ptr::null_mut();
    }

    unsafe {
        let cache = &*cache;
        let pattern_rust = c_to_pattern(pattern);

        let mut trace_msgs = Vec::new();
        let results = cache.query_all(&pattern_rust, &mut trace_msgs);

        // Convert trace messages
        let (trace_c, count) = trace_msgs_to_c(&trace_msgs);
        *trace = trace_c;
        *trace_count = count;

        if results.is_empty() {
            *matches_count = 0;
            return ptr::null_mut();
        }

        // Convert results to C representation
        let mut matches_c = Vec::with_capacity(results.len());
        for match_obj in &results {
            let match_c = font_match_to_c(match_obj);
            matches_c.push(Box::into_raw(Box::new(match_c)));
        }

        *matches_count = matches_c.len();
        let ptr = matches_c.as_mut_ptr();
        mem::forget(matches_c);

        ptr
    }
}

/// Query fonts for text
#[no_mangle]
pub extern "C" fn fc_cache_query_for_text(
    cache: *const FcFontCache,
    pattern: *const FcPatternC,
    text: *const c_char,
    trace: *mut *mut FcTraceMsgC,
    trace_count: *mut usize,
    matches_count: *mut usize,
) -> *mut *mut FcFontMatchC {
    if cache.is_null()
        || pattern.is_null()
        || text.is_null()
        || trace.is_null()
        || trace_count.is_null()
        || matches_count.is_null()
    {
        return ptr::null_mut();
    }

    unsafe {
        let cache = &*cache;
        let pattern_rust = c_to_pattern(pattern);
        let text_rust = CStr::from_ptr(text).to_string_lossy().into_owned();

        let mut trace_msgs = Vec::new();
        let results = cache.query_for_text(&pattern_rust, &text_rust, &mut trace_msgs);

        // Convert trace messages
        let (trace_c, count) = trace_msgs_to_c(&trace_msgs);
        *trace = trace_c;
        *trace_count = count;

        if results.is_empty() {
            *matches_count = 0;
            return ptr::null_mut();
        }

        // Convert results to C representation
        let mut matches_c = Vec::with_capacity(results.len());
        for match_obj in &results {
            let match_c = font_match_to_c(match_obj);
            matches_c.push(Box::into_raw(Box::new(match_c)));
        }

        *matches_count = matches_c.len();
        let ptr = matches_c.as_mut_ptr();
        mem::forget(matches_c);

        ptr
    }
}
