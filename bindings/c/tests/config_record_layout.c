#include "duallity.h"
#include <stddef.h>

/* Compile on each target ABI; do not freeze a 64-bit pointer layout. */
_Static_assert(sizeof(DuallityRecordHeaderV1) == 16,
               "record header must have the stable 16-byte prefix");
_Static_assert(offsetof(DuallityRestrictionV1, header) == 0,
               "restriction header must be first");
_Static_assert(offsetof(DuallityOperationV1, header) == 0,
               "operation header must be first");
_Static_assert(offsetof(DuallityGeneralizedLimitsV1, header) == 0,
               "limits header must be first");
_Static_assert(offsetof(DuallityWfstOptionsV1, header) == 0,
               "options header must be first");
_Static_assert(offsetof(DuallityCacheStatisticsV1, header) == 0,
               "statistics header must be first");
_Static_assert(sizeof(((DuallityWfstOptionsV1*)0)->operation_count) == 8,
               "counts use fixed-width wire integers");
_Static_assert(sizeof(((DuallityCacheStatisticsV1*)0)->hits) == 8,
               "statistics use fixed-width wire integers");
_Static_assert(offsetof(DuallityFzfConfigV1, header) == 0,
               "FZF config header must be first");
_Static_assert(offsetof(DuallityFzfScoreV1, header) == 0,
               "FZF score header must be first");
_Static_assert(offsetof(DuallityFzfStatisticsV1, header) == 0,
               "FZF statistics header must be first");
_Static_assert(offsetof(DuallityFzfHitV1, header) == 0,
               "FZF hit header must be first");
_Static_assert(sizeof(((DuallityFzfConfigV1*)0)->max_work_units) == 8,
               "FZF work bounds use fixed-width wire integers");
_Static_assert(sizeof(((DuallityFzfHitV1*)0)->score) == 4,
               "FZF scores use signed 32-bit wire integers");

int main(void) {
    DuallityWfstOptionsV1 options = {0};
    options.header.struct_size = (uint32_t)sizeof options;
    options.header.record_version = 1;
    options.cache_policy = DUALLITY_CACHE_ALL;
    return options.header.reserved != 0;
}
