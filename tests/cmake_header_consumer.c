#include "sonalloy.h"

int main(void) {
    SonalloyProcessSpec spec;
    SonalloyProcessContext context;
    SonalloyStringView definition;
    uint8_t supported = 0;

    spec.sample_rate = 48000.0;
    spec.max_block_size = 256;
    spec.input_channels = 0;
    spec.output_channels = 2;

    context.absolute_frame = 0;
    context.tempo_bpm = 120.0;
    context.beat_position = 0.0;
    context.bar_position = 0.0;
    context.time_signature_numerator = 4;
    context.time_signature_denominator = 4;
    context.transport_state = SONALLOY_TRANSPORT_PLAYING;

    definition.data = "{}";
    definition.length = 2;

    (void)spec;
    (void)context;
    (void)definition;
    if (sonalloy_has_capability(
            SONALLOY_CAPABILITY_REALTIME_RUNTIME_UPDATE,
            &supported) != SONALLOY_OK) {
        return 1;
    }
    return supported == 1 ? 0 : 1;
}
