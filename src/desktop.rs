//! Desktop font preferences.
//!
//! Two halves, deliberately separated:
//!
//! - [`FcDesktopFont::parse`] and [`FcDesktopFont::parse_qt`] turn the strings
//!   desktops store their font choices in into a family name and style. Pure
//!   string work: no processes, no files, no environment.
//! - [`FcDesktopFonts::detect`] asks the running desktop what those strings
//!   are — the XDG Settings Portal, then GNOME's `gsettings`, then KDE's
//!   `kdeglobals`. It spawns processes, so it lives behind the off-by-default
//!   `desktop-detect` feature and is never called on your behalf.
//!
//! Nothing here decides which generic a preference applies to; that is a
//! policy the embedder owns. Feed the result to
//! [`FcFallbackConfig::prefer_for`](crate::FcFallbackConfig::prefer_for):
//!
//! ```no_run
//! # use rust_fontconfig::*;
//! # fn f(cache: &FcFontCache, from_the_desktop: &str) -> Option<()> {
//! let ui = FcDesktopFont::parse(from_the_desktop)?;
//! cache.modify_fallback_config(|c| {
//!     c.prefer_for(&[GenericFamily::SystemUi, GenericFamily::UiSansSerif], ui.family);
//! });
//! # Some(()) }
//! ```

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::FcWeight;

/// Style keywords Pango accepts, lowercased and stripped of `-`.
/// Peeled off the end of a description before what remains is taken as the
/// family list.
const PANGO_STYLE_KEYWORDS: &[&str] = &[
    // style
    "normal",
    "roman",
    "oblique",
    "italic",
    // variant
    "smallcaps",
    "allsmallcaps",
    "petitecaps",
    "allpetitecaps",
    "unicase",
    "titlecaps",
    // weight
    "thin",
    "ultralight",
    "extralight",
    "light",
    "semilight",
    "demilight",
    "book",
    "regular",
    "medium",
    "semibold",
    "demibold",
    "bold",
    "ultrabold",
    "extrabold",
    "heavy",
    "black",
    "ultraheavy",
    "extrablack",
    // stretch
    "ultracondensed",
    "extracondensed",
    "condensed",
    "semicondensed",
    "semiexpanded",
    "expanded",
    "extraexpanded",
    "ultraexpanded",
    // gravity
    "notrotated",
    "south",
    "upsidedown",
    "north",
    "rotatedleft",
    "east",
    "rotatedright",
    "west",
];

/// Numeric weight for a Pango weight keyword, on the CSS scale.
fn pango_weight(key: &str) -> Option<u16> {
    Some(match key {
        "thin" => 100,
        "ultralight" | "extralight" => 200,
        "light" => 300,
        "semilight" | "demilight" => 350,
        "book" => 380,
        "regular" | "normal" => 400,
        "medium" => 500,
        "semibold" | "demibold" => 600,
        "bold" => 700,
        "ultrabold" | "extrabold" => 800,
        "heavy" | "black" => 900,
        "ultraheavy" | "extrablack" => 1000,
        _ => return None,
    })
}

