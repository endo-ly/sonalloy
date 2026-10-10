#include "sonalloy.h"

int main() {
    uint8_t supported = 0;
    if (sonalloy_has_capability(
            SONALLOY_CAPABILITY_REALTIME_RUNTIME_UPDATE,
            &supported) != SONALLOY_OK) {
        return 1;
    }
    return supported == 1 ? 0 : 1;
}
