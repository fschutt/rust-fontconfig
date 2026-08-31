//! OS-specific font configuration: directories, common families, and font file constants.
//!
//! All hardcoded data is returned as `&'static` references to avoid allocation.
//!
//! The per-OS tables ([`system_font_dirs`], [`font_directories`],
//! [`common_font_families`]) remain public for direct callers, but the
//! async registry no longer reads them on its own: it consumes host
//! knowledge exclusively through an injected [`FcScanConfig`], and
//! [`FcScanConfig::os_defaults`] is the single place where these tables
//! become registry behavior.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use std::path::{Path, PathBuf};

use crate::FcFontCache;
use crate::OperatingSystem;
use crate::UnicodeRange;

/// Generic CSS font family keywords (CSS Fonts Level 4 §2.1.1), as the
/// author writes them. [`GenericFamily::from_css`] is the parser.
pub const GENERIC_FAMILIES: &[&str] = &[
    "serif", "sans-serif", "monospace", "cursive", "fantasy", "system-ui",
    "ui-serif", "ui-sans-serif", "ui-monospace", "ui-rounded",
    "emoji", "math", "fangsong",
];

/// Check whether `family` is a generic CSS font family (case-insensitive,
/// separators ignored).
pub fn is_generic_family(family: &str) -> bool {
    GenericFamily::from_css(family).is_some()
}

/// A CSS generic font family.
///
/// Generic families are not fonts; they are requests the host answers. What
/// stands behind each one is configuration ([`FcFallbackConfig`]), and
/// [`GenericFamily::parent`] says which configuration a less common generic
/// borrows when it has none of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GenericFamily {
    Serif,
    SansSerif,
    Monospace,
    Cursive,
    Fantasy,
    SystemUi,
    UiSerif,
    UiSansSerif,
    UiMonospace,
    UiRounded,
    Emoji,
    Math,
    Fangsong,
}

impl GenericFamily {
    /// Every generic, in [`GENERIC_FAMILIES`] order.
    pub const ALL: &'static [GenericFamily] = &[
        GenericFamily::Serif,
        GenericFamily::SansSerif,
        GenericFamily::Monospace,
        GenericFamily::Cursive,
        GenericFamily::Fantasy,
        GenericFamily::SystemUi,
        GenericFamily::UiSerif,
        GenericFamily::UiSansSerif,
        GenericFamily::UiMonospace,
        GenericFamily::UiRounded,
        GenericFamily::Emoji,
        GenericFamily::Math,
        GenericFamily::Fangsong,
    ];

    /// Parse a CSS keyword. Case-insensitive; separators are ignored, so
    /// `"Sans-Serif"`, `"sans serif"` and the normalized `"sansserif"` all
    /// parse. Anything else — a real family name — is `None`.
    pub fn from_css(name: &str) -> Option<Self> {
        let key: String = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        Some(match key.as_str() {
            "serif" => GenericFamily::Serif,
            "sansserif" => GenericFamily::SansSerif,
            "monospace" => GenericFamily::Monospace,
            "cursive" => GenericFamily::Cursive,
            "fantasy" => GenericFamily::Fantasy,
            "systemui" => GenericFamily::SystemUi,
            "uiserif" => GenericFamily::UiSerif,
            "uisansserif" => GenericFamily::UiSansSerif,
            "uimonospace" => GenericFamily::UiMonospace,
            "uirounded" => GenericFamily::UiRounded,
            "emoji" => GenericFamily::Emoji,
            "math" => GenericFamily::Math,
            "fangsong" => GenericFamily::Fangsong,
            _ => return None,
        })
    }

    /// The CSS keyword.
    pub fn as_css(self) -> &'static str {
        match self {
            GenericFamily::Serif => "serif",
            GenericFamily::SansSerif => "sans-serif",
            GenericFamily::Monospace => "monospace",
            GenericFamily::Cursive => "cursive",
            GenericFamily::Fantasy => "fantasy",
            GenericFamily::SystemUi => "system-ui",
            GenericFamily::UiSerif => "ui-serif",
            GenericFamily::UiSansSerif => "ui-sans-serif",
            GenericFamily::UiMonospace => "ui-monospace",
            GenericFamily::UiRounded => "ui-rounded",
            GenericFamily::Emoji => "emoji",
            GenericFamily::Math => "math",
            GenericFamily::Fangsong => "fangsong",
        }
    }

    /// The generic whose configuration stands in when this one has none.
    /// The three root generics have no parent.
    pub fn parent(self) -> Option<Self> {
        match self {
            GenericFamily::Serif | GenericFamily::SansSerif | GenericFamily::Monospace => None,
            GenericFamily::UiSerif | GenericFamily::Fangsong => Some(GenericFamily::Serif),
            GenericFamily::UiMonospace => Some(GenericFamily::Monospace),
            GenericFamily::Cursive
            | GenericFamily::Fantasy
            | GenericFamily::SystemUi
            | GenericFamily::UiSansSerif
            | GenericFamily::UiRounded
            | GenericFamily::Emoji
            | GenericFamily::Math => Some(GenericFamily::SansSerif),
        }
    }

    /// `self`, then its parents, root last.
    fn lineage(self) -> impl Iterator<Item = GenericFamily> {
        let mut next = Some(self);
        core::iter::from_fn(move || {
            let current = next?;
            next = current.parent();
            Some(current)
        })
    }
}

