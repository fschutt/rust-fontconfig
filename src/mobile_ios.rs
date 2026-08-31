//! iOS-only font discovery via CoreText.
//!
//! On iOS, `read_dir` is denied on system font directories by the sandbox,
//! but the files are readable if their exact paths are known. We use
//! `CTFontCollectionCreateFromAvailableFonts` to query available fonts and extract
//! their paths via `kCTFontURLAttribute`.

use alloc::vec::Vec;
use core::ffi::c_void;
use std::collections::BTreeSet;
use std::os::raw::{c_long, c_uchar};
use std::path::PathBuf;

#[repr(C)]
pub(crate) struct __CFArray(c_void);
#[repr(C)]
pub(crate) struct __CFURL(c_void);
#[repr(C)]
pub(crate) struct __CFString(c_void);
#[repr(C)]
pub(crate) struct __CFDictionary(c_void);
#[repr(C)]
pub(crate) struct __CTFontCollection(c_void);
#[repr(C)]
pub(crate) struct __CTFontDescriptor(c_void);

type CFArrayRef = *const __CFArray;
type CFURLRef = *const __CFURL;
type CFStringRef = *const __CFString;
type CFDictionaryRef = *const __CFDictionary;
type CTFontCollectionRef = *const __CTFontCollection;
type CTFontDescriptorRef = *const __CTFontDescriptor;
type CFIndex = c_long;

#[link(name = "CoreText", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    // kCTFontURLAttribute: the file URL of a font descriptor (iOS 3.2+).
    static kCTFontURLAttribute: CFStringRef;

    // Available-fonts collection + its descriptors (iOS 7+).
    fn CTFontCollectionCreateFromAvailableFonts(options: CFDictionaryRef) -> CTFontCollectionRef;
    fn CTFontCollectionCreateMatchingFontDescriptors(collection: CTFontCollectionRef)
        -> CFArrayRef;
    // CTFontDescriptorCopyAttribute returns a +1 (owned) CFType (iOS 3.2+).
    fn CTFontDescriptorCopyAttribute(
        descriptor: CTFontDescriptorRef,
        attribute: CFStringRef,
    ) -> *const c_void;

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

/// Enumerate every available font's on-disk path via CoreText.
pub(crate) fn copy_available_font_urls() -> Vec<PathBuf> {
    // Deduplicate: multiple font faces map to the same file URL.
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();

    unsafe {
        let collection = CTFontCollectionCreateFromAvailableFonts(core::ptr::null());
        if collection.is_null() {
            return Vec::new();
        }
        let descriptors = CTFontCollectionCreateMatchingFontDescriptors(collection);
        CFRelease(collection as *const c_void);
        if descriptors.is_null() {
            return Vec::new();
        }

        let count = CFArrayGetCount(descriptors);
        let mut buf = [0u8; 4096];

        for i in 0..count {
            let desc = CFArrayGetValueAtIndex(descriptors, i) as CTFontDescriptorRef;
            if desc.is_null() {
                continue;
            }
            // Owned CFURLRef must be released.
            let url = CTFontDescriptorCopyAttribute(desc, kCTFontURLAttribute) as CFURLRef;
            if url.is_null() {
                continue; // Data fonts have no file URL.
            }
            let ok =
                CFURLGetFileSystemRepresentation(url, true, buf.as_mut_ptr(), buf.len() as CFIndex);
            if ok {
                let nul_idx = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                if nul_idx != 0 {
                    if let Ok(s) = core::str::from_utf8(&buf[..nul_idx]) {
                        seen.insert(PathBuf::from(s));
                    }
                }
            }
            CFRelease(url as *const c_void);
        }

        CFRelease(descriptors as *const c_void);
    }

    seen.into_iter().collect()
}
