//! Precomputed font fallback chains for resolving font stacks.
//!
//! A [`FontFallbackChain`] is a self-contained snapshot that maps a CSS font stack
//! to the exact fonts that will be used for any character, without requiring
//! further metadata lookups during layout.
//!
//! Resolution order:
//!
//! 1. The CSS stack in order.
//! 2. The script fallback group whose block contains the character.
//! 3. The configured last resort, without a coverage check.

use alloc::collections::BTreeSet;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::config::{FcFallbackConfig, GenericFamily};
use crate::{
    FcFontCache, FcFontCacheInner, FcPattern, FcWeight, FontId, FontMatch, FontMatchNoFallback,
    PatternMatch, ResolvedFontRun, TraceMsg, UnicodeRange,
    DEFAULT_UNICODE_FALLBACK_SCRIPTS,
};

/// How many faces of one family a chain keeps (Regular, Bold, Italic, …
/// ranked by closeness to the requested style).
pub const MAX_FACES_PER_FAMILY: usize = 5;

/// How many coverage-ranked fonts a script fallback group keeps beyond the
/// configured preferences.
pub const MAX_AUTO_FALLBACKS_PER_SCRIPT: usize = 4;

/// `css_source` reported by [`FontFallbackChain::resolve_char`] for a font
/// taken from [`FontFallbackChain::unicode_fallbacks`].
pub const UNICODE_FALLBACK_SOURCE: &str = "(unicode-fallback)";

/// `css_source` reported by [`FontFallbackChain::resolve_char`] for the
/// configured last resort.
pub const LAST_RESORT_SOURCE: &str = "(last-resort)";

/// Fonts to try for characters inside one Unicode block, best first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptFallbackGroup {
    /// The block this group serves. Groups of one chain do not overlap.
    pub range: UnicodeRange,
    /// Candidates in order. A font may appear in several groups — one per
    /// block it covers.
    pub fonts: Vec<FontMatch>,
}

/// The fonts one entry of the CSS font stack resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssFallbackGroup {
    /// The CSS font-family entry as the caller wrote it.
    pub css_name: String,
    /// Base candidates, best style match first.
    pub fonts: Vec<FontMatch>,
    /// Per-script preferred fonts (used by generic families).
    pub script_fonts: Vec<ScriptFallbackGroup>,
}

/// A resolved font fallback chain for one CSS font stack and style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFallbackChain {
    /// The CSS stack, entry by entry.
    pub css_fallbacks: Vec<CssFallbackGroup>,
    /// One group per requested script block that has any candidate. Fonts
    /// already reachable through `css_fallbacks` are not repeated here.
    pub unicode_fallbacks: Vec<ScriptFallbackGroup>,
    /// [`FcFallbackConfig::last_resort`], resolved. The first entry is used
    /// for any character nothing above covers.
    pub last_resort: Vec<FontMatch>,
    /// The stack as requested, unexpanded.
    pub original_stack: Vec<String>,
}

impl FontFallbackChain {
    /// A chain that resolves nothing, for `stack`.
    pub fn empty(stack: &[String]) -> Self {
        Self {
            css_fallbacks: Vec::new(),
            unicode_fallbacks: Vec::new(),
            last_resort: Vec::new(),
            original_stack: stack.to_vec(),
        }
    }

    /// Returns the resolved font for a given codepoint and the CSS fallback tier it came from.
    pub fn resolve_codepoint(&self, cp: u32) -> Option<(FontId, &str)> {
        for group in &self.css_fallbacks {
            for script in &group.script_fonts {
                if script.range.start <= cp && cp <= script.range.end {
                    if let Some(m) = script.fonts.iter().find(|m| covers(&m.unicode_ranges, cp)) {
                        return Some((m.id, group.css_name.as_str()));
                    }
                }
            }
            if let Some(m) = group.fonts.iter().find(|m| covers(&m.unicode_ranges, cp)) {
                return Some((m.id, group.css_name.as_str()));
            }
        }
        for script in &self.unicode_fallbacks {
            if script.range.start <= cp && cp <= script.range.end {
                if let Some(m) = script.fonts.iter().find(|m| covers(&m.unicode_ranges, cp)) {
                    return Some((m.id, UNICODE_FALLBACK_SOURCE));
                }
            }
        }
        self.last_resort.first().map(|m| (m.id, LAST_RESORT_SOURCE))
    }