/// Lowercase `token` and drop everything that is not ASCII alphanumeric, so
/// `Ultra-Bold`, `ultrabold` and `Ultra Bold`'s halves compare alike.
fn key_of(token: &str) -> String {
    token
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// A desktop's configured font, parsed out of the string the desktop stores.
///
/// Produced by [`FcDesktopFont::parse`] (Pango/GNOME) or
/// [`FcDesktopFont::parse_qt`] (Qt/KDE). Stretch, variant and gravity
/// keywords are recognized so they do not leak into the family name, but are
/// not reported: what a fallback configuration needs is the family.
#[derive(Debug, Clone, PartialEq)]
pub struct FcDesktopFont {
    /// The first family of [`families`](Self::families).
    pub family: String,
    /// Every family in the description, in order. Pango allows a
    /// comma-separated list; most desktops write exactly one.
    pub families: Vec<String>,
    /// [`FcWeight::Normal`] when the description names no weight.
    pub weight: FcWeight,
    /// The description asked for italic.
    pub italic: bool,
    /// The description asked for oblique.
    pub oblique: bool,
    /// Point size, when the description carries one. A `px`-suffixed absolute
    /// size is recognized and stripped, but reported as `None`.
    pub size_pt: Option<f32>,
}

impl FcDesktopFont {
    /// A font with `family` and no style.
    pub fn new(family: impl Into<String>) -> Self {
        let family = family.into();
        Self {
            families: alloc::vec![family.clone()],
            family,
            weight: FcWeight::Normal,
            italic: false,
            oblique: false,
            size_pt: None,
        }
    }

    /// Parse a Pango font description: `FAMILY-LIST [STYLE-OPTIONS] [SIZE]`.
    ///
    /// This is what GNOME's `org.gnome.desktop.interface font-name` and
    /// anything else built on Pango stores. Surrounding single or double
    /// quotes (as `gsettings get` prints them) are stripped, as is a trailing
    /// `@variations` suffix.
    ///
    /// Returns `None` when nothing is left to use as a family.
    ///
    /// ```
    /// # use rust_fontconfig::{FcDesktopFont, FcWeight};
    /// let f = FcDesktopFont::parse("'Cantarell Bold 11'").unwrap();
    /// assert_eq!(f.family, "Cantarell");
    /// assert_eq!(f.weight, FcWeight::Bold);
    /// assert_eq!(f.size_pt, Some(11.0));
    /// ```
    ///
    /// Style keywords are only peeled off the end, so a family that ends in
    /// one keeps it when nothing follows: `"Book Antiqua"` parses as the
    /// family `Book Antiqua`, while `"Noto Sans Light"` parses as `Noto Sans`
    /// at [`FcWeight::Light`]. Pango is ambiguous here in exactly the same
    /// way.
    pub fn parse(input: &str) -> Option<Self> {
        let input = input.trim();
        let input = input.trim_matches('\'').trim_matches('"').trim();
        // `@wght=700,wdth=100` — font variations, not part of the family.
        let input = input.split('@').next().unwrap_or(input).trim();
        if input.is_empty() {
            return None;
        }

        let mut tokens: Vec<&str> = input.split_whitespace().collect();
        let mut weight = FcWeight::Normal;
        let mut italic = false;
        let mut oblique = false;
        let mut size_pt = None;

        // Size, if the last token is one.
        if let Some(last) = tokens.last() {
            let (number, is_px) = match last.strip_suffix("px") {
                Some(n) => (n, true),
                None => (*last, false),
            };
            if !number.is_empty() {
                if let Ok(parsed) = number.parse::<f32>() {
                    if parsed.is_finite() && parsed > 0.0 {
                        if !is_px {
                            size_pt = Some(parsed);
                        }
                        tokens.pop();
                    }
                }
            }
        }

        // Style options, peeled off the end while a family would remain.
        while tokens.len() > 1 {
            let key = key_of(tokens[tokens.len() - 1]);
            if let Some(rest) = key.strip_prefix("weight") {
                // `Weight=380`, a numeric weight.
                if let Ok(value) = rest.parse::<u16>() {
                    weight = FcWeight::from_u16(value);
                    tokens.pop();
                    continue;
                }
            }
            if !PANGO_STYLE_KEYWORDS.contains(&key.as_str()) {
                break;
            }
            match key.as_str() {
                "italic" => italic = true,
                "oblique" => oblique = true,
                _ => {
                    if let Some(value) = pango_weight(&key) {
                        weight = FcWeight::from_u16(value);
                    }
                }
            }
            tokens.pop();
        }

        let families = split_families(&tokens.join(" "));
        let family = families.first()?.clone();
        Some(Self {
            family,
            families,
            weight,
            italic,
            oblique,
            size_pt,
        })
    }

    /// Parse a Pango description out of the GVariant text `gsettings` and
    /// `gdbus` print, then hand it to [`parse`](Self::parse).
    ///
    /// `gsettings get` prints a bare string, `gdbus call` wraps it in a tuple
    /// and one or two variant layers. The innermost single-quoted run is the
    /// value; text with no quotes at all is parsed as-is.
    ///
    /// ```
    /// # use rust_fontconfig::FcDesktopFont;
    /// // gsettings get org.gnome.desktop.interface font-name
    /// assert_eq!(FcDesktopFont::parse_gvariant("'Cantarell 11'\n").unwrap().family, "Cantarell");
    /// // gdbus call ... Settings.ReadOne
    /// assert_eq!(FcDesktopFont::parse_gvariant("(<'Cantarell 11'>,)").unwrap().family, "Cantarell");
    /// // gdbus call ... Settings.Read (double-wrapped)
    /// assert_eq!(FcDesktopFont::parse_gvariant("(<<'Cantarell 11'>>,)").unwrap().family, "Cantarell");
    /// ```
    pub fn parse_gvariant(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        let inner = match (raw.find('\''), raw.rfind('\'')) {
            (Some(start), Some(end)) if end > start => &raw[start + 1..end],
            _ => raw,
        };
        Self::parse(inner)
    }

    /// Parse a Qt font description: `family,pointSize,pixelSize,styleHint,weight,italic,...`.
    ///
    /// This is what KDE writes into `kdeglobals` (`[General] font=`, `fixed=`)
    /// and what `QFont::toString` produces. Fields after the ones named are
    /// ignored, and missing fields are allowed.
    ///
    /// ```
    /// # use rust_fontconfig::{FcDesktopFont, FcWeight};
    /// let f = FcDesktopFont::parse_qt("Noto Sans,10,-1,5,50,0,0,0,0,0").unwrap();
    /// assert_eq!(f.family, "Noto Sans");
    /// assert_eq!(f.weight, FcWeight::Normal);
    /// assert_eq!(f.size_pt, Some(10.0));
    /// ```
    ///
    /// Qt writes weight on its own 0-99 scale in the older format and on the
    /// CSS 1-1000 scale in the newer one. A value at or below 99 is read as
    /// the Qt scale.
    pub fn parse_qt(input: &str) -> Option<Self> {
        let input = input.trim();
        let mut fields = input.split(',');
        let family = fields.next()?.trim().to_string();
        if family.is_empty() {
            return None;
        }

        let field = |fields: &mut core::str::Split<char>| -> Option<f32> {
            fields.next()?.trim().parse::<f32>().ok()
        };

        let size_pt = field(&mut fields).filter(|s| *s > 0.0);
        let _pixel_size = field(&mut fields);
        let _style_hint = field(&mut fields);
        let weight = match field(&mut fields) {
            Some(w) if w > 0.0 => FcWeight::from_u16(qt_weight(w)),
            _ => FcWeight::Normal,
        };
        let italic = matches!(field(&mut fields), Some(v) if v != 0.0);

        Some(Self {
            families: alloc::vec![family.clone()],
            family,
            weight,
            italic,
            oblique: false,
            size_pt,
        })
    }
}

/// Qt weights are 0-99 in the legacy format and 1-1000 in the current one.
fn qt_weight(value: f32) -> u16 {
    let value = value.round().clamp(0.0, 1000.0) as u16;
    if value > 99 {
        return value;
    }
    // Qt's legacy anchors: Light 25, Normal 50, DemiBold 63, Bold 75, Black 87.
    match value {
        0..=12 => 100,
        13..=24 => 200,
        25..=37 => 300,
        38..=56 => 400,
        57..=62 => 500,
        63..=69 => 600,
        70..=80 => 700,
        81..=86 => 800,
        _ => 900,
    }
}

/// Split a comma-separated family list, dropping empty entries.
fn split_families(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|f| f.trim().trim_matches('"').trim())
        .filter(|f| !f.is_empty())
        .map(|f| f.to_string())
        .collect()
}

