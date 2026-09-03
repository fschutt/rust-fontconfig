use alloc::string::String;

/// Known font file extensions (lowercase).
pub const FONT_EXTENSIONS: &[&str] = &["ttf", "otf", "ttc", "woff", "woff2", "dfont"];

/// Size (in bytes) of the head/tail samples taken by `content_dedup_hash_u64`.
pub const CONTENT_DEDUP_SAMPLE_BYTES: usize = 4096;

/// Deterministic 64-bit "cheap" content hash derived from `(file_size, first 4 KiB, last 4 KiB)`.
pub fn content_dedup_hash_u64(bytes: &[u8]) -> u64 {
    let len = bytes.len();
    let head_len = len.min(CONTENT_DEDUP_SAMPLE_BYTES);
    let tail_len = (len - head_len).min(CONTENT_DEDUP_SAMPLE_BYTES);
    let tail_start = len - tail_len;
    // Mix size first so two equal head+tail samples with different
    // lengths produce different hashes.
    let mut seed_buf = [0u8; 8];
    seed_buf.copy_from_slice(&(len as u64).to_le_bytes());
    let seed = content_hash_u64(&seed_buf);
    let head = content_hash_u64(&bytes[..head_len]);
    let tail = content_hash_u64(&bytes[tail_start..tail_start + tail_len]);
    // Combine — wrapping_mul + xor avalanches the three sub-hashes
    // reasonably without needing a separate mixing function.
    const K: u64 = 0x9E3779B97F4A7C15;
    let mut h = seed;
    h ^= head;
    h = h.wrapping_mul(K);
    h ^= tail;
    h = h.wrapping_mul(K);
    h ^= h >> 33;
    h
}

/// Deterministic 64-bit content hash over an arbitrary byte slice.
pub fn content_hash_u64(bytes: &[u8]) -> u64 {
    // Golden-ratio multiplier; used by xxhash and others as a simple
    // avalanche-friendly constant.
    const K: u64 = 0x9E3779B97F4A7C15;

    let mut h: u64 = K ^ (bytes.len() as u64);
    let chunks = bytes.chunks_exact(8);
    let remainder = chunks.remainder();
    for chunk in chunks {
        let mut arr = [0u8; 8];
        arr.copy_from_slice(chunk);
        let v = u64::from_le_bytes(arr);
        h = h.wrapping_add(v).wrapping_mul(K);
        h ^= h >> 33;
    }
    // Fold in any 1..7 trailing bytes.
    let mut tail: u64 = 0;
    for (i, b) in remainder.iter().enumerate() {
        tail |= (*b as u64) << (i * 8);
    }
    h = h.wrapping_add(tail).wrapping_mul(K);
    h ^= h >> 33;
    h = h.wrapping_mul(K);
    h ^= h >> 33;
    h
}

/// Normalize a family/font name for comparison: lowercase, strip all non-alphanumeric characters.
pub fn normalize_family_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Check if a file has a recognized font extension.
#[cfg(feature = "std")]
pub fn is_font_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| {
            let lower = ext.to_lowercase();
            FONT_EXTENSIONS.contains(&lower.as_str())
        })
        .unwrap_or(false)
}

/// Breadth-first walk of a directory yielding paths to files with recognized font extensions.
#[cfg(feature = "std")]
pub fn collect_font_files(root: &std::path::Path) -> alloc::vec::Vec<std::path::PathBuf> {
    use std::collections::{BTreeSet, VecDeque};
    const MAX_DEPTH: usize = 32;

    let mut files = alloc::vec::Vec::new();
    let mut visited: BTreeSet<std::path::PathBuf> = BTreeSet::new();
    let mut queue: VecDeque<(std::path::PathBuf, usize)> = VecDeque::new();
    queue.push_back((root.to_path_buf(), 0));
    while let Some((dir, depth)) = queue.pop_front() {
        let identity = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        if !visited.insert(identity) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth < MAX_DEPTH {
                    queue.push_back((path, depth + 1));
                }
            } else if is_font_file(&path) {
                files.push(path);
            }
        }
    }
    files
}

#[cfg(test)]
#[path = "utils_test.rs"]
mod utils_test;