/// Preferred families for one script block, optionally only when a specific
/// generic family was requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FcScriptFallback {
    /// Characters this preference applies to.
    pub range: UnicodeRange,
    /// `Some(g)`: only when the stack asked for `g` (or a generic that
    /// borrows from `g`, see [`GenericFamily::parent`]). `None`: for every
    /// stack, after any generic-specific preferences.
    pub generic: Option<GenericFamily>,
    /// Family names, best first.
    pub families: Vec<String>,
}

/// Everything the chain builder knows about the host, injected.
///
/// Mirrors [`FcScanConfig`] for the resolution side: the crate does not
/// decide which font stands behind `sans-serif` on a box, which font a
/// Hiragana run should prefer, or what to draw when nothing covers a
/// character. The embedder does, and this is how it says so.
///
/// * [`FcFontCache::default`] carries an empty configuration.
/// * [`FcFontCache::build`] parses the platform configuration where there
///   is one (Linux `fonts.conf` aliases) and fills the gaps from
///   [`FcFallbackConfig::os_defaults`].
/// * [`FcFontCache::set_fallback_config`] replaces it at any time.
///
/// Every family name is matched by normalized equality (case and separators
/// ignored) against the installed fonts; names that are not installed are
/// skipped, so listing candidates that may be absent is fine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FcFallbackConfig {
    /// Base candidates per generic family, best first. A generic without an
    /// entry borrows its [`parent`](GenericFamily::parent)'s.
    pub generic_families: BTreeMap<GenericFamily, Vec<String>>,
    /// Substitutions for named families that are not installed, keyed by
    /// the normalized family name (`"arial"` → `["Liberation Sans"]`).
    pub substitutions: BTreeMap<String, Vec<String>>,
    /// Per-script preferences, in priority order.
    pub script_fallbacks: Vec<FcScriptFallback>,
    /// Families used for any character no other font in a chain covers,
    /// **without** a coverage check: the font whose `.notdef` glyph the
    /// embedder wants drawn. Empty means "report no font".
    pub last_resort: Vec<String>,
    /// The generic whose per-script preferences serve a stack that names no
    /// generic at all (fontconfig appends `sans-serif` to such patterns).
    pub default_generic: GenericFamily,
}

impl Default for FcFallbackConfig {
    fn default() -> Self {
        Self::empty()
    }
}

/// Unicode blocks the built-in tables refer to.
pub mod blocks {
    use crate::UnicodeRange;

    pub const ARABIC: UnicodeRange = UnicodeRange { start: 0x0600, end: 0x06FF };
    pub const HEBREW: UnicodeRange = UnicodeRange { start: 0x0590, end: 0x05FF };
    pub const THAI: UnicodeRange = UnicodeRange { start: 0x0E00, end: 0x0E7F };
    pub const CJK_SYMBOLS_AND_PUNCTUATION: UnicodeRange = UnicodeRange { start: 0x3000, end: 0x303F };
    pub const HIRAGANA: UnicodeRange = UnicodeRange { start: 0x3040, end: 0x309F };
    pub const KATAKANA: UnicodeRange = UnicodeRange { start: 0x30A0, end: 0x30FF };
    pub const CJK_UNIFIED_IDEOGRAPHS: UnicodeRange = UnicodeRange { start: 0x4E00, end: 0x9FFF };
    pub const HANGUL_SYLLABLES: UnicodeRange = UnicodeRange { start: 0xAC00, end: 0xD7A3 };
    pub const HALFWIDTH_AND_FULLWIDTH_FORMS: UnicodeRange = UnicodeRange { start: 0xFF00, end: 0xFFEF };
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn push_unique(out: &mut Vec<String>, name: &str) {
    if !out.iter().any(|e| e.eq_ignore_ascii_case(name)) {
        out.push(name.to_string());
    }
}

impl FcFallbackConfig {
    /// No candidates, no substitutions, no preferences, no last resort. A
    /// generic family then resolves to the best-styled registered font.
    pub fn empty() -> Self {
        Self {
            generic_families: BTreeMap::new(),
            substitutions: BTreeMap::new(),
            script_fallbacks: Vec::new(),
            last_resort: Vec::new(),
            default_generic: GenericFamily::SansSerif,
        }
    }

