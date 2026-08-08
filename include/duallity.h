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
#define DUALLITY_API_REVISION 1u

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
DUALLITY_API void duallity_wfst_free(DuallityWfst* wfst);
/* On success, out_resource owns one retain. */
DUALLITY_API DuallityStatus duallity_wfst_resource(
    const DuallityWfst* wfst, VtResource* out_resource);
DUALLITY_API void duallity_resource_release(VtResource resource);

#ifdef __cplusplus
}
#endif

#endif /* DUALLITY_H */
