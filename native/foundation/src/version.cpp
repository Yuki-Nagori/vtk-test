#include <panta/foundation/version.hpp>

namespace panta::foundation {

Version native_version() noexcept {
    return {PANTA_NATIVE_VERSION_MAJOR, PANTA_NATIVE_VERSION_MINOR, PANTA_NATIVE_VERSION_PATCH};
}

} // namespace panta::foundation