    /// Similar to `resolve_codepoint`, but takes a `char` and unused cache reference
    /// for backward compatibility with 4.x.
    pub fn resolve_char(&self, _cache: &FcFontCache, ch: char) -> Option<(FontId, String)> {
        self.resolve_codepoint(ch as u32)
            .map(|(id, source)| (id, source.to_string()))
    }

    /// Per-character resolution of `text`.
    pub fn resolve_text(
        &self,
        cache: &FcFontCache,
        text: &str,
    ) -> Vec<(char, Option<(FontId, String)>)> {
        text.chars()
            .map(|ch| (ch, self.resolve_char(cache, ch)))
            .collect()
    }

    /// Split `text` into runs of consecutive characters that resolve to the
    /// same font. This is the shaping entry point: shape each run with its
    /// font. A run whose `font_id` is `None` has no font in the chain.
    pub fn query_for_text(&self, _cache: &FcFontCache, text: &str) -> Vec<ResolvedFontRun> {
        let mut runs: Vec<ResolvedFontRun> = Vec::new();
        let mut current: Option<(Option<FontId>, &str)> = None;
        let mut run_start = 0usize;

        for (byte_idx, ch) in text.char_indices() {
            let resolved = match self.resolve_codepoint(ch as u32) {
                Some((id, source)) => (Some(id), source),
                None => (None, ""),
            };
            match current {
                Some((font, _)) if font == resolved.0 => {}
                Some((font, source)) => {
                    runs.push(ResolvedFontRun {
                        text: text[run_start..byte_idx].to_string(),
                        start_byte: run_start,
                        end_byte: byte_idx,
                        font_id: font,
                        css_source: source.to_string(),
                    });
                    run_start = byte_idx;
                    current = Some(resolved);
                }
                None => current = Some(resolved),
            }
        }

        if let Some((font, source)) = current {
            if run_start < text.len() {
                runs.push(ResolvedFontRun {
                    text: text[run_start..].to_string(),
                    start_byte: run_start,
                    end_byte: text.len(),
                    font_id: font,
                    css_source: source.to_string(),
                });
            }
        }

        runs
    }

    /// Every font in the chain, in resolution order, each once.
    pub fn fonts(&self) -> impl Iterator<Item = &FontMatch> {
        let mut seen = BTreeSet::new();
        self.css_fallbacks
            .iter()
            .flat_map(|g| {
                g.script_fonts
                    .iter()
                    .flat_map(|s| s.fonts.iter())
                    .chain(g.fonts.iter())
            })
            .chain(self.unicode_fallbacks.iter().flat_map(|s| s.fonts.iter()))
            .chain(self.last_resort.iter())
            .filter(move |m| seen.insert(m.id))
    }
}

/// Memo key for resolved chains. The scripts are hashed in canonical form,
/// so `None` and an explicit default set share a slot and order does not
/// matter.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct FontChainCacheKey {
    pub(crate) font_families: Vec<String>,
    pub(crate) weight: FcWeight,
    pub(crate) italic: PatternMatch,
    pub(crate) oblique: PatternMatch,
    pub(crate) scripts_hash: u64,
}

fn hash_scripts(ranges: &[UnicodeRange]) -> u64 {
    let mut sorted: Vec<UnicodeRange> = ranges.to_vec();
    sorted.sort();
    sorted.dedup();
    let mut buf = Vec::with_capacity(sorted.len() * 8);
    for r in &sorted {
        buf.extend_from_slice(&r.start.to_le_bytes());
        buf.extend_from_slice(&r.end.to_le_bytes());
    }
    crate::utils::content_hash_u64(&buf)
}

/// The ordering used wherever font candidates compete. Smaller is better.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RankKey {
    /// Number of requested codepoints the font does not cover.
    pub deficit: u32,
    /// Style score against the request (smaller is closer).
    pub style: i32,
    /// 0 for upright, 1 for italic (prefers upright on ties).
    pub italic: u8,
    /// How much of the font's coverage lies outside the requested block (prefers dedicated fonts).
    pub dedication_inv: u64,
    /// Total codepoint coverage (prefers narrower fonts on ties).
    pub breadth: u64,
    /// Font name, for deterministic tie-breaking.
    pub name: String,
}

