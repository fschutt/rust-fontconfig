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
 
 #ifdef __cplusplus
 }
 #endif
 
 #endif /* RUST_FONTCONFIG_H */