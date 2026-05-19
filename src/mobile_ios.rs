//! iOS-only font discovery via CoreText.
//!
//! On iOS, system fonts (`Helvetica.ttc`, `SFNS.ttc`, `PingFang.ttc`, …) live
//! under `/System/Library/Fonts/{,Core,AssetsV2}/` and the per-app CoreText
//! cache. The app sandbox denies `open(2)` on those paths even when the
//! files are world-readable, so a plain `read_dir` returns nothing.
//!
//! `CTFontManagerCopyAvailableFontURLs` (iOS 13+) returns a `CFArrayRef` of
//! sandbox-mediated `CFURLRef`s that *are* openable from within the app
//! sandbox. We extract a UTF-8 filesystem path from each URL via
//! `CFURLGetFileSystemRepresentation` and feed the resulting `PathBuf`s into
//! the same `FcParseFont` path the desktop arms use.
//!
//! Reference: Apple *Core Text Programming Guide*; WWDC22 "What's new in
//! Core Text" (CTFontManagerCopyAvailableFontURLs).

use alloc::vec::Vec;
use core::ffi::c_void;
use std::os::raw::{c_char, c_long, c_uchar};
use std::path::PathBuf;

#[repr(C)]
pub(crate) struct __CFArray(c_void);
#[repr(C)]
pub(crate) struct __CFURL(c_void);

type CFArrayRef = *const __CFArray;
type CFURLRef = *const __CFURL;
type CFIndex = c_long;

#[link(name = "CoreText", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CTFontManagerCopyAvailableFontURLs() -> CFArrayRef;
    fn CFArrayGetCount(theArray: CFArrayRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(theArray: CFArrayRef, idx: CFIndex) -> *const c_void;
    fn CFURLGetFileSystemRepresentation(
        url: CFURLRef,
        resolve_against_base: bool,
        buffer: *mut c_uchar,
        max_buf_len: CFIndex,
    ) -> bool;
    fn CFRelease(cf: *const c_void);
}

/// Enumerate every system + bundled font URL via CoreText and return their
/// on-disk paths. Returns an empty vec on iOS <13 (where the symbol resolves
/// at link time but may return NULL — defensively guarded).
pub(crate) fn copy_available_font_urls() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();

    unsafe {
        let urls_array: CFArrayRef = CTFontManagerCopyAvailableFontURLs();
        if urls_array.is_null() {
            return out;
        }

        let count = CFArrayGetCount(urls_array);
        out.reserve(count.max(0) as usize);

        // PATH_MAX on Darwin is 1024 bytes. Some CoreText caches expose paths
        // through symlinked subtrees that exceed 256B — give ourselves room.
        let mut buf = [0u8; 4096];

        for i in 0..count {
            let url = CFArrayGetValueAtIndex(urls_array, i) as CFURLRef;
            if url.is_null() {
                continue;
            }
            let ok = CFURLGetFileSystemRepresentation(
                url,
                true,
                buf.as_mut_ptr(),
                buf.len() as CFIndex,
            );
            if !ok {
                continue;
            }
            // Null-terminated UTF-8 from CFURLGetFileSystemRepresentation.
            let nul_idx = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            if nul_idx == 0 {
                continue;
            }
            // Path may be ASCII-clean (most Apple fonts) or contain non-ASCII
            // (third-party fonts with localized names). `from_utf8_lossy` is
            // safe: the parser will reject anything that isn't a valid font.
            let s = core::str::from_utf8(&buf[..nul_idx]).unwrap_or("");
            if !s.is_empty() {
                out.push(PathBuf::from(s));
            }
        }

        CFRelease(urls_array as *const c_void);
    }

    out
}
