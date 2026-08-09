/*
 * family_pipeline.c — end-to-end family pipeline across FOUR separately-built
 * cdylibs in a single process, over the shared vinary-tree resource ABI.
 *
 * This is the C-ABI counterpart of the in-process Rust integration test
 * `duallity/tests/family_pipeline.rs` (commit 1e10ac5). It exercises the same
 * call sequence and asserts the same results, but the four surfaces are linked
 * as independent shared libraries whose C ABIs are disjoint (each was built
 * with only its own `--features ffi`, so the exported symbol sets never
 * collide):
 *
 *     liblibdictenstein.so   ldict_*      (producer:  vt.dictionary.v1)
 *     libduallity.so         duallity_*   (adapter:   vt.dictionary.v1 -> vt.scalar-wfst.1)
 *     liblling_llang.so      lling_*      (composer:  vt.scalar-wfst.1 x vt.scalar-wfst.1)
 *     libliblevenshtein.so   llev_*       (consumer:  vt.dictionary.v1 -> cursor)
 *
 * The full chain, entirely over the shared resource ABI:
 *
 *     libdictenstein DynamicDawg          (producer: vt.dictionary.v1)
 *           | ldict_dictionary_resource   (borrowed resource; NOT released)
 *           v
 *     duallity_wfst_new  -- Levenshtein WFST -->  vt.scalar-wfst.1  (capture-once)
 *           | duallity_wfst_resource       (owned retain; released)
 *           v  o  (lling_wfst_compose with a case-mapping WFST term -> UPPER(term))
 *     composed WFST  -- traverse -->  { UPPER(term) : lev(query,term) <= d }
 *
 * The composed language is cross-checked three ways for agreement:
 *   1. against the golden set derived from tests/fixtures/family_pipeline_golden.tsv;
 *   2. against a liblevenshtein cursor (llev_*) over the SAME dictionary
 *      resource — the independent consumer of the same producer;
 *   3. under ~5 dictionary mutations performed AFTER capture, proving the WFST
 *      and the cursor are isolated on their captured revision (a fresh cursor
 *      over the mutated dictionary must drift from the golden).
 *
 * A second phase drives the whole chain over an INSTRUMENTED C provider (a
 * hand-rolled vt.dictionary.v1 counting dictionary, the C port of
 * duallity/tests/support/counting_dictionary.rs) and tears the chain down in
 * both orders, asserting the retain/release ledger balances to zero (no leaked
 * snapshot retain, no double free).
 *
 * Correspondence with the Rust reference:
 *   - DUAL-FAM-1: composed traversal == golden == liblevenshtein cursor.
 *   - DUAL-FAM-2: mid-flight mutations do not change the captured language.
 *   - DUAL-FAM-3: the retain/release ledger balances to zero in both teardown
 *     orders.
 *
 * Build/run: see bindings/c/tests/README.md and the `c-family-pipeline` CI job
 * in .github/workflows/ci.yml.
 */

#include <ctype.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Each of these pulls in vinary_tree_interop.h (guarded, so it is defined
 * exactly once). The four status/enum families use disjoint prefixes, so no
 * enumerator or macro collides across the headers. */
#include "vinary_tree_interop.h"
#include "libdictenstein.h"
#include "duallity.h"
#include "lling_llang.h"
#include "liblevenshtein.h"

/* ------------------------------------------------------------------------- */
/* Assertion + reporting                                                     */
/* ------------------------------------------------------------------------- */

static unsigned g_passed = 0;

static void check_impl(int condition, const char *message, const char *file, int line) {
    if (!condition) {
        fprintf(stderr, "FAIL [%s:%d]: %s\n", file, line, message);
        fprintf(stderr, "  ldict:    %s\n", ldict_last_error_message());
        fprintf(stderr, "  duallity: %s\n", duallity_last_error_message());
        fprintf(stderr, "  lling:    %s\n", lling_last_error_message());
        fprintf(stderr, "  llev:     %s\n", llev_last_error_message());
        exit(1);
    }
    g_passed++;
    printf("  PASS: %s\n", message);
}

#define check(condition, message) check_impl((condition), (message), __FILE__, __LINE__)

static void *xmalloc(size_t bytes) {
    void *pointer = malloc(bytes == 0 ? 1 : bytes);
    if (pointer == NULL) {
        fprintf(stderr, "FAIL: out of memory (%zu bytes)\n", bytes);
        exit(1);
    }
    return pointer;
}

static char *xstrdup(const char *source) {
    size_t length = strlen(source);
    char *copy = xmalloc(length + 1);
    memcpy(copy, source, length + 1);
    return copy;
}

/* A fresh string that is `prefix` with one ASCII byte appended. */
static char *str_concat_byte(const char *prefix, char byte) {
    size_t length = strlen(prefix);
    char *result = xmalloc(length + 2);
    memcpy(result, prefix, length);
    result[length] = byte;
    result[length + 1] = '\0';
    return result;
}

