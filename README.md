# rust-fontconfig
Pure-Rust rewrite of the Linux fontconfig library (no system dependencies) - using ttf-parser and allsorts

## Motivation

There are a number of reasons why I want to have a pure-Rust version of fontconfig:

- fontconfig with all dependencies (expat and freetype) is ~190.000 lines of C (extremely bloated for what it does)
- fontconfig, freetype, expat and basically any kind of parsing in C is a common attack vector (via maliciously crafted fonts).
  The Rust versions (ttf-parser, allsorts) all check boundaries before accessing memory, so attacks
  via font files should be less common.
- it gets rid of the cmake / cc dependencies necessary to build [azul](https://azul.rs) on Linux
- fontconfig isn't really a "hard" library to rewrite, it just parses fonts and selects fonts by their name
- Rust has existing xml parsers and font parsers, just use those
- it allows fontconfig libraries to be purely statically linked
- font parsing can be easily multithreaded
- it reduces the number of necessary non-Rust dependencies on Linux for azul to 0
- fontconfig (or at least the Rust bindings) do not allow you to store an in-memory cache, only an on-disk cache,
  requiring disk access on every query (= slow)
- possible no_std support for minimal binaries?

Now for the more practical reasons:

- libfontconfig 0.12.x sometimes hangs and crashes ([see issue](https://github.com/maps4print/azul/issues/110))
- libfontconfig introduces build issues with cmake / cc ([see issue](https://github.com/maps4print/azul/issues/206))
- To support font fallback in CSS selectors and text runs based on Unicode ranges, 
  you have to do several calls into C, since fontconfig doesn't handle that
- The rust rewrite uses multithreading and memory mapping, since that is faster than
  reading each file individually
- The rust rewrite only parses the font tables necessary to select the name, 
  not the entire font
- The rust rewrite uses very few allocations (some are necessary because of 
  UTF-16 / UTF-8 conversions and multithreading lifetime issues)

## Performance

Currently the performance is pretty bad, since there is no caching. 
Querying a single font takes ~150ms, so you should only do it once at startup.

For the use in GUI libraries / applications, you should build a cache of
all system-available fonts + properties once at startup and then select the
proper font at runtime.
