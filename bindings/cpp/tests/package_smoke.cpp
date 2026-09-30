#include <duallity.hpp>
#include <cassert>
#include <type_traits>

int main() {
    using namespace vinary_tree::duallity;
    static_assert(!std::is_copy_constructible_v<wfst>);
    static_assert(std::is_move_constructible_v<wfst>);
    if (duallity_abi_version() != DUALLITY_ABI_VERSION ||
        duallity_api_revision() < DUALLITY_CONFIG_API_REVISION) return 1;

    const auto defaults = default_options();
    assert(defaults.header.struct_size == sizeof(defaults));
    assert(defaults.header.record_version == DUALLITY_CONFIG_RECORD_VERSION);
    assert(defaults.maximum_distance == 2);
    assert(defaults.cache_policy == DUALLITY_CACHE_ALL);

    /* Both old and new constructors remain callable from installed headers.
     * A null dictionary must report a typed failure, never produce a handle. */
    VtResource missing{};
    try {
        wfst old(missing, "cat", 1);
        return 2;
    } catch (const error& failure) {
        assert(failure.status() != DUALLITY_STATUS_OK);
    }
    try {
        wfst configured(missing, "cat", defaults);
        return 3;
    } catch (const error& failure) {
        assert(failure.status() != DUALLITY_STATUS_OK);
    }
    auto malformed = defaults;
    malformed.header.reserved = 1;
    try {
        wfst invalid(missing, "cat", malformed);
        return 4;
    } catch (const error& failure) {
        assert(failure.status() == DUALLITY_STATUS_INVALID_ARGUMENT);
    }
    return 0;
}
