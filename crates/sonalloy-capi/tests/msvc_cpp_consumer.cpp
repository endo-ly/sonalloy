#include <vector>

#include "sonalloy.h"

int main() {
    std::vector<int> marker = {1};
    if (marker.front() != 1) {
        return 1;
    }

    SonalloyProcessSpec spec = {48000.0, 256, 0, 2};
    SonalloyProcessContext context = {
        0, 120.0, 0.0, 0.0, 4, 4, SONALLOY_TRANSPORT_PLAYING
    };

    SonalloyCompiledInstrument* compiled = nullptr;
    SonalloyDiagnostics* diagnostics = nullptr;
    const char definition[] = "{}";
    SonalloyStringView definition_view = {definition, sizeof(definition) - 1};
    SonalloyStringView base_dir = {".", 1};
    (void)sonalloy_compile_json(
        definition_view, base_dir, spec, &compiled, &diagnostics);

    SonalloyRuntime* runtime = nullptr;
    (void)sonalloy_runtime_create(compiled, &runtime);
    (void)sonalloy_runtime_prepare(runtime, spec);
    (void)sonalloy_runtime_activate(runtime);
    (void)sonalloy_runtime_process(
        runtime, &context, nullptr, 0, nullptr, 0, nullptr, 0, 0);
    sonalloy_runtime_destroy(runtime);
    sonalloy_compiled_destroy(compiled);
    sonalloy_diagnostics_destroy(diagnostics);

    return sonalloy_c_api_version() == 1 ? 0 : 1;
}
