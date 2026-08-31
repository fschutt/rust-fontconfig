//! Precomputed font fallback chains.
//!
//! fontconfig answers "which font?" per query: the application itemizes
//! text into runs, asks `FcFontSort` for each run's language, and walks the
//! sorted list. This crate is used by layout engines that do the opposite:
//! they ask **once** per CSS font stack — possibly before the layout pass,
//! possibly while the async registry is still parsing fonts — and then
//! resolve every character of a mixed-script document against the result
//! without touching the cache again. That changes the *shape* of the answer,
//! not the rules behind it:
//!
//! * A [`FontFallbackChain`] is a **self-contained snapshot**. Every
//!   [`FontMatch`] carries the coverage it had when the chain was built, and
//!   [`FontFallbackChain::resolve_char`] reads nothing else — no lock, no
//!   metadata lookup, no allocation per character.
//! * Because one chain serves every script in the document, the per-script
//!   ordering that fontconfig gets from the query's `lang` has to live
//!   **inside** the chain: generic families carry per-script preferred fonts
//!   ([`CssFallbackGroup::script_fonts`]) and the coverage-gated fallbacks are
//!   grouped per script block ([`FontFallbackChain::unicode_fallbacks`]).
//! * The `scripts_hint` bounds the precomputation. A chain built for
//!   `Some(&[])` carries no script tier at all; `None` uses
//!   [`DEFAULT_UNICODE_FALLBACK_SCRIPTS`].
//!
//! Resolution order for a character, first hit wins:
//!
//! 1. The CSS stack in order. For a generic family, the preferred fonts of
//!    the character's script come before the family's base fonts (this is
//!    what browsers do: `sans-serif` is a per-script default, not one font).
//! 2. The script fallback group whose block contains the character:
//!    configured preferences first, then any registered font that covers the
//!    block, ranked by [`RankKey`].
//! 3. The configured last resort, **without** a coverage check — the font
//!    whose `.notdef` the embedder wants drawn. Empty by default, in which
//!    case the answer is `None`.
//!
//! Everything the chain builder knows about the host — which families stand
//! behind `sans-serif`, which fonts to prefer for Hiragana, what to
//! substitute for a missing `Arial`, what the last resort is — comes from the
//! injected [`FcFallbackConfig`]. There is no built-in table on this path;
//! [`FcFallbackConfig::os_defaults`] is the explicit opt-in.
//!
//! What is deliberately **not** a ranking signal anywhere in this module is
//! the breadth of a font's coverage. Ranking by "covers the most blocks"
//! selects the pan-Unicode bitmap font for every script it happens to cover
//! (GitHub issue #26). Coverage gates; it does not score. Among fonts that
//! cover a block equally well, the more *dedicated* font wins — the one whose
//! coverage is mostly that block — and a narrower font beats a wider one.

use alloc::collections::BTreeSet;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::config::{FcFallbackConfig, GenericFamily};
use crate::{
    FcFontCache, FcFontCacheInner, FcPattern, FcWeight, FontId, FontMatch, FontMatchNoFallback,
    OperatingSystem, PatternMatch, ResolvedFontRun, TraceMsg, UnicodeRange,
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
    /// The CSS font-family entry as the caller wrote it (`"Arial"`,
    /// `"sans-serif"`).
    pub css_name: String,
    /// Base candidates, best style match first. For a named family these are
    /// its faces (or its configured substitutions when it is not installed);
    /// for a generic family, the configured candidates for that generic.
    pub fonts: Vec<FontMatch>,
    /// Per-script preferred fonts. Only generic families have these; they
    /// are consulted before `fonts` for characters inside their block.
    pub script_fonts: Vec<ScriptFallbackGroup>,
}

