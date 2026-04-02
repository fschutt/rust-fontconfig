/**
 * @file example_registry.c
 * @brief Demonstrates the async registry (background thread) API.
 *
 * The registry spawns background threads to scan and parse system fonts
 * while the main thread does other work. At layout time, request_fonts()
 * blocks only until the specific fonts needed are ready.
 *
 * Build (macOS):
 *   make
 *   gcc -Wall -g -I include -o example_registry ffi/example_registry.c \
 *       -L. -lrust_fontconfig
 *   ./example_registry
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "rust_fontconfig.h"

/* ── Helpers ─────────────────────────────────────────────────────────────── */

static void print_separator(const char* title) {
    printf("\n");
    printf("======================================================================\n");
    printf("  %s\n", title);
    printf("======================================================================\n\n");
}

static void print_font_id(const FcFontId* id) {
    char buf[64];
    if (fc_font_id_to_string(id, buf, sizeof(buf))) {
        printf("%s", buf);
    } else {
        printf("(unknown)");
    }
}

/* ── Demo 1: Basic registry lifecycle ─────────────────────────────────── */

static void demo_basic_lifecycle(void) {
    print_separator("Demo 1: Basic Registry Lifecycle");

    /* 1. Create the registry (instant — no scanning yet) */
    printf("1. Creating registry...\n");
    FcFontRegistry registry = fc_registry_new();
    if (!registry) {
        fprintf(stderr, "Failed to create registry\n");
        return;
    }
    printf("   Done.\n\n");

    /* 2. Spawn background threads (returns immediately) */
    printf("2. Spawning scout + builder threads...\n");
    fc_registry_spawn(registry);
    printf("   Done (threads running in background).\n\n");

    /* 3. Simulate doing other work while fonts load */
    printf("3. Doing other work (window creation, DOM parsing, etc.)...\n");
    printf("   Scout complete? %s\n",
           fc_registry_is_scan_complete(registry) ? "yes" : "not yet");
    printf("   Build complete? %s\n",
           fc_registry_is_build_complete(registry) ? "yes" : "not yet");
    printf("\n");

    /* 4. Request the fonts we actually need (blocks until ready) */
    printf("4. Requesting fonts we need for layout...\n");

    const char* stack0[] = {"Arial", "Helvetica", "sans-serif"};
    const char* stack1[] = {"Courier New", "monospace"};
    const char** stacks[] = {stack0, stack1};
    size_t counts[] = {3, 2};
    size_t num_chains = 0;

    FcFontChain* chains = fc_registry_request_fonts(
        registry, stacks, counts, 2, &num_chains);

    printf("   Got %zu font chains.\n\n", num_chains);

    /* 5. Use the chains to resolve text */
    if (chains && num_chains >= 2) {
        /* Take a snapshot for query_for_text */
        FcFontCache cache = fc_registry_snapshot(registry);

        const char* text = "Hello, World!";
        size_t runs_count = 0;
        FcResolvedFontRun* runs = fc_chain_query_for_text(
            chains[0], cache, text, &runs_count);

        printf("5. Text \"%s\" resolved to %zu run(s):\n", text, runs_count);
        for (size_t i = 0; i < runs_count; i++) {
            printf("   Run %zu: \"%s\"", i, runs[i].text);
            if (runs[i].has_font) {
                printf(" -> font ");
                print_font_id(&runs[i].font_id);
                printf(" (via %s)", runs[i].css_source);
            } else {
                printf(" -> (no font)");
            }
            printf("\n");
        }

        fc_resolved_runs_free(runs, runs_count);
        fc_cache_free(cache);
    }
    printf("\n");

    /* 6. Check final status */
    printf("6. Final status:\n");
    printf("   Scout complete? %s\n",
           fc_registry_is_scan_complete(registry) ? "yes" : "no");
    printf("   Build complete? %s\n",
           fc_registry_is_build_complete(registry) ? "yes" : "no");

    size_t font_count = 0;
    FcFontInfo* fonts = fc_registry_list_fonts(registry, &font_count);
    printf("   Total fonts loaded: %zu\n", font_count);
    fc_font_info_free(fonts, font_count);

    /* 7. Clean up (shuts down threads) */
    for (size_t i = 0; i < num_chains; i++) {
        fc_font_chain_free(chains[i]);
    }
    fc_registry_chains_free(chains, num_chains);
    fc_registry_free(registry);
    printf("\n   Registry freed.\n");
}

