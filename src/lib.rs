extern crate xmlparser;
extern crate mmapio;
extern crate allsorts;
extern crate ttf_parser;

use std::thread;
use std::path::PathBuf;

#[derive(Clone)]
#[repr(C)]
pub struct FcPattern {
    pub postscript_name: String,
    pub italic: bool,
    pub oblique: bool,
    pub bold: bool,
    pub monospace: bool,
}

impl Default for FcPattern {
    fn default() -> Self {
        FcPattern {
            postscript_name: String::new(),
            italic: false,
            oblique: false,
            bold: false,
            monospace: false,
        }
    }
}

#[derive(Clone)]
#[repr(C)]
pub struct FcFontPath {
    pub path: PathBuf,
}

#[no_mangle]
pub extern "C" fn FcLocateFont(ptr: *mut FcFontPath, pattern: FcPattern) -> bool {
    match FcLocateFontInner(pattern) {
        Some(s) => {
            unsafe { (*ptr).path = s.path; }
            true
        },
        None => false
    }
}

pub fn FcLocateFontInner(pattern: FcPattern) -> Option<FcFontPath> {

    use std::path::Path;
    use std::fs::File;
    use mmapio::MmapOptions;
    use xmlparser::Tokenizer;
    use xmlparser::Token::*;
    use std::io::Read;
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

    FcScanDirectories(font_paths, pattern)
}

pub fn FcScanDirectories(paths: &[(Option<&str>, &str)], pattern: FcPattern) -> Option<FcFontPath> {

    // scan directories in parallel
    let mut threads = (0..32).map(|_| None).collect::<Vec<_>>();

    for (p_id, (prefix, p)) in paths.iter().enumerate() {
        let mut path = match prefix {
            // "xdg" => ,
            None => PathBuf::new(),
            Some(s) => PathBuf::from(s),
        };
        path.push(p);
        let pattern_clone = pattern.clone();
        threads[p_id] = Some(thread::spawn(move || FcScanSingleDirectoryRecursive(path, pattern_clone)));
    }

    for t in threads.iter_mut() {
        let t_result = match t.take() {
            Some(s) => s,
            None => continue,
        };
        let t_result = t_result.join().ok()?;
        match t_result {
            Some(c) => return Some(c),
            None => { },
        }
    }

    None
}

pub fn FcScanSingleDirectoryRecursive(dir: PathBuf, pattern: FcPattern)-> Option<FcFontPath> {

    // NOTE: threads are fast, especially on linux. USE THEM.
    let mut threads = Vec::new();

    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        let pathbuf = path.to_path_buf();
        let pattern_clone = pattern.clone();
        if path.is_dir() {
            threads.push(thread::spawn(move || FcScanSingleDirectoryRecursive(pathbuf, pattern_clone)));
        } else {
            threads.push(thread::spawn(move || FcFontMatchesPattern(pathbuf, pattern_clone)));
        }
    }

    for t in threads {
        let t_result = t.join().ok()?;
        match t_result {
            Some(c) => { return Some(c); },
            None => { },
        }
    }

    None
}

/*

*/
pub fn FcFontMatchesPattern(filepath: PathBuf, pattern: FcPattern)-> Option<FcFontPath> {

    use allsorts::{
        tag,
        binary::read::ReadScope,
        font_data::FontData,
        layout::{LayoutCache, GDEFTable, GPOS, GSUB},
        tables::{
            FontTableProvider, HheaTable, MaxpTable, HeadTable,
            loca::LocaTable,
            cmap::CmapSubtable,
            glyf::{GlyfTable, Glyph, GlyfRecord},
        },
        tables::cmap::owned::CmapSubtable as OwnedCmapSubtable,
    };
    use std::fs::File;
    use mmapio::MmapOptions;

    // font_index = 0 - TODO!
    let font_index = 0;

    // try parsing the font file and see if the postscript name matches
    let file = File::open(filepath.clone()).ok()?;
    let font_bytes = unsafe { MmapOptions::new().map(&file).ok()? };
    let scope = ReadScope::new(&font_bytes[..]);
    let font_file = scope.read::<FontData<'_>>().ok()?;
    let provider = font_file.table_provider(font_index).ok()?;

    // parse the font from owned-ttf-parser, to get the outline
    // parsing the outline needs the following tables:
    //
    //     required:
    //          head, hhea, maxp, glyf, cff1, loca
    //     optional for variable fonts:
    //          gvar, cff2


    let head_data = provider.table_data(tag::HEAD).ok()??.into_owned();
    let maxp_data = provider.table_data(tag::MAXP).ok()??.into_owned();
    let loca_data = provider.table_data(tag::LOCA).ok()??.into_owned();
    let glyf_data = provider.table_data(tag::GLYF).ok()??.into_owned();
    let hhea_data = provider.table_data(tag::HHEA).ok()??.into_owned();
    let name_data = provider.table_data(tag::NAME).ok()??.into_owned();

    // required tables first
    let mut outline_font_tables = vec![
        Ok((ttf_parser::Tag::from_bytes(b"head"), Some(head_data.as_ref()))),
        Ok((ttf_parser::Tag::from_bytes(b"loca"), Some(loca_data.as_ref()))),
        Ok((ttf_parser::Tag::from_bytes(b"hhea"), Some(hhea_data.as_ref()))),
        Ok((ttf_parser::Tag::from_bytes(b"maxp"), Some(maxp_data.as_ref()))),
        Ok((ttf_parser::Tag::from_bytes(b"glyf"), Some(glyf_data.as_ref()))),
        Ok((ttf_parser::Tag::from_bytes(b"name"), Some(name_data.as_ref()))),
    ];

    let face_tables = ttf_parser::FaceTables::from_table_provider(
        outline_font_tables.into_iter()
    ).ok()?;

    face_tables
    .names()
    .find_map(|name| {
        let name = name.to_string()?;
        if name.as_str() == &pattern.postscript_name {
            Some(FcFontPath { path: filepath.clone() })
        } else {
            None
        }
    })
}
