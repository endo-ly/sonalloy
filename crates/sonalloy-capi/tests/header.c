#include "sonalloy.h"

int main(void) {
    SonalloyProcessSpec spec = {48000.0, 256, 0, 2};
    SonalloyProcessContext context = {
        0, 120.0, 0.0, 0.0, 4, 4, SONALLOY_TRANSPORT_PLAYING
    };
    (void)spec;
    (void)context;
    return (int)sonalloy_c_api_version();
}