    /// The tables this crate used to hard-code, as an explicit opt-in: base
    /// candidates for `serif`, `sans-serif` and `monospace`, and per-script
    /// preferences for CJK, Arabic, Hebrew and Thai, per `os`.
    pub fn os_defaults(os: OperatingSystem) -> Self {
        use blocks::*;
        use GenericFamily::{Monospace, SansSerif, Serif};

        let mut config = Self::empty();
        let mut generic = |g: GenericFamily, list: &[&str]| {
            config.generic_families.insert(g, names(list));
        };

        match os {
            OperatingSystem::Windows => {
                generic(Serif, &["Times New Roman"]);
                generic(SansSerif, &["Segoe UI", "Tahoma", "Microsoft Sans Serif", "MS Sans Serif", "Helv"]);
                generic(Monospace, &["Segoe UI Mono", "Courier New", "Cascadia Code", "Cascadia Mono", "Consolas"]);
            }
            OperatingSystem::Linux => {
                generic(Serif, &[
                    "Times", "Times New Roman", "DejaVu Serif", "Free Serif",
                    "Noto Serif", "Bitstream Vera Serif", "Roman", "Regular",
                ]);
                generic(SansSerif, &["Ubuntu", "Arial", "DejaVu Sans", "Noto Sans", "Liberation Sans"]);
                generic(Monospace, &[
                    "Source Code Pro", "Cantarell", "DejaVu Sans Mono",
                    "Roboto Mono", "Ubuntu Monospace", "Droid Sans Mono",
                ]);
            }
            OperatingSystem::MacOS | OperatingSystem::IOS => {
                generic(Serif, &["Times New Roman", "Times", "New York", "Palatino"]);
                generic(SansSerif, &[
                    "San Francisco", ".AppleSystemUIFont", ".SFUIText", ".SFUI-Regular",
                    "Helvetica Neue", "Helvetica", "Lucida Grande",
                ]);
                generic(Monospace, &["SF Mono", "Menlo", "Monaco", "Courier", "Oxygen Mono", "Source Code Pro", "Fira Mono"]);
            }
            OperatingSystem::Android => {
                generic(Serif, &["Noto Serif", "Roboto Serif", "Droid Serif"]);
                generic(SansSerif, &["Roboto", "Roboto-Regular", "Noto Sans", "Droid Sans"]);
                generic(Monospace, &["Roboto Mono", "Droid Sans Mono", "Noto Sans Mono", "DejaVu Sans Mono"]);
            }
            OperatingSystem::Wasm => {}
        }

        let mut script = |g: GenericFamily, range: UnicodeRange, list: &[&str]| {
            config.script_fallbacks.push(FcScriptFallback {
                range,
                generic: Some(g),
                families: names(list),
            });
        };
        // CJK: ideographs are shared by Chinese, Japanese and Korean, so
        // their list keeps the historical order; kana are Japanese and
        // Hangul is Korean, so those blocks put the matching font first.
        let mut cjk = |g: GenericFamily, ideographs: &[&str], kana: &[&str], hangul: &[&str]| {
            for block in [CJK_SYMBOLS_AND_PUNCTUATION, CJK_UNIFIED_IDEOGRAPHS, HALFWIDTH_AND_FULLWIDTH_FORMS] {
                script(g, block, ideographs);
            }
            for block in [HIRAGANA, KATAKANA] {
                script(g, block, kana);
            }
            script(g, HANGUL_SYLLABLES, hangul);
        };

        match os {
            OperatingSystem::Windows => {
                cjk(Serif, &["MS Mincho", "SimSun", "MingLiU"], &["MS Mincho", "SimSun", "MingLiU"], &["SimSun", "MS Mincho", "MingLiU"]);
                cjk(
                    SansSerif,
                    &["Microsoft YaHei", "MS Gothic", "Malgun Gothic", "SimHei"],
                    &["MS Gothic", "Microsoft YaHei", "Malgun Gothic", "SimHei"],
                    &["Malgun Gothic", "Microsoft YaHei", "MS Gothic", "SimHei"],
                );
                cjk(Monospace, &["MS Gothic", "SimHei"], &["MS Gothic", "SimHei"], &["MS Gothic", "SimHei"]);
                script(Serif, ARABIC, &["Traditional Arabic"]);
                script(SansSerif, ARABIC, &["Segoe UI Arabic"]);
                script(SansSerif, HEBREW, &["Segoe UI Hebrew"]);
                script(SansSerif, THAI, &["Leelawadee UI"]);
            }
            OperatingSystem::Linux => {
                cjk(
                    Serif,
                    &["Noto Serif CJK SC", "Noto Serif CJK JP", "Noto Serif CJK KR"],
                    &["Noto Serif CJK JP", "Noto Serif CJK SC", "Noto Serif CJK KR"],
                    &["Noto Serif CJK KR", "Noto Serif CJK SC", "Noto Serif CJK JP"],
                );
                cjk(
                    SansSerif,
                    &["Noto Sans CJK SC", "Noto Sans CJK JP", "Noto Sans CJK KR", "WenQuanYi Micro Hei", "Droid Sans Fallback"],
                    &["Noto Sans CJK JP", "Noto Sans CJK SC", "Noto Sans CJK KR", "WenQuanYi Micro Hei", "Droid Sans Fallback"],
                    &["Noto Sans CJK KR", "Noto Sans CJK SC", "Noto Sans CJK JP", "WenQuanYi Micro Hei", "Droid Sans Fallback"],
                );
                cjk(
                    Monospace,
                    &["Noto Sans Mono CJK SC", "Noto Sans Mono CJK JP", "WenQuanYi Zen Hei Mono"],
                    &["Noto Sans Mono CJK JP", "Noto Sans Mono CJK SC", "WenQuanYi Zen Hei Mono"],
                    &["Noto Sans Mono CJK SC", "Noto Sans Mono CJK JP", "WenQuanYi Zen Hei Mono"],
                );
                script(Serif, ARABIC, &["Noto Serif Arabic"]);
                script(SansSerif, ARABIC, &["Noto Sans Arabic"]);
                script(SansSerif, HEBREW, &["Noto Sans Hebrew"]);
                script(SansSerif, THAI, &["Noto Sans Thai"]);
            }
            OperatingSystem::MacOS | OperatingSystem::IOS => {
                cjk(
                    Serif,
                    &["Hiragino Mincho ProN", "STSong", "AppleMyungjo"],
                    &["Hiragino Mincho ProN", "STSong", "AppleMyungjo"],
                    &["AppleMyungjo", "Hiragino Mincho ProN", "STSong"],
                );
                cjk(
                    SansSerif,
                    &["Hiragino Sans", "Hiragino Kaku Gothic ProN", "PingFang SC", "PingFang TC", "Apple SD Gothic Neo"],
                    &["Hiragino Sans", "Hiragino Kaku Gothic ProN", "PingFang SC", "PingFang TC", "Apple SD Gothic Neo"],
                    &["Apple SD Gothic Neo", "Hiragino Sans", "Hiragino Kaku Gothic ProN", "PingFang SC", "PingFang TC"],
                );
                cjk(Monospace, &["Hiragino Sans", "PingFang SC"], &["Hiragino Sans", "PingFang SC"], &["Hiragino Sans", "PingFang SC"]);
                script(Serif, ARABIC, &["Geeza Pro"]);
                script(SansSerif, ARABIC, &["Geeza Pro"]);
                script(SansSerif, HEBREW, &["Arial Hebrew"]);
                script(SansSerif, THAI, &["Thonburi"]);
            }
            OperatingSystem::Android => {
                cjk(
                    Serif,
                    &["Noto Serif CJK SC", "Noto Serif CJK JP", "Noto Serif CJK KR"],
                    &["Noto Serif CJK JP", "Noto Serif CJK SC", "Noto Serif CJK KR"],
                    &["Noto Serif CJK KR", "Noto Serif CJK SC", "Noto Serif CJK JP"],
                );
                cjk(
                    SansSerif,
                    &["Noto Sans CJK SC", "Noto Sans CJK JP", "Noto Sans CJK KR", "Droid Sans Fallback"],
                    &["Noto Sans CJK JP", "Noto Sans CJK SC", "Noto Sans CJK KR", "Droid Sans Fallback"],
                    &["Noto Sans CJK KR", "Noto Sans CJK SC", "Noto Sans CJK JP", "Droid Sans Fallback"],
                );
                cjk(
                    Monospace,
                    &["Noto Sans Mono CJK SC", "Noto Sans Mono CJK JP"],
                    &["Noto Sans Mono CJK JP", "Noto Sans Mono CJK SC"],
                    &["Noto Sans Mono CJK SC", "Noto Sans Mono CJK JP"],
                );
                script(Serif, ARABIC, &["Noto Naskh Arabic"]);
                script(SansSerif, ARABIC, &["Noto Sans Arabic"]);
                script(SansSerif, HEBREW, &["Noto Sans Hebrew"]);
                script(SansSerif, THAI, &["Noto Sans Thai"]);
            }
            OperatingSystem::Wasm => {}
        }

        config
    }

