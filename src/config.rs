//! OS-specific font configuration.
//!
//! Contains default font directories, generic CSS families, and fallback configuration.
//! Hardcoded data is returned as `&'static` references to avoid allocation.
use crate::FcFontCache;
use crate::OperatingSystem;
use crate::UnicodeRange;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use std::path::{Path, PathBuf};

/// Generic CSS font family keywords (CSS Fonts Level 4).

/// Style tokens to filter out when guessing family names from filenames.
/// These are the weight/style/width suffixes commonly appended to font filenames
/// (e.g. "ArialBold.ttf", "NotoSans-SemiBold.otf"). Used by the scout thread
/// to extract the base family name from a filename.
pub const FONT_STYLE_TOKENS: &[&str] = &[
    "Regular",
    "Bold",
    "Italic",
    "Light",
    "Medium",
    "Thin",
    "Black",
    "ExtraLight",
    "ExtraBold",
    "SemiBold",
    "DemiBold",
    "Heavy",
    "Oblique",
    "Condensed",
    "Expanded",
    // The tokenizer splits compound styles (e.g. "SemiBold" → "Semi" + "Bold"),
    // so we need the modifier prefixes as standalone style tokens too.
    "Extra",
    "Semi",
    "Demi",
];

/// Check whether `family` is a generic CSS font family (case-insensitive).
pub fn is_generic_family(family: &str) -> bool {
    GenericFamily::from_css(family).is_some()
}

/// A CSS generic font family.
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

    /// Parse a CSS keyword (case-insensitive, separators ignored).
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

/// Preferred families for a Unicode script block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FcScriptFallback {
    /// Characters this preference applies to.
    pub range: UnicodeRange,
    /// Generic family this preference applies to, if any.
    pub generic: Option<GenericFamily>,
    /// Family names, best first.
    pub families: Vec<String>,
}