/* Uppercase (ASCII) copy of `byte_len` raw bytes, NUL-terminated. */
static char *upper_dup(const void *data, size_t byte_len) {
    const unsigned char *bytes = (const unsigned char *)data;
    char *result = xmalloc(byte_len + 1);
    for (size_t index = 0; index < byte_len; index++) {
        result[index] = (char)toupper(bytes[index]);
    }
    result[byte_len] = '\0';
    return result;
}

/* ------------------------------------------------------------------------- */
/* LangMap: an accepted-language map, `output term -> best (min) weight`.     */
/* ------------------------------------------------------------------------- */

typedef struct {
    char *term;
    double dist;
} LangEntry;

typedef struct {
    LangEntry *items;
    size_t len;
    size_t cap;
} LangMap;

static int lang_get(const LangMap *map, const char *term, double *out) {
    for (size_t index = 0; index < map->len; index++) {
        if (strcmp(map->items[index].term, term) == 0) {
            if (out != NULL) {
                *out = map->items[index].dist;
            }
            return 1;
        }
    }
    return 0;
}

/* Insert `term`, or lower an existing entry to `dist` (min-plus aggregation). */
static void lang_put_min(LangMap *map, const char *term, double dist) {
    for (size_t index = 0; index < map->len; index++) {
        if (strcmp(map->items[index].term, term) == 0) {
            if (dist < map->items[index].dist) {
                map->items[index].dist = dist;
            }
            return;
        }
    }
    if (map->len == map->cap) {
        map->cap = map->cap == 0 ? 8 : map->cap * 2;
        map->items = realloc(map->items, map->cap * sizeof(*map->items));
        if (map->items == NULL) {
            fprintf(stderr, "FAIL: out of memory growing LangMap\n");
            exit(1);
        }
    }
    map->items[map->len].term = xstrdup(term);
    map->items[map->len].dist = dist;
    map->len++;
}

static int lang_equal(const LangMap *left, const LangMap *right) {
    if (left->len != right->len) {
        return 0;
    }
    for (size_t index = 0; index < left->len; index++) {
        double other = 0.0;
        if (!lang_get(right, left->items[index].term, &other)) {
            return 0;
        }
        /* Edit distances are integer-valued and exactly representable in f64,
         * so exact comparison is sound (mirrors the Rust BTreeMap equality). */
        if (left->items[index].dist != other) {
            return 0;
        }
    }
    return 1;
}

static void lang_free(LangMap *map) {
    for (size_t index = 0; index < map->len; index++) {
        free(map->items[index].term);
    }
    free(map->items);
    map->items = NULL;
    map->len = 0;
    map->cap = 0;
}

/* ------------------------------------------------------------------------- */
/* WFST walker: transduced language of a live vt.scalar-wfst.1 resource.      */
/*                                                                           */
/* Port of duallity/tests/support/wfst_walk.rs `WfstView::language(true)`:   */
/* a min-plus (tropical) traversal that pages arcs at VT_RECOMMENDED_ARC_    */
/* BATCH, enforces the paging law, and reads the accepted set as             */
/* `output term -> edit distance`.                                           */
/* ------------------------------------------------------------------------- */

typedef struct {
    uint64_t state;
    char *output;
    double weight;
} Frame;

typedef struct {
    uint64_t state;
    char *output;
    double weight;
} Memo;

/* Return 1 (skip) if (state, output) was already seen with weight <= `weight`;
 * otherwise record `weight` for (state, output) and return 0 (proceed). */
static int memo_should_skip(Memo **memo, size_t *len, size_t *cap, uint64_t state,
                            const char *output, double weight) {
    for (size_t index = 0; index < *len; index++) {
        if ((*memo)[index].state == state && strcmp((*memo)[index].output, output) == 0) {
            if ((*memo)[index].weight <= weight) {
                return 1;
            }
            (*memo)[index].weight = weight;
            return 0;
        }
    }
    if (*len == *cap) {
        *cap = *cap == 0 ? 64 : *cap * 2;
        *memo = realloc(*memo, *cap * sizeof(**memo));
        if (*memo == NULL) {
            fprintf(stderr, "FAIL: out of memory growing memo\n");
            exit(1);
        }
    }
    (*memo)[*len].state = state;
    (*memo)[*len].output = xstrdup(output);
    (*memo)[*len].weight = weight;
    (*len)++;
    return 0;
}