    /// Base candidates for `generic`, borrowing from its
    /// [`parent`](GenericFamily::parent) when it has no entry of its own.
    pub fn generic_candidates(&self, generic: GenericFamily) -> &[String] {
        generic
            .lineage()
            .find_map(|g| self.generic_families.get(&g))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Substitutions for the named `family` (normalized lookup).
    pub fn substitutions_for(&self, family: &str) -> &[String] {
        self.substitutions
            .get(&crate::utils::normalize_family_name(family))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Preferred families for characters in `block`: entries for `generic`
    /// (and the generics it borrows from) first, then generic-agnostic
    /// entries. `generic = None` returns only the latter. Deduplicated,
    /// order preserved.
    pub fn script_candidates(&self, generic: Option<GenericFamily>, block: &UnicodeRange) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(generic) = generic {
            for g in generic.lineage() {
                for entry in &self.script_fallbacks {
                    if entry.generic == Some(g) && entry.range.overlaps(block) {
                        entry.families.iter().for_each(|f| push_unique(&mut out, f));
                    }
                }
            }
        }
        for entry in &self.script_fallbacks {
            if entry.generic.is_none() && entry.range.overlaps(block) {
                entry.families.iter().for_each(|f| push_unique(&mut out, f));
            }
        }
        out
    }

    /// Every family name a generic may resolve to for text in `ranges`:
    /// script preferences for the overlapping blocks first, then the base
    /// candidates.
    pub fn expand_generic(&self, generic: GenericFamily, ranges: &[UnicodeRange]) -> Vec<String> {
        let mut out = Vec::new();
        for block in ranges {
            self.script_candidates(Some(generic), block)
                .iter()
                .for_each(|f| push_unique(&mut out, f));
        }
        self.generic_candidates(generic)
            .iter()
            .for_each(|f| push_unique(&mut out, f));
        out
    }

    /// [`expand_generic`](Self::expand_generic) for a generic keyword; a
    /// named family expands to itself followed by its substitutions.
    pub fn expand_family(&self, family: &str, ranges: &[UnicodeRange]) -> Vec<String> {
        match GenericFamily::from_css(family) {
            Some(generic) => self.expand_generic(generic, ranges),
            None => {
                let mut out = Vec::new();
                push_unique(&mut out, family);
                self.substitutions_for(family)
                    .iter()
                    .for_each(|f| push_unique(&mut out, f));
                out
            }
        }
    }

    /// Every family name a chain for `stack` with script blocks `ranges`
    /// could contain, in chain order. This is what the async registry
    /// parses ahead of resolving the chain, so the two agree by
    /// construction.
    pub fn candidate_families(&self, stack: &[String], ranges: &[UnicodeRange]) -> Vec<String> {
        let mut out = Vec::new();
        let mut any_generic = false;
        for family in stack {
            any_generic |= GenericFamily::from_css(family).is_some();
            self.expand_family(family, ranges)
                .iter()
                .for_each(|f| push_unique(&mut out, f));
        }
        for block in ranges {
            let generic = if any_generic { None } else { Some(self.default_generic) };
            self.script_candidates(generic, block)
                .iter()
                .for_each(|f| push_unique(&mut out, f));
        }
        self.last_resort.iter().for_each(|f| push_unique(&mut out, f));
        out
    }

    /// Fill what this configuration leaves unsaid from `defaults`: generics
    /// and substitutions without an entry, script blocks without a
    /// preference for the same generic, and an empty last resort. Entries
    /// already present are never reordered or extended — the configured
    /// authority wins, the defaults are the last resort.
    pub fn merge_defaults(&mut self, defaults: &FcFallbackConfig) {
        for (generic, families) in &defaults.generic_families {
            self.generic_families
                .entry(*generic)
                .or_insert_with(|| families.clone());
        }
        for (family, replacements) in &defaults.substitutions {
            self.substitutions
                .entry(family.clone())
                .or_insert_with(|| replacements.clone());
        }
        for entry in &defaults.script_fallbacks {
            let already = self
                .script_fallbacks
                .iter()
                .any(|e| e.generic == entry.generic && e.range.overlaps(&entry.range));
            if !already {
                self.script_fallbacks.push(entry.clone());
            }
        }
        if self.last_resort.is_empty() {
            self.last_resort = defaults.last_resort.clone();
        }
    }

    /// Take over parsed platform aliases (`fonts.conf` `<alias><prefer>`):
    /// a generic keyword becomes that generic's base candidates, anything
    /// else a named-family substitution. Keys are normalized family names.
    pub fn absorb_system_aliases(&mut self, aliases: BTreeMap<String, Vec<String>>) {
        for (key, prefs) in aliases {
            match GenericFamily::from_css(&key) {
                Some(generic) => {
                    self.generic_families.insert(generic, prefs);
                }
                None => {
                    self.substitutions.insert(key, prefs);
                }
            }
        }
    }
}

/// Style tokens to filter out when guessing family names from filenames.
///
/// These are the weight/style/width suffixes commonly appended to font filenames
/// (e.g. "ArialBold.ttf", "NotoSans-SemiBold.otf"). Used by the scout thread
/// to extract the base family name from a filename.
pub const FONT_STYLE_TOKENS: &[&str] = &[
    "Regular", "Bold", "Italic", "Light", "Medium", "Thin",
    "Black", "ExtraLight", "ExtraBold", "SemiBold", "DemiBold",
    "Heavy", "Oblique", "Condensed", "Expanded",
    // The tokenizer splits compound styles (e.g. "SemiBold" → "Semi" + "Bold"),
    // so we need the modifier prefixes as standalone style tokens too.
    "Extra", "Semi", "Demi",
];

/// Static system font directories per OS. No allocation.
///
/// These are the well-known, fixed paths. User-specific directories
/// (which require env var resolution) are added by [`font_directories`].
pub fn system_font_dirs(os: OperatingSystem) -> &'static [&'static str] {
    match os {
        OperatingSystem::MacOS => &[
            "/System/Library/Fonts",
            "/Library/Fonts",
            "/System/Library/AssetsV2",
        ],
        OperatingSystem::Linux => &[
            "/usr/share/fonts",
            "/usr/local/share/fonts",
        ],
        // Android system-font directories are world-readable. Vendor partitions
        // (`/product/fonts`, `/system_ext/fonts`) carry OEM-specific families
        // (Samsung One UI, MIUI, EMUI). `/data/fonts` is the user-selected
        // font directory exposed by recent OEM ROMs.
        OperatingSystem::Android => &[
            "/system/fonts",
            "/product/fonts",
            "/system_ext/fonts",
            "/data/fonts",
        ],
        // iOS bundles system fonts under sandboxed paths that cannot be
        // enumerated with a plain `read_dir`. The cache enumerates them via
        // `CTFontManagerCopyAvailableFontURLs` in `lib.rs::build_inner`; the
        // returned `CFURL`s point inside `/System/Library/...` paths that are
        // openable through the CoreText I/O bridge even though the underlying
        // directory is unreadable.
        OperatingSystem::IOS => &[],
        // Windows paths require env var resolution — handled in font_directories()
        OperatingSystem::Windows => &[],
        OperatingSystem::Wasm => &[],
    }
}

/// All font directories (system + user-specific).
///
/// Combines the static [`system_font_dirs`] with user-specific paths
/// resolved from environment variables (`HOME`, `SystemRoot`, etc.).
pub fn font_directories(os: OperatingSystem) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = system_font_dirs(os)
        .iter()
        .map(PathBuf::from)
        .collect();