/// Fallback configuration for font resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FcFallbackConfig {
    /// Base candidates per generic family, best first.
    pub generic_families: BTreeMap<GenericFamily, Vec<String>>,
    /// Substitutions for named families that are not installed.
    pub substitutions: BTreeMap<String, Vec<String>>,
    /// Per-script preferences, in priority order.
    pub script_fallbacks: Vec<FcScriptFallback>,
    /// Last resort families used when no other font provides coverage.
    pub last_resort: Vec<String>,
    /// Default generic family when a stack doesn't specify one.
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

    pub const ARABIC: UnicodeRange = UnicodeRange {
        start: 0x0600,
        end: 0x06FF,
    };
    pub const HEBREW: UnicodeRange = UnicodeRange {
        start: 0x0590,
        end: 0x05FF,
    };
    pub const THAI: UnicodeRange = UnicodeRange {
        start: 0x0E00,
        end: 0x0E7F,
    };
    pub const CJK_SYMBOLS_AND_PUNCTUATION: UnicodeRange = UnicodeRange {
        start: 0x3000,
        end: 0x303F,
    };
    pub const HIRAGANA: UnicodeRange = UnicodeRange {
        start: 0x3040,
        end: 0x309F,
    };
    pub const KATAKANA: UnicodeRange = UnicodeRange {
        start: 0x30A0,
        end: 0x30FF,
    };
    pub const CJK_UNIFIED_IDEOGRAPHS: UnicodeRange = UnicodeRange {
        start: 0x4E00,
        end: 0x9FFF,
    };
    pub const HANGUL_SYLLABLES: UnicodeRange = UnicodeRange {
        start: 0xAC00,
        end: 0xD7A3,
    };
    pub const HALFWIDTH_AND_FULLWIDTH_FORMS: UnicodeRange = UnicodeRange {
        start: 0xFF00,
        end: 0xFFEF,
    };
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
    /// Returns an empty fallback configuration.
    pub fn empty() -> Self {
        Self {
            generic_families: BTreeMap::new(),
            substitutions: BTreeMap::new(),
            script_fallbacks: Vec::new(),
            last_resort: Vec::new(),
            default_generic: GenericFamily::SansSerif,
        }
    }

    /// Returns the default OS-specific fallback configuration.
    pub fn os_defaults(os: OperatingSystem) -> Self {
        use blocks::*;
        use GenericFamily::{Monospace, SansSerif, Serif, SystemUi};
        let mut config = Self::empty();
        let mut generic = |g: GenericFamily, list: &[&str]| {
            config.generic_families.insert(g, names(list));
        };
        match os {
            OperatingSystem::Windows => {
                generic(Serif, &["Times New Roman"]);
                generic(
                    SansSerif,
                    &[
                        "Segoe UI",
                        "Tahoma",
                        "Microsoft Sans Serif",
                        "MS Sans Serif",
                        "Helv",
                    ],
                );
                generic(
                    Monospace,
                    &[
                        "Segoe UI Mono",
                        "Courier New",
                        "Cascadia Code",
                        "Cascadia Mono",
                        "Consolas",
                    ],
                );
            }
            OperatingSystem::Linux => {
                generic(
                    Serif,
                    &[
                        "Times",
                        "Times New Roman",
                        "DejaVu Serif",
                        "Free Serif",
                        "Noto Serif",
                        "Bitstream Vera Serif",
                        "Roman",
                        "Regular",
                    ],
                );
                generic(
                    SansSerif,
                    &[
                        "Ubuntu",
                        "Arial",
                        "DejaVu Sans",
                        "Noto Sans",
                        "Liberation Sans",
                    ],
                );
                generic(
                    Monospace,
                    &[
                        "Source Code Pro",
                        "Cantarell",
                        "DejaVu Sans Mono",
                        "Roboto Mono",
                        "Ubuntu Monospace",
                        "Droid Sans Mono",
                    ],
                );
            }
            OperatingSystem::MacOS | OperatingSystem::IOS => {
                generic(
                    SystemUi,
                    &[
                        "San Francisco",
                        "SFNS",
                        "SFNSDisplay",
                        "SFNSText",
                        "SFUI",
                        ".AppleSystemUIFont",
                        ".SFUIText",
                        ".SFUI-Regular",
                        "System Font",
                    ],
                );
                generic(Serif, &["Times New Roman", "Times", "New York", "Palatino"]);
                generic(SansSerif, &["Helvetica Neue", "Helvetica", "Lucida Grande"]);
                generic(
                    Monospace,
                    &[
                        "SF Mono",
                        "Menlo",
                        "Monaco",
                        "Courier",
                        "Oxygen Mono",
                        "Source Code Pro",
                        "Fira Mono",
                    ],
                );
            }
            OperatingSystem::Android => {
                generic(Serif, &["Noto Serif", "Roboto Serif", "Droid Serif"]);
                generic(
                    SansSerif,
                    &["Roboto", "Roboto-Regular", "Noto Sans", "Droid Sans"],
                );
                generic(
                    Monospace,
                    &[
                        "Roboto Mono",
                        "Droid Sans Mono",
                        "Noto Sans Mono",
                        "DejaVu Sans Mono",
                    ],
                );
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
            for block in [
                CJK_SYMBOLS_AND_PUNCTUATION,
                CJK_UNIFIED_IDEOGRAPHS,
                HALFWIDTH_AND_FULLWIDTH_FORMS,
            ] {
                script(g, block, ideographs);
            }
            for block in [HIRAGANA, KATAKANA] {
                script(g, block, kana);
            }
            script(g, HANGUL_SYLLABLES, hangul);
        };
        match os {
            OperatingSystem::Windows => {
                cjk(
                    Serif,
                    &["MS Mincho", "SimSun", "MingLiU"],
                    &["MS Mincho", "SimSun", "MingLiU"],
                    &["SimSun", "MS Mincho", "MingLiU"],
                );
                cjk(
                    SansSerif,
                    &["Microsoft YaHei", "MS Gothic", "Malgun Gothic", "SimHei"],
                    &["MS Gothic", "Microsoft YaHei", "Malgun Gothic", "SimHei"],
                    &["Malgun Gothic", "Microsoft YaHei", "MS Gothic", "SimHei"],
                );
                cjk(
                    Monospace,
                    &["MS Gothic", "SimHei"],
                    &["MS Gothic", "SimHei"],
                    &["MS Gothic", "SimHei"],
                );
                script(Serif, ARABIC, &["Traditional Arabic"]);
                script(SansSerif, ARABIC, &["Segoe UI Arabic"]);
                script(SansSerif, HEBREW, &["Segoe UI Hebrew"]);
                script(SansSerif, THAI, &["Leelawadee UI"]);
            }
            OperatingSystem::Linux => {
                cjk(
                    Serif,
                    &[
                        "Noto Serif CJK SC",
                        "Noto Serif CJK JP",
                        "Noto Serif CJK KR",
                    ],
                    &[
                        "Noto Serif CJK JP",
                        "Noto Serif CJK SC",
                        "Noto Serif CJK KR",
                    ],
                    &[
                        "Noto Serif CJK KR",
                        "Noto Serif CJK SC",
                        "Noto Serif CJK JP",
                    ],
                );
                cjk(
                    SansSerif,
                    &[
                        "Noto Sans CJK SC",
                        "Noto Sans CJK JP",
                        "Noto Sans CJK KR",
                        "WenQuanYi Micro Hei",
                        "Droid Sans Fallback",
                    ],
                    &[
                        "Noto Sans CJK JP",
                        "Noto Sans CJK SC",
                        "Noto Sans CJK KR",
                        "WenQuanYi Micro Hei",
                        "Droid Sans Fallback",
                    ],
                    &[
                        "Noto Sans CJK KR",
                        "Noto Sans CJK SC",
                        "Noto Sans CJK JP",
                        "WenQuanYi Micro Hei",
                        "Droid Sans Fallback",
                    ],
                );
                cjk(
                    Monospace,
                    &[
                        "Noto Sans Mono CJK SC",
                        "Noto Sans Mono CJK JP",
                        "WenQuanYi Zen Hei Mono",
                    ],
                    &[
                        "Noto Sans Mono CJK JP",
                        "Noto Sans Mono CJK SC",
                        "WenQuanYi Zen Hei Mono",
                    ],
                    &[
                        "Noto Sans Mono CJK SC",
                        "Noto Sans Mono CJK JP",
                        "WenQuanYi Zen Hei Mono",
                    ],
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
                    &[
                        "Hiragino Sans",
                        "Hiragino Kaku Gothic ProN",
                        "PingFang SC",
                        "PingFang TC",
                        "Apple SD Gothic Neo",
                    ],
                    &[
                        "Hiragino Sans",
                        "Hiragino Kaku Gothic ProN",
                        "PingFang SC",
                        "PingFang TC",
                        "Apple SD Gothic Neo",
                    ],
                    &[
                        "Apple SD Gothic Neo",
                        "Hiragino Sans",
                        "Hiragino Kaku Gothic ProN",
                        "PingFang SC",
                        "PingFang TC",
                    ],
                );
                cjk(
                    Monospace,
                    &["Hiragino Sans", "PingFang SC"],
                    &["Hiragino Sans", "PingFang SC"],
                    &["Hiragino Sans", "PingFang SC"],
                );
                script(Serif, ARABIC, &["Geeza Pro"]);
                script(SansSerif, ARABIC, &["Geeza Pro"]);
                script(SansSerif, HEBREW, &["Arial Hebrew"]);
                script(SansSerif, THAI, &["Thonburi"]);
            }
            OperatingSystem::Android => {
                cjk(
                    Serif,
                    &[
                        "Noto Serif CJK SC",
                        "Noto Serif CJK JP",
                        "Noto Serif CJK KR",
                    ],
                    &[
                        "Noto Serif CJK JP",
                        "Noto Serif CJK SC",
                        "Noto Serif CJK KR",
                    ],
                    &[
                        "Noto Serif CJK KR",
                        "Noto Serif CJK SC",
                        "Noto Serif CJK JP",
                    ],
                );
                cjk(
                    SansSerif,
                    &[
                        "Noto Sans CJK SC",
                        "Noto Sans CJK JP",
                        "Noto Sans CJK KR",
                        "Droid Sans Fallback",
                    ],
                    &[
                        "Noto Sans CJK JP",
                        "Noto Sans CJK SC",
                        "Noto Sans CJK KR",
                        "Droid Sans Fallback",
                    ],
                    &[
                        "Noto Sans CJK KR",
                        "Noto Sans CJK SC",
                        "Noto Sans CJK JP",
                        "Droid Sans Fallback",
                    ],
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

    /// Replace the preferred families for `generic`.
    ///
    /// The previous list is discarded, so `generic` resolves to `families` or
    /// to nothing. Use [`prefer`](Self::prefer) to keep the old list as a
    /// fallback for the case where `families` is not installed.
    pub fn set_generic(&mut self, generic: GenericFamily, families: Vec<String>) -> &mut Self {
        self.generic_families.insert(generic, families);
        self
    }

    /// Put `family` first among `generic`'s candidates, keeping the rest.
    ///
    /// This is the shape a desktop preference wants: the configured font wins
    /// when it is installed, and the built-in list still answers when it is
    /// not. An entry equal to `family` (ASCII case-insensitive) is moved to
    /// the front rather than duplicated.
    ///
    /// A generic with no list of its own inherits one from its
    /// [`parent`](GenericFamily::parent); preferring a family gives it a list
    /// of its own, so the inherited candidates are copied in behind `family`.
    pub fn prefer(&mut self, generic: GenericFamily, family: impl Into<String>) -> &mut Self {
        let family = family.into();
        let mut list = match self.generic_families.remove(&generic) {
            Some(list) => list,
            None => self.generic_candidates(generic).to_vec(),
        };
        list.retain(|f| !f.eq_ignore_ascii_case(&family));
        list.insert(0, family);
        self.generic_families.insert(generic, list);
        self
    }

    /// [`prefer`](Self::prefer) `family` for every generic in `generics`.
    pub fn prefer_for(
        &mut self,
        generics: &[GenericFamily],
        family: impl Into<String>,
    ) -> &mut Self {
        let family = family.into();
        for generic in generics {
            self.prefer(*generic, family.clone());
        }
        self
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

    /// Returns the preferred fallback families for a given unicode range.
    /// Checks the specified generic family first (if any), followed by
    /// generic-agnostic fallbacks. Results are deduplicated and keep their order.
    pub fn script_candidates(
        &self,
        generic: Option<GenericFamily>,
        block: &UnicodeRange,
    ) -> Vec<String> {
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

    /// Expands a CSS font stack and unicode ranges into a complete, ordered
    /// list of candidate font families to search for.
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
            let generic = if any_generic {
                None
            } else {
                Some(self.default_generic)
            };
            self.script_candidates(generic, block)
                .iter()
                .for_each(|f| push_unique(&mut out, f));
        }
        self.last_resort
            .iter()
            .for_each(|f| push_unique(&mut out, f));
        out
    }

    /// Merges missing configuration values from `defaults` into this config.
    /// Does not overwrite or reorder existing entries.
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

    /// Extract all unique font families listed in this fallback configuration.
    pub fn extract_all_families(&self) -> Vec<String> {
        let mut out = Vec::new();
        for families in self.generic_families.values() {
            families.iter().for_each(|f| push_unique(&mut out, f));
        }
        for replacements in self.substitutions.values() {
            replacements.iter().for_each(|f| push_unique(&mut out, f));
        }
        for entry in &self.script_fallbacks {
            entry.families.iter().for_each(|f| push_unique(&mut out, f));
        }
        self.last_resort
            .iter()
            .for_each(|f| push_unique(&mut out, f));
        out
    }

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

/// Static system font directories per OS.
/// All font directories (system + user-specific).
pub fn font_directories(os: OperatingSystem) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match os {
        OperatingSystem::MacOS => {
            dirs.push(PathBuf::from("/System/Library/Fonts"));
            dirs.push(PathBuf::from("/Library/Fonts"));
            dirs.push(PathBuf::from("/System/Library/AssetsV2"));
            if let Ok(home) = std::env::var("HOME") {
                dirs.push(PathBuf::from(format!("{}/Library/Fonts", home)));
            }
        }
        OperatingSystem::Linux => {
            dirs.push(PathBuf::from("/usr/share/fonts"));
            dirs.push(PathBuf::from("/usr/local/share/fonts"));
            if let Ok(home) = std::env::var("HOME") {
                dirs.push(PathBuf::from(format!("{}/.fonts", home)));
                dirs.push(PathBuf::from(format!("{}/.local/share/fonts", home)));
            }
        }
        OperatingSystem::Windows => {
            let system_root = std::env::var("SystemRoot")
                .or_else(|_| std::env::var("WINDIR"))
                .unwrap_or_else(|_| "C:\\Windows".to_string());
            let user_profile =
                std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\Default".to_string());
            dirs.push(PathBuf::from(format!("{}\\Fonts", system_root)));
            dirs.push(PathBuf::from(format!(
                "{}\\AppData\\Local\\Microsoft\\Windows\\Fonts",
                user_profile
            )));
        }
        OperatingSystem::Android => {
            dirs.push(PathBuf::from("/system/fonts"));
            dirs.push(PathBuf::from("/product/fonts"));
            dirs.push(PathBuf::from("/system_ext/fonts"));
            dirs.push(PathBuf::from("/data/fonts"));
        }
        OperatingSystem::IOS | OperatingSystem::Wasm => {}
    }

    dirs
}

/// Common font families for priority boosting, as human-readable names.
/// These families will be parsed first so likely-needed fonts are available sooner.
pub fn common_font_families(os: OperatingSystem) -> &'static [&'static str] {
    match os {
        OperatingSystem::MacOS => &[
            // System UI fonts (actual filenames use SFNS prefix)
            "San Francisco",
            "SFNS",
            "System Font",
            // Sans-serif
            "Helvetica Neue",
            "Helvetica",
            "Arial",
            "Lucida Grande",
            // Serif
            "Times New Roman",
            "Georgia",
            // Monospace
            "Menlo",
            "SF Mono",
            "Courier",
        ],
        OperatingSystem::Linux => &[
            // Sans-serif
            "DejaVu Sans",
            "Ubuntu",
            "Roboto",
            "Noto Sans",
            "Liberation Sans",
            "Droid Sans",
            "Arial",
            // Serif
            "DejaVu Serif",
            "Noto Serif",
            // Monospace
            "DejaVu Sans Mono",
        ],
        OperatingSystem::Windows => &[
            // Sans-serif
            "Segoe UI",
            "Arial",
            "Tahoma",
            "Verdana",
            // Serif
            "Times New Roman",
            "Calibri",
            // Monospace
            "Consolas",
            "Courier New",
        ],
        OperatingSystem::IOS => &[
            // System UI fonts (filenames use SFNS/SFUI prefix)
            "San Francisco",
            "SFNS",
            "SFNSDisplay",
            "SFNSText",
            "SFUI",
            ".AppleSystemUIFont",
            "System Font",
            // Sans-serif
            "Helvetica Neue",
            "Helvetica",
            "Avenir",
            "Avenir Next",
            // Serif
            "Times New Roman",
            "Georgia",
            // Monospace
            "Menlo",
            "SF Mono",
            "Courier",
        ],
        OperatingSystem::Android => &[
            // System UI fonts
            "Roboto",
            "Roboto Flex",
            "Roboto Condensed",
            // Sans-serif
            "Noto Sans",
            "Droid Sans",
            // Serif
            "Noto Serif",
            "Roboto Serif",
            "Droid Serif",
            // Monospace
            "Roboto Mono",
            "Droid Sans Mono",
            "Noto Sans Mono",
        ],
        OperatingSystem::Wasm => &[],
    }
}

/// Configuration for the font scanner: directories to search and families to prioritize.
/// Injected by the embedder via `FcFontRegistry::new_with_config`.
#[derive(Debug, Clone, PartialEq)]
pub struct FcScanConfig {
    /// Directories to scan recursively. Empty = scan nothing.
    pub font_dirs: Vec<PathBuf>,
    /// Human-readable family names whose files the scout parses first
    /// (they become token sets via [`tokenize_lowercase`]).
    pub priority_families: Vec<String>,
}

impl FcScanConfig {
    /// Returns the default OS-specific scan configuration.
    pub fn os_defaults(os: OperatingSystem) -> Self {
        Self {
            font_dirs: font_directories(os),
            priority_families: FcFallbackConfig::os_defaults(os).extract_all_families(),
        }
    }
    /// Returns an empty scan configuration.
    pub fn empty() -> Self {
        Self {
            font_dirs: Vec::new(),
            priority_families: Vec::new(),
        }
    }
    /// Pre-tokenizes priority families for faster matching against filenames.
    pub fn priority_token_sets(&self) -> Vec<Vec<String>> {
        self.priority_families
            .iter()
            .map(|family| tokenize_lowercase(family))
            .collect()
    }
}

/// Pre-tokenize common font families for efficient per-file matching.
/// Call this once before iterating over font files, then pass the result
/// to [`matches_common_family_tokens`] for each file.
pub fn tokenize_common_families(os: OperatingSystem) -> Vec<Vec<String>> {
    FcScanConfig::os_defaults(os).priority_token_sets()
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
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    tokenize_font_stem(stem).join("")
}

#[cfg(test)]
#[path = "config_test.rs"]
mod tests;