static void wfst_language(VtResource resource, LangMap *accepted) {
    check(resource.context != NULL && resource.vtable != NULL, "composed resource is non-null");
    check(resource.vtable->query_interface != NULL, "composed resource has query_interface");

    const void *interface = NULL;
    VtStatus status = resource.vtable->query_interface(
        resource.context, &VT_WFST_INTERFACE_ID, VT_WFST_INTERFACE_VERSION, &interface);
    check(status == VT_STATUS_OK && interface != NULL, "composed resource publishes vt.scalar-wfst.1");
    const VtWfstVTable *table = (const VtWfstVTable *)interface;
    check(table->start != NULL && table->state_info != NULL && table->state_arcs != NULL,
          "scalar-wfst vtable is complete");

    uint64_t start = 0;
    check(table->start(resource.context, &start) == VT_STATUS_OK, "scalar-wfst start()");

    Frame *stack = NULL;
    size_t stack_len = 0;
    size_t stack_cap = 0;
    Memo *memo = NULL;
    size_t memo_len = 0;
    size_t memo_cap = 0;

    stack_cap = 16;
    stack = xmalloc(stack_cap * sizeof(*stack));
    stack[0].state = start;
    stack[0].output = xstrdup("");
    stack[0].weight = 0.0;
    stack_len = 1;

    size_t guard = 0;
    while (stack_len > 0) {
        Frame frame = stack[--stack_len];
        if (++guard > 4000000u) {
            check(0, "traversal converges (no runaway cycle)");
        }

        if (memo_should_skip(&memo, &memo_len, &memo_cap, frame.state, frame.output, frame.weight)) {
            free(frame.output);
            continue;
        }

        uint8_t valid = 0;
        uint8_t is_final = 0;
        double final_weight = 0.0;
        check(table->state_info(resource.context, frame.state, &valid, &is_final, &final_weight)
                  == VT_STATUS_OK,
              "scalar-wfst state_info()");
        if (!valid) {
            free(frame.output);
            continue;
        }
        if (is_final) {
            lang_put_min(accepted, frame.output, frame.weight + final_weight);
        }

        size_t offset = 0;
        for (;;) {
            VtWfstArc page[VT_RECOMMENDED_ARC_BATCH];
            size_t written = 0;
            size_t total = 0;
            check(table->state_arcs(resource.context, frame.state, offset, page,
                                    VT_RECOMMENDED_ARC_BATCH, &written, &total)
                      == VT_STATUS_OK,
                  "scalar-wfst state_arcs()");
            check(written <= VT_RECOMMENDED_ARC_BATCH, "arcs written within capacity");
            check(offset + written <= total, "arcs page does not run past total");

            if (stack_len + written + 1 > stack_cap) {
                while (stack_len + written + 1 > stack_cap) {
                    stack_cap *= 2;
                }
                stack = realloc(stack, stack_cap * sizeof(*stack));
                if (stack == NULL) {
                    fprintf(stderr, "FAIL: out of memory growing frontier\n");
                    exit(1);
                }
            }
            for (size_t index = 0; index < written; index++) {
                VtWfstArc arc = page[index];
                char *next_output;
                if (arc.has_output == 1) {
                    check(arc.output_label < 0x80,
                          "output label is an ASCII scalar (golden alphabet)");
                    next_output = str_concat_byte(frame.output, (char)arc.output_label);
                } else {
                    next_output = xstrdup(frame.output);
                }
                stack[stack_len].state = arc.target_state;
                stack[stack_len].output = next_output;
                stack[stack_len].weight = frame.weight + arc.weight;
                stack_len++;
            }
            offset += written;
            if (offset >= total || written == 0) {
                break;
            }
        }
        free(frame.output);
    }

    for (size_t index = 0; index < memo_len; index++) {
        free(memo[index].output);
    }
    free(memo);
    free(stack);
}

/* ------------------------------------------------------------------------- */
/* Case-mapping WFST built via the lling_* builder ABI.                      */
/*                                                                           */
/* A single-state transducer with, for every distinct lowercase letter `c`   */
/* across `terms`, a self-loop c : UPPER(c) at weight zero, so it uppercases  */
/* any input string. Port of `case_mapping_resource` in the Rust reference.  */
/* ------------------------------------------------------------------------- */

static void build_case_mapper(const char *const *terms, size_t term_count, LlingWfst **out_wfst,
                              VtResource *out_resource) {
    int seen[256] = {0};
    for (size_t index = 0; index < term_count; index++) {
        for (const char *cursor = terms[index]; *cursor != '\0'; cursor++) {
            seen[(unsigned char)*cursor] = 1;
        }
    }

    LlingWfstBuilder *builder = NULL;
    check(lling_wfst_builder_new(&builder) == LLING_STATUS_OK, "case mapper builder new");
    uint32_t state = 0;
    check(lling_wfst_builder_add_state(builder, &state) == LLING_STATUS_OK, "case mapper add_state");
    check(lling_wfst_builder_set_start(builder, state) == LLING_STATUS_OK, "case mapper set_start");
    check(lling_wfst_builder_set_final(builder, state, 0.0) == LLING_STATUS_OK,
          "case mapper set_final");
    for (int code = 0; code < 256; code++) {
        if (!seen[code]) {
            continue;
        }
        check(lling_wfst_builder_add_arc(builder, state, (uint64_t)code, 1,
                                         (uint64_t)toupper(code), 1, state, 0.0)
                  == LLING_STATUS_OK,
              "case mapper self-loop arc");
    }

    LlingWfst *wfst = NULL;
    check(lling_wfst_builder_build(builder, &wfst) == LLING_STATUS_OK, "case mapper build");
    lling_wfst_builder_free(builder);

    VtResource resource = {0};
    check(lling_wfst_resource(wfst, &resource) == LLING_STATUS_OK, "case mapper resource");
    *out_wfst = wfst;
    *out_resource = resource;
}

