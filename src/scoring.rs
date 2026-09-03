//! Priority queue types and scoring heuristics for font build jobs.

use alloc::collections::btree_map::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use std::collections::HashSet;
use std::path::PathBuf;

use crate::config;
use crate::utils::normalize_family_name;
use crate::FcPattern;

// ── Priority Queue Types ────────────────────────────────────────────────────

/// Priority levels for font build jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
/// Everything else found by Scout.
    Low = 0,
/// Disk cache hit (cheap deserialization).
    Medium = 1,
/// Common OS default fonts (sans-serif, serif, monospace).
    High = 2,
/// Main thread is blocked waiting for this font.
    Critical = 3,
}

/// A job for the Builder pool to process.
#[derive(Debug, Clone)]
pub struct FcBuildJob {
/// How urgently this font file needs to be parsed.
    pub priority: Priority,
/// Absolute path to the font file on disk.
    pub path: PathBuf,
/// Face index within the font file (for `.ttc` collections).
    pub font_index: Option<usize>,
/// Normalized family name guessed from the filename (lowercase, no separators).
    pub guessed_family: String,
}

impl PartialEq for FcBuildJob {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.path == other.path
    }
}
impl Eq for FcBuildJob {}

impl PartialOrd for FcBuildJob {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FcBuildJob {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority.cmp(&other.priority)
    }
}

// ── Scout Priority Assignment ───────────────────────────────────────────────

/// Assign initial priority for a font file discovered by the scout.
pub fn assign_scout_priority(
    file_tokens: &[String],
    common_token_sets: &[Vec<String>],
) -> Priority {
    if config::matches_common_family_tokens(file_tokens, common_token_sets) {
        Priority::High
    } else {
        Priority::Low
    }
}

// ── Path Lookup Helpers ─────────────────────────────────────────────────────

/// Find all known file paths that match a normalized family name.
pub fn find_family_paths(
    family: &str,
    known_paths: &BTreeMap<String, Vec<PathBuf>>,
) -> Vec<PathBuf> {
    let mut result = HashSet::new();

    // Exact match
    if let Some(paths) = known_paths.get(family) {
        result.extend(paths.iter().cloned());
    }

    // Fuzzy substring match
    for (known_fam, paths) in known_paths.iter() {
        if known_fam != family
            && (known_fam.contains(family) || family.contains(known_fam.as_str()))
        {
            result.extend(paths.iter().cloned());
        }
    }

    result.into_iter().collect()
}

/// Find paths for `families` that haven't been fully parsed yet.
pub fn find_incomplete_paths(
    families: &[String],
    known_paths: &BTreeMap<String, Vec<PathBuf>>,
    completed_paths: &HashSet<PathBuf>,
) -> Vec<(PathBuf, String)> {
    families
        .iter()
        .flat_map(|family| {
            find_family_paths(family, known_paths)
                .into_iter()
                .filter(|p| !completed_paths.contains(p))
                .map(move |p| (p, family.clone()))
        })
        .collect()
}

/// Check whether a normalized family name exists in the pattern cache.
pub fn family_exists_in_patterns<'a>(
    family: &str,
    patterns: impl Iterator<Item = &'a FcPattern>,
) -> bool {
    patterns.into_iter().any(|p| {
        p.name
            .as_ref()
            .map(|n| normalize_family_name(n) == family)
            .unwrap_or(false)
            || p.family
                .as_ref()
                .map(|f| normalize_family_name(f) == family)
                .unwrap_or(false)
    })
}

#[cfg(test)]
#[path = "scoring_test.rs"]
mod scoring_test;
