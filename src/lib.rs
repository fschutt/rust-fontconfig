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

extern crate xmlparser;
extern crate mmapio;
extern crate allsorts;

extern crate core;
extern crate alloc;

#[cfg(feature = "std")]
use std::thread;
#[cfg(feature = "std")]
use std::path::PathBuf;
use alloc::string::String;
use alloc::collections::btree_map::BTreeMap;

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
    pub name: Option<String>,
    pub family: Option<String>,
    pub italic: PatternMatch,
    pub oblique: PatternMatch,
    pub bold: PatternMatch,
    pub monospace: PatternMatch,
}

#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq)]
#[repr(C)]
pub struct FcFontPath {
    pub path: String,
    pub font_index: usize,
}

#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq)]
pub struct FcFontCache {
    map: BTreeMap<FcPattern, FcFontPath>
}

impl FcFontCache {

    /// Builds a new font cache from all fonts discovered on the system
    ///
    /// NOTE: Performance-intensive, should only be called on startup!
    #[cfg(feature = "std")]
    pub fn build() -> Self {

        #[cfg(target_os = "linux")] {
            FcFontCache {
                map: FcScanDirectories().unwrap_or_default().into_iter().collect()
            }
        }

        #[cfg(target_os = "windows")] {
            FcFontCache {
                map: FcScanSingleDirectoryRecursive(PathBuf::from("C:\\Windows\\Fonts\\"))
                .unwrap_or_default().into_iter().collect()
            }
        }

        #[cfg(target_os = "macos")] {
            FcFontCache {
                map: FcScanSingleDirectoryRecursive(PathBuf::from("~/Library/Fonts"))
                .unwrap_or_default().into_iter().collect()
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

        let result1 = self.map
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

    use std::path::Path;
    use xmlparser::Tokenizer;
    use xmlparser::Token::*;
    use std::fs;

    let fontconfig_path = Path::new("/etc/fonts/fonts.conf");

    if !fontconfig_path.exists() {
        return None;
    }

    // let file = File::open(fontconfig_path).ok()?;
    let xml_utf8 = fs::read_to_string(fontconfig_path).ok()?;
    // let mmap = unsafe { MmapOptions::new().map(&file).ok()? };
    // let xml_utf8 = std::str::from_utf8(&file[..]).ok()?;

    let mut font_paths_count = 0;
    let mut font_paths = [(None, "");32];

    let mut current_prefix: Option<&str> = None;
    let mut current_dir: Option<&str> = None;
    let mut is_in_dir = false;

    for token in Tokenizer::from(xml_utf8.as_str()) {
        let token = token.ok()?;
        match token {
            ElementStart { local, .. } => {
                if local.as_str() != "dir" {
                    continue;
                }

                if is_in_dir { return None; /* error: nested <dir></dir> tags */ }
                is_in_dir = true;
                current_dir = None;
            },
            Text { text, .. } => {
                let text = text.as_str().trim();
                if text.is_empty() {
                    continue;
                }
                if is_in_dir {
                    current_dir = Some(text);
                }
            },
            Attribute { local, value, .. } => {
                if !is_in_dir {
                    continue;
                }
                // attribute on <dir node>
                if local.as_str() == "prefix" {
                    current_prefix = Some(value.as_str());
                }
            },
            ElementEnd { end, .. } => {
                let end_tag = match end {
                    xmlparser::ElementEnd::Close(_, a) => a,
                    _ => continue,
                };

                if end_tag.as_str() != "dir" {
                    continue;
                }

                if !is_in_dir {
                    continue;
                }

                if let Some(d) = current_dir.as_ref() {
                    if font_paths_count >= font_paths.len() {
                        return None; // error: exceeded maximum number of font paths
                    }

                    font_paths[font_paths_count] = (current_prefix, d);
                    font_paths_count += 1;
                    is_in_dir = false;
                    current_dir = None;
                    current_prefix = None;
                }
            },
            _ => { },
        }
    }

    let font_paths = &font_paths[0..font_paths_count];

    if font_paths.is_empty() {
        return None;
    }

    FcScanDirectoriesInner(font_paths)
}

