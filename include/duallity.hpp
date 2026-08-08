#ifndef DUALLITY_HPP
#define DUALLITY_HPP

#include "duallity.h"
#include <cstdint>
#include <stdexcept>
#include <string_view>
#include <utility>

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
    ~resource() { duallity_resource_release(value_); }
    [[nodiscard]] VtResource get() const noexcept { return value_; }
private:
    VtResource value_{};
};

class wfst final {
public:
    wfst(VtResource dictionary, std::string_view query, std::size_t maximum_distance,
         DuallityAlgorithm algorithm = DUALLITY_ALGORITHM_STANDARD,
         DuallityWfstKind kind = DUALLITY_WFST_LEVENSHTEIN) {
        check(duallity_wfst_new(dictionary,
            reinterpret_cast<const std::uint8_t*>(query.data()), query.size(),
            maximum_distance, algorithm, kind, &value_));
    }
    wfst(const wfst&) = delete;
    wfst& operator=(const wfst&) = delete;
    wfst(wfst&& other) noexcept : value_(std::exchange(other.value_, nullptr)) {}
    ~wfst() { duallity_wfst_free(value_); }
    [[nodiscard]] resource retained_resource() const {
        VtResource result{};
        check(duallity_wfst_resource(value_, &result));
        return resource(result);
    }
private:
    DuallityWfst* value_ = nullptr;
};

} // namespace vinary_tree::duallity

#endif /* DUALLITY_HPP */
