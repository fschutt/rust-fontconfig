//! Library for getting and matching system fonts with
//! minimal dependencies
//!
//! # Usage
//!
//! ```rust
//! use rust_fontconfig::{FcFontCache, FcPattern};
//!
//! fn main() {
//!
//!     let cache = FcFontCache::build();
//!     let result = cache.query(&FcPattern {
//!         name: Some(String::from("Arial")),
//!         .. Default::default()
//!     });
//!
//!     println!("font path: {:?}", result);
//! }
//! ```

#![allow(non_snake_case)]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate allsorts_no_std;
extern crate mmapio;
extern crate xmlparser;

extern crate alloc;
extern crate core;

use alloc::collections::btree_map::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::path::PathBuf;

#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq)]
#[repr(C)]
pub enum PatternMatch {
    True,
    False,
    DontCare,
}

impl PatternMatch {
    fn into_option(&self) -> Option<bool> {
        match self {
            PatternMatch::True => Some(true),
            PatternMatch::False => Some(false),
            PatternMatch::DontCare => None,
        }
    }
}

impl Default for PatternMatch {
    fn default() -> Self {
        PatternMatch::DontCare
    }
}

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
    pub weight: usize,
    // start..end unicode range
    pub unicode_range: [usize; 2],
}

#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq)]
#[repr(C)]
pub struct FcFontPath {
    pub path: String,
    pub font_index: usize,
}

#[derive(Debug, Default, Clone, PartialOrd, Ord, PartialEq, Eq)]
pub struct FcFontCache {
    map: BTreeMap<FcPattern, FcFontPath>,
}

impl FcFontCache {
    /// Builds a new font cache from all fonts discovered on the system
    ///
    /// NOTE: Performance-intensive, should only be called on startup!
    #[cfg(feature = "std")]
    pub fn build() -> Self {
        #[cfg(target_os = "linux")]
        {
            FcFontCache {
                map: FcScanDirectories()
                    .unwrap_or_default()
                    .into_iter()
                    .collect(),
            }
        }

        #[cfg(target_os = "windows")]
        {
            FcFontCache {
                map: FcScanSingleDirectoryRecursive(PathBuf::from("C:\\Windows\\Fonts\\"))
                    .into_iter()
                    .collect(),
            }
        }

        #[cfg(target_os = "macos")]
        {
            FcFontCache {
                map: FcScanSingleDirectoryRecursive(PathBuf::from("~/Library/Fonts"))
                    .into_iter()
                    .collect(),
            }
        }
    }

    /// Returns the list of fonts and font patterns
    pub fn list(&self) -> &BTreeMap<FcPattern, FcFontPath> {
        &self.map
    }

    /// Queries a font from the in-memory `font -> file` mapping
    pub fn query(&self, pattern: &FcPattern) -> Option<&FcFontPath> {
        let name_needs_to_match = pattern.name.is_some();
        let family_needs_to_match = pattern.family.is_some();

        let italic_needs_to_match = pattern.italic.into_option();
        let oblique_needs_to_match = pattern.oblique.into_option();
        let bold_needs_to_match = pattern.bold.into_option();
        let monospace_needs_to_match = pattern.monospace.into_option();

        let result1 = self
            .map
            .iter() // TODO: par_iter!
            .find(|(k, _)| {
                let name_matches = k.name == pattern.name;
                let family_matches = k.family == pattern.family;
                let italic_matches = k.italic == pattern.italic;
                let oblique_matches = k.oblique == pattern.oblique;
                let bold_matches = k.bold == pattern.bold;
                let monospace_matches = k.monospace == pattern.monospace;

                if name_needs_to_match && !name_matches {
                    return false;
                }

                if family_needs_to_match && !family_matches {
                    return false;
                }

                if let Some(italic_m) = italic_needs_to_match {
                    if italic_matches != italic_m {
                        return false;
                    }
                }

                if let Some(oblique_m) = oblique_needs_to_match {
                    if oblique_matches != oblique_m {
                        return false;
                    }
                }

                if let Some(bold_m) = bold_needs_to_match {
                    if bold_matches != bold_m {
                        return false;
                    }
                }

                if let Some(monospace_m) = monospace_needs_to_match {
                    if monospace_matches != monospace_m {
                        return false;
                    }
                }

                true
            });

        if let Some((_, r1)) = result1.as_ref() {
            return Some(r1);
        }

        None
    }
}