/// A resolved font fallback chain for one CSS font stack and style.
///
/// See the [module documentation](self) for the resolution order. The chain
/// is a snapshot: fonts registered after it was built are not in it. The
/// cache invalidates its chain memo on every insert, so re-resolving the
/// same stack after a registry scan picks them up.
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

    /// The font for `cp`, with the name of the tier it came from: a CSS
    /// family name, [`UNICODE_FALLBACK_SOURCE`] or [`LAST_RESORT_SOURCE`].
    ///
    /// Pure and allocation-free; reads only the chain.
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

    /// The font for `ch` and the tier it came from. `None` when no font in
    /// the chain covers it and no last resort is configured.
    ///
    /// The `cache` argument is unused: the chain is self-contained. It is
    /// kept so 4.x call sites compile unchanged.
    pub fn resolve_char(&self, _cache: &FcFontCache, ch: char) -> Option<(FontId, String)> {
        self.resolve_codepoint(ch as u32)
            .map(|(id, source)| (id, source.to_string()))
    }

    /// Per-character resolution of `text`.
    pub fn resolve_text(&self, cache: &FcFontCache, text: &str) -> Vec<(char, Option<(FontId, String)>)> {
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

/// The one ordering used wherever candidates compete. Smaller is better;
/// fields compare in declaration order.
///
/// * `deficit` — codepoints of the requested block the font does **not**
///   cover. Coverage of *this* block, never of everything.
/// * `style` — [`FcFontCache::calculate_style_score`] against the request.
/// * `italic` — upright before italic when everything else ties.
/// * `dedication_inv` — how much of the font's total coverage lies outside
///   the block, scaled. A font that is mostly this script beats a
///   pan-Unicode font that merely includes it.
/// * `breadth` — total coverage; narrower wins remaining ties (a text face
///   over a symbol-and-everything face).
/// * `name` — deterministic last word.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RankKey {
    pub deficit: u32,
    pub style: i32,
    pub italic: u8,
    pub dedication_inv: u64,
    pub breadth: u64,
    pub name: String,
}

