#ifndef SONALLOY_H
#define SONALLOY_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct SonalloyCompiledInstrument SonalloyCompiledInstrument;
typedef struct SonalloyRuntime SonalloyRuntime;
typedef struct SonalloyPreparedUpdate SonalloyPreparedUpdate;
typedef struct SonalloyDiagnostics SonalloyDiagnostics;
typedef struct SonalloyReclaimable SonalloyReclaimable;

typedef enum SonalloyResult {
    SONALLOY_OK = 0,
    SONALLOY_INVALID_ARGUMENT = 1,
    SONALLOY_INVALID_STATE = 2,
    SONALLOY_COMPILE_FAILED = 3,
    SONALLOY_PREPARE_FAILED = 4,
    SONALLOY_PROCESS_FAILED = 5,
    SONALLOY_UPDATE_INCOMPATIBLE = 6,
    SONALLOY_UPDATE_CAPACITY_EXCEEDED = 7,
    SONALLOY_TRANSITION_BUSY = 8,
    SONALLOY_INTERNAL_PANIC = 255
} SonalloyResult;

typedef struct SonalloyStringView {
    const char* data;
    size_t length;
} SonalloyStringView;

typedef struct SonalloyProcessSpec {
    double sample_rate;
    uint32_t max_block_size;
    uint32_t input_channels;
    uint32_t output_channels;
} SonalloyProcessSpec;

typedef enum SonalloyTransportState {
    SONALLOY_TRANSPORT_STOPPED = 0,
    SONALLOY_TRANSPORT_PLAYING = 1
} SonalloyTransportState;

typedef struct SonalloyProcessContext {
    uint64_t absolute_frame;
    double tempo_bpm;
    double beat_position;
    double bar_position;
    uint16_t time_signature_numerator;
    uint16_t time_signature_denominator;
    uint32_t transport_state;
} SonalloyProcessContext;

typedef enum SonalloyEventType {
    SONALLOY_EVENT_NOTE_ON = 1,
    SONALLOY_EVENT_NOTE_OFF = 2,
    SONALLOY_EVENT_SUSTAIN = 3,
    SONALLOY_EVENT_PARAMETER_CHANGE = 4,
    SONALLOY_EVENT_PITCH_BEND = 5,
    SONALLOY_EVENT_MOD_WHEEL = 6,
    SONALLOY_EVENT_AFTERTOUCH = 7
} SonalloyEventType;

typedef struct SonalloyEvent {
    uint32_t sample_offset;
    uint32_t event_type;
    uint64_t note_id;
    uint64_t parameter_catalog_revision;
    uint32_t parameter_handle;
    uint8_t note_number;
    uint8_t velocity;
    uint8_t bool_value;
    uint8_t reserved;
    float value;
} SonalloyEvent;

typedef struct SonalloyParameterDescriptor {
    SonalloyStringView id;
    uint32_t owner_kind;
    uint32_t owner_index;
    uint32_t owner_sub_index;
    uint32_t owner_axis;
    uint32_t unit;
    uint32_t scale;
    float min;
    float max;
    float default_value;
    float smoothing_seconds;
} SonalloyParameterDescriptor;

typedef struct SonalloyPublishOutcome {
    uint64_t generation_id;
    uint64_t parameter_catalog_revision;
    uint32_t reported_latency_frames;
    uint32_t required_input_channels;
} SonalloyPublishOutcome;

typedef struct SonalloyRuntimeErrorInfo {
    uint32_t code;
    uint32_t detail_kind;
    uint64_t value_a;
    uint64_t value_b;
} SonalloyRuntimeErrorInfo;

typedef struct SonalloyDiagnosticView {
    uint32_t code;
    uint32_t severity;
    SonalloyStringView path;
    SonalloyStringView message;
    SonalloyStringView detail;
} SonalloyDiagnosticView;

enum {
    SONALLOY_CAPABILITY_REALTIME_RUNTIME_UPDATE = 1,
    SONALLOY_CAPABILITY_EXTERNAL_AUDIO_INPUT = 2,
    SONALLOY_CAPABILITY_TRANSPORT_CONTEXT = 3,
    SONALLOY_CAPABILITY_PARAMETER_CATALOG_REVISION = 4,
    SONALLOY_CAPABILITY_NOTE_EXPRESSION = 5,
    SONALLOY_CAPABILITY_STATE_SERIALIZATION = 6,
    SONALLOY_CAPABILITY_NEURAL_BACKEND = 7
};