/* ------------------------------------------------------------------------- */
/* Instrumented counting vt.dictionary.v1 provider (C port of                */
/* tests/support/counting_dictionary.rs).                                    */
/*                                                                           */
/* Each context is an independently reference-counted handle that shares one  */
/* immutable Model (trie + atomic metrics). The metrics count:               */
/*   into_raw  — every context handed out (initial resource + each snapshot)  */
/*   retain    — every resource-vtable retain                                 */
/*   release   — every resource-vtable release                               */
/* The ledger balances when (into_raw + retain) - release == 0.              */
/* ------------------------------------------------------------------------- */

typedef struct {
    uint64_t label;
    uint64_t child;
} CEdge;

typedef struct {
    int is_final;
    CEdge *edges;
    size_t edge_count;
} CNode;

typedef struct {
    CNode *nodes;
    size_t node_count;
    size_t len;
    _Atomic size_t model_refs;
    _Atomic size_t into_raw;
    _Atomic size_t retain;
    _Atomic size_t release;
    _Atomic size_t snapshot;
    _Atomic size_t root;
    _Atomic size_t len_calls;
    _Atomic size_t is_final_calls;
    _Atomic size_t edge_calls;
} CModel;

typedef struct {
    _Atomic size_t refs;
    CModel *model;
} CContext;

static const VtResourceVTable COUNTING_RESOURCE_VTABLE;
static const VtDictionaryVTable COUNTING_DICT_VTABLE;

static void cmodel_retain(CModel *model) {
    atomic_fetch_add_explicit(&model->model_refs, 1, memory_order_relaxed);
}

static void cmodel_release(CModel *model) {
    if (atomic_fetch_sub_explicit(&model->model_refs, 1, memory_order_acq_rel) == 1) {
        for (size_t index = 0; index < model->node_count; index++) {
            free(model->nodes[index].edges);
        }
        free(model->nodes);
        free(model);
    }
}

static VtResource cmake_resource(CModel *model) {
    atomic_fetch_add_explicit(&model->into_raw, 1, memory_order_relaxed);
    cmodel_retain(model);
    CContext *context = xmalloc(sizeof(*context));
    atomic_init(&context->refs, 1);
    context->model = model;
    VtResource resource = {0};
    resource.context = context;
    resource.vtable = &COUNTING_RESOURCE_VTABLE;
    return resource;
}

static void counting_retain(void *raw) {
    CContext *context = (CContext *)raw;
    atomic_fetch_add_explicit(&context->model->retain, 1, memory_order_relaxed);
    atomic_fetch_add_explicit(&context->refs, 1, memory_order_relaxed);
}

static void counting_release(void *raw) {
    CContext *context = (CContext *)raw;
    atomic_fetch_add_explicit(&context->model->release, 1, memory_order_relaxed);
    if (atomic_fetch_sub_explicit(&context->refs, 1, memory_order_acq_rel) == 1) {
        CModel *model = context->model;
        free(context);
        cmodel_release(model);
    }
}

static VtStatus counting_query_interface(void *raw, const VtInterfaceId *interface_id,
                                         uint32_t minimum_version, const void **out_vtable) {
    (void)raw;
    if (interface_id == NULL || out_vtable == NULL) {
        return VT_STATUS_NULL_POINTER;
    }
    if (memcmp(interface_id->bytes, VT_DICTIONARY_INTERFACE_ID.bytes, 16) != 0
        || minimum_version > VT_DICTIONARY_INTERFACE_VERSION) {
        return VT_STATUS_UNSUPPORTED;
    }
    *out_vtable = &COUNTING_DICT_VTABLE;
    return VT_STATUS_OK;
}

static VtStatus counting_snapshot(void *raw, VtResource *out_snapshot) {
    if (out_snapshot == NULL) {
        return VT_STATUS_NULL_POINTER;
    }
    CContext *context = (CContext *)raw;
    atomic_fetch_add_explicit(&context->model->snapshot, 1, memory_order_relaxed);
    /* The model is immutable, so a snapshot is a fresh retain of the same
     * revision (shared metrics included). */
    *out_snapshot = cmake_resource(context->model);
    return VT_STATUS_OK;
}

static VtStatus counting_root(void *raw, uint64_t *out_node) {
    if (out_node == NULL) {
        return VT_STATUS_NULL_POINTER;
    }
    CContext *context = (CContext *)raw;
    atomic_fetch_add_explicit(&context->model->root, 1, memory_order_relaxed);
    *out_node = 0;
    return VT_STATUS_OK;
}