/// What a desktop was asked for its font choices.
///
/// Every field is `None` when the desktop did not answer. Which generic each
/// role belongs to is the embedder's decision — see the module docs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FcDesktopFonts {
    /// The interface font (GNOME `font-name`, KDE `[General] font`).
    pub ui: Option<FcDesktopFont>,
    /// The document font (GNOME `document-font-name`).
    pub document: Option<FcDesktopFont>,
    /// The fixed-width font (GNOME `monospace-font-name`, KDE `[General] fixed`).
    pub monospace: Option<FcDesktopFont>,
}

impl FcDesktopFonts {
    /// Parse the `[General]` section of a `kdeglobals` file.
    ///
    /// Pure string work — [`from_kdeglobals`](Self::from_kdeglobals) is the
    /// one that goes looking for the file.
    pub fn from_kdeglobals_str(text: &str) -> Self {
        let mut out = Self::default();
        let mut in_general = false;
        for line in text.lines() {
            let line = line.trim();
            if let Some(section) = line.strip_prefix('[') {
                in_general = section
                    .trim_end_matches(']')
                    .eq_ignore_ascii_case("General");
                continue;
            }
            if !in_general {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let parsed = FcDesktopFont::parse_qt(value);
            match key.trim() {
                "font" => out.ui = parsed,
                "fixed" => out.monospace = parsed,
                _ => {}
            }
        }
        out
    }
}

impl FcDesktopFonts {
    /// Fill this set's empty roles from `other`, keeping what is already set.
    ///
    /// Sources answer partially — `kdeglobals` has no document font — so
    /// [`detect`](Self::detect) layers them instead of taking the first one
    /// that answers at all.
    pub fn fill_from(&mut self, other: Self) -> &mut Self {
        if self.ui.is_none() {
            self.ui = other.ui;
        }
        if self.document.is_none() {
            self.document = other.document;
        }
        if self.monospace.is_none() {
            self.monospace = other.monospace;
        }
        self
    }
}

#[cfg(feature = "desktop-detect")]
impl FcDesktopFonts {
    /// Ask the running desktop for its configured fonts.
    ///
    /// **This talks to the system.** Nothing in this crate calls it for you —
    /// `FcFontCache::build` and `FcFontRegistry::new` never spawn a process.
    ///
    /// On Linux it consults, in order, filling only what is still unset:
    ///
    /// | Source | How | Notes |
    /// |---|---|---|
    /// | [XDG Settings Portal](Self::from_xdg_portal) | `gdbus call --session --dest org.freedesktop.portal.Desktop …` | The one that is right inside a Flatpak or Snap sandbox |
    /// | [GNOME](Self::from_gsettings) | `gsettings get org.gnome.desktop.interface font-name` | Reports the sandbox's own values when sandboxed, hence the order |
    /// | [KDE](Self::from_kdeglobals) | reads `$XDG_CONFIG_HOME/kdeglobals` | Tried first when `$XDG_CURRENT_DESKTOP` names KDE |
    ///
    /// Every other platform answers with [`FcDesktopFonts::default`]: macOS
    /// does not let the user change the UI font, and Windows keeps its choice
    /// in a binary `LOGFONT` that needs a Win32 call this crate will not make.
    /// Use [`FcDesktopFont::new`] with what your own code found there.
    ///
    /// There is no `fc-match` step: since 5.0 the `fonts.conf` tree is parsed
    /// directly, so `FcFontCache::build` already knows what `fc-match` would
    /// have said.
    pub fn detect() -> Self {
        #[cfg(all(target_os = "linux", not(target_family = "wasm")))]
        {
            let kde_first = std::env::var("XDG_CURRENT_DESKTOP")
                .map(|v| v.to_ascii_uppercase().contains("KDE"))
                .unwrap_or(false);
            let mut out = Self::default();
            if kde_first {
                out.fill_from(Self::from_kdeglobals());
            }
            out.fill_from(Self::from_xdg_portal());
            out.fill_from(Self::from_gsettings());
            if !kde_first {
                out.fill_from(Self::from_kdeglobals());
            }
            out
        }
        #[cfg(not(all(target_os = "linux", not(target_family = "wasm"))))]
        {
            Self::default()
        }
    }