impl RankKey {
    /// Rank `candidate` for characters of `block` under `style`. `None` when
    /// the candidate covers nothing of the block.
    pub fn for_block(
        style: &FcPattern,
        candidate: &FcPattern,
        block: &UnicodeRange,
    ) -> Option<Self> {
        let overlap = overlap_size(&candidate.unicode_ranges, block);
        if overlap == 0 {
            return None;
        }
        let breadth = breadth(&candidate.unicode_ranges);
        Some(Self {
            deficit: range_width(block).saturating_sub(overlap),
            style: FcFontCache::calculate_style_score(style, candidate),
            italic: (candidate.italic == PatternMatch::True) as u8,
            dedication_inv: breadth.saturating_mul(1024) / overlap as u64,
            breadth,
            name: candidate.name.clone().unwrap_or_default(),
        })
    }

    /// Rank `candidate` based only on style. Narrower fonts win ties.
    pub fn for_style(style: &FcPattern, candidate: &FcPattern) -> Self {
        Self {
            deficit: 0,
            style: FcFontCache::calculate_style_score(style, candidate),
            italic: (candidate.italic == PatternMatch::True) as u8,
            dedication_inv: 0,
            breadth: breadth(&candidate.unicode_ranges),
            name: candidate.name.clone().unwrap_or_default(),
        }
    }

    /// Rank `candidate` against a requested range set by style and missing coverage.
    pub fn for_request(
        style: &FcPattern,
        candidate: &FcPattern,
        requested: &[UnicodeRange],
    ) -> Self {
        let (requested_width, overlap) = requested.iter().fold((0u32, 0u32), |(w, o), r| {
            (
                w.saturating_add(range_width(r)),
                o.saturating_add(overlap_size(&candidate.unicode_ranges, r)),
            )
        });
        Self {
            deficit: 0,
            style: FcFontCache::calculate_style_score(style, candidate),
            italic: (candidate.italic == PatternMatch::True) as u8,
            dedication_inv: requested_width.saturating_sub(overlap) as u64,
            breadth: breadth(&candidate.unicode_ranges),
            name: candidate.name.clone().unwrap_or_default(),
        }
    }
}

/// Returns `true` if `ranges` contains `cp`. `ranges` must be sorted and disjoint.
pub fn covers(ranges: &[UnicodeRange], cp: u32) -> bool {
    let i = ranges.partition_point(|r| r.end < cp);
    ranges.get(i).is_some_and(|r| r.start <= cp)
}

/// Width of a range in codepoints.
pub fn range_width(r: &UnicodeRange) -> u32 {
    r.end.saturating_sub(r.start).saturating_add(1)
}

/// Number of codepoints in `block` that `ranges` cover, capped at the block's width.
pub fn overlap_size(ranges: &[UnicodeRange], block: &UnicodeRange) -> u32 {
    let mut total = 0u32;
    let first = ranges.partition_point(|r| r.end < block.start);
    for r in &ranges[first..] {
        if r.start > block.end {
            break;
        }
        let start = r.start.max(block.start);
        let end = r.end.min(block.end);
        if start <= end {
            total = total.saturating_add(end - start + 1);
        }
    }
    total.min(range_width(block))
}

/// Total coverage in codepoints.
pub fn breadth(ranges: &[UnicodeRange]) -> u64 {
    ranges.iter().map(|r| range_width(r) as u64).sum()
}

fn style_request(weight: FcWeight, italic: PatternMatch, oblique: PatternMatch) -> FcPattern {
    FcPattern {
        weight,
        italic,
        oblique,
        ..Default::default()
    }
}

fn font_match(id: FontId, meta: &FcPattern) -> FontMatch {
    FontMatch {
        id,
        unicode_ranges: meta.unicode_ranges.clone(),
        fallbacks: Vec::new(),
    }
}

fn ids(fonts: &[FontMatch]) -> impl Iterator<Item = FontId> + '_ {
    fonts.iter().map(|m| m.id)
}

/// The installed faces of `family` (normalized name equality), best style
/// first, skipping `exclude`.
fn faces_for_family(
    state: &FcFontCacheInner,
    family: &str,
    style: &FcPattern,
    exclude: &BTreeSet<FontId>,
) -> Vec<FontMatch> {
    let key = crate::utils::normalize_family_name(family);
    if key.is_empty() {
        return Vec::new();
    }
    let Some(ids) = state.family_index.get(&key) else {
        return Vec::new();
    };
    let mut ranked: Vec<(RankKey, FontId, &FcPattern)> = ids
        .iter()
        .filter(|id| !exclude.contains(id))
        .filter_map(|id| {
            let meta = state.metadata.get(id)?;
            Some((RankKey::for_style(style, meta), *id, meta))
        })
        .collect();
    ranked.sort();
    ranked.truncate(MAX_FACES_PER_FAMILY);
    ranked
        .into_iter()
        .map(|(_, id, meta)| font_match(id, meta))
        .collect()
}