    match os {
        OperatingSystem::MacOS => {
            if let Ok(home) = std::env::var("HOME") {
                dirs.push(PathBuf::from(format!("{}/Library/Fonts", home)));
            }
        }
        OperatingSystem::Linux => {
            if let Ok(home) = std::env::var("HOME") {
                dirs.push(PathBuf::from(format!("{}/.fonts", home)));
                dirs.push(PathBuf::from(format!("{}/.local/share/fonts", home)));
            }
        }
        OperatingSystem::Windows => {
            let system_root = std::env::var("SystemRoot")
                .or_else(|_| std::env::var("WINDIR"))
                .unwrap_or_else(|_| "C:\\Windows".to_string());
            let user_profile = std::env::var("USERPROFILE")
                .unwrap_or_else(|_| "C:\\Users\\Default".to_string());
            dirs.push(PathBuf::from(format!("{}\\Fonts", system_root)));
            dirs.push(PathBuf::from(format!(
                "{}\\AppData\\Local\\Microsoft\\Windows\\Fonts",
                user_profile
            )));
        }
        // No env-var-resolved user-font dir on iOS (no $HOME inside the sandbox)
        // or Android (apps own /data/data/<package>/files/fonts but that's a
        // private app dir, not a fontconfig directory).
        OperatingSystem::IOS | OperatingSystem::Android => {}
        OperatingSystem::Wasm => {}
    }