    /// The XDG Settings Portal, over `gdbus`.
    ///
    /// ```text
    /// gdbus call --session --dest org.freedesktop.portal.Desktop \
    ///   --object-path /org/freedesktop/portal/desktop \
    ///   --method org.freedesktop.portal.Settings.ReadOne \
    ///   org.gnome.desktop.interface font-name
    /// ```
    ///
    /// This is the only source that reports the *host's* settings from inside
    /// a Flatpak or Snap sandbox. Portals older than version 2 have no
    /// `ReadOne`, so `Read` is tried as well.
    #[cfg(all(target_os = "linux", not(target_family = "wasm")))]
    pub fn from_xdg_portal() -> Self {
        Self {
            ui: portal_font("font-name"),
            document: portal_font("document-font-name"),
            monospace: portal_font("monospace-font-name"),
        }
    }

    /// GNOME and anything else on the `org.gnome.desktop.interface` schema.
    ///
    /// ```text
    /// gsettings get org.gnome.desktop.interface font-name
    /// ```
    ///
    /// Inside a sandbox this answers with the sandbox's own values, not the
    /// host's — [`from_xdg_portal`](Self::from_xdg_portal) is asked first for
    /// that reason.
    #[cfg(all(target_os = "linux", not(target_family = "wasm")))]
    pub fn from_gsettings() -> Self {
        Self {
            ui: gsettings_font("font-name"),
            document: gsettings_font("document-font-name"),
            monospace: gsettings_font("monospace-font-name"),
        }
    }