/// The faces of every family in `families`, in that order, each font once,
/// skipping `exclude`.
fn faces_for_families(
    state: &FcFontCacheInner,
    families: &[String],
    style: &FcPattern,
    exclude: &BTreeSet<FontId>,
) -> Vec<FontMatch> {
    let mut out: Vec<FontMatch> = Vec::new();
    let mut seen = exclude.clone();
    for family in families {
        for m in faces_for_family(state, family, style, &seen) {
            seen.insert(m.id);
            out.push(m);
        }
    }
    out
}

/// Finds the best faces matching the requested style globally, ignoring names.
fn faces_unconfigured(state: &FcFontCacheInner, style: &FcPattern) -> Vec<FontMatch> {
    let mut ranked: Vec<(RankKey, FontId, &FcPattern)> = state
        .metadata
        .iter()
        .map(|(id, meta)| (RankKey::for_style(style, meta), *id, meta))
        .collect();
    ranked.sort();
    ranked.truncate(MAX_FACES_PER_FAMILY);
    ranked
        .into_iter()
        .map(|(_, id, meta)| font_match(id, meta))
        .collect()
}

/// Every registered font that covers any of `block`, best first by
/// [`RankKey::for_block`], at most `limit`, skipping `exclude`.
fn ranked_coverage_candidates(
    state: &FcFontCacheInner,
    block: &UnicodeRange,
    style: &FcPattern,
    exclude: &BTreeSet<FontId>,
    limit: usize,
) -> Vec<FontMatch> {
    let mut ranked: Vec<(RankKey, FontId, &FcPattern)> = state
        .metadata
        .iter()
        .filter(|(id, _)| !exclude.contains(id))
        .filter_map(|(id, meta)| Some((RankKey::for_block(style, meta, block)?, *id, meta)))
        .collect();
    ranked.sort();
    ranked.truncate(limit);
    ranked
        .into_iter()
        .map(|(_, id, meta)| font_match(id, meta))
        .collect()
}

/// Build the chain for `stack` from the cache state and `config`. Pure over
/// its inputs; the caller holds the read guard.
pub(crate) fn build_chain(
    state: &FcFontCacheInner,
    config: &FcFallbackConfig,
    stack: &[String],
    style: &FcPattern,
    scripts: &[UnicodeRange],
) -> FontFallbackChain {
    let mut css_base: BTreeSet<FontId> = BTreeSet::new();
    let mut css_all: BTreeSet<FontId> = BTreeSet::new();
    let mut generics_in_stack: Vec<GenericFamily> = Vec::new();
    let mut css_fallbacks: Vec<CssFallbackGroup> = Vec::with_capacity(stack.len());

    for family in stack {
        match GenericFamily::from_css(family) {
            Some(generic) => {
                if !generics_in_stack.contains(&generic) {
                    generics_in_stack.push(generic);
                }
                let fonts =
                    faces_for_families(state, config.generic_candidates(generic), style, &css_base);
                css_base.extend(ids(&fonts));
                css_all.extend(ids(&fonts));

                let mut script_fonts = Vec::new();
                for block in scripts {
                    let names = config.script_candidates(Some(generic), block);
                    let fonts = faces_for_families(state, &names, style, &css_base);
                    if !fonts.is_empty() {
                        css_all.extend(ids(&fonts));
                        script_fonts.push(ScriptFallbackGroup {
                            range: *block,
                            fonts,
                        });
                    }
                }
                css_fallbacks.push(CssFallbackGroup {
                    css_name: family.clone(),
                    fonts,
                    script_fonts,
                });
            }
            None => {
                let mut fonts = faces_for_family(state, family, style, &css_base);
                if fonts.is_empty() {
                    fonts = faces_for_families(
                        state,
                        config.substitutions_for(family),
                        style,
                        &css_base,
                    );
                }
                css_base.extend(ids(&fonts));
                css_all.extend(ids(&fonts));
                css_fallbacks.push(CssFallbackGroup {
                    css_name: family.clone(),
                    fonts,
                    script_fonts: Vec::new(),
                });
            }
        }
    }

    // If the stack matched nothing, fallback to best unconfigured fonts.
    if css_all.is_empty() {
        for group in &mut css_fallbacks {
            if GenericFamily::from_css(&group.css_name).is_some() {
                group.fonts = faces_unconfigured(state, style);
                css_all.extend(ids(&group.fonts));
            }
        }
    }

    // Script tier: Uses preferences from stack generics or the default generic.
    let preference_generics: Vec<GenericFamily> = if generics_in_stack.is_empty() {
        alloc::vec![config.default_generic]
    } else {
        generics_in_stack
    };
    let mut unicode_fallbacks: Vec<ScriptFallbackGroup> = Vec::new();
    for block in scripts {
        let mut fonts: Vec<FontMatch> = Vec::new();
        let mut seen = css_all.clone();
        let mut take = |found: Vec<FontMatch>, seen: &mut BTreeSet<FontId>| {
            for m in found {
                seen.insert(m.id);
                fonts.push(m);
            }
        };
        for generic in &preference_generics {
            let names = config.script_candidates(Some(*generic), block);
            take(faces_for_families(state, &names, style, &seen), &mut seen);
        }
        let names = config.script_candidates(None, block);
        take(faces_for_families(state, &names, style, &seen), &mut seen);
        take(
            ranked_coverage_candidates(state, block, style, &seen, MAX_AUTO_FALLBACKS_PER_SCRIPT),
            &mut seen,
        );
        if !fonts.is_empty() {
            unicode_fallbacks.push(ScriptFallbackGroup {
                range: *block,
                fonts,
            });
        }
    }

    let last_resort = faces_for_families(state, &config.last_resort, style, &BTreeSet::new());

    FontFallbackChain {
        css_fallbacks,
        unicode_fallbacks,
        last_resort,
        original_stack: stack.to_vec(),
    }
}

