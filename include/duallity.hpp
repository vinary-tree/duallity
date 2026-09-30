#ifndef DUALLITY_HPP
#define DUALLITY_HPP

#include "duallity.h"
#include <cstdint>
#include <cstddef>
#include <string>
#include <stdexcept>
#include <string_view>
#include <utility>
#include <vector>

namespace vinary_tree::duallity {

class error final : public std::runtime_error {
public:
    explicit error(DuallityStatus status)
        : std::runtime_error(duallity_last_error_message()), status_(status) {}
    [[nodiscard]] DuallityStatus status() const noexcept { return status_; }
private:
    DuallityStatus status_;
};

inline void check(DuallityStatus status) {
    if (status != DUALLITY_STATUS_OK) throw error(status);
}

class resource final {
public:
    explicit resource(VtResource value) noexcept : value_(value) {}
    resource(const resource&) = delete;
    resource& operator=(const resource&) = delete;
    resource(resource&& other) noexcept : value_(std::exchange(other.value_, {})) {}
    resource& operator=(resource&& other) noexcept {
        if (this != &other) {
            duallity_resource_release(value_);
            value_ = std::exchange(other.value_, {});
        }
        return *this;
    }
    ~resource() { duallity_resource_release(value_); }
    [[nodiscard]] VtResource get() const noexcept { return value_; }
private:
    VtResource value_{};
};

/* These values own their text; unlike the C readback pointers, they may
 * outlive both the WFST handle and every retained resource. */
struct restriction_snapshot {
    std::string source;
    std::string target;
};

struct operation_snapshot {
    std::uint64_t consume_x = 0;
    std::uint64_t consume_y = 0;
    double weight = 0;
    std::uint32_t applicability = 0;
    std::string name;
    std::vector<restriction_snapshot> restrictions;
};

struct configuration_snapshot {
    std::uint32_t kind = 0;
    std::uint32_t algorithm = 0;
    std::uint64_t maximum_distance = 0;
    std::uint32_t cache_policy = 0;
    std::uint64_t cache_capacity = 0;
    bool has_limits = false;
    DuallityGeneralizedLimitsV1 limits{};
    std::vector<operation_snapshot> operations;
};

inline DuallityWfstOptionsV1 default_options() {
    DuallityWfstOptionsV1 result{};
    result.header = {static_cast<std::uint32_t>(sizeof(result)),
                     DUALLITY_CONFIG_RECORD_VERSION, 0};
    check(duallity_wfst_options_default(&result));
    return result;
}

inline std::string copy_text(const std::uint8_t* data, std::uint64_t length) {
    if (length == 0) return {};
    return {reinterpret_cast<const char*>(data), static_cast<std::size_t>(length)};
}

class wfst final {
public:
    wfst(VtResource dictionary, std::string_view query, std::size_t maximum_distance,
         DuallityAlgorithm algorithm = DUALLITY_ALGORITHM_STANDARD,
         DuallityWfstKind kind = DUALLITY_WFST_LEVENSHTEIN) {
        check(duallity_wfst_new(dictionary,
            reinterpret_cast<const std::uint8_t*>(query.data()), query.size(),
            maximum_distance, algorithm, kind, &value_));
    }
    wfst(VtResource dictionary, std::string_view query,
         const DuallityWfstOptionsV1& options) {
        check(duallity_wfst_new_configured_ref(&dictionary,
            reinterpret_cast<const std::uint8_t*>(query.data()), query.size(),
            &options, &value_));
    }
    wfst(const wfst&) = delete;
    wfst& operator=(const wfst&) = delete;
    wfst(wfst&& other) noexcept : value_(std::exchange(other.value_, nullptr)) {}
    wfst& operator=(wfst&& other) noexcept {
        if (this != &other) {
            duallity_wfst_free(value_);
            value_ = std::exchange(other.value_, nullptr);
        }
        return *this;
    }
    ~wfst() { duallity_wfst_free(value_); }
    [[nodiscard]] resource retained_resource() const {
        VtResource result{};
        check(duallity_wfst_resource(value_, &result));
        return resource(result);
    }
    [[nodiscard]] configuration_snapshot options() const {
        DuallityWfstOptionsV1 raw{};
        raw.header = {static_cast<std::uint32_t>(sizeof(raw)),
                      DUALLITY_CONFIG_RECORD_VERSION, 0};
        check(duallity_wfst_options_get(value_, &raw));
        configuration_snapshot result;
        result.kind = raw.kind;
        result.algorithm = raw.algorithm;
        result.maximum_distance = raw.maximum_distance;
        result.cache_policy = raw.cache_policy;
        result.cache_capacity = raw.cache_capacity;
        if (raw.limits != nullptr) {
            result.has_limits = true;
            result.limits = *raw.limits;
        }
        result.operations.reserve(static_cast<std::size_t>(raw.operation_count));
        for (std::uint64_t i = 0; i < raw.operation_count; ++i) {
            const auto* bytes = reinterpret_cast<const std::uint8_t*>(raw.operations);
            const auto& item = *reinterpret_cast<const DuallityOperationV1*>(
                bytes + i * raw.operation_stride);
            operation_snapshot copied;
            copied.consume_x = item.consume_x;
            copied.consume_y = item.consume_y;
            copied.weight = item.weight;
            copied.applicability = item.applicability;
            copied.name = copy_text(item.name_data, item.name_len);
            copied.restrictions.reserve(static_cast<std::size_t>(item.restriction_count));
            for (std::uint64_t j = 0; j < item.restriction_count; ++j) {
                const auto* restriction_bytes =
                    reinterpret_cast<const std::uint8_t*>(item.restrictions);
                const auto& pair = *reinterpret_cast<const DuallityRestrictionV1*>(
                    restriction_bytes + j * item.restriction_stride);
                copied.restrictions.push_back({
                    copy_text(pair.source_data, pair.source_len),
                    copy_text(pair.target_data, pair.target_len)});
            }
            result.operations.push_back(std::move(copied));
        }
        return result;
    }
    [[nodiscard]] DuallityCacheStatisticsV1 cache_statistics() const {
        DuallityCacheStatisticsV1 result{};
        result.header = {static_cast<std::uint32_t>(sizeof(result)),
                         DUALLITY_CONFIG_RECORD_VERSION, 0};
        check(duallity_wfst_cache_statistics(value_, &result));
        return result;
    }
    void clear_cache() { check(duallity_wfst_cache_clear(value_)); }
    void set_cache_policy(DuallityCachePolicyV1 policy, std::uint64_t capacity = 0) {
        check(duallity_wfst_cache_set_policy(value_, policy, capacity));
    }
private:
    DuallityWfst* value_ = nullptr;
};

} // namespace vinary_tree::duallity

#endif /* DUALLITY_HPP */
