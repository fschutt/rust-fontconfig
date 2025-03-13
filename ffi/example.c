#include <stdio.h>
#include <stdlib.h>
#include "rust_fontconfig.h"

void print_font_match(FcFontMatch* match) {
    if (!match) {
        printf("No match found\n");
        return;
    }

    char id_str[40];
    if (fc_font_id_to_string(&match->id, id_str, sizeof(id_str))) {
        printf("Font ID: %s\n", id_str);
    } else {
        printf("Font ID: <error converting ID>\n");
    }

    printf("Unicode ranges: %zu\n", match->unicode_ranges_count);
    for (size_t i = 0; i < match->unicode_ranges_count; i++) {
        printf("  Range %zu: U+%04X - U+%04X\n", i, 
               match->unicode_ranges[i].start, 
               match->unicode_ranges[i].end);
    }

    printf("Fallbacks: %zu\n", match->fallbacks_count);
    for (size_t i = 0; i < match->fallbacks_count; i++) {
        FcFontMatchNoFallback* fallback = &match->fallbacks[i];
        
        if (fc_font_id_to_string(&fallback->id, id_str, sizeof(id_str))) {
            printf("  Fallback %zu: %s (%zu ranges)\n", i, id_str, 
                   fallback->unicode_ranges_count);
        }
    }
    printf("\n");
}

int main() {
    // Build the font cache
    printf("Building font cache...\n");
    FcFontCache cache = fc_cache_build();
    if (!cache) {
        printf("Failed to build font cache\n");
        return 1;
    }
    printf("Font cache built successfully\n");

    // Create a pattern to query
    FcPattern* pattern = fc_pattern_new();
    if (!pattern) {
        printf("Failed to create pattern\n");
        fc_cache_free(cache);
        return 1;
    }

    // Set pattern properties
    fc_pattern_set_name(pattern, "Arial");

    // Query for a font
    printf("Querying for 'Arial'...\n");
    FcTraceMsg* trace = NULL;
    size_t trace_count = 0;
    FcFontMatch* match = fc_cache_query(cache, pattern, &trace, &trace_count);

    // Print the result
    print_font_match(match);

    // Print trace messages
    printf("Trace messages: %zu\n", trace_count);
    for (size_t i = 0; i < trace_count; i++) {
        FcTraceMsg* msg = &trace[i];
        FcReasonType reason_type = fc_trace_get_reason_type(msg);
        
        printf("  Trace %zu: Level=%d, Path=%s, Reason=%d\n", 
               i, msg->level, msg->path ? msg->path : "<null>", reason_type);
    }

    // Query for monospace fonts
    printf("\nQuerying for monospace fonts...\n");
    fc_pattern_free(pattern);
    pattern = fc_pattern_new();
    fc_pattern_set_monospace(pattern, FC_MATCH_TRUE);

    size_t matches_count = 0;
    FcFontMatch** matches = fc_cache_query_all(cache, pattern, &trace, &trace_count, &matches_count);

    printf("Found %zu monospace fonts\n", matches_count);
    if (matches_count > 0 && matches) {
        print_font_match(matches[0]);
    }

    // Query for multilingual text
    printf("\nQuerying for multilingual text...\n");
    const char* text = "Hello 你好 Здравствуйте";
    
    fc_pattern_free(pattern);
    pattern = fc_pattern_new();
    
    matches = fc_cache_query_for_text(cache, pattern, text, &trace, &trace_count, &matches_count);
    
    printf("Found %zu fonts for multilingual text\n", matches_count);
    
    // Cleanup
    fc_trace_free(trace, trace_count);
    fc_font_match_free(match);
    if (matches) {
        fc_font_matches_free(matches, matches_count);
    }
    fc_pattern_free(pattern);
    fc_cache_free(cache);
    
    return 0;
}