static VtStatus counting_len(void *raw, size_t *out_len, uint8_t *out_known) {
    if (out_len == NULL || out_known == NULL) {
        return VT_STATUS_NULL_POINTER;
    }
    CContext *context = (CContext *)raw;
    atomic_fetch_add_explicit(&context->model->len_calls, 1, memory_order_relaxed);
    *out_len = context->model->len;
    *out_known = 1;
    return VT_STATUS_OK;
}

static VtStatus counting_node_is_final(void *raw, uint64_t node, uint8_t *out_is_final) {
    if (out_is_final == NULL) {
        return VT_STATUS_NULL_POINTER;
    }
    CContext *context = (CContext *)raw;
    atomic_fetch_add_explicit(&context->model->is_final_calls, 1, memory_order_relaxed);
    if (node >= context->model->node_count) {
        return VT_STATUS_INVALID_ARGUMENT;
    }
    *out_is_final = (uint8_t)(context->model->nodes[node].is_final ? 1 : 0);
    return VT_STATUS_OK;
}

static VtStatus counting_node_edges(void *raw, uint64_t node, size_t start,
                                    VtDictionaryEdge *out_edges, size_t capacity,
                                    size_t *out_written, size_t *out_total) {
    if ((capacity != 0 && out_edges == NULL) || out_written == NULL || out_total == NULL) {
        return VT_STATUS_NULL_POINTER;
    }
    CContext *context = (CContext *)raw;
    atomic_fetch_add_explicit(&context->model->edge_calls, 1, memory_order_relaxed);
    if (node >= context->model->node_count) {
        return VT_STATUS_INVALID_ARGUMENT;
    }
    CNode *entry = &context->model->nodes[node];
    size_t total = entry->edge_count;
    size_t written = 0;
    for (size_t index = start; index < total && written < capacity; index++) {
        out_edges[written].label = entry->edges[index].label;
        out_edges[written].node = entry->edges[index].child;
        written++;
    }
    *out_written = written;
    *out_total = total;
    return VT_STATUS_OK;
}

static const VtResourceVTable COUNTING_RESOURCE_VTABLE = {
    .struct_size = sizeof(VtResourceVTable),
    .abi_version = VT_ABI_VERSION,
    .reserved = 0,
    .retain = counting_retain,
    .release = counting_release,
    .query_interface = counting_query_interface,
};

static const VtDictionaryVTable COUNTING_DICT_VTABLE = {
    .struct_size = sizeof(VtDictionaryVTable),
    .interface_version = VT_DICTIONARY_INTERFACE_VERSION,
    .unit_domain = VT_UNIT_DOMAIN_UNICODE_SCALAR,
    .value_domain = VT_VALUE_DOMAIN_UNIT,
    .flags = VT_DICTIONARY_FLAG_IMMUTABLE | VT_DICTIONARY_FLAG_PARALLEL_REENTRANT,
    .snapshot = counting_snapshot,
    .root = counting_root,
    .len = counting_len,
    .node_is_final = counting_node_is_final,
    /* node_value_u64 and node_transition intentionally NULL: leaving
     * node_transition unset routes every access through the node_edges paging
     * loop (the surface finding F3 covers), matching the Rust fixture. */
    .node_edges = counting_node_edges,
};

/* Add a child edge (label -> child) to node `parent`, growing its edge list. */
static void cnode_push_edge(CNode *parent, uint64_t label, uint64_t child) {
    CEdge *grown = realloc(parent->edges, (parent->edge_count + 1) * sizeof(*grown));
    if (grown == NULL) {
        fprintf(stderr, "FAIL: out of memory growing trie node\n");
        exit(1);
    }
    parent->edges = grown;
    parent->edges[parent->edge_count].label = label;
    parent->edges[parent->edge_count].child = child;
    parent->edge_count++;
}

static int cedge_cmp(const void *left, const void *right) {
    const CEdge *a = (const CEdge *)left;
    const CEdge *b = (const CEdge *)right;
    if (a->label < b->label) {
        return -1;
    }
    if (a->label > b->label) {
        return 1;
    }
    return 0;
}

/* Build an honest trie over `terms` with Unicode-scalar (ASCII here) labels,
 * ascending-sorted edges. model_refs starts at 1 (the owning handle's ref). */