    /// KDE's `kdeglobals`, under `$XDG_CONFIG_HOME` (or `~/.config`).
    ///
    /// Reads `[General] font` and `[General] fixed`. KDE stores no document
    /// font, so that role stays `None`.
    #[cfg(all(target_os = "linux", not(target_family = "wasm")))]
    pub fn from_kdeglobals() -> Self {
        let path = match std::env::var_os("XDG_CONFIG_HOME") {
            Some(dir) if !dir.is_empty() => std::path::PathBuf::from(dir),
            _ => match std::env::var_os("HOME") {
                Some(home) => std::path::PathBuf::from(home).join(".config"),
                None => return Self::default(),
            },
        }
        .join("kdeglobals");
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::from_kdeglobals_str(&text),
            Err(_) => Self::default(),
        }
    }
}

/// Run `command` with `args` and parse its stdout as a desktop font.
#[cfg(all(
    feature = "desktop-detect",
    target_os = "linux",
    not(target_family = "wasm")
))]
fn command_font(command: &str, args: &[&str]) -> Option<FcDesktopFont> {
    let output = std::process::Command::new(command)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    FcDesktopFont::parse_gvariant(core::str::from_utf8(&output.stdout).ok()?)
}

/// One `gsettings get org.gnome.desktop.interface <key>`.
#[cfg(all(
    feature = "desktop-detect",
    target_os = "linux",
    not(target_family = "wasm")
))]
fn gsettings_font(key: &str) -> Option<FcDesktopFont> {
    command_font("gsettings", &["get", "org.gnome.desktop.interface", key])
}

/// One `org.freedesktop.portal.Settings` read, `ReadOne` then `Read`.
#[cfg(all(
    feature = "desktop-detect",
    target_os = "linux",
    not(target_family = "wasm")
))]
fn portal_font(key: &str) -> Option<FcDesktopFont> {
    const PORTAL: &[&str] = &[
        "call",
        "--session",
        "--dest",
        "org.freedesktop.portal.Desktop",
        "--object-path",
        "/org/freedesktop/portal/desktop",
        "--method",
    ];
    for method in [
        "org.freedesktop.portal.Settings.ReadOne",
        "org.freedesktop.portal.Settings.Read",
    ] {
        let mut args = PORTAL.to_vec();
        args.push(method);
        args.push("org.gnome.desktop.interface");
        args.push(key);
        if let Some(font) = command_font("gdbus", &args) {
            return Some(font);
        }
    }
    None
}
