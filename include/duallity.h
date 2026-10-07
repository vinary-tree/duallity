/* Stable project-owned C API for duallity WFST adapters. */
#ifndef DUALLITY_H
#define DUALLITY_H

#include <stddef.h>
#include <stdint.h>
#ifndef VT_INTEROP_HEADER
#define VT_INTEROP_HEADER "vinary_tree_interop.h"
#endif
#include VT_INTEROP_HEADER

#if defined(_WIN32) || defined(__CYGWIN__)
#  if defined(DUALLITY_BUILDING_DLL)
#    define DUALLITY_API __declspec(dllexport)
#  elif defined(DUALLITY_USING_DLL)
#    define DUALLITY_API __declspec(dllimport)
#  else
#    define DUALLITY_API
#  endif
#elif defined(__GNUC__) || defined(__clang__)
#  define DUALLITY_API __attribute__((visibility("default")))
#else
#  define DUALLITY_API
#endif

#ifdef __cplusplus
extern "C" {
#endif

#define DUALLITY_ABI_VERSION 1u
#define DUALLITY_API_REVISION 6u

typedef enum DuallityStatus {
    DUALLITY_STATUS_OK = 0,
    DUALLITY_STATUS_INVALID_ARGUMENT = 1,
    DUALLITY_STATUS_INVALID_UTF8 = 2,
    DUALLITY_STATUS_NULL_POINTER = 3,
    DUALLITY_STATUS_PANIC = 4,
    DUALLITY_STATUS_INCOMPATIBLE_RESOURCE = 5,
    DUALLITY_STATUS_PROVIDER_ERROR = 6,
    DUALLITY_STATUS_LIMIT_EXCEEDED = 7
} DuallityStatus;

typedef enum DuallityAlgorithm {
    DUALLITY_ALGORITHM_STANDARD = 0,
    DUALLITY_ALGORITHM_TRANSPOSITION = 1,
    DUALLITY_ALGORITHM_MERGE_AND_SPLIT = 2,
    DUALLITY_ALGORITHM_DAMERAU_LEVENSHTEIN = 3
} DuallityAlgorithm;

typedef enum DuallityWfstKind {
    DUALLITY_WFST_LEVENSHTEIN = 0,
    DUALLITY_WFST_UNIVERSAL_STANDARD = 1,
    DUALLITY_WFST_UNIVERSAL_TRANSPOSITION = 2,
    DUALLITY_WFST_UNIVERSAL_MERGE_AND_SPLIT = 3,
    DUALLITY_WFST_GENERALIZED_STANDARD = 4,
    DUALLITY_WFST_GENERALIZED_TRANSPOSITION = 5,
    DUALLITY_WFST_GENERALIZED_MERGE_AND_SPLIT = 6,
    DUALLITY_WFST_GENERALIZED_PHONETIC = 7,
    DUALLITY_WFST_FZF = 8
} DuallityWfstKind;

/*
 * Revision-3 configuration record layouts. The new constructor/control
 * symbols require duallity_api_revision() >= 3. Existing revision-2
 * functions and ABI version 1 are unchanged.
 *
 * Every header declares its readable byte extent. Version 1 readers require
 * the complete known prefix and zero for all reserved and unknown trailing
 * bytes. The caller keeps input arrays alive through the constructor call;
 * successful construction deep-copies every custom operation and restriction.
 */
#define DUALLITY_CONFIG_API_REVISION 3u
#define DUALLITY_CONFIG_RECORD_VERSION 1u
#define DUALLITY_CONFIG_MAX_RECORD_BYTES 4096u
#define DUALLITY_CONFIG_MAX_OPERATIONS 4096u
#define DUALLITY_CONFIG_MAX_RESTRICTION_PAIRS 4096u
#define DUALLITY_CONFIG_MAX_CUSTOM_TEXT_BYTES 1048576u

typedef struct DuallityRecordHeaderV1 {
    uint32_t struct_size;
    uint32_t record_version;
    uint64_t reserved;
} DuallityRecordHeaderV1;

typedef enum DuallityCachePolicyV1 {
    DUALLITY_CACHE_ALL = 0,
    DUALLITY_NO_CACHE = 1,
    DUALLITY_LRU = 2
} DuallityCachePolicyV1;

typedef enum DuallityOperationApplicabilityV1 {
    DUALLITY_APPLICABILITY_ANY = 0,
    DUALLITY_APPLICABILITY_EQUAL = 1,
    DUALLITY_APPLICABILITY_ADJACENT_TRANSPOSE = 2,
    DUALLITY_APPLICABILITY_LISTED = 3
} DuallityOperationApplicabilityV1;

typedef struct DuallityRestrictionV1 {
    DuallityRecordHeaderV1 header;
    const uint8_t* source_data;
    uint64_t source_len;
    const uint8_t* target_data;
    uint64_t target_len;
    uint64_t reserved[2];
} DuallityRestrictionV1;

typedef struct DuallityOperationV1 {
    DuallityRecordHeaderV1 header;
    uint64_t consume_x;
    uint64_t consume_y;
    double weight;
    uint32_t applicability;
    uint32_t reserved_zero;
    const uint8_t* name_data;
    uint64_t name_len;
    const DuallityRestrictionV1* restrictions;
    uint64_t restriction_count;
    uint64_t restriction_stride;
    uint64_t reserved[2];
} DuallityOperationV1;

typedef struct DuallityGeneralizedLimitsV1 {
    DuallityRecordHeaderV1 header;
    uint64_t max_query_bytes;
    uint64_t max_query_scalars;
    uint64_t max_operation_source_scalars;
    uint64_t max_operation_query_scalars;
    uint64_t max_retained_dictionary_nodes;
    uint64_t max_retained_wfst_states;
    uint64_t max_paths_per_expansion;
    uint64_t max_work_units_per_expansion;
    uint64_t reserved[2];
} DuallityGeneralizedLimitsV1;

typedef struct DuallityWfstOptionsV1 {
    DuallityRecordHeaderV1 header;
    uint32_t kind;
    uint32_t algorithm;
    uint64_t maximum_distance;
    uint32_t cache_policy;
    uint32_t reserved_zero;
    uint64_t cache_capacity;
    const DuallityGeneralizedLimitsV1* limits;
    const DuallityOperationV1* operations;
    uint64_t operation_count;
    uint64_t operation_stride;
    uint64_t reserved[2];
} DuallityWfstOptionsV1;

typedef struct DuallityCacheStatisticsV1 {
    DuallityRecordHeaderV1 header;
    uint64_t hits;
    uint64_t misses;
    uint64_t faults;
    uint64_t uncacheable_results;
    uint64_t insertions;
    uint64_t evictions;
    uint64_t raced_publications;
    uint64_t clears;
    uint64_t resident_states;
    uint64_t recency_records;
    uint64_t reserved[2];
} DuallityCacheStatisticsV1;

/* Revision-5 native WallBreaker result ingress. All records and term buffers
 * are borrowed for the call and copied into the returned WFST. The caller
 * supplies the complete verified result set from one dictionary revision.
 * At most 4096 results, 1 MiB of UTF-8 term bytes, 256 scalars per term or
 * query, and maximum_distance <= 8 are accepted. Empty terms are valid. */
typedef struct DuallityWallBreakerResultV1 {
    DuallityRecordHeaderV1 header;
    const uint8_t* term_data;
    uint64_t term_len;
    uint64_t distance;
    uint64_t reserved[2];
} DuallityWallBreakerResultV1;

/* Revision-6 FZF scoring, bounded ranking, and configured WFST records.
 * All fields are native-endian C ABI values. Unknown trailing bytes and all
 * reserved fields must be zero. Ranking hit text is borrowed until free. */
typedef enum DuallityFzfSchemeV1 {
    DUALLITY_FZF_SCHEME_DEFAULT = 0,
    DUALLITY_FZF_SCHEME_PATH = 1,
    DUALLITY_FZF_SCHEME_HISTORY = 2
} DuallityFzfSchemeV1;

typedef struct DuallityFzfConfigV1 {
    DuallityRecordHeaderV1 header;
    uint32_t case_sensitive;
    uint32_t scheme;
    uint64_t top_k;
    uint64_t max_query_chars;
    uint64_t max_candidate_chars;
    uint64_t max_work_units;
    uint32_t cache_policy;
    uint32_t reserved_zero;
    uint64_t cache_capacity;
    uint64_t reserved[2];
} DuallityFzfConfigV1;

typedef struct DuallityFzfScoreV1 {
    DuallityRecordHeaderV1 header;
    uint32_t matched;
    int32_t score;
    int32_t maximum_score;
    uint32_t reserved_zero;
    uint64_t reserved[2];
} DuallityFzfScoreV1;

typedef struct DuallityFzfStatisticsV1 {
    DuallityRecordHeaderV1 header;
    uint64_t columns_computed;
    uint64_t candidates_scored;
    uint64_t prefixes_pruned;
    uint64_t score_bound_prefixes_pruned;
    uint64_t length_prefixes_pruned;
    uint64_t upper_bounds_computed;
    uint64_t result_count;
    uint64_t work_units;
    uint64_t reserved[2];
} DuallityFzfStatisticsV1;

typedef struct DuallityFzfHitV1 {
    DuallityRecordHeaderV1 header;
    const uint8_t* term_data;
    uint64_t term_len;
    int32_t score;
    uint32_t reserved_zero;
    uint64_t reserved[2];
} DuallityFzfHitV1;

typedef struct DuallityWfst DuallityWfst;
typedef struct DuallityFzfRanking DuallityFzfRanking;

/* Revision-4 phonetic constructors return owned vt.scalar-wfst.1 resources. */
typedef struct DuallityPhoneticRuleV1 {
    DuallityRecordHeaderV1 header;
    const uint8_t* input_data;
    size_t input_len;
    const uint8_t* output_data;
    size_t output_len;
    double cost;
    int32_t priority;
    uint32_t reserved;
} DuallityPhoneticRuleV1;

DUALLITY_API uint32_t duallity_abi_version(void);
DUALLITY_API uint32_t duallity_api_revision(void);
DUALLITY_API const char* duallity_last_error_message(void);
DUALLITY_API DuallityStatus duallity_wfst_new(
    VtResource dictionary,
    const uint8_t* query_data,
    size_t query_len,
    size_t maximum_distance,
    uint32_t algorithm,
    uint32_t kind,
    DuallityWfst** out_wfst);
/* Pointer form for FFIs that cannot pass C aggregates by value. */
DUALLITY_API DuallityStatus duallity_wfst_new_ref(
    const VtResource* dictionary,
    const uint8_t* query_data,
    size_t query_len,
    size_t maximum_distance,
    uint32_t algorithm,
    uint32_t kind,
    DuallityWfst** out_wfst);
/* Caller sets header.struct_size, record_version=1, reserved=0. */
DUALLITY_API DuallityStatus duallity_wfst_options_default(
    DuallityWfstOptionsV1* out_options);
/* All nested inputs are borrowed only for the call and deep-copied. */
DUALLITY_API DuallityStatus duallity_wfst_new_configured_ref(
    const VtResource* dictionary,
    const uint8_t* query_data,
    size_t query_len,
    const DuallityWfstOptionsV1* options,
    DuallityWfst** out_wfst);
DUALLITY_API void duallity_wfst_free(DuallityWfst* wfst);
/* On success, out_resource owns one retain. */
DUALLITY_API DuallityStatus duallity_wfst_resource(
    const DuallityWfst* wfst, VtResource* out_resource);
/* Nested pointers are read-only and expire when the WFST handle is freed. */
DUALLITY_API DuallityStatus duallity_wfst_options_get(
    const DuallityWfst* wfst,
    DuallityWfstOptionsV1* out_options);
DUALLITY_API DuallityStatus duallity_wfst_cache_statistics(
    const DuallityWfst* wfst,
    DuallityCacheStatisticsV1* out_statistics);
DUALLITY_API DuallityStatus duallity_wfst_cache_clear(DuallityWfst* wfst);
DUALLITY_API DuallityStatus duallity_wfst_cache_set_policy(
    DuallityWfst* wfst,
    uint32_t policy,
    uint64_t capacity);
DUALLITY_API DuallityStatus duallity_fzf_config_default(
    DuallityFzfConfigV1* out_config);
DUALLITY_API DuallityStatus duallity_fzf_score(
    const uint8_t* query_data, size_t query_len,
    const uint8_t* candidate_data, size_t candidate_len,
    const DuallityFzfConfigV1* options, DuallityFzfScoreV1* out_score);
DUALLITY_API DuallityStatus duallity_fzf_wfst_new_ref(
    const VtResource* dictionary, const uint8_t* query_data, size_t query_len,
    const DuallityFzfConfigV1* options, DuallityWfst** out_wfst);
DUALLITY_API DuallityStatus duallity_fzf_wfst_config_get(
    const DuallityWfst* wfst, DuallityFzfConfigV1* out_config);
DUALLITY_API DuallityStatus duallity_fzf_rank_ref(
    const VtResource* dictionary, const uint8_t* query_data, size_t query_len,
    const DuallityFzfConfigV1* options, DuallityFzfRanking** out_ranking);
DUALLITY_API DuallityStatus duallity_fzf_ranking_len(
    const DuallityFzfRanking* ranking, uint64_t* out_len);
DUALLITY_API DuallityStatus duallity_fzf_ranking_get(
    const DuallityFzfRanking* ranking, uint64_t index, DuallityFzfHitV1* out_hit);
DUALLITY_API DuallityStatus duallity_fzf_ranking_statistics(
    const DuallityFzfRanking* ranking, DuallityFzfStatisticsV1* out_stats);
DUALLITY_API void duallity_fzf_ranking_free(DuallityFzfRanking* ranking);
/* Available in native-bindings-full builds at API revision 5. The returned
 * handle supports resource retention, cache statistics, clear and policy
 * controls, and must be released with duallity_wfst_free. */
DUALLITY_API DuallityStatus duallity_wallbreaker_wfst_new_results(
    const uint8_t* query_data,
    size_t query_len,
    uint32_t algorithm_value,
    uint64_t maximum_distance,
    const DuallityWallBreakerResultV1* results,
    uint64_t result_count,
    uint64_t result_stride,
    uint32_t cache_policy,
    uint64_t cache_capacity,
    DuallityWfst** out_wfst);
DUALLITY_API void duallity_resource_release(VtResource resource);

/* Available when the native library is built with phonetic-rules. Caller owns
 * every successful out_resource and releases it through the resource vtable.
 * Cache policy: 0=all, 1=none, 2=LRU (positive capacity or default when zero).
 * All input buffers and records are borrowed for the call only. */
DUALLITY_API DuallityStatus duallity_phonetic_nfa_new(
    const uint8_t* pattern_data, size_t pattern_len,
    const uint8_t* alphabet_data, size_t alphabet_len,
    double phonetic_weight, uint32_t cache_policy, uint64_t cache_capacity,
    VtResource* out_resource);
DUALLITY_API DuallityStatus duallity_phonetic_product_new_ref(
    const VtResource* dictionary,
    const uint8_t* pattern_data, size_t pattern_len,
    uint32_t maximum_distance, double phonetic_weight, double edit_weight,
    uint32_t cache_policy, uint64_t cache_capacity,
    VtResource* out_resource);
DUALLITY_API DuallityStatus duallity_phonetic_rewrite_new(
    const DuallityPhoneticRuleV1* rules, size_t rule_count,
    uint8_t allow_identity, uint32_t cache_policy, uint64_t cache_capacity,
    VtResource* out_resource);
/* Built-in unconditional rules: locale 0=English, 1=German, 2=French. */
DUALLITY_API DuallityStatus duallity_phonetic_rewrite_builtin_new(
    uint32_t locale, uint8_t allow_identity,
    uint32_t cache_policy, uint64_t cache_capacity,
    VtResource* out_resource);

#ifdef __cplusplus
}
#endif

#endif /* DUALLITY_H */