static CModel *cmodel_from_terms(const char *const *terms, size_t term_count) {
    size_t node_cap = 8;
    CNode *nodes = xmalloc(node_cap * sizeof(*nodes));
    size_t node_count = 1;
    nodes[0].is_final = 0;
    nodes[0].edges = NULL;
    nodes[0].edge_count = 0;
    size_t len = 0;

    for (size_t term_index = 0; term_index < term_count; term_index++) {
        size_t current = 0;
        for (const char *cursor = terms[term_index]; *cursor != '\0'; cursor++) {
            uint64_t label = (uint64_t)(unsigned char)*cursor;
            size_t child = 0;
            int found = 0;
            for (size_t edge = 0; edge < nodes[current].edge_count; edge++) {
                if (nodes[current].edges[edge].label == label) {
                    child = (size_t)nodes[current].edges[edge].child;
                    found = 1;
                    break;
                }
            }
            if (!found) {
                if (node_count == node_cap) {
                    node_cap *= 2;
                    nodes = realloc(nodes, node_cap * sizeof(*nodes));
                    if (nodes == NULL) {
                        fprintf(stderr, "FAIL: out of memory growing trie\n");
                        exit(1);
                    }
                }
                child = node_count;
                nodes[child].is_final = 0;
                nodes[child].edges = NULL;
                nodes[child].edge_count = 0;
                node_count++;
                cnode_push_edge(&nodes[current], label, (uint64_t)child);
            }
            current = child;
        }
        if (!nodes[current].is_final) {
            nodes[current].is_final = 1;
            len++;
        }
    }

    for (size_t index = 0; index < node_count; index++) {
        if (nodes[index].edge_count > 1) {
            qsort(nodes[index].edges, nodes[index].edge_count, sizeof(CEdge), cedge_cmp);
        }
    }

    CModel *model = xmalloc(sizeof(*model));
    model->nodes = nodes;
    model->node_count = node_count;
    model->len = len;
    atomic_init(&model->model_refs, 1);
    atomic_init(&model->into_raw, 0);
    atomic_init(&model->retain, 0);
    atomic_init(&model->release, 0);
    atomic_init(&model->snapshot, 0);
    atomic_init(&model->root, 0);
    atomic_init(&model->len_calls, 0);
    atomic_init(&model->is_final_calls, 0);
    atomic_init(&model->edge_calls, 0);
    return model;
}

/* ------------------------------------------------------------------------- */
/* Phase A: the real family pipeline over a libdictenstein DynamicDawg.       */
/* ------------------------------------------------------------------------- */

static void collect_cursor_batch(LangMap *ball, const LlevMatchBatchView *view) {
    for (size_t index = 0; index < view->len; index++) {
        const LlevMatch *match = &view->matches[index];
        check(match->unit_domain == VT_UNIT_DOMAIN_UNICODE_SCALAR,
              "cursor term is a Unicode scalar (UTF-8) term");
        char *upper = upper_dup(match->term_data, match->byte_len);
        lang_put_min(ball, upper, (double)match->distance);
        free(upper);
    }
}

/* Drain a cursor to completion into `ball`, releasing every leased batch. */
static void drain_cursor(LlevQueryCursor *cursor, LangMap *ball, const char *what) {
    LlevMatchBatchView view = {0};
    for (;;) {
        LlevStatus status = llev_query_cursor_next_batch(cursor, 4, &view);
        if (status == LLEV_STATUS_END) {
            break;
        }
        check(status == LLEV_STATUS_OK, what);
        collect_cursor_batch(ball, &view);
        check(llev_query_cursor_release_batch(cursor, view.generation) == LLEV_STATUS_OK,
              "release leased cursor batch");
    }
}