    dirs
}

/// Common font families for priority boosting, as human-readable names.
/// No allocation — returns a static slice.
///
/// These are the most commonly needed system fonts per OS. Wrapped into
/// [`FcScanConfig::os_defaults`], they tell the scout thread which fonts
/// to parse first so likely-needed families are available sooner. The
/// scout itself only ever sees the injected [`FcScanConfig`]; this table
/// is a guess, and embedders that know their actual UI font should
/// inject that instead.
///
/// The names here are the canonical human-readable forms. Use
/// [`matches_common_family`] for token-based matching against filenames.
pub fn common_font_families(os: OperatingSystem) -> &'static [&'static str] {
    match os {
        OperatingSystem::MacOS => &[
            // System UI fonts (actual filenames use SFNS prefix)
            "San Francisco", "SFNS", "System Font",
            // Sans-serif
            "Helvetica Neue", "Helvetica", "Arial", "Lucida Grande",
            // Serif
            "Times New Roman", "Georgia",
            // Monospace
            "Menlo", "SF Mono", "Courier",
        ],
        OperatingSystem::Linux => &[
            // Sans-serif
            "DejaVu Sans", "Ubuntu", "Roboto", "Noto Sans",
            "Liberation Sans", "Droid Sans", "Arial",
            // Serif
            "DejaVu Serif", "Noto Serif",
            // Monospace
            "DejaVu Sans Mono",
        ],
        OperatingSystem::Windows => &[
            // Sans-serif
            "Segoe UI", "Arial", "Tahoma", "Verdana",
            // Serif
            "Times New Roman", "Calibri",
            // Monospace
            "Consolas", "Courier New",
        ],
        OperatingSystem::IOS => &[
            // System UI fonts (filenames use SFNS/SFUI prefix)
            "San Francisco", "SFNS", "SFNSDisplay", "SFNSText", "SFUI",
            ".AppleSystemUIFont", "System Font",
            // Sans-serif
            "Helvetica Neue", "Helvetica", "Avenir", "Avenir Next",
            // Serif
            "Times New Roman", "Georgia",
            // Monospace
            "Menlo", "SF Mono", "Courier",
        ],
        OperatingSystem::Android => &[
            // System UI fonts
            "Roboto", "Roboto Flex", "Roboto Condensed",
            // Sans-serif
            "Noto Sans", "Droid Sans",
            // Serif
            "Noto Serif", "Roboto Serif", "Droid Serif",
            // Monospace
            "Roboto Mono", "Droid Sans Mono", "Noto Sans Mono",
        ],
        OperatingSystem::Wasm => &[],
    }
}

/// Pre-tokenize common font families for efficient per-file matching.
///
/// Call this once before iterating over font files, then pass the result
/// to [`matches_common_family_tokens`] for each file.
pub fn tokenize_common_families(os: OperatingSystem) -> Vec<Vec<String>> {
    common_font_families(os)
        .iter()
        .map(|family| tokenize_lowercase(family))
        .collect()
}

/// Host knowledge the registry needs but must not invent: where fonts live
/// and which families deserve parse priority.
///
/// Injected by the embedder via `FcFontRegistry::new_with_config`. This
/// crate used to decide both tables on its own (via [`font_directories`]
/// and [`common_font_families`]), which gets it backwards: an embedder
/// whose detected system UI font was not in the guessed list paid the
/// first layout in .notdef tofu, because the scout parsed hundreds of
/// other files before reaching the one family the UI was about to ask
/// for. The host knows its font locations and its UI font; this crate
/// does not.
///
/// The old tables survive in exactly one place:
/// [`FcScanConfig::os_defaults`] is the explicitly-chosen fallback that
/// carries them. `FcFontRegistry::new()` opts into it for you, so
/// existing callers keep the old behavior unchanged.
#[derive(Debug, Clone, PartialEq)]
pub struct FcScanConfig {
    /// Directories to scan recursively. Empty = scan nothing.
    pub font_dirs: Vec<PathBuf>,
    /// Human-readable family names whose files the scout parses first
    /// (they become token sets via [`tokenize_lowercase`]).
    pub priority_families: Vec<String>,
}

