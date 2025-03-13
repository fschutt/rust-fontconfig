/**
 * @file rust_fontconfig.h
 * @brief C API for rust-fontconfig
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
 
 /**
  * Query a font from the cache
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
  * Query all fonts matching a pattern
  * @param cache The font cache
  * @param pattern The pattern to match
  * @param trace Array to store trace messages
  * @param trace_count Pointer to trace count (will be updated)
  * @param matches_count Pointer to matches count (will be updated)
  * @return Array of font matches or NULL if no matches found
  */
 FcFontMatch** fc_cache_query_all(FcFontCache cache, const FcPattern* pattern, FcTraceMsg** trace, size_t* trace_count, size_t* matches_count);
 
 /**
  * Free an array of font matches
  */
 void fc_font_matches_free(FcFontMatch** matches, size_t count);
 
 /**
  * Query fonts for text
  * @param cache The font cache
  * @param pattern The base pattern
  * @param text The text to find fonts for
  * @param trace Array to store trace messages
  * @param trace_count Pointer to trace count (will be updated)
  * @param matches_count Pointer to matches count (will be updated)
  * @return Array of font matches or NULL if no matches found
  */
 FcFontMatch** fc_cache_query_for_text(FcFontCache cache, const FcPattern* pattern, const char* text, FcTraceMsg** trace, size_t* trace_count, size_t* matches_count);
 
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
  * Get all available fonts in the cache
  * @param cache The font cache
  * @param count Pointer to store the number of fonts found
  * @return Array of font information or NULL if none found (must be freed)
  */
 FcFontInfo* fc_cache_list_fonts(FcFontCache cache, size_t* count);
 
 /**
  * Free array of font info
  */
 void fc_font_info_free(FcFontInfo* info, size_t count);
 
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
 
 #ifdef __cplusplus
 }
 #endif
 
 #endif /* RUST_FONTCONFIG_H */