#[cfg(feature = "std")]
fn FcScanDirectoriesInner(paths: &[(Option<&str>, &str)]) -> Option<Vec<(FcPattern, FcFontPath)>> {

    // scan directories in parallel
    let mut threads = (0..32).map(|_| None).collect::<Vec<_>>();
    let mut result = Vec::new();

    for (p_id, (prefix, p)) in paths.iter().enumerate() {
        let mut path = match prefix {
            // "xdg" => ,
            None => PathBuf::new(),
            Some(s) => PathBuf::from(s),
        };
        path.push(p);
        threads[p_id] = Some(thread::spawn(move || FcScanSingleDirectoryRecursive(path)));
    }

    for t in threads.iter_mut() {

        let t_result = match t.take() {
            Some(s) => s,
            None => continue,
        };

        let mut t_result = t_result.join().ok()?;

        match &mut t_result {
            Some(c) => { result.append(c); },
            None => { },
        }
    }

    Some(result)
}

#[cfg(feature = "std")]
fn FcScanSingleDirectoryRecursive(dir: PathBuf)-> Option<Vec<(FcPattern, FcFontPath)>> {

    // NOTE: Threads are fast, especially on linux. USE THEM.
    let mut threads = Vec::new();

    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        let pathbuf = path.to_path_buf();
        if path.is_dir() {
            threads.push(Some(thread::spawn(move || FcScanSingleDirectoryRecursive(pathbuf))));
        } else {
            threads.push(Some(thread::spawn(move || FcParseFont(pathbuf))));
        }
    }

    let mut results = Vec::new();

    for t in threads.iter_mut() {
        let mut t_result = t.take().and_then(|q| q.join().ok().and_then(|o| o)).unwrap_or_default();
        results.append(&mut t_result);
    }

    Some(results)
}

#[cfg(feature = "std")]
fn FcParseFont(filepath: PathBuf)-> Option<Vec<(FcPattern, FcFontPath)>> {

    use allsorts::{
        tag,
        binary::read::ReadScope,
        font_data::FontData,
        tables::{
            FontTableProvider, NameTable, HeadTable,
        }
    };
    use std::fs::File;
    use mmapio::MmapOptions;
    use std::collections::BTreeSet;
    use allsorts::get_name::fontcode_get_name;

    const FONT_SPECIFIER_NAME_ID: u16 = 4;
    const FONT_SPECIFIER_FAMILY_ID: u16 = 1;

    // font_index = 0 - TODO: iterate through fonts in font file properly!
    let font_index = 0;

    // try parsing the font file and see if the postscript name matches
    let file = File::open(filepath.clone()).ok()?;
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

    let patterns = name_table.name_records
    .iter() // TODO: par_iter
    .filter_map(|name_record| {
        let name_id = name_record.name_id;
        if name_id == FONT_SPECIFIER_FAMILY_ID {
            let family = fontcode_get_name(&name_data, FONT_SPECIFIER_FAMILY_ID).ok()??;
            f_family = Some(family.to_string_lossy().to_string());
            None
        } else if name_id == FONT_SPECIFIER_NAME_ID {
            let family = f_family.as_ref()?;
            let name = fontcode_get_name(&name_data, FONT_SPECIFIER_NAME_ID).ok()??;
            let name = name.to_string_lossy().to_string();
            if name.is_empty() {
                None
            } else {
                Some((FcPattern {
                    name: Some(name),
                    family: Some(family.clone()),
                    bold: if is_bold { PatternMatch::True } else { PatternMatch::False },
                    italic: if is_italic { PatternMatch::True } else { PatternMatch::False },
                    .. Default::default() // TODO!
                }, font_index))
            }
        } else {
            None
        }
    }).collect::<BTreeSet<_>>();

    Some(
        patterns
        .into_iter()
        .map(|(pat, index)| (pat, FcFontPath {
            path: filepath.clone().to_string_lossy().to_string(),
            font_index: index
        }))
        .collect()
    )
}