impl FcScanConfig {
    /// The tables this crate used to hard-code, as an explicit opt-in:
    /// [`font_directories`] (system + env-var-resolved user dirs) plus
    /// [`common_font_families`] for `os`.
    pub fn os_defaults(os: OperatingSystem) -> Self {
        Self {
            font_dirs: font_directories(os),
            priority_families: common_font_families(os)
                .iter()
                .map(|family| family.to_string())
                .collect(),
        }
    }

    /// Scan nothing, prioritize nothing. For embedders that supply every
    /// directory themselves or work from memory fonts only.
    pub fn empty() -> Self {
        Self {
            font_dirs: Vec::new(),
            priority_families: Vec::new(),
        }
    }

    /// Pre-tokenize [`FcScanConfig::priority_families`] for per-file
    /// matching, mirroring [`tokenize_common_families`]. Pass the result
    /// to [`matches_common_family_tokens`] or the scout's
    /// `assign_scout_priority`.
    pub fn priority_token_sets(&self) -> Vec<Vec<String>> {
        self.priority_families
            .iter()
            .map(|family| tokenize_lowercase(family))
            .collect()
    }
}

/// Check if a set of filename tokens matches any pre-tokenized common family.
///
/// Both sides are joined into a single normalized string (tokens concatenated),
/// then checked for substring containment. This handles cases where the tokenizer
/// produces different splits for the same underlying name (e.g. `"SFMono"` stays
/// as one token from a filename, but `"SF Mono"` splits into `["sf", "mono"]`).
pub fn matches_common_family_tokens(
    file_tokens: &[String],
    common_token_sets: &[Vec<String>],
) -> bool {
    let file_joined: String = file_tokens.concat();
    common_token_sets.iter().any(|family_tokens| {
        let family_joined: String = family_tokens.concat();
        file_joined.contains(&family_joined)
    })
}

/// Tokenize a name into lowercase tokens (no style filtering).
///
/// Useful for priority scoring where style tokens like "Bold" are still relevant.
pub fn tokenize_lowercase(name: &str) -> Vec<String> {
    FcFontCache::extract_font_name_tokens(name)
        .into_iter()
        .map(|t| t.to_lowercase())
        .collect()
}

/// Extract non-style tokens from a font filename stem.
///
/// Tokenizes using CamelCase boundaries, hyphens, underscores, and spaces,
/// then filters out style tokens (Bold, Italic, Regular, etc.).
/// Returns lowercased tokens suitable for family name matching.
///
/// # Examples
///
/// - `"ArialBold"` → `["arial"]`
/// - `"NotoSansJP-Regular"` → `["noto", "sans", "jp"]`
/// - `"HelveticaNeue-BoldItalic"` → `["helvetica", "neue"]`
pub fn tokenize_font_stem(stem: &str) -> Vec<String> {
    tokenize_lowercase(stem)
        .into_iter()
        .filter(|t| !FONT_STYLE_TOKENS.iter().any(|s| s.eq_ignore_ascii_case(t)))
        .collect()
}

