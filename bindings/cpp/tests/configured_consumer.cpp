#include <cassert>
#include <cstdint>
#include <cstring>
#include <utility>

#include "duallity.hpp"
#include "libdictenstein.h"

int main() {
    using namespace vinary_tree::duallity;
    assert(duallity_api_revision() >= DUALLITY_CONFIG_API_REVISION);

    LdictDictionary* dictionary = nullptr;
    assert(ldict_dynamic_dawg_new(VT_UNIT_DOMAIN_UNICODE_SCALAR, &dictionary) ==
           LDICT_STATUS_OK);
    const auto* term = reinterpret_cast<const std::uint8_t*>("cat");
    LdictOptionalU64 no_value{};
    std::uint8_t inserted = 0;
    assert(ldict_dictionary_insert_text(dictionary, term, 3, no_value, &inserted) ==
           LDICT_STATUS_OK);
    assert(inserted == 1);
    VtResource borrowed{};
    assert(ldict_dictionary_resource(dictionary, &borrowed) == LDICT_STATUS_OK);

    resource surviving(VtResource{});
    {
        wfst old(borrowed, "cat", 1);
        assert(old.options().kind == DUALLITY_WFST_LEVENSHTEIN);
        surviving = old.retained_resource();
    }
    assert(surviving.get().context != nullptr);

    auto options = default_options();
    options.kind = DUALLITY_WFST_GENERALIZED_STANDARD;
    options.maximum_distance = 1;
    options.cache_policy = DUALLITY_LRU;
    options.cache_capacity = 2;
    std::uint8_t name[] = {'c', 'u', 's', 't', 'o', 'm'};
    DuallityOperationV1 operation{};
    operation.header = {static_cast<std::uint32_t>(sizeof(operation)), 1, 0};
    operation.consume_x = 1;
    operation.consume_y = 1;
    operation.weight = 1.0;
    operation.applicability = DUALLITY_APPLICABILITY_ANY;
    operation.name_data = name;
    operation.name_len = sizeof(name);
    options.operations = &operation;
    options.operation_count = 1;
    options.operation_stride = sizeof(operation);

    configuration_snapshot copied;
    {
        wfst configured(borrowed, "cat", options);
        name[0] = 'X';
        copied = configured.options();
        assert(copied.kind == DUALLITY_WFST_GENERALIZED_STANDARD);
        assert(copied.cache_policy == DUALLITY_LRU);
        assert(copied.cache_capacity == 2);
        assert(copied.operations.size() == 1);
        assert(copied.operations[0].name == "custom");
        configured.clear_cache();
        assert(configured.cache_statistics().clears >= 1);
        configured.set_cache_policy(DUALLITY_NO_CACHE);
        assert(configured.options().cache_policy == DUALLITY_NO_CACHE);
        surviving = configured.retained_resource();
    }
    assert(copied.operations[0].name == "custom");
    auto malformed = default_options();
    malformed.header.reserved = 1;
    try {
        wfst invalid(borrowed, "cat", malformed);
        return 1;
    } catch (const error& failure) {
        assert(failure.status() == DUALLITY_STATUS_INVALID_ARGUMENT);
    }
    ldict_dictionary_free(dictionary);
    assert(surviving.get().context != nullptr);
    return 0;
}