impl FcFontCache {
    /// Resolve a fallback chain for a CSS font stack with the default script set.
    pub fn resolve_font_chain(
        &self,
        font_families: &[String],
        weight: FcWeight,
        italic: PatternMatch,
        oblique: PatternMatch,
        trace: &mut Vec<TraceMsg>,
    ) -> FontFallbackChain {
        self.resolve_font_chain_with_scripts(font_families, weight, italic, oblique, None, trace)
    }

    /// Resolve a fallback chain, building script fallback groups for the requested unicode blocks.
    pub fn resolve_font_chain_with_scripts(
        &self,
        font_families: &[String],
        weight: FcWeight,
        italic: PatternMatch,
        oblique: PatternMatch,
        scripts_hint: Option<&[UnicodeRange]>,
        _trace: &mut Vec<TraceMsg>,
    ) -> FontFallbackChain {
        let scripts = scripts_hint.unwrap_or(DEFAULT_UNICODE_FALLBACK_SCRIPTS);
        let key = FontChainCacheKey {
            font_families: font_families.to_vec(),
            weight,
            italic,
            oblique,
            scripts_hash: hash_scripts(scripts),
        };

        {
            let memo = match self.shared.chain_cache.lock() {
                Ok(m) => m,
                Err(e) => match e {},
            };
            if let Some(chain) = memo.get(&key) {
                return chain.clone();
            }
        }

        let chain = {
            let state = self.state_read();
            build_chain(
                &state,
                &state.fallback_config,
                font_families,
                &style_request(weight, italic, oblique),
                scripts,
            )
        };

        let mut memo = match self.shared.chain_cache.lock() {
            Ok(m) => m,
            Err(e) => match e {},
        };
        memo.insert(key, chain.clone());
        chain
    }

    /// Finds all registered fonts covering part of `font_id`'s coverage, ranked by closeness.
    pub fn compute_fallbacks(
        &self,
        font_id: &FontId,
        _trace: &mut Vec<TraceMsg>,
    ) -> Vec<FontMatchNoFallback> {
        let state = self.state_read();
        let Some(pattern) = state.metadata.get(font_id) else {
            return Vec::new();
        };
        let requested: &[UnicodeRange] = if pattern.unicode_ranges.is_empty() {
            DEFAULT_UNICODE_FALLBACK_SCRIPTS
        } else {
            &pattern.unicode_ranges
        };
        let mut ranked: Vec<(RankKey, FontId, &FcPattern)> = state
            .metadata
            .iter()
            .filter(|(id, meta)| {
                *id != font_id
                    && requested
                        .iter()
                        .any(|r| overlap_size(&meta.unicode_ranges, r) > 0)
            })
            .map(|(id, meta)| (RankKey::for_request(pattern, meta, requested), *id, meta))
            .collect();
        ranked.sort();
        ranked
            .into_iter()
            .map(|(_, id, meta)| FontMatchNoFallback {
                id,
                unicode_ranges: meta.unicode_ranges.clone(),
            })
            .collect()
    }
}