/// Guess the font family name from a filename, using tokenization.
///
/// Extracts non-style tokens from the filename stem and joins them
/// into a single normalized string (lowercase, no separators).
///
/// # Examples
///
/// - `"ArialBold.ttf"` → `"arial"`
/// - `"NotoSansJP-Regular.otf"` → `"notosansjp"`
/// - `"Helvetica Neue Bold Italic.ttf"` → `"helveticaneue"`
pub fn guess_family_from_filename(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    tokenize_font_stem(stem).join("")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Generic families ─────────────────────────────────────────────────

    #[test]
    fn generic_families_recognized() {
        assert!(is_generic_family("sans-serif"));
        assert!(is_generic_family("Sans-Serif")); // case-insensitive
        assert!(is_generic_family("monospace"));
        assert!(is_generic_family("SERIF"));
        assert!(!is_generic_family("Arial"));
        assert!(!is_generic_family("Noto Sans"));
    }

    // ── Constants ────────────────────────────────────────────────────────

    #[test]
    fn font_style_tokens_covers_common_styles() {
        for token in &[
            "Regular", "Bold", "Italic", "Light", "Medium",
            "Thin", "Black", "Oblique", "SemiBold",
        ] {
            assert!(
                FONT_STYLE_TOKENS.contains(token),
                "missing style token: {}", token
            );
        }
    }

    // ── system_font_dirs ────────────────────────────────────────────────

    #[test]
    fn system_font_dirs_static_and_nonempty() {
        assert!(!system_font_dirs(OperatingSystem::MacOS).is_empty());
        assert!(!system_font_dirs(OperatingSystem::Linux).is_empty());
        assert!(system_font_dirs(OperatingSystem::Wasm).is_empty());
    }

    // ── common_font_families ────────────────────────────────────────────

    #[test]
    fn common_font_families_nonempty_for_desktop() {
        assert!(!common_font_families(OperatingSystem::MacOS).is_empty());
        assert!(!common_font_families(OperatingSystem::Linux).is_empty());
        assert!(!common_font_families(OperatingSystem::Windows).is_empty());
        assert!(common_font_families(OperatingSystem::Wasm).is_empty());
    }

    // ── FcScanConfig ────────────────────────────────────────────────────

    #[test]
    fn os_defaults_carry_the_legacy_tables() {
        for os in [
            OperatingSystem::Linux,
            OperatingSystem::Windows,
            OperatingSystem::MacOS,
        ] {
            let config = FcScanConfig::os_defaults(os);

            assert!(!config.font_dirs.is_empty(), "no dirs for {:?}", os);
            assert!(
                !config.priority_families.is_empty(),
                "no families for {:?}", os
            );

            // os_defaults must be exactly the old hard-coded behavior:
            // same dirs, same families, same token sets.
            assert_eq!(config.font_dirs, font_directories(os));
            let legacy: Vec<String> = common_font_families(os)
                .iter()
                .map(|f| f.to_string())
                .collect();
            assert_eq!(config.priority_families, legacy);
            assert_eq!(
                config.priority_token_sets(),
                tokenize_common_families(os)
            );
        }
    }

    #[test]
    fn empty_scan_config_scans_and_prioritizes_nothing() {
        let config = FcScanConfig::empty();
        assert!(config.font_dirs.is_empty());
        assert!(config.priority_families.is_empty());
        assert!(config.priority_token_sets().is_empty());
    }

    // ── guess_family_from_filename ──────────────────────────────────────

    #[test]
    fn guess_family_strips_style_suffixes() {
        assert_eq!(
            guess_family_from_filename(Path::new("ArialBold.ttf")),
            "arial"
        );
        assert_eq!(
            guess_family_from_filename(Path::new("NotoSansJP-Regular.otf")),
            "notosansjp"
        );
        assert_eq!(
            guess_family_from_filename(Path::new("Helvetica Neue Bold Italic.ttf")),
            "helveticaneue"
        );
    }

    #[test]
    fn guess_family_handles_underscores() {
        assert_eq!(
            guess_family_from_filename(Path::new("Liberation_Sans_Bold.ttf")),
            "liberationsans"
        );
    }

    #[test]
    fn guess_family_handles_compound_styles() {
        assert_eq!(
            guess_family_from_filename(Path::new("LiberationSans-BoldItalic.ttf")),
            "liberationsans"
        );
        assert_eq!(
            guess_family_from_filename(Path::new("DejaVuSansMono-ExtraBold.ttf")),
            "dejavusansmono"
        );
        assert_eq!(
            guess_family_from_filename(Path::new("SFMono-SemiBold.otf")),
            "sfmono"
        );
    }

    // ── token-based matching ────────────────────────────────────────────

    #[test]
    fn matches_common_family_macos() {
        let common = tokenize_common_families(OperatingSystem::MacOS);

        // "SFNSDisplay" → tokens ["sfns", "display"] → matches "SFNS"
        let tokens = tokenize_all("SFNSDisplay");
        assert!(matches_common_family_tokens(&tokens, &common));

        // "HelveticaNeue" → tokens ["helvetica", "neue"] → matches "Helvetica Neue"
        let tokens = tokenize_all("HelveticaNeue");
        assert!(matches_common_family_tokens(&tokens, &common));

        // "Arial" → matches "Arial"
        let tokens = tokenize_all("Arial");
        assert!(matches_common_family_tokens(&tokens, &common));

        // "SomeRandomFont" → no match
        let tokens = tokenize_all("SomeRandomFont");
        assert!(!matches_common_family_tokens(&tokens, &common));
    }

    #[test]
    fn matches_common_family_linux() {
        let common = tokenize_common_families(OperatingSystem::Linux);

        let tokens = tokenize_all("DejaVuSans");
        assert!(matches_common_family_tokens(&tokens, &common));

        let tokens = tokenize_all("NotoSansCJK");
        assert!(matches_common_family_tokens(&tokens, &common));

        let tokens = tokenize_all("UbuntuMono-Regular");
        assert!(matches_common_family_tokens(&tokens, &common));
    }

    #[test]
    fn matches_common_family_windows() {
        let common = tokenize_common_families(OperatingSystem::Windows);

        let tokens = tokenize_all("SegoeUI-Regular");
        assert!(matches_common_family_tokens(&tokens, &common));

        let tokens = tokenize_all("Consolas");
        assert!(matches_common_family_tokens(&tokens, &common));
    }

    // ── tokenize_font_stem ──────────────────────────────────────────────

    #[test]
    fn tokenize_font_stem_filters_styles() {
        assert_eq!(tokenize_font_stem("ArialBold"), vec!["arial"]);
        assert_eq!(
            tokenize_font_stem("NotoSansJP-Regular"),
            vec!["noto", "sans", "jp"]
        );
        // "SFMono" stays as one token (consecutive uppercase → no CamelCase split)
        assert_eq!(
            tokenize_font_stem("SFMono-SemiBold"),
            vec!["sfmono"]
        );
    }

    /// Helper: tokenize a stem into all lowercase tokens (including style tokens).
    fn tokenize_all(stem: &str) -> Vec<String> {
        tokenize_lowercase(stem)
    }
}
