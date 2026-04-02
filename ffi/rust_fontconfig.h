/**
 * @file rust_fontconfig.h
 * @brief C API for rust-fontconfig
 * 
 * Version 1.2.0 introduces a two-step font resolution API:
 * 1. Create a font chain with fc_resolve_font_chain()
 * 2. Query fonts for text with fc_chain_query_for_text()
 * 
 * This replaces the old fc_cache_query_all() and fc_cache_query_for_text() functions.
 */

 #ifndef RUST_FONTCONFIG_H
 #define RUST_FONTCONFIG_H
 
 #include <stddef.h>
 #include <stdint.h>
 #include <stdbool.h>
 
 #ifdef __cplusplus
 extern "C" {
 #endif
 
 /**
  * Font ID type for identifying fonts
  */
 typedef struct {
     uint64_t high;
     uint64_t low;
 } FcFontId;
 
 /**
  * Pattern match type
  */
 typedef enum {
     FC_MATCH_TRUE = 0,
     FC_MATCH_FALSE = 1,
     FC_MATCH_DONT_CARE = 2
 } FcPatternMatch;
 
 /**
  * Font weight values as defined in CSS specification
  */
 typedef enum {
     FC_WEIGHT_THIN = 100,
     FC_WEIGHT_EXTRA_LIGHT = 200,
     FC_WEIGHT_LIGHT = 300,
     FC_WEIGHT_NORMAL = 400,
     FC_WEIGHT_MEDIUM = 500,
     FC_WEIGHT_SEMI_BOLD = 600,
     FC_WEIGHT_BOLD = 700,
     FC_WEIGHT_EXTRA_BOLD = 800,
     FC_WEIGHT_BLACK = 900
 } FcWeight;
 
 /**
  * CSS font-stretch values
  */
 typedef enum {
     FC_STRETCH_ULTRA_CONDENSED = 1,
     FC_STRETCH_EXTRA_CONDENSED = 2,
     FC_STRETCH_CONDENSED = 3,
     FC_STRETCH_SEMI_CONDENSED = 4,
     FC_STRETCH_NORMAL = 5,
     FC_STRETCH_SEMI_EXPANDED = 6,
     FC_STRETCH_EXPANDED = 7,
     FC_STRETCH_EXTRA_EXPANDED = 8,
     FC_STRETCH_ULTRA_EXPANDED = 9
 } FcStretch;
 
 /**
  * Unicode range representation
  */
 typedef struct {
     uint32_t start;
     uint32_t end;
 } FcUnicodeRange;
 
 /**
  * Font metadata structure
  */
 typedef struct {
     char* copyright;
     char* designer;
     char* designer_url;
     char* font_family;
     char* font_subfamily;
     char* full_name;
     char* id_description;
     char* license;
     char* license_url;
     char* manufacturer;
     char* manufacturer_url;
     char* postscript_name;
     char* preferred_family;
     char* preferred_subfamily;
     char* trademark;
     char* unique_id;
     char* version;
 } FcFontMetadata;

 /**
  * Hinting style for font rendering (from Linux fonts.conf)
  */
 typedef enum {
     FC_HINT_NONE = 0,
     FC_HINT_SLIGHT = 1,
     FC_HINT_MEDIUM = 2,
     FC_HINT_FULL = 3
 } FcHintStyle;

 /**
  * Subpixel rendering order (from Linux fonts.conf)
  */
 typedef enum {
     FC_RGBA_UNKNOWN = 0,
     FC_RGBA_RGB = 1,
     FC_RGBA_BGR = 2,
     FC_RGBA_VRGB = 3,
     FC_RGBA_VBGR = 4,
     FC_RGBA_NONE = 5
 } FcRgba;

 /**
  * LCD filter mode for subpixel rendering (from Linux fonts.conf)
  */
 typedef enum {
     FC_LCD_NONE = 0,
     FC_LCD_DEFAULT = 1,
     FC_LCD_LIGHT = 2,
     FC_LCD_LEGACY = 3
 } FcLcdFilter;

 /**
  * Per-font rendering configuration from system font config.
  *
  * On Linux, populated from fonts.conf <match target="font"> rules.
  * On other platforms, all fields are -1 (unset).
  *
  * A value of -1 means "use system default" for integer fields.
  * For double fields, -1.0 means "use system default".
  */
 typedef struct {
     int antialias;          /**< -1=unset, 0=false, 1=true */
     int hinting;            /**< -1=unset, 0=false, 1=true */
     int hintstyle;          /**< -1=unset, or FcHintStyle value */
     int autohint;           /**< -1=unset, 0=false, 1=true */
     int rgba;               /**< -1=unset, or FcRgba value */
     int lcdfilter;          /**< -1=unset, or FcLcdFilter value */
     int embeddedbitmap;     /**< -1=unset, 0=false, 1=true */
     int embolden;           /**< -1=unset, 0=false, 1=true */
     double dpi;             /**< -1.0=unset, or positive DPI value */
     double scale;           /**< -1.0=unset, or positive scale factor */
     int minspace;           /**< -1=unset, 0=false, 1=true */
 } FcFontRenderConfig;

 /**
  * Font pattern for matching
  */
 typedef struct {
     char* name;
     char* family;
     FcPatternMatch italic;
     FcPatternMatch oblique;
     FcPatternMatch bold;
     FcPatternMatch monospace;
     FcPatternMatch condensed;
     FcWeight weight;
     FcStretch stretch;
     FcUnicodeRange* unicode_ranges;
     size_t unicode_ranges_count;
     FcFontMetadata metadata;
     FcFontRenderConfig render_config;
 } FcPattern;
 
 /**
  * Font match fallback (without nested fallbacks)
  */
 typedef struct {
     FcFontId id;
     FcUnicodeRange* unicode_ranges;
     size_t unicode_ranges_count;
 } FcFontMatchNoFallback;
 
 /**
  * Font match result
  */
 typedef struct {
     FcFontId id;
     FcUnicodeRange* unicode_ranges;
     size_t unicode_ranges_count;
     FcFontMatchNoFallback* fallbacks;
     size_t fallbacks_count;
 } FcFontMatch;
 
 /**
  * Font cache opaque pointer
  */
 typedef struct FcFontCacheStruct* FcFontCache;

 /**
  * Font fallback chain opaque pointer (new in 1.2.0)
  * 
  * Represents a resolved chain of fonts for a CSS font-family stack.
  * Create with fc_resolve_font_chain(), free with fc_font_chain_free().
  */
 typedef struct FcFontFallbackChainC* FcFontChain;

 /**
  * Resolved font run for text (new in 1.2.0)
  * 
  * Represents a run of text that should be rendered with a specific font.
  */
 typedef struct {
     char* text;           /**< The text for this run */
     size_t start_byte;    /**< Start byte offset in original text */
     size_t end_byte;      /**< End byte offset in original text */
     FcFontId font_id;     /**< Font ID for this run */
     bool has_font;        /**< Whether font_id is valid */
     char* css_source;     /**< Which CSS font-family this came from */
 } FcResolvedFontRun;

 /**
  * CSS fallback group (new in 1.2.0)
  * 
  * Groups fonts by their CSS source name.
  */
 typedef struct {
     char* css_name;                  /**< The CSS font name */
     FcFontMatchNoFallback* fonts;    /**< Array of font matches */
     size_t fonts_count;              /**< Number of fonts */
 } FcCssFallbackGroup;
 
 /**
  * Trace message level
  */
 typedef enum {
     FC_TRACE_DEBUG = 0,
     FC_TRACE_INFO = 1,
     FC_TRACE_WARNING = 2,
     FC_TRACE_ERROR = 3
 } FcTraceLevel;
 
 /**
  * Trace message struct for debugging font matching
  */
 typedef struct {
     FcTraceLevel level;
     char* path;
     void* reason; // Opaque reason pointer, use fc_trace_get_reason_* functions
 } FcTraceMsg;
 
 /**
  * Reason type for trace messages
  */
 typedef enum {
     FC_REASON_NAME_MISMATCH = 0,
     FC_REASON_FAMILY_MISMATCH = 1,
     FC_REASON_STYLE_MISMATCH = 2,
     FC_REASON_WEIGHT_MISMATCH = 3,
     FC_REASON_STRETCH_MISMATCH = 4,
     FC_REASON_UNICODE_RANGE_MISMATCH = 5,
     FC_REASON_SUCCESS = 6
 } FcReasonType;
 
 /**
  * Path to a font file
  */
 typedef struct {
     char* path;
     size_t font_index;
 } FcFontPath;
 
 /**
  * In-memory font data
  */
 typedef struct {
     uint8_t* bytes;
     size_t bytes_len;
     size_t font_index;
     char* id;
 } FcFont;
 
 /**
  * Font info entry with ID and name
  */
 typedef struct {
     FcFontId id;
     char* name;
     char* family;
 } FcFontInfo;

 /* ============================================================================
  * Cache Management
  * ============================================================================ */

 /** 
  * Create a new font ID
  */
 FcFontId fc_font_id_new(void);
 
 /**
  * Create a new font cache
  */
 FcFontCache fc_cache_build(void);
 
 /**
  * Free the font cache
  */
 void fc_cache_free(FcFontCache cache);
 
 /**
  * Add in-memory fonts to the cache
  */
 void fc_cache_add_memory_fonts(FcFontCache cache, FcPattern* patterns, FcFont* fonts, size_t count);

 /**
  * Get all available fonts in the cache
  * @param cache The font cache
  * @param count Pointer to store the number of fonts found
  * @return Array of font information or NULL if none found (must be freed)
  */
 FcFontInfo* fc_cache_list_fonts(FcFontCache cache, size_t* count);

 /* ============================================================================
  * Pattern Management
  * ============================================================================ */
 
 /**
  * Create a new default pattern
  */
 FcPattern* fc_pattern_new(void);
 
 /**
  * Free a pattern
  */
 void fc_pattern_free(FcPattern* pattern);
 
 /**
  * Set pattern name
  */
 void fc_pattern_set_name(FcPattern* pattern, const char* name);
 
 /**
  * Set pattern family
  */
 void fc_pattern_set_family(FcPattern* pattern, const char* family);
 
 /**
  * Set pattern italic
  */
 void fc_pattern_set_italic(FcPattern* pattern, FcPatternMatch italic);
 
 /**
  * Set pattern bold
  */
 void fc_pattern_set_bold(FcPattern* pattern, FcPatternMatch bold);
 
 /**
  * Set pattern monospace
  */
 void fc_pattern_set_monospace(FcPattern* pattern, FcPatternMatch monospace);
 
 /**
  * Set pattern weight
  */
 void fc_pattern_set_weight(FcPattern* pattern, FcWeight weight);
 
 /**
  * Set pattern stretch
  */
 void fc_pattern_set_stretch(FcPattern* pattern, FcStretch stretch);
 
 /**
  * Add unicode range to pattern
  */
 void fc_pattern_add_unicode_range(FcPattern* pattern, uint32_t start, uint32_t end);

 /* ============================================================================
  * Single Font Query (unchanged)
  * ============================================================================ */
 
 /**
  * Query a single font from the cache
  * @param cache The font cache
  * @param pattern The pattern to match
  * @param trace Array to store trace messages
  * @param trace_count Pointer to trace count (will be updated)
  * @return Font match or NULL if no match found
  */
 FcFontMatch* fc_cache_query(FcFontCache cache, const FcPattern* pattern, FcTraceMsg** trace, size_t* trace_count);
 
 /**
  * Free a font match
  */
 void fc_font_match_free(FcFontMatch* match);

 /**
  * Free an array of font matches
  */
 void fc_font_matches_free(FcFontMatch** matches, size_t count);

 /* ============================================================================
  * Two-Step Font Resolution API (new in 1.2.0)
  * 
  * Use this API for CSS-style font resolution:
  * 1. fc_resolve_font_chain() - resolve CSS font-family stack to a font chain
  * 2. fc_chain_query_for_text() - query which fonts to use for specific text
  * ============================================================================ */

 /**
  * Resolve a font chain from CSS font families (new in 1.2.0)
  * 
  * This is the first step in the two-step font resolution process.
  * The font chain is cached internally, so calling this multiple times
  * with the same parameters is efficient.
  * 
  * @param cache The font cache
  * @param families Array of CSS font family names (e.g., ["Arial", "sans-serif"])
  * @param families_count Number of family names
  * @param weight Font weight
  * @param italic Whether to match italic fonts (FC_MATCH_TRUE, FC_MATCH_FALSE, FC_MATCH_DONT_CARE)
  * @param oblique Whether to match oblique fonts
  * @param trace Array to store trace messages
  * @param trace_count Pointer to trace count (will be updated)
  * @return Font fallback chain or NULL on error (must be freed with fc_font_chain_free)
  * 
  * Example:
  * @code
  * const char* families[] = {"Arial", "Helvetica", "sans-serif"};
  * FcTraceMsg* trace = NULL;
  * size_t trace_count = 0;
  * 
  * FcFontChain chain = fc_resolve_font_chain(cache, families, 3, 
  *     FC_WEIGHT_NORMAL, FC_MATCH_FALSE, FC_MATCH_FALSE,
  *     &trace, &trace_count);
  * 
  * // Use chain with fc_chain_query_for_text()...
  * 
  * fc_font_chain_free(chain);
  * fc_trace_free(trace, trace_count);
  * @endcode
  */
 FcFontChain fc_resolve_font_chain(
     FcFontCache cache,
     const char** families,
     size_t families_count,
     FcWeight weight,
     FcPatternMatch italic,
     FcPatternMatch oblique,
     FcTraceMsg** trace,
     size_t* trace_count
 );

 /**
  * Free a font fallback chain (new in 1.2.0)
  */
 void fc_font_chain_free(FcFontChain chain);

 /**
  * Query which fonts should be used for a text string (new in 1.2.0)
  * 
  * This is the second step in the two-step font resolution process.
  * Returns runs of consecutive characters that use the same font.
  * 
  * @param chain The font fallback chain (from fc_resolve_font_chain)
  * @param cache The font cache
  * @param text The text to find fonts for
  * @param runs_count Pointer to store number of runs (will be updated)
  * @return Array of font runs or NULL on error (must be freed with fc_resolved_runs_free)
  * 
  * Example:
  * @code
  * size_t runs_count = 0;
  * FcResolvedFontRun* runs = fc_chain_query_for_text(chain, cache, 
  *     "Hello 世界!", &runs_count);
  * 
  * for (size_t i = 0; i < runs_count; i++) {
  *     if (runs[i].has_font) {
  *         // Shape runs[i].text with font runs[i].font_id
  *     }
  * }
  * 
  * fc_resolved_runs_free(runs, runs_count);
  * @endcode
  */
 FcResolvedFontRun* fc_chain_query_for_text(
     FcFontChain chain,
     FcFontCache cache,
     const char* text,
     size_t* runs_count
 );

 /**
  * Free an array of resolved font runs (new in 1.2.0)
  */
 void fc_resolved_runs_free(FcResolvedFontRun* runs, size_t count);

 /**
  * Get the original CSS font stack from a font chain (new in 1.2.0)
  * @param chain The font chain
  * @param stack_count Pointer to store number of family names
  * @return Array of font family names (must be freed with fc_string_array_free)
  */
 char** fc_chain_get_original_stack(FcFontChain chain, size_t* stack_count);

 /**
  * Free a string array (new in 1.2.0)
  */
 void fc_string_array_free(char** arr, size_t count);

 /**
  * Get CSS fallback groups from a font chain (new in 1.2.0)
  * @param chain The font chain
  * @param groups_count Pointer to store number of groups
  * @return Array of CSS fallback groups (must be freed with fc_css_fallback_groups_free)
  */
 FcCssFallbackGroup* fc_chain_get_css_fallbacks(FcFontChain chain, size_t* groups_count);

 /**
  * Free CSS fallback groups (new in 1.2.0)
  */
 void fc_css_fallback_groups_free(FcCssFallbackGroup* groups, size_t count);

 /* ============================================================================
  * Font Data Access
  * ============================================================================ */
 
 /**
  * Get font path by ID
  * @return NULL if not found
  */
 FcFontPath* fc_cache_get_font_path(FcFontCache cache, const FcFontId* id);
 
 /**
  * Free font path
  */
 void fc_font_path_free(FcFontPath* path);
 
 /**
  * Create a new in-memory font
  * @param bytes The font file data (will be copied)
  * @param bytes_len Length of the font data
  * @param font_index Index of the font in a collection (0 for single fonts)
  * @param id Unique identifier for the font
  * @return A new font object or NULL on error
  */
 FcFont* fc_font_new(const uint8_t* bytes, size_t bytes_len, size_t font_index, const char* id);
 
 /**
  * Free an in-memory font
  */
 void fc_font_free(FcFont* font);

 /**
  * Get metadata by font ID
  * @param cache The font cache
  * @param id The font ID
  * @return Metadata or NULL if not found (must be freed)
  */
 FcFontMetadata* fc_cache_get_font_metadata(FcFontCache cache, const FcFontId* id);
 
 /**
  * Free font metadata
  */
 void fc_font_metadata_free(FcFontMetadata* metadata);

 /**
  * Get per-font render config by font ID.
  * Returns a struct with -1 values for unset fields.
  * On non-Linux, all fields are -1 (system defaults).
  */
 FcFontRenderConfig fc_cache_get_render_config(FcFontCache cache, const FcFontId* id);

 /* ============================================================================
  * Trace and Debug
  * ============================================================================ */
 
 /**
  * Get trace reason type
  */
 FcReasonType fc_trace_get_reason_type(const FcTraceMsg* trace);
 
 /**
  * Free trace messages
  */
 void fc_trace_free(FcTraceMsg* trace, size_t count);
 
 /**
  * Convert font ID to string
  * @param id Font ID
  * @param buffer Output buffer
  * @param buffer_size Size of output buffer
  * @return true if successful
  */
 bool fc_font_id_to_string(const FcFontId* id, char* buffer, size_t buffer_size);
 
 /**
  * Free array of font info
  */
 void fc_font_info_free(FcFontInfo* info, size_t count);

 /* ============================================================================
  * Async Registry API (background thread scanning)
  *
  * The registry spawns background threads to scan and parse system fonts
  * while your application does other work (window creation, DOM construction).
  *
  * Workflow:
  *   1. fc_registry_new()    — create the registry (instant)
  *   2. fc_registry_spawn()  — launch scout + builder threads (instant)
  *   3. ... do other work ...
  *   4. fc_registry_request_fonts()  — block until needed fonts are ready
  *   5. Use chains with fc_chain_query_for_text()
  *   6. fc_registry_free()   — shutdown threads + free
  *
  * All fc_registry_* functions are thread-safe.
  * ============================================================================ */

 /**
  * Font registry opaque pointer
  *
  * Wraps an FcFontCache behind a lock so background threads can populate it
  * while the main thread reads from it.
  */
 typedef struct FcFontRegistryStruct* FcFontRegistry;

 /**
  * Create a new font registry. Returns immediately (no scanning yet).
  * Call fc_registry_spawn() to start background threads.
  */
 FcFontRegistry fc_registry_new(void);

 /**
  * Spawn the Scout thread and Builder pool. Returns immediately.
  *
  * - Scout (~5-20ms): enumerates font directories, assigns priorities.
  * - Builders (N threads): parse font files from the priority queue.
  *
  * Common OS fonts (Arial, San Francisco, etc.) are prioritized.
  */
 void fc_registry_spawn(FcFontRegistry registry);

 /**
  * Block until the requested font families are loaded, then return
  * resolved font chains.
  *
  * @param registry        The font registry
  * @param family_stacks   Array of font-family stacks. Each stack is an
  *                         array of C strings (e.g., {"Arial","sans-serif"}).
  * @param stack_counts    Number of families in each stack
  * @param num_stacks      Number of stacks
  * @param out_count       Receives the number of returned chains
  * @return Array of FcFontChain pointers (one per stack). Free each chain
  *         with fc_font_chain_free(), then the array with
  *         fc_registry_chains_free().
  *
  * Hard timeout: 5 seconds. If fonts are not found by then, returns the
  * best available match.
  *
  * Example:
  * @code
  * const char* stack0[] = {"Arial", "Helvetica", "sans-serif"};
  * const char* stack1[] = {"Fira Code", "monospace"};
  * const char** stacks[] = {stack0, stack1};
  * size_t counts[] = {3, 2};
  * size_t num_chains = 0;
  *
  * FcFontChain* chains = fc_registry_request_fonts(
  *     registry, stacks, counts, 2, &num_chains);
  *
  * // Use chains[0], chains[1] with fc_chain_query_for_text()...
  *
  * for (size_t i = 0; i < num_chains; i++)
  *     fc_font_chain_free(chains[i]);
  * fc_registry_chains_free(chains, num_chains);
  * @endcode
  */
 FcFontChain* fc_registry_request_fonts(
     FcFontRegistry registry,
     const char*** family_stacks,
     const size_t* stack_counts,
     size_t num_stacks,
     size_t* out_count
 );

 /**
  * Free the array returned by fc_registry_request_fonts().
  * Does NOT free the individual chains — use fc_font_chain_free() for each.
  */
 void fc_registry_chains_free(FcFontChain* chains, size_t count);

 /**
  * Check if the scout has finished enumerating all font directories.
  * Non-blocking.
  */
 bool fc_registry_is_scan_complete(FcFontRegistry registry);

 /**
  * Check if all queued font files have been parsed. Non-blocking.
  */
 bool fc_registry_is_build_complete(FcFontRegistry registry);

 /**
  * Signal all background threads to shut down. Non-blocking.
  */
 void fc_registry_shutdown(FcFontRegistry registry);

 /**
  * Free a font registry. Shuts down background threads if still running.
  */
 void fc_registry_free(FcFontRegistry registry);

 /**
  * Query a single font from the registry (thread-safe).
  * @return Font match or NULL if no match found (must be freed with fc_font_match_free)
  */
 FcFontMatch* fc_registry_query(FcFontRegistry registry, const FcPattern* pattern);

 /**
  * List all fonts currently loaded in the registry.
  * @param registry The font registry
  * @param count Pointer to store the number of fonts found
  * @return Array of font info (must be freed with fc_font_info_free)
  */
 FcFontInfo* fc_registry_list_fonts(FcFontRegistry registry, size_t* count);

 /**
  * Resolve a font chain from the registry (thread-safe).
  * Uses whatever fonts are currently loaded — call after request_fonts()
  * for complete results.
  */
 FcFontChain fc_registry_resolve_font_chain(
     FcFontRegistry registry,
     const char** families,
     size_t families_count,
     FcWeight weight,
     FcPatternMatch italic,
     FcPatternMatch oblique
 );

 /**
  * Get font path by ID from the registry.
  * @return NULL if not found (must be freed with fc_font_path_free)
  */
 FcFontPath* fc_registry_get_font_path(FcFontRegistry registry, const FcFontId* id);

 /**
  * Get font metadata by ID from the registry.
  * @return NULL if not found (must be freed with fc_font_metadata_free)
  */
 FcFontMetadata* fc_registry_get_metadata(FcFontRegistry registry, const FcFontId* id);

 /**
  * Take a snapshot of the registry as an immutable FcFontCache.
  * Useful for passing to fc_chain_query_for_text() without holding
  * the registry lock.
  * @return Font cache (must be freed with fc_cache_free)
  */
 FcFontCache fc_registry_snapshot(FcFontRegistry registry);

 /**
  * Get per-font render config by font ID from the registry.
  */
 FcFontRenderConfig fc_registry_get_render_config(FcFontRegistry registry, const FcFontId* id);

 #ifdef __cplusplus
 }
 #endif

 #endif /* RUST_FONTCONFIG_H */