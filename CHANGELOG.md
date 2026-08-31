# Changelog

All notable changes to this project will be documented in this file.

## [5.0.0] - 2026-08-31

### Fixed

- **Fallback fonts are chosen per script from configuration, never by
  breadth of coverage (issue #26).** On Windows, `resolve_font_chain(["sans-serif"])`
  rendered Japanese with GNU Unifont. Two things conspired: the scripts hint
  never reached generic-family expansion (`expand_font_families(.., &[])`),
  so the per-script OS tables were dead code and no Japanese font entered
  the CSS chain; and `find_unicode_fallbacks` ranked every candidate by how
  many requested blocks it covered, then greedily set-covered the document —
  so the pan-Unicode font won every script it included, by construction.
  Making coverage measurement more accurate (4.4.8's cmap-authoritative
  coverage) made that worse, not better: Unifont's score became more
  legitimately dominant. The reporter's "it worked until I added Arabic" was
  a tie broken alphabetically.

  The chain is now built in three tiers, in resolution order, and the
  per-script structure lives *inside* the precomputed chain (this crate's
  callers resolve once per stack, ahead of layout and possibly while the
  async registry is still parsing, so there is no per-run query to carry a
  language the way fontconfig's `FcFontSort` does):

  1. The CSS stack. A generic family carries per-script preferred fonts
     (`CssFallbackGroup::script_fonts`, consulted before its base fonts for
     characters in that block — what browsers do with `sans-serif`).
  2. A coverage-gated group per requested script block
     (`FontFallbackChain::unicode_fallbacks: Vec<ScriptFallbackGroup>`):
     configured preferences first, then any registered font covering the
     block, ranked by one `fallback::RankKey` — coverage *of that block*,
     style closeness, upright before italic, dedication (how much of the
     font is this script), narrower before wider, name. Breadth is never a
     bonus; the dedicated font beats the everything-font.
  3. An explicit last resort (`FcFallbackConfig::last_resort`), used
     without a coverage check for whatever nothing else covers — the font
     whose `.notdef` the embedder wants drawn.

  The scripts hint bounds the precomputation: `Some(&[])` builds no script
  tier at all, `None` uses `DEFAULT_UNICODE_FALLBACK_SCRIPTS`.

- **Fonts loaded from the disk manifest were invisible to named-family
  lookup.** `load_from_disk_cache_at` never called `index_pattern_family`
  (4.5.0's family index), only the no-op token indexer. Every insert now
  goes through one function (`insert_disk_font` / `insert_memory_font`),
  so the four maps and the index cannot drift apart again.
  `insert_fast_pattern` also invalidates memoized chains now.

- **`build_complete` waits for in-flight parses.** The completion check
  was "scout done and queue empty": a builder that had popped the last
  job was still parsing it while another found the queue empty, won the
  transition, woke every waiter and persisted the manifest — short by up
  to N−1 fonts. The registry counts in-flight jobs under the queue lock
  and completes only when that count is zero. New:
  `FcFontRegistry::set_persist_on_complete(false)` for embedders that
  persist on their own schedule (and for tests).

- **Relative `fonts.conf` includes resolve like fontconfig.** The stock
  `<include ignore_missing="yes">conf.d</include>` was resolved against
  the process's working directory, so on an ordinary distribution the
  whole `conf.d` tree — where every `<alias>` lives — was silently
  skipped and the config-first expansion never saw it. `FcSystemConfig`
  (`parse_tree`, `from_system`) follows includes depth-first in document
  order, reads a directory's `[0-9]*.conf` files in name order, reads
  each file once, and resolves relative names against `$FONTCONFIG_PATH`
  and the root configuration directory; `prefix="relative"` is
  supported. The parser is compiled and tested on every platform
  (`xmlparser` is now pulled in by `parsing`), and
  `FcFontRegistry::new()` consults the configuration too — its `<dir>`
  entries join the scan and its aliases seed the fallback configuration.

- **`PatternMatch` values match the C header again.** Reordering the Rust
  variants had made `DontCare=0, True=1, False=2` while the header says
  `TRUE=0, FALSE=1, DONT_CARE=2`; the enum crosses the boundary by value,
  so every C caller's `FC_MATCH_FALSE` arrived as `True`. Discriminants
  are explicit now and a test parses the header to hold them in place.

- **The disk-cache tests run on a CI row where threads exist.**
  `--all-features` enables `single-thread-unsafe-locks`, under which no
  scout is ever spawned, and that was the only row running
  `tests/disk_cache_persistence.rs` — so it timed out everywhere. The
  file is compiled out under that feature and the matrix gains an
  explicit `cache,async-registry,parsing` row.

- **One record per font.** The cache kept a second copy of every pattern
  in a map keyed by the whole `FcPattern`, and that copy was what
  `list()`, `len()`, `query()` and the registry read. Two different files
  with identical name tables collapsed into one entry (the last insert
  won, the first id was orphaned), and every insert compared seventeen
  metadata strings. `metadata` is the one record store; duplicates are
  decided on insert — a disk font by (file, face, pattern), a memory font
  by (pattern, face, bytes) — and both insert paths return the id the
  font is registered under. `list()` is in registration order now.

- **`FcFontRenderConfig` equality and ordering agree.** A derived
  `PartialEq`/`PartialOrd` (floats) next to a hand-written `Ord` (bit
  patterns) disagreed on NaN and failed clippy's
  `derive_ord_xor_partial_ord`; all three go through one `cmp`.

- **No lost wake-ups, no leaked threads.** `scan_complete` and
  `build_complete` were stored and notified without the mutex the
  waiters check them under, so a notify between a waiter's check and its
  wait was lost and the waiter slept to its deadline (up to 5 s). Both
  are published under `completed_paths` now. Builders held an
  `Arc<FcFontRegistry>`, so `Drop` never ran and in lazy mode N threads
  polled for the life of the process; they hold a `Weak` and exit within
  one step of the last handle being dropped.

- **The font-file walk is cycle-safe.** Both scanners recursed through
  directory symlinks unguarded; a link cycle overflowed the thread's
  stack, which aborts the process. `utils::collect_font_files` visits
  each directory once by canonical path, bounds the depth, and keeps font
  files by extension — the synchronous scan no longer opens and mmaps
  every regular file under its roots.

- **Panics never cross the C boundary.** Since Rust 1.81 an unwinding
  panic in an `extern "C"` frame aborts the process; every export now
  runs inside `catch_unwind` and returns null/false/zero on panic.
  `cargo clippy --all-features` passes.

### Added

- **`FcFallbackConfig` — the resolution side of `FcScanConfig`.** Everything
  the chain builder knows about the host is injected: `generic_families`
  (base candidates per `GenericFamily`), `substitutions` (missing named
  family → replacements), `script_fallbacks` (`FcScriptFallback { range,
  generic, families }`), `last_resort`, `default_generic`. There is no
  built-in table on the resolve path; `FcFallbackConfig::os_defaults(os)`
  is the explicit opt-in carrying the old tables (with kana preferring the
  Japanese font and Hangul the Korean one, where the old single CJK list
  could not distinguish). `FcFallbackConfig::empty()` configures nothing;
  `merge_defaults` fills only what a configuration leaves unsaid;
  `absorb_system_aliases` takes over parsed `fonts.conf` `<alias><prefer>`
  entries; `candidate_families(stack, scripts)` lists exactly the families
  a chain can contain, which is what the async registry now parses ahead
  of resolving — the two agree by construction.

  `FcFontCache::{fallback_config, set_fallback_config, with_fallback_config}`;
  `FcFontCache::default()` carries an empty configuration; `build()` and
  `build_with_families()` parse the platform aliases where there are any and
  fill the gaps from `os_defaults`. `FcFontRegistry::new_with_configs(scan,
  fallback)` injects both sides; `new_with_config(scan)` and `new()` keep
  their behaviour by opting into `os_defaults`.

- **`GenericFamily`**: the thirteen CSS Fonts 4 generics as a type, with
  `from_css` (case and separators ignored), `as_css`, and `parent` — the
  generic a less common one borrows configuration from. The three copies of
  the keyword list are gone; `is_generic_family` is `from_css(..).is_some()`.

- **`FontFallbackChain`**: `last_resort`, `empty(stack)`,
  `resolve_codepoint(cp) -> Option<(FontId, &str)>` (allocation-free),
  `fonts()` (every font once, in resolution order). `resolve_char`,
  `resolve_text` and `query_for_text` read only the chain — no lock, no
  metadata clone per character; the `cache` argument is kept so call sites
  compile unchanged. `CssFallbackGroup::script_fonts`.

### Changed

- **Coverage is exact: every codepoint the cmap maps, read from its
  segments.** It used to be a list of whole Unicode blocks, each marked
  covered when at least half of two to six sampled codepoints mapped to
  a glyph, from a fixed list of 50 BMP blocks, unioned with the OS/2
  `ulUnicodeRange` bits decoded through a table misaligned with the
  spec from bit 12 on. A face with three of six probed ideographs
  "covered" all 20,992; anything outside the list — Tibetan, Braille,
  CJK Extension A, every emoji, every astral script — was invisible.
  Format 4 is now read segment by segment, format 12 group by group,
  at a cost proportional to the segment count and no per-codepoint
  work; OS/2 is not consulted for coverage (fontconfig never did). The
  bit table and both probes are gone. Coverage is normalized on insert
  and `covers`/`overlap_size` binary-search it; `has_*_ranges` test
  overlap with the block. The manifest version is 3 — a v2 manifest is
  block-rounded and gets rescanned. Expect more ranges per font in
  `FcPattern::unicode_ranges` and a larger manifest.

- **`resolve_char` is honest again.** It returns `None` when no font in the
  chain covers the character and no last resort is configured. The
  "if exactly one font is registered, return it for everything" branch is
  gone; an embedder that wants that configures `last_resort`.
- **Public `query` uses the shared ranking**: memory fonts first, then
  style, then how much of the requested coverage the font misses, narrower
  before wider. It no longer prefers the widest font — `query(name: "Arial")`
  picks "Arial", not "Arial Unicode MS". `compute_fallbacks` ranks the same
  way over the font's own coverage.
- The chain memo key hashes the effective script set, so `None` and an
  explicit default set share a slot.

### Removed

- The token-fuzzy path: `fuzzy_query_by_name`, `token_index`, `font_tokens`
  and the no-op `index_pattern_tokens` (a no-op in every build since 4.4.3,
  so the path could not execute). `find_unicode_fallbacks`,
  `calculate_font_similarity_score`, `query_internal`, and the
  `system_aliases` cache state (now part of the configuration).
  `src/lib.rs` is 1,300 lines shorter; chain building lives in
  `src/fallback.rs`.

### Deprecated

Thin wrappers over `FcFallbackConfig::os_defaults`, removed in the next
major: `OperatingSystem::{get_serif_fonts, get_sans_serif_fonts,
get_monospace_fonts, expand_generic_family}`, `expand_font_families`,
`FcFontCache::{expand_font_families_config_first, system_alias_prefs,
resolve_font_chain_with_os}`.

### Tests

- `tests/issue_26_unicode_fallback_ranking.rs`: ten hermetic tests with
  three mock fonts shaped like a Windows box (Segoe UI, MS Gothic, Unifont)
  — the first tests in the crate where more than one font covers the same
  script and the assertion is *which one wins*. They cover the reporter's
  scenario, the unconfigured ranking, `Some(&[])`, configured preferences
  over ranking, the default generic for generic-less stacks, the last
  resort, substitutions, memo invalidation on config change, and that the
  registry's prefetch list covers the chain.

## [4.6.0] - 2026-08-29

### Added

- **Scan directories and priority families are injected, not invented.**
  The async registry used to decide for itself where fonts live and which
  families deserve parse priority, by reading the per-OS tables
  (`config::font_directories` / `config::common_font_families`) directly
  from the scout thread. Both tables are guesses, and a wrong guess is
  expensive: an embedder whose detected system UI font was not in the
  guessed priority list (Cantarell on GNOME, for one) paid the first
  layout in .notdef tofu while the builder pool chewed through every
  other font on disk. The host knows where its fonts live and which
  family its UI is about to ask for; this crate does not.

  New `config::FcScanConfig { font_dirs, priority_families }` carries
  that host knowledge, injected via
  `FcFontRegistry::new_with_config(FcScanConfig)`. The scout reads only
  the injected config. The old tables survive in exactly one place:
  `FcScanConfig::os_defaults(os)` is the explicitly-chosen fallback that
  wraps them, and `FcFontRegistry::new()` keeps its behavior by
  delegating to it. `FcScanConfig::empty()` scans nothing (memory fonts
  only); `FcScanConfig::priority_token_sets()` pre-tokenizes the
  families for the scout's matcher; `FcFontRegistry::scan_dirs()`
  exposes what a registry is configured to scan.

  The `config` free functions are unchanged and stay public - direct
  callers (and the synchronous `FcFontCache::build` paths) are not
  affected. Additive only: no existing item was renamed or removed.

## [4.5.0] - 2026-08-06

### Performance

- **Family lookup is an index probe, not a scan of every font.**
  `query_by_family_normalized` walked the entire pattern map and allocated
  a normalized `String` per face on every call. It is the only path a
  specific (non-generic) family name can take — `fuzzy_query_by_name` is a
  no-op on the azul web fork, so every name falls through to it — which made
  a lookup O(fonts) with two allocations each, hit or miss.

  A downstream measurement (azul, system font set): ~0.52 ms per lookup, and
  a CSS stack whose generics expand to the system's `<alias><prefer>` lists
  asks ~150 times, so 74 ms of a 177 ms cold document layout was spent here,
  most of it on families nobody has installed.

  `FcFontCacheInner` now carries `family_index: BTreeMap<String, Vec<FontId>>`,
  built at insertion from the normalized `family` and `name`. A lookup is one
  probe and a MISS costs nothing.

  Deliberately maintained by its own `index_pattern_family` rather than
  folded into `index_pattern_tokens`: that one is a no-op on the azul web
  fork (its unicode tokenizer traps under the lift), and the family index
  has to exist on every target or specific family names stop resolving.

  No API change — the version is bumped to 4.5.0 rather than a patch because
  the internal state layout changed, and consumers pinning `<4.5` should
  bump deliberately.

## [4.4.9] - 2026-07-29

### Fixed

- **A font without an OS/2 table was reported as not a font at all.**
  `parse_font_faces` read the table with

      let os2_data = provider.table_data(tag::OS_2).ok()??;

  so a missing OS/2 short-circuited the whole function and `FcParseFontBytes`
  returned `None` for the entire face — even though allsorts parses such fonts
  perfectly well. OS/2 is *optional* in TrueType; only OpenType requires it.

  This is not a hypothetical shape. printpdf embeds the 14 standard PDF fonts
  (Helvetica, Times, Courier, Symbol, ZapfDingbats) as TrueType subsets whose
  table directory is `cmap cvt fpgm glyf head hhea hmtx loca maxp name post
  prep` — no OS/2. All 14 failed to parse, so none of them could be registered
  as memory fonts, and `font-family: Helvetica` in printpdf's HTML renderer
  resolved to nothing and silently fell back to a substitute face.

  Nothing in the function actually needed OS/2. `head.macStyle` already
  supplied bold and italic twelve lines earlier, `post.isFixedPitch` plus the
  `hmtx` width scan cover monospace, and coverage has been cmap-authoritative
  since 4.4.8. OS/2 is now optional; when it is absent:

  - `weight` falls back to the `head.macStyle` bold bit, so the face lands on
    `Bold` or `Normal` rather than a precise weight class
  - `stretch` defaults to `Normal`, `oblique` to false
  - the `ulUnicodeRange` bits claim nothing, leaving coverage to come entirely
    from the cmap — which is where it already came from

  Faces that do carry an OS/2 table are unaffected: every value is read exactly
  as before.

## [4.4.8] - 2026-07-28

### Fixed

- **Font coverage was capped by OS/2's claims, so fonts were unmatchable for
  codepoints they can actually draw.** A font's `unicode_ranges` was
  effectively `(OS/2 ulUnicodeRange claims ∩ cmap reality)`:
  `verify_unicode_ranges_with_cmap` iterates the OS/2 ranges and keeps the ones
  the cmap confirms, so it can only ever *shrink* the set, and the full cmap
  analysis only ran when OS/2 was **all-zero**. Nothing could add a block the
  cmap covers but OS/2 failed to claim.

  Fonts get these bits wrong in both directions. Over-claiming was already
  handled; under-claiming was invisible. Noto Sans CJK's JP face has Hangul
  glyphs in its cmap but leaves the Hangul bits clear, so on a stock Ubuntu box
  with NotoSansCJK installed — where `fc-match :charset=D55C` happily answers
  "Noto Sans CJK JP" — this crate reported **zero** fonts covering Hangul and
  `한` resolved to no font at all. fontconfig has no such failure mode:
  `FcFreeTypeCharSet` walks the cmap and `ulUnicodeRange` never bounds coverage.

  Coverage is now cmap-authoritative. The over-claim pruning stays, then every
  block the cmap actually covers is unioned in; OS/2 is demoted to a hint that
  can only lose an argument with the cmap. On the same box: 0 → 30 fonts
  correctly reporting Hangul coverage.

  The two sources use different block boundaries, so the union overlaps — and
  `calculate_unicode_coverage` *ranks fallback candidates by summing range
  widths*, so an un-coalesced union would double-count and inflate scores (the
  failure shape where a CJK megafont wins a Latin run). Ranges are therefore
  coalesced into a sorted, disjoint set.

### Added

- `FcFontCache::query_with_fallback` — a **total** resolve, the `fc-match`
  contract. `query` stays fallible on purpose: it answers "was this exact
  request satisfiable?", which a caller reporting an unresolved font-family
  needs. A *renderer* must not be handed that `None` — either the text
  silently vanishes, or the caller invents its own fallback whose font is not
  registered where the renderer later looks it up by hash. Relaxation mirrors
  fontconfig's own: the pattern as given → `name`/`family` cleared but weight,
  slant, monospace and coverage kept (a Bold request must not silently become
  Regular) → coverage only. An empty cache is the only legitimate `None`.

- `FcFontCache::normalize_unicode_ranges` — coalesce ranges into a sorted,
  disjoint set, keeping `calculate_unicode_coverage` honest.

### Performance

- The cmap block probe now stops as soon as a block's verdict is decided
  instead of only on success, so blocks a font lacks — the common case, a Latin
  face covers a handful of the ~50 probed — no longer test every sample
  codepoint to confirm a foregone conclusion. Identical verdicts.
  Full scan of 431 system fonts: ~108ms → ~94ms, against ~67ms for the
  OS/2-bounded scan this replaces.

## [4.4.3] - 2026-06-06

### Fixed

- **Bundled in-memory fonts were unusable on caches with no system fonts**
  (headless / wasm / embedder-bundled-font setups). A font registered via
  `FcFontCache::with_memory_fonts` with a naive pattern (the font bytes plus
  a name, but an empty `unicode_ranges`) could never be selected to shape
  any character. Two independent root causes, both fixed:

  1. `with_memory_fonts` / `with_memory_font_with_id` stored the empty
     `unicode_ranges` verbatim, and `FontFallbackChain::resolve_char`
     deliberately skips any font that reports no coverage. They now
     auto-populate `unicode_ranges` from the font's cmap/OS2 tables when the
     caller leaves them empty, reusing the exact pipeline the on-disk
     builder uses (`FcParseFontBytes` → `parse_font_faces`). This requires
     the `parsing` feature; without it the caller-supplied pattern is stored
     unchanged and the caller must populate `unicode_ranges` themselves.

  2. Generic CSS families (`serif` / `sans-serif` / `monospace`) were
     expanded to a hardcoded list of real per-OS font names and the generic
     name itself was dropped, so a registered memory font (whatever its
     family name) was never reached. The chain builder now falls back to a
     generic `name: None` query for the originally-requested generic
     family **only when the expanded OS-specific stack matched nothing**, so
     systems with real fonts are unaffected and any fallback match comes
     after real matches.

## [4.4.0] - 2026-05-23

### Changed

- Bumped `allsorts-azul` 0.16.2 → 0.16.4 (semver-compatible patch bump
  within the 0.16 line).

### Fixed

- **WASM builds**: `FontBytes::Mmapped(mmapio::Mmap)` was gated only on
  `std`, but `mmapio` is excluded on `target_family = "wasm"`, so the
  variant referenced a crate that isn't linked there. The variant and
  its match arms are now gated on `not(target_family = "wasm")`; WASM
  targets fall back to `FontBytes::Owned`.
- **C bindings (`ffi`) + examples**: realigned to the v4.x shared-cache
  API — `FontSource` → `OwnedFontSource` and
  `FcFontRegistry::into_fc_font_cache()` → `shared_cache()`, and the
  `&FcPattern` borrow now expected by `calculate_style_score`. The
  exported C ABI is unchanged.
- `--all-features` dead-code warning for `pattern_from_filename`: gated
  to match its sole caller, `build_from_filenames`
  (`std` + `not(parsing)`).
- `test_operating_system_font_expansion` assertions updated to match the
  macOS/iOS serif + sans-serif expansion lists shipped in 4.3.0.

## [4.2.1] - 2026-04-18

### Fixed

- **`FcFontRegistry::request_fonts`: build_queue leak after `build_complete`**.
  Promote the existing `cache_loaded` fast-path at the top of the function
  into a joint `cache_loaded || build_complete` short-circuit. Once
  either is true, the pattern map is fully settled; walking `known_paths`
  to compute "missing" / "incomplete" family lists and pushing
  `FcBuildJob` items into `build_queue` is wasted work whose only
  observable effect is a steady leak of ~13 KiB per call — the builder
  threads have shut down and nothing drains the queue.

  Discovered via per-phase heap probes in a downstream resize-loop
  regression (~100 MiB RSS growth across a 5-second interactive
  session). Each call was retaining ~158 `FcBuildJob` items, one per
  permanently-missing system family (Arabic / CJK / etc.).

### Added

- Optional fine-grained heap probes inside `request_fonts`, gated
  behind `AZ_PROFILE=heap,jsonl,detail` + `AZ_PROFILE_OUT=<path>`.
  Permanent diagnostic infrastructure for future memory
  investigations; inert unless both env vars are set.

- `FcFontCache::chain_cache_len()` — cheap accessor returning the
  current number of cached resolved chains.

## [3.3.0] - 2026-04-14

### Added

- **`FcFontCache::get_font_bytes_arc`**: Returns font bytes as a
  shared `Arc<[u8]>`. Multiple `FontId`s backed by the same file
  content (every face of a `.ttc`, or two paths holding identical
  bytes) now return the *same* `Arc`, so downstream parsers that
  hold the bytes no longer duplicate them per face. The existing
  `get_font_bytes -> Vec<u8>` is kept as a thin wrapper.

- **`FcFontPath::bytes_hash: u64`**: Deterministic 64-bit content
  hash of the file's byte contents, computed once per file at
  parse time. Used as the key for the Arc-sharing cache. A value
  of `0` means "not computed" (e.g. built from a filename-only
  scan, or loaded from a legacy v1 disk cache) and callers should
  treat it as opaque.

- **`DEFAULT_UNICODE_FALLBACK_SCRIPTS`** (pub const): The 7 script
  blocks `resolve_font_chain` pulls in by default (Cyrillic, Arabic,
  Devanagari, Hiragana, Katakana, CJK Unified, Hangul).

- **`FcFontCache::resolve_font_chain_with_scripts`**: New primary
  entry point for fallback-chain resolution. Accepts
  `scripts_hint: Option<&[UnicodeRange]>`:
  - `None` → current behaviour (all 7 default scripts).
  - `Some(&[])` → no Unicode fallbacks attached (for ASCII-only
    documents this avoids dragging Arial Unicode MS and CJK
    fonts into the resolved chain).
  - `Some(&[CJK])` → only CJK fallback attached.

  The chain cache is keyed so a no-scripts-hint resolution can't
  be served from a slot filled by an all-scripts resolution.

- `utils::content_hash_u64` — stable-across-runs 64-bit byte hash.

### Changed

- Disk-cache `FontManifest::CURRENT_VERSION` bumped from `1` → `2`
  to persist `bytes_hash` per file. Existing v1 caches are
  invalidated on load (triggers a clean re-scan).

- `FontCacheEntry` now has a `bytes_hash: u64` field
  (`#[serde(default)]` for forward-compat).

### Unchanged / Back-compat

- `resolve_font_chain` / `resolve_font_chain_with_os` keep their
  signatures and their "default 7 scripts" behaviour.
- `get_font_bytes` keeps its `Option<Vec<u8>>` signature; it now
  just clones from `get_font_bytes_arc` internally.

## [2.0.0] - 2026-02-14

### Breaking Changes

- **`FontId` now uses atomic counter instead of `SystemTime`**: Font IDs are now
  assigned via a global atomic counter (`AtomicU128`), making them deterministic
  and reproducible across runs. Code that compared `FontId` values across sessions
  or relied on their magnitude encoding time will break.

### Added

- **`FcFontRegistry`**: New async font registry with background scanning and
  on-demand font loading. Requires the `async-registry` feature.
  - `FcFontRegistry::new()` — creates a new registry (returns `Arc<Self>`)
  - `register_memory_fonts()` — register in-memory fonts with priority
  - `spawn_scout_and_builders()` — start background directory scanning + font parsing
  - `request_fonts()` — request specific font families (prioritized loading)
  - `into_fc_font_cache()` — convert to `FcFontCache` for compatibility
  - `shutdown()`, `is_scan_complete()`, `is_build_complete()`, `progress()`

- **Disk cache** (`cache` feature): Serializes parsed font metadata to disk via
  `bincode`/`serde`, dramatically speeding up subsequent launches.
  - `FcFontRegistry::load_from_disk_cache()` / `save_to_disk_cache()`
  - `FontManifest`, `FontCacheEntry`, `FontIndexEntry` structs

- **`FcFontCache::build_with_families()`**: Build a cache that only scans and
  parses fonts matching specific family names, much faster than `build()` when
  you know which fonts you need.

- **`Debug` impl for `FcFontRegistry`**: Shows registry state (scan progress,
  font counts, memory fonts).

### Fixed

- **Italic font race condition**: `FcFontRegistry` now waits for all font file
  variants (regular, bold, italic, etc.) to be parsed before resolving font
  queries, preventing cases where italic variants were missing from results.

- **Font scoring**: When style is `DontCare`, prefer `Normal` over `Italic`
  variants. This fixes cases where italic fonts were incorrectly chosen as the
  default match.

- **Memory font preference**: `query()` now prefers memory fonts over disk fonts
  when both match equally, ensuring programmatically registered fonts take
  priority.

- **Test fix**: Arial Regular test pattern now explicitly sets `bold: False`
  instead of `DontCare` for correct scoring behavior.

## [1.2.2] - 2025-12-01

### Added

- `FcParseFontBytes`: Parse in-memory font data without building a full cache.

## [1.2.1] - 2025-11-26

### Fixed

- **Issue #15**: Windows font paths no longer assume C: drive. Now uses `SystemRoot`/`WINDIR` environment variable for system fonts and `USERPROFILE` for user fonts, with proper fallbacks.

- **Issue #17**: Removed duplicate `FcFontCache::build()` implementation that caused compilation errors when building without `std` or `parsing` features.

- **Issue #18**: Fixed compilation without `parsing` feature. All `allsorts` imports and dependent functions are now properly guarded with `#[cfg(feature = "parsing")]`.

## [1.2.0] - 2025-06-03

### Breaking Changes

- **`resolve_font_chain()` signature changed**: The `text` parameter has been removed. Font chains are now resolved based on CSS properties only (font-family, weight, italic, oblique), not text content.
  
  Old API:
  ```rust
  cache.resolve_font_chain(&families, text, weight, italic, oblique, &mut trace)
  ```
  
  New API:
  ```rust
  cache.resolve_font_chain(&families, weight, italic, oblique, &mut trace)
  ```

- **`query_all()` method removed**: Use `cache.list()` with filtering instead.
  
  Old API:
  ```rust
  let fonts = cache.query_all(&pattern, &mut trace);
  ```
  
  New API:
  ```rust
  let fonts: Vec<_> = cache.list().into_iter()
      .filter(|(pattern, _id)| /* your filter */)
      .collect();
  ```

- **`query_for_text()` moved to `FontFallbackChain`**: Text-to-font resolution now requires a font chain first.
  
  Old API:
  ```rust
  let fonts = cache.query_for_text(&pattern, text, &mut trace);
  ```
  
  New API:
  ```rust
  let chain = cache.resolve_font_chain(&families, weight, italic, oblique, &mut trace);
  let font_runs = chain.query_for_text(&cache, text);
  ```

### Added

- **`FontFallbackChain::resolve_text()`**: Returns per-character font assignments as `Vec<(char, Option<(FontId, String)>)>` for fine-grained control.

- **`FontFallbackChain::resolve_char()`**: Resolve a single character to its font.

- **`CssFallbackGroup` struct**: Groups fonts by their CSS source name, making it clear which CSS font-family each font came from.

- **Font chain caching**: Identical CSS font-family stacks now share cached font chains, improving performance when the same fonts are used with different text content.

### Changed

- **Architecture**: The new two-step workflow (chain resolution → text querying) better matches CSS/browser font handling semantics and enables better caching.

- **Performance**: Font chains are now cached by CSS properties, avoiding redundant font resolution for the same font-family declarations.

### Rationale

The API was refactored to separate concerns:
1. **Font chain resolution** (`resolve_font_chain`): Determines which fonts to use based on CSS font-family, weight, and style. This is typically done once per CSS declaration.
2. **Text-to-font mapping** (`resolve_text`/`query_for_text`): Maps text content to specific fonts in the chain. This is done per text string to render.

This separation enables:
- Better caching (same CSS fonts can be reused for different text)
- Clearer API semantics matching CSS behavior
- More efficient text layout pipelines

## [1.1.0] - 2025-11-25

### Added

- Better font resolution algorithms
- Performance improvements for font matching

## [1.0.3] - Previous

### Added

- Derive `Hash` on public types

## [1.0.2] - Previous

- Bug fixes and improvements

## [1.0.1] - Previous

- Bug fixes

## [1.0.0] - Previous

- Initial stable release
- Font matching by name, family, and style properties
- Unicode range support
- In-memory font loading
- C API bindings
- Cross-platform support (Windows, macOS, Linux, WASM)
