#include "sonalloy.h"

#include <cstddef>

namespace {

constexpr SonalloyProcessSpec kSpec{48'000.0, 256, 0, 2};
constexpr SonalloyProcessContext kContext{
    0, 120.0, 0.0, 0.0, 4, 4, SONALLOY_TRANSPORT_PLAYING};

int exercise_runtime_lifecycle() {
    const char definition[] = "{}";
    const SonalloyStringView definition_view{definition, sizeof(definition) - 1};
    const SonalloyStringView base_dir{".", 1};

    SonalloyCompiledInstrument* compiled = nullptr;
    SonalloyDiagnostics* diagnostics = nullptr;
    (void)sonalloy_compile_json(
        definition_view, base_dir, kSpec, &compiled, &diagnostics);
    if (diagnostics != nullptr) {
        sonalloy_diagnostics_destroy(diagnostics);
    }

    SonalloyRuntime* runtime = nullptr;
    (void)sonalloy_runtime_create(compiled, &runtime);
    (void)sonalloy_runtime_prepare(runtime, kSpec);
    (void)sonalloy_runtime_activate(runtime);
    (void)sonalloy_runtime_process(runtime, &kContext, nullptr, 0, nullptr, 0, nullptr, 0, 0);
    sonalloy_runtime_destroy(runtime);
    sonalloy_compiled_destroy(compiled);
    return 0;
}

} // namespace

int main() {
    if (exercise_runtime_lifecycle() != 0) {
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