#[cfg(feature = "std")]
fn FcScanDirectories() -> Option<Vec<(FcPattern, FcFontPath)>> {
    use std::fs;

    let mut font_paths = Vec::with_capacity(32);
    let mut fontconfig_file_paths = Vec::with_capacity(32);
    let mut include_paths = Vec::with_capacity(32);

    fontconfig_file_paths.push("/etc/fonts/fonts.conf".into());

    loop {
        for fontconfig_path in fontconfig_file_paths.drain(..) {
            if let Ok(xml_utf8) = fs::read_to_string(fontconfig_path) {
                ParseFontsConf(&xml_utf8, &mut font_paths, &mut include_paths);
            } else {
                continue;
            }
        }

        if include_paths.is_empty() {
            break;
        }

        for include_path in include_paths.drain(..) {
            if let Ok(dir) = fs::read_dir(include_path) {
                for entry in dir {
                    if let Ok(entry) = entry {
                        if entry.file_type().map_or(false, |t| t.is_symlink() || t.is_file()) {
                            fontconfig_file_paths.push(entry.path());
                        }
                    }
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
//
// NOTE: This function also works on no_std
fn ParseFontsConf(
    input: &str,
    font_paths: &mut Vec<(Option<String>, String)>,
    include_paths: &mut Vec<String>,
) {
    use xmlparser::Token::*;
    use xmlparser::Tokenizer;

    let mut current_prefix: Option<&str> = None;
    let mut current_text: Option<&str> = None;
    let mut is_in_dir = false;
    let mut is_in_include = false;

    for token in Tokenizer::from(input) {
        let token = match token {
            Ok(t) => t,
            _ => return,
        };
        match token {
            ElementStart { local, .. } => {
                match local.as_str() {
                    "dir" => {
                        if is_in_dir {
                            return; /* error: nested <dir></dir> tags */
                        }
                        is_in_dir = true;
                        current_text = None;
                    }
                    "include" => {
                        if is_in_include {
                            return; /* error: nested <include></include> tags */
                        }
                        is_in_include = true;
                        current_text = None;
                    }
                    _ => continue,
                }
            }
            Text { text, .. } => {
                let text = text.as_str().trim();
                if text.is_empty() {
                    continue;
                }
                if is_in_dir {
                    current_text = Some(text);
                } else if is_in_include {
                    current_text = Some(text);
                }
            }
            Attribute { local, value, .. } => {
                if !is_in_dir {
                    continue;
                }
                // attribute on <dir node>
                if local.as_str() == "prefix" {
                    current_prefix = Some(value.as_str());
                }
            }
            ElementEnd { end, .. } => {
                let end_tag = match end {
                    xmlparser::ElementEnd::Close(_, a) => a,
                    _ => continue,
                };

                match end_tag.as_str() {
                    "dir" if is_in_dir => {
                        if let Some(d) = current_text {
                            font_paths.push((current_prefix.map(Into::into), d.into()));
                            current_text = None;
                            current_prefix = None;
                        }
                        is_in_dir = false;
                    }
                    "include" if is_in_include => {
                        if let Some(d) = current_text {
                            include_paths.push(d.into());
                            current_text = None;
                        }
                        is_in_include = false;
                    }
                    _ => continue,
                }
            }
            _ => {}
        }
    }
}

#[cfg(feature = "std")]
fn FcScanDirectoriesInner(paths: &[(Option<String>, String)]) -> Vec<(FcPattern, FcFontPath)> {
    use rayon::prelude::*;

    // scan directories in parallel
    paths
        .par_iter()
        .flat_map(|(prefix, p)| {
            let mut path = match prefix {
                // "xdg" => ,
                None => PathBuf::new(),
                Some(s) => PathBuf::from(s),
            };
            path.push(p);
            FcScanSingleDirectoryRecursive(path)
        })
        .collect()
}

#[cfg(feature = "std")]
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

#[cfg(feature = "std")]
fn FcParseFontFiles(files_to_parse: &[PathBuf]) -> Vec<(FcPattern, FcFontPath)> {
    use rayon::prelude::*;

    let result = files_to_parse
        .par_iter()
        .filter_map(|file| FcParseFont(file))
        .collect::<Vec<Vec<_>>>();

    result.into_iter().flat_map(|f| f.into_iter()).collect()
}

#[cfg(feature = "std")]
fn FcParseFont(filepath: &PathBuf) -> Option<Vec<(FcPattern, FcFontPath)>> {
    use allsorts_no_std::{
        binary::read::ReadScope,
        font_data::FontData,
        get_name::fontcode_get_name,
        tables::{FontTableProvider, HeadTable, NameTable},
        tag,
    };
    use mmapio::MmapOptions;
    use std::collections::BTreeSet;
    use std::fs::File;

    const FONT_SPECIFIER_NAME_ID: u16 = 4;
    const FONT_SPECIFIER_FAMILY_ID: u16 = 1;

    // font_index = 0 - TODO: iterate through fonts in font file properly!
    let font_index = 0;

    // try parsing the font file and see if the postscript name matches
    let file = File::open(filepath).ok()?;
    let font_bytes = unsafe { MmapOptions::new().map(&file).ok()? };
    let scope = ReadScope::new(&font_bytes[..]);
    let font_file = scope.read::<FontData<'_>>().ok()?;
    let provider = font_file.table_provider(font_index).ok()?;

    let head_data = provider.table_data(tag::HEAD).ok()??.into_owned();
    let head_table = ReadScope::new(&head_data).read::<HeadTable>().ok()?;

    let is_bold = head_table.is_bold();
    let is_italic = head_table.is_italic();

    let name_data = provider.table_data(tag::NAME).ok()??.into_owned();
    let name_table = ReadScope::new(&name_data).read::<NameTable>().ok()?;

    // one font can support multiple patterns
    let mut f_family = None;

    let patterns = name_table
        .name_records
        .iter() // TODO: par_iter
        .filter_map(|name_record| {
            let name_id = name_record.name_id;
            if name_id == FONT_SPECIFIER_FAMILY_ID {
                let family = fontcode_get_name(&name_data, FONT_SPECIFIER_FAMILY_ID).ok()??;
                f_family = Some(family);
                None
            } else if name_id == FONT_SPECIFIER_NAME_ID {
                let family = f_family.as_ref()?;
                let name = fontcode_get_name(&name_data, FONT_SPECIFIER_NAME_ID).ok()??;
                if name.is_empty() {
                    None
                } else {
                    Some((
                        FcPattern {
                            name: Some(name),
                            family: Some(family.clone()),
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
                            ..Default::default() // TODO!
                        },
                        font_index,
                    ))
                }
            } else {
                None
            }
        })
        .collect::<BTreeSet<_>>();

    Some(
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
            .collect(),
    )
}