/* ── Demo 2: Multilingual text with priority loading ─────────────────── */

static void demo_multilingual(void) {
    print_separator("Demo 2: Multilingual Priority Loading");

    FcFontRegistry registry = fc_registry_new();
    fc_registry_spawn(registry);

    /* Request fonts for multilingual text */
    const char* stack[] = {"Arial", "Noto Sans", "Noto Sans CJK SC", "sans-serif"};
    const char** stacks[] = {stack};
    size_t counts[] = {4};
    size_t num_chains = 0;

    printf("Requesting fonts for multilingual layout...\n");
    FcFontChain* chains = fc_registry_request_fonts(
        registry, stacks, counts, 1, &num_chains);

    if (chains && num_chains > 0) {
        FcFontCache cache = fc_registry_snapshot(registry);

        /* Test with various scripts */
        const char* texts[] = {
            "Hello World",
            "Bonjour le monde",
            "Hallo Welt",
        };
        size_t num_texts = sizeof(texts) / sizeof(texts[0]);

        for (size_t t = 0; t < num_texts; t++) {
            size_t runs_count = 0;
            FcResolvedFontRun* runs = fc_chain_query_for_text(
                chains[0], cache, texts[t], &runs_count);

            printf("\n  \"%s\"\n", texts[t]);
            for (size_t i = 0; i < runs_count; i++) {
                printf("    [%zu..%zu] \"%s\"",
                       runs[i].start_byte, runs[i].end_byte, runs[i].text);
                if (runs[i].has_font) {
                    printf(" -> ");
                    print_font_id(&runs[i].font_id);
                }
                printf("\n");
            }
            fc_resolved_runs_free(runs, runs_count);
        }

        for (size_t i = 0; i < num_chains; i++) {
            fc_font_chain_free(chains[i]);
        }
        fc_registry_chains_free(chains, num_chains);
        fc_cache_free(cache);
    }

    fc_registry_free(registry);
}

/* ── Demo 3: Query and metadata from registry ────────────────────────── */

static void demo_query_and_metadata(void) {
    print_separator("Demo 3: Query + Metadata from Registry");

    FcFontRegistry registry = fc_registry_new();
    fc_registry_spawn(registry);

    /* Request a specific font to ensure it's loaded */
    const char* stack[] = {"Times New Roman", "serif"};
    const char** stacks[] = {stack};
    size_t counts[] = {2};
    size_t num_chains = 0;

    FcFontChain* chains = fc_registry_request_fonts(
        registry, stacks, counts, 1, &num_chains);

    /* Query from the registry */
    FcPattern* pattern = fc_pattern_new();
    fc_pattern_set_name(pattern, "Times New Roman");

    FcFontMatch* match = fc_registry_query(registry, pattern);
    if (match) {
        printf("Found: ");
        print_font_id(&match->id);
        printf("\n");

        /* Get metadata */
        FcFontMetadata* meta = fc_registry_get_metadata(registry, &match->id);
        if (meta) {
            if (meta->full_name)
                printf("  Full name:  %s\n", meta->full_name);
            if (meta->font_family)
                printf("  Family:     %s\n", meta->font_family);
            if (meta->version)
                printf("  Version:    %s\n", meta->version);
            if (meta->manufacturer)
                printf("  Vendor:     %s\n", meta->manufacturer);
            fc_font_metadata_free(meta);
        }

        /* Get font path */
        FcFontPath* path = fc_registry_get_font_path(registry, &match->id);
        if (path) {
            printf("  Path:       %s (index %zu)\n", path->path, path->font_index);
            fc_font_path_free(path);
        }

        fc_font_match_free(match);
    } else {
        printf("Font not found.\n");
    }

    fc_pattern_free(pattern);
    for (size_t i = 0; i < num_chains; i++) {
        fc_font_chain_free(chains[i]);
    }
    fc_registry_chains_free(chains, num_chains);
    fc_registry_free(registry);
}