static void run_family_pipeline(void) {
    printf("[phase A] real family pipeline: libdictenstein -> duallity -> lling-llang / liblevenshtein\n");

    /* 1. Build a small DynamicDawg over five terms via the ldict_* C ABI. */
    LdictDictionary *dictionary = NULL;
    check(ldict_dynamic_dawg_new(VT_UNIT_DOMAIN_UNICODE_SCALAR, &dictionary) == LDICT_STATUS_OK,
          "ldict_dynamic_dawg_new (UnicodeScalar)");
    const char *original[] = {"cat", "car", "cot", "dog", "cats"};
    LdictOptionalU64 no_value = {.value = 0, .has_value = 0, .reserved = {0}};
    for (size_t index = 0; index < 5; index++) {
        uint8_t inserted = 0;
        check(ldict_dictionary_insert_text(dictionary, (const uint8_t *)original[index],
                                           strlen(original[index]), no_value, &inserted)
                  == LDICT_STATUS_OK,
              "ldict_dictionary_insert_text");
        check(inserted == 1, "original term newly inserted");
    }

    /* Borrowed resource — valid while `dictionary` is alive; NOT released. A
     * retaining consumer (duallity snapshot, llev retain) takes its own retain. */
    VtResource dictionary_resource = {0};
    check(ldict_dictionary_resource(dictionary, &dictionary_resource) == LDICT_STATUS_OK,
          "ldict_dictionary_resource (borrowed)");

    /* 2. Capture the duallity Levenshtein WFST (snapshots the dictionary once). */
    DuallityWfst *wfst = NULL;
    check(duallity_wfst_new(dictionary_resource, (const uint8_t *)"cat", 3, 1,
                            DUALLITY_ALGORITHM_STANDARD, DUALLITY_WFST_LEVENSHTEIN, &wfst)
              == DUALLITY_STATUS_OK,
          "duallity_wfst_new (capture-once)");

    /* 3a. Export the WFST's scalar-WFST resource (owned retain; released later). */
    VtResource duallity_resource = {0};
    check(duallity_wfst_resource(wfst, &duallity_resource) == DUALLITY_STATUS_OK,
          "duallity_wfst_resource (owned retain)");

    /* A liblevenshtein cursor over the SAME producer. query_utf8 captures the
     * revision now; pull one batch to lock it in before the mutations. */
    LlevTransducer *transducer = NULL;
    check(llev_transducer_new(&dictionary_resource, LLEV_ALGORITHM_STANDARD, &transducer)
              == LLEV_STATUS_OK,
          "llev_transducer_new over the dictionary resource");
    LlevQueryCursor *cursor = NULL;
    check(llev_transducer_query_utf8(transducer, "cat", 3, 1, LLEV_QUERY_ORDER_TRAVERSAL, &cursor)
              == LLEV_STATUS_OK,
          "llev_transducer_query_utf8");

    LangMap cursor_ball = {0};
    LlevMatchBatchView view = {0};
    LlevStatus first = llev_query_cursor_next_batch(cursor, 4, &view);
    check(first == LLEV_STATUS_OK || first == LLEV_STATUS_END, "first cursor batch");
    if (first == LLEV_STATUS_OK) {
        collect_cursor_batch(&cursor_ball, &view);
        check(llev_query_cursor_release_batch(cursor, view.generation) == LLEV_STATUS_OK,
              "release first cursor batch");
    }

    /* Case mapper over the ORIGINAL alphabet. */
    LlingWfst *mapper = NULL;
    VtResource mapper_resource = {0};
    build_case_mapper(original, 5, &mapper, &mapper_resource);

    /* ~5 mutations AFTER both captures. If either capture leaked into the live
     * dictionary, the observed languages would drift from the golden. */
    uint8_t flag = 0;
    check(ldict_dictionary_remove_text(dictionary, (const uint8_t *)"car", 3, &flag)
                  == LDICT_STATUS_OK
              && flag == 1,
          "remove car");
    check(ldict_dictionary_remove_text(dictionary, (const uint8_t *)"cot", 3, &flag)
                  == LDICT_STATUS_OK
              && flag == 1,
          "remove cot");
    check(ldict_dictionary_insert_text(dictionary, (const uint8_t *)"cab", 3, no_value, &flag)
                  == LDICT_STATUS_OK
              && flag == 1,
          "insert cab");
    check(ldict_dictionary_insert_text(dictionary, (const uint8_t *)"caw", 3, no_value, &flag)
                  == LDICT_STATUS_OK
              && flag == 1,
          "insert caw");
    check(ldict_dictionary_insert_text(dictionary, (const uint8_t *)"bat", 3, no_value, &flag)
                  == LDICT_STATUS_OK
              && flag == 1,
          "insert bat");

    /* Drain the rest of the cursor AFTER the mutations: an isolated cursor still
     * yields its captured (original) revision. */
    drain_cursor(cursor, &cursor_ball, "drain original cursor batch");

    /* 3b. Compose the duallity WFST resource with the case mapper via lling. */
    LlingWfst *composed = NULL;
    check(lling_wfst_compose(duallity_resource, mapper_resource, &composed) == LLING_STATUS_OK,
          "lling_wfst_compose (duallity WFST o case mapper)");
    VtResource composed_resource = {0};
    check(lling_wfst_resource(composed, &composed_resource) == LLING_STATUS_OK,
          "lling_wfst_resource (composed, owned retain)");

    /* 4. Traverse the composed resource and cross-check three ways. */
    LangMap composed_language = {0};
    wfst_language(composed_resource, &composed_language);

    LangMap golden = {0};
    lang_put_min(&golden, "CAR", 1.0);
    lang_put_min(&golden, "CAT", 0.0);
    lang_put_min(&golden, "CATS", 1.0);
    lang_put_min(&golden, "COT", 1.0);

    check(lang_equal(&composed_language, &golden), "composed traversal == golden (DUAL-FAM-1)");
    check(lang_equal(&cursor_ball, &golden), "liblevenshtein cursor == golden (DUAL-FAM-1)");
    check(lang_equal(&composed_language, &cursor_ball), "composed traversal == cursor (DUAL-FAM-1)");

    /* DUAL-FAM-2: a FRESH cursor over the mutated dictionary reflects the
     * mutations, confirming the captures were genuinely isolated (not vacuously
     * equal because nothing changed). */
    LlevQueryCursor *live_cursor = NULL;
    check(llev_transducer_query_utf8(transducer, "cat", 3, 1, LLEV_QUERY_ORDER_TRAVERSAL,
                                     &live_cursor)
              == LLEV_STATUS_OK,
          "fresh query over mutated dictionary");
    LangMap live = {0};
    drain_cursor(live_cursor, &live, "drain live cursor batch");
    check(!lang_equal(&live, &golden), "live dictionary reflects the mutations (DUAL-FAM-2)");
    check(lang_get(&live, "CAB", NULL), "live view sees the inserted term CAB");
    check(!lang_get(&live, "CAR", NULL), "live view sees the removed term CAR gone");

    /* Teardown. */
    lang_free(&composed_language);
    lang_free(&cursor_ball);
    lang_free(&golden);
    lang_free(&live);
    lling_resource_release(composed_resource);
    lling_wfst_free(composed);
    lling_resource_release(mapper_resource);
    lling_wfst_free(mapper);
    check(llev_query_cursor_free(live_cursor) == LLEV_STATUS_OK, "free live cursor");
    check(llev_query_cursor_free(cursor) == LLEV_STATUS_OK, "free cursor");
    llev_transducer_free(transducer);
    duallity_resource_release(duallity_resource);
    duallity_wfst_free(wfst);
    /* dictionary_resource is a borrow; it is NOT released. */
    ldict_dictionary_free(dictionary);
}

