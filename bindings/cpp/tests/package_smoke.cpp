#include <duallity.hpp>

int main() {
    return duallity_abi_version() == DUALLITY_ABI_VERSION ? 0 : 1;
}