uint32_t sonalloy_c_api_version(void);
SonalloyResult sonalloy_has_capability(uint32_t capability, uint8_t* out_supported);

SonalloyResult sonalloy_compile_json(
    SonalloyStringView definition_json,
    SonalloyStringView definition_base_dir,
    SonalloyProcessSpec process_spec,
    SonalloyCompiledInstrument** out_compiled,
    SonalloyDiagnostics** out_diagnostics);

uint32_t sonalloy_compiled_reported_latency_frames(
    const SonalloyCompiledInstrument* compiled);
uint32_t sonalloy_compiled_required_input_channels(
    const SonalloyCompiledInstrument* compiled);
uint64_t sonalloy_compiled_parameter_catalog_revision(
    const SonalloyCompiledInstrument* compiled);
uint32_t sonalloy_compiled_parameter_count(
    const SonalloyCompiledInstrument* compiled);
SonalloyResult sonalloy_compiled_parameter_descriptor(
    const SonalloyCompiledInstrument* compiled,
    uint32_t index,
    SonalloyParameterDescriptor* out_descriptor);
SonalloyResult sonalloy_compiled_parameter_handle(
    const SonalloyCompiledInstrument* compiled,
    SonalloyStringView parameter_id,
    uint32_t* out_handle);
SonalloyResult sonalloy_compiled_parameter_normalize(
    const SonalloyCompiledInstrument* compiled,
    uint32_t handle,
    float native_value,
    float* out_normalized);
SonalloyResult sonalloy_compiled_parameter_denormalize(
    const SonalloyCompiledInstrument* compiled,
    uint32_t handle,
    float normalized,
    float* out_native_value);
void sonalloy_compiled_destroy(SonalloyCompiledInstrument* compiled);

uint32_t sonalloy_diagnostics_count(const SonalloyDiagnostics* diagnostics);
SonalloyResult sonalloy_diagnostics_get(
    const SonalloyDiagnostics* diagnostics,
    uint32_t index,
    SonalloyDiagnosticView* out_diagnostic);
void sonalloy_diagnostics_destroy(SonalloyDiagnostics* diagnostics);

SonalloyResult sonalloy_runtime_create(
    const SonalloyCompiledInstrument* compiled,
    SonalloyRuntime** out_runtime);
SonalloyResult sonalloy_runtime_prepare(
    SonalloyRuntime* runtime,
    SonalloyProcessSpec spec);
SonalloyResult sonalloy_runtime_activate(SonalloyRuntime* runtime);
SonalloyResult sonalloy_runtime_reset(SonalloyRuntime* runtime);
SonalloyResult sonalloy_runtime_deactivate(SonalloyRuntime* runtime);
SonalloyResult sonalloy_runtime_process(
    SonalloyRuntime* runtime,
    const SonalloyProcessContext* context,
    const SonalloyEvent* events,
    uint32_t event_count,
    const float* const* input_channels,
    uint32_t input_channel_count,
    float* const* output_channels,
    uint32_t output_channel_count,
    uint32_t frames);
SonalloyResult sonalloy_update_prepare(
    const SonalloyCompiledInstrument* compiled,
    SonalloyProcessSpec spec,
    SonalloyPreparedUpdate** out_update);
SonalloyResult sonalloy_runtime_publish(
    SonalloyRuntime* runtime,
    SonalloyPreparedUpdate* update,
    SonalloyPublishOutcome* out_outcome);
SonalloyResult sonalloy_runtime_take_reclaimable(
    SonalloyRuntime* runtime,
    SonalloyReclaimable** out_reclaimable);
void sonalloy_reclaimable_destroy(SonalloyReclaimable* reclaimable);
uint32_t sonalloy_runtime_state(const SonalloyRuntime* runtime);
uint64_t sonalloy_runtime_generation_id(const SonalloyRuntime* runtime);
uint64_t sonalloy_runtime_stale_parameter_event_count(
    const SonalloyRuntime* runtime);
SonalloyResult sonalloy_runtime_last_error(
    const SonalloyRuntime* runtime,
    SonalloyRuntimeErrorInfo* out_error);
void sonalloy_runtime_destroy(SonalloyRuntime* runtime);
void sonalloy_update_destroy(SonalloyPreparedUpdate* update);

#ifdef __cplusplus
}
#endif

#endif /* SONALLOY_H */