/* ── Demo 4: Compare old (blocking) vs new (async) API ───────────────── */

static void demo_old_vs_new(void) {
    print_separator("Demo 4: Old API (blocking) vs New API (async)");

    /* --- Old API: blocks until ALL fonts are scanned --- */
    printf("OLD API: fc_cache_build() — scans ALL system fonts upfront...\n");
    FcFontCache old_cache = fc_cache_build();

    size_t old_count = 0;
    FcFontInfo* old_fonts = fc_cache_list_fonts(old_cache, &old_count);
    printf("  Loaded %zu fonts (had to wait for all of them).\n", old_count);
    fc_font_info_free(old_fonts, old_count);

    /* Query with old API */
    FcPattern* pattern = fc_pattern_new();
    fc_pattern_set_name(pattern, "Arial");
    FcTraceMsg* trace = NULL;
    size_t trace_count = 0;
    FcFontMatch* old_match = fc_cache_query(old_cache, pattern, &trace, &trace_count);
    if (old_match) {
        printf("  Old API found Arial: ");
        print_font_id(&old_match->id);
        printf("\n");
        fc_font_match_free(old_match);
    }
    fc_trace_free(trace, trace_count);
    fc_cache_free(old_cache);

    printf("\n");

    /* --- New API: only blocks for the fonts we need --- */
    printf("NEW API: fc_registry_new() + fc_registry_request_fonts()...\n");
    FcFontRegistry registry = fc_registry_new();
    fc_registry_spawn(registry);

    const char* stack[] = {"Arial", "sans-serif"};
    const char** stacks[] = {stack};
    size_t counts[] = {2};
    size_t num_chains = 0;

    FcFontChain* chains = fc_registry_request_fonts(
        registry, stacks, counts, 1, &num_chains);
    printf("  Got %zu chain(s) — only waited for Arial + sans-serif.\n",
           num_chains);

    /* The registry may still be loading other fonts in the background */
    size_t new_count = 0;
    FcFontInfo* new_fonts = fc_registry_list_fonts(registry, &new_count);
    printf("  Fonts loaded so far: %zu (background threads still working)\n",
           new_count);
    printf("  Build complete? %s\n",
           fc_registry_is_build_complete(registry) ? "yes" : "not yet");
    fc_font_info_free(new_fonts, new_count);

    /* Query same font from the new API */
    FcFontMatch* new_match = fc_registry_query(registry, pattern);
    if (new_match) {
        printf("  New API found Arial: ");
        print_font_id(&new_match->id);
        printf("\n");
        fc_font_match_free(new_match);
    }

    fc_pattern_free(pattern);
    for (size_t i = 0; i < num_chains; i++) {
        fc_font_chain_free(chains[i]);
    }
    fc_registry_chains_free(chains, num_chains);
    fc_registry_free(registry);
}

/* ── Main ────────────────────────────────────────────────────────────── */

int main(int argc, char** argv) {
    const char* demo = (argc > 1) ? argv[1] : "all";

    if (strcmp(demo, "all") == 0 || strcmp(demo, "1") == 0) {
        demo_basic_lifecycle();
    }
    if (strcmp(demo, "all") == 0 || strcmp(demo, "2") == 0) {
        demo_multilingual();
    }
    if (strcmp(demo, "all") == 0 || strcmp(demo, "3") == 0) {
        demo_query_and_metadata();
    }
    if (strcmp(demo, "all") == 0 || strcmp(demo, "4") == 0) {
        demo_old_vs_new();
    }

    return 0;
}
