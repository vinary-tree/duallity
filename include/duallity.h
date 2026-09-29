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
#define DUALLITY_API_REVISION 3u

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

typedef struct DuallityWfst DuallityWfst;

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
DUALLITY_API void duallity_resource_release(VtResource resource);

#ifdef __cplusplus
}
#endif

#endif /* DUALLITY_H */
