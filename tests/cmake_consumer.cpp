#include "sonalloy.h"

#include <stddef.h>
#include <string.h>

namespace {

const SonalloyProcessSpec kSpec = {48000.0, 256, 0, 2};

const SonalloyProcessContext kContext = {
    0, 120.0, 0.0, 0.0, 4, 4, SONALLOY_TRANSPORT_PLAYING};

const char kDefinition[] =
    "{\"schema_version\":6,"
    "\"metadata\":{\"name\":\"CMake Consumer\"},"
    "\"performance\":{\"mode\":\"polyphonic\",\"polyphony\":1,"
    "\"voice_stealing\":\"quietest_releasing_then_oldest\"},"
    "\"layers\":[{"
    "\"id\":\"body\",\"enabled\":true,"
    "\"trigger\":{\"event\":\"note_on\",\"key_min\":0,\"key_max\":127,"
    "\"velocity_min\":1,\"velocity_max\":127},"
    "\"gain_db\":-6.0,\"pan\":0.0,\"tuning_cents\":0.0,"
    "\"envelope\":{\"attack_seconds\":0.0,\"decay_seconds\":0.0,"
    "\"sustain_level\":1.0,\"release_seconds\":0.01},"
    "\"generator\":{\"oscillator\":{\"waveform\":{\"type\":\"sine\"},"
    "\"phase_reset\":true,\"phase\":0.0}}}],"
    "\"voice_processors\":[],\"global_processors\":[]}";

SonalloyStringView as_view(const char* text) {
    SonalloyStringView view;
    view.data = text;
    view.length = (size_t)strlen(text);
    return view;
}

int compile_definition(SonalloyCompiledInstrument** compiled) {
    const SonalloyStringView base_dir = {".", 1};
    SonalloyDiagnostics* diagnostics = 0;
    const SonalloyResult result = sonalloy_compile_json(
        as_view(kDefinition), base_dir, kSpec, compiled, &diagnostics);
    if (diagnostics != 0) {
        sonalloy_diagnostics_destroy(diagnostics);
    }
    return result == SONALLOY_OK ? 0 : 1;
}

int run_runtime(SonalloyCompiledInstrument* compiled) {
    SonalloyRuntime* runtime = 0;
    if (sonalloy_runtime_create(compiled, &runtime) != SONALLOY_OK) {
        return 1;
    }
    if (sonalloy_runtime_prepare(runtime, kSpec) != SONALLOY_OK) {
        sonalloy_runtime_destroy(runtime);
        return 1;
    }
    if (sonalloy_runtime_activate(runtime) != SONALLOY_OK) {
        sonalloy_runtime_destroy(runtime);
        return 1;
    }

    float left[256];
    float right[256];
    float* channels[2];
    channels[0] = left;
    channels[1] = right;
    SonalloyEvent event;
    memset(&event, 0, sizeof(event));
    event.event_type = SONALLOY_EVENT_NOTE_ON;
    event.note_id = 1;
    event.note_number = 60;
    event.velocity = 100;
    const SonalloyResult processed = sonalloy_runtime_process(
        runtime, &kContext, &event, 1, 0, 0, channels, 2, 256);
    if (processed != SONALLOY_OK) {
        sonalloy_runtime_destroy(runtime);
        return 1;
    }

    sonalloy_runtime_destroy(runtime);
    return 0;
}

} // namespace

int main() {
    SonalloyCompiledInstrument* compiled = 0;
    if (compile_definition(&compiled) != 0) {
        return 1;
    }
    const int lifecycle = run_runtime(compiled);
    sonalloy_compiled_destroy(compiled);
    if (lifecycle != 0) {
        return 1;
    }

    uint8_t supported = 0;
    if (sonalloy_has_capability(
            SONALLOY_CAPABILITY_REALTIME_RUNTIME_UPDATE,
            &supported) != SONALLOY_OK) {
        return 1;
    }
    return supported == 1 ? 0 : 1;
}