/* ------------------------------------------------------------------------- */
/* Phase B: retain/release ledger over the instrumented C provider.          */
/* ------------------------------------------------------------------------- */

static void run_ledger(int source_first) {
    printf("[phase B] retain/release ledger (source_first=%d)\n", source_first);

    const char *terms[] = {"cat", "cot", "cats"};
    CModel *model = cmodel_from_terms(terms, 3);
    VtResource source = cmake_resource(model);

    DuallityWfst *wfst = NULL;
    check(duallity_wfst_new(source, (const uint8_t *)"cat", 3, 1, DUALLITY_ALGORITHM_STANDARD,
                            DUALLITY_WFST_LEVENSHTEIN, &wfst)
              == DUALLITY_STATUS_OK,
          "duallity_wfst_new over the instrumented provider");
    check(atomic_load_explicit(&model->snapshot, memory_order_relaxed) == 1,
          "snapshot fired exactly once at construction (capture-once)");

    VtResource duallity_resource = {0};
    check(duallity_wfst_resource(wfst, &duallity_resource) == DUALLITY_STATUS_OK,
          "duallity_wfst_resource over the instrumented provider");

    LlingWfst *mapper = NULL;
    VtResource mapper_resource = {0};
    build_case_mapper(terms, 3, &mapper, &mapper_resource);

    LlingWfst *composed = NULL;
    check(lling_wfst_compose(duallity_resource, mapper_resource, &composed) == LLING_STATUS_OK,
          "lling_wfst_compose over the instrumented provider");
    VtResource composed_resource = {0};
    check(lling_wfst_resource(composed, &composed_resource) == LLING_STATUS_OK,
          "composed resource over the instrumented provider");

    LangMap observed = {0};
    wfst_language(composed_resource, &observed);
    check(lang_get(&observed, "CAT", NULL), "instrumented pipeline still transduces CAT");
    lang_free(&observed);

    /* Drop the composition retains and the case mapper before the ledger check;
     * this releases lling's captured retain on the duallity WFST resource. */
    lling_resource_release(composed_resource);
    lling_wfst_free(composed);
    lling_resource_release(mapper_resource);
    lling_wfst_free(mapper);

    /* The source snapshot is retained by the WFST chain until every scalar-WFST
     * retain is released, so it must survive an early source release. */
    if (source_first) {
        duallity_resource_release(source);
        duallity_resource_release(duallity_resource);
        duallity_wfst_free(wfst);
    } else {
        duallity_resource_release(duallity_resource);
        duallity_wfst_free(wfst);
        duallity_resource_release(source);
    }

    size_t into_raw = atomic_load_explicit(&model->into_raw, memory_order_relaxed);
    size_t retain = atomic_load_explicit(&model->retain, memory_order_relaxed);
    size_t release = atomic_load_explicit(&model->release, memory_order_relaxed);
    check(into_raw == 2, "exactly two contexts handed out (source + one snapshot)");
    check(retain == 0, "no extra resource-vtable retains taken on the dictionary");
    /* outstanding = (into_raw + retain) - release == 0 */
    check(into_raw + retain == release, "retain/release ledger balances to zero (DUAL-FAM-3)");

    cmodel_release(model);
}

/* ------------------------------------------------------------------------- */
/* main                                                                      */
/* ------------------------------------------------------------------------- */

int main(void) {
    printf("family_pipeline: four disjoint cdylibs in one process\n");

    /* Smoke: every one of the four C ABIs is linked and callable. */
    check(ldict_abi_version() == LDICT_ABI_VERSION, "libdictenstein ABI version");
    check(duallity_abi_version() == DUALLITY_ABI_VERSION, "duallity ABI version");
    check(lling_abi_version() == LLING_ABI_VERSION, "lling-llang ABI version");
    check(llev_abi_version() == LLEV_ABI_VERSION, "liblevenshtein ABI version");

    run_family_pipeline();
    run_ledger(1);
    run_ledger(0);

    printf("\nOK: all %u assertions passed across four cdylibs.\n", g_passed);
    return 0;
}
