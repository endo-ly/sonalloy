#include "sonalloy.h"

int main(void) {
    SonalloyProcessSpec spec = {48000.0, 256, 0, 2};
    SonalloyProcessContext context = {
        0, 120.0, 0.0, 0.0, 4, 4, SONALLOY_TRANSPORT_PLAYING
    };
    SonalloyEvent event;
    uint8_t supported = 0;
    (void)spec;
    (void)context;
    event.event_type = SONALLOY_EVENT_PARAMETER_RAMP;
    (void)event;
    if (sonalloy_has_capability(
            SONALLOY_CAPABILITY_REALTIME_RUNTIME_UPDATE,
            &supported) != SONALLOY_OK) {
        return 1;
    }
    return 0;
}