impl RankKey {
    /// Rank `candidate` for characters of `block` under `style`. `None` when
    /// the candidate covers nothing of the block.
    pub fn for_block(style: &FcPattern, candidate: &FcPattern, block: &UnicodeRange) -> Option<Self> {
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

    /// Rank `candidate` on style alone, narrower fonts first among ties.
    /// Used where no script is being asked for: faces of one family, and
    /// the stand-in for a generic family nothing is configured for.
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

    /// Rank `candidate` against a whole requested range set (the public
    /// `query` path): style first, then how much of the request it misses.
    pub fn for_request(style: &FcPattern, candidate: &FcPattern, requested: &[UnicodeRange]) -> Self {
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

/// `true` when `ranges` contain `cp`. An empty list covers nothing — a font
/// that reported no coverage is never assumed to draw everything.
pub fn covers(ranges: &[UnicodeRange], cp: u32) -> bool {
    ranges.iter().any(|r| r.start <= cp && cp <= r.end)
}

/// Width of a range in codepoints.
pub fn range_width(r: &UnicodeRange) -> u32 {
    r.end.saturating_sub(r.start).saturating_add(1)
}

/// Codepoints of `block` that `ranges` cover, capped at the block's width
/// (so overlapping input ranges cannot inflate it).
pub fn overlap_size(ranges: &[UnicodeRange], block: &UnicodeRange) -> u32 {
    let mut total = 0u32;
    for r in ranges {
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
    ranked.into_iter().map(|(_, id, meta)| font_match(id, meta)).collect()
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

/// The best faces in the whole cache for `style`, ignoring names — what a
/// generic family resolves to when nothing configured for it is installed.
/// Narrower fonts first among style ties: a text face over a
/// symbol-and-everything face.
fn faces_unconfigured(state: &FcFontCacheInner, style: &FcPattern) -> Vec<FontMatch> {
    let mut ranked: Vec<(RankKey, FontId, &FcPattern)> = state
        .metadata
        .iter()
        .map(|(id, meta)| (RankKey::for_style(style, meta), *id, meta))
        .collect();
    ranked.sort();
    ranked.truncate(MAX_FACES_PER_FAMILY);
    ranked.into_iter().map(|(_, id, meta)| font_match(id, meta)).collect()
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
    ranked.into_iter().map(|(_, id, meta)| font_match(id, meta)).collect()
}

/// Build the chain for `stack` from the cache state and `config`. Pure over
/// its inputs; the caller holds the read guard.
///
/// Deduplication follows resolution order: a base list skips fonts already
/// in an earlier base list; the script tier skips everything in the CSS
/// tier (those fonts are tried first anyway, with the same coverage check);
/// within one group every font appears once. A font may sit in several
/// script groups — one per block it covers — and the last resort is never
/// deduplicated, because it is consulted without a coverage check.
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
                let fonts = faces_for_families(state, config.generic_candidates(generic), style, &css_base);
                css_base.extend(ids(&fonts));
                css_all.extend(ids(&fonts));

                let mut script_fonts = Vec::new();
                for block in scripts {
                    let names = config.script_candidates(Some(generic), block);
                    let fonts = faces_for_families(state, &names, style, &css_base);
                    if !fonts.is_empty() {
                        css_all.extend(ids(&fonts));
                        script_fonts.push(ScriptFallbackGroup { range: *block, fonts });
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
                    fonts = faces_for_families(state, config.substitutions_for(family), style, &css_base);
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

    // A generic family always resolves to *something* while any font is
    // registered (CSS guarantees generics map to a font; fc-match never
    // fails). Only when the whole stack matched nothing, so an installed
    // named family or a configured generic is never shadowed by a guess.
    if css_all.is_empty() {
        for group in &mut css_fallbacks {
            if GenericFamily::from_css(&group.css_name).is_some() {
                group.fonts = faces_unconfigured(state, style);
                css_all.extend(ids(&group.fonts));
            }
        }
    }

    // Script tier. Preferences come from the stack's generics — already in
    // the CSS tier, so this adds nothing for them — or, for a stack without
    // a generic, from the configured default generic (fontconfig appends
    // `sans-serif` to every pattern that names no generic; same idea).
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
            unicode_fallbacks.push(ScriptFallbackGroup { range: *block, fonts });
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
    /// Resolve a fallback chain for a CSS font stack with the default script
    /// set ([`DEFAULT_UNICODE_FALLBACK_SCRIPTS`]).
    ///
    /// Equivalent to [`resolve_font_chain_with_scripts`](Self::resolve_font_chain_with_scripts)
    /// with `scripts_hint = None`. Chains are memoized per (stack, style,
    /// scripts); the memo is cleared whenever a font is inserted or the
    /// [`FcFallbackConfig`] changes.
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

    /// Resolve a fallback chain, building script fallback groups for exactly
    /// the blocks in `scripts_hint`.
    ///
    /// * `None` — [`DEFAULT_UNICODE_FALLBACK_SCRIPTS`].
    /// * `Some(&[])` — no script tier: an ASCII-only document pulls no CJK or
    ///   Arabic fonts into the chain.
    /// * `Some(blocks)` — usually the blocks the document's text actually
    ///   uses.
    ///
    /// Generic families resolve through the cache's [`FcFallbackConfig`]
    /// (see [`FcFontCache::set_fallback_config`]); named families through
    /// the family index, then their configured substitutions.
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

    /// Resolve a chain as if this cache were configured with
    /// [`FcFallbackConfig::os_defaults(os)`], keeping the cache's own
    /// substitutions and last resort. Bypasses the chain memo.
    ///
    /// Kept for 4.x callers; the operating system is no longer an input to
    /// resolution. Inject the configuration instead:
    /// `cache.set_fallback_config(FcFallbackConfig::os_defaults(os))`.
    #[deprecated(
        since = "5.0.0",
        note = "inject the tables: `cache.set_fallback_config(FcFallbackConfig::os_defaults(os))`, then `resolve_font_chain`"
    )]
    pub fn resolve_font_chain_with_os(
        &self,
        font_families: &[String],
        weight: FcWeight,
        italic: PatternMatch,
        oblique: PatternMatch,
        _trace: &mut Vec<TraceMsg>,
        os: OperatingSystem,
    ) -> FontFallbackChain {
        let state = self.state_read();
        let mut config = FcFallbackConfig::os_defaults(os);
        config.merge_defaults(&state.fallback_config);
        build_chain(
            &state,
            &config,
            font_families,
            &style_request(weight, italic, oblique),
            DEFAULT_UNICODE_FALLBACK_SCRIPTS,
        )
    }

    /// Fonts that can stand in for `font_id`: every registered font covering
    /// part of its coverage, ranked by [`RankKey::for_request`] over that
    /// coverage. Used by the C API; not needed for chain resolution.
    pub fn compute_fallbacks(&self, font_id: &FontId, _trace: &mut Vec<TraceMsg>) -> Vec<FontMatchNoFallback> {
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
                *id != font_id && requested.iter().any(|r| overlap_size(&meta.unicode_ranges, r) > 0)
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
