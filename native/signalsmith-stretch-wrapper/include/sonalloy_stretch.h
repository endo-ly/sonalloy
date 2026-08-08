#ifndef SONALLOY_STRETCH_H
#define SONALLOY_STRETCH_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct sonalloy_stretch sonalloy_stretch;

enum sonalloy_stretch_result {
    SONALLOY_STRETCH_OK = 0,
    SONALLOY_STRETCH_INVALID_ARGUMENT = 1,
    SONALLOY_STRETCH_NULL_HANDLE = 2,
    SONALLOY_STRETCH_NOT_PREPARED = 3,
    SONALLOY_STRETCH_NATIVE_EXCEPTION = 5,
    SONALLOY_STRETCH_NON_FINITE = 6
};

const char* sonalloy_stretch_backend_version(void);

sonalloy_stretch* sonalloy_stretch_create(void);
void sonalloy_stretch_destroy(sonalloy_stretch* handle);
int32_t sonalloy_stretch_prepare(
    sonalloy_stretch* handle,
    int32_t channels,
    double sample_rate,
    uint32_t max_input_frames,
    uint32_t max_output_frames
);
int32_t sonalloy_stretch_reset(sonalloy_stretch* handle);
int32_t sonalloy_stretch_set_pitch(sonalloy_stretch* handle, double semitones);
int32_t sonalloy_stretch_seek(
    sonalloy_stretch* handle,
    const float* const* input_buffers,
    uint32_t input_frames,
    double playback_rate
);
int32_t sonalloy_stretch_process(
    sonalloy_stretch* handle,
    const float* const* input_buffers,
    uint32_t input_frames,
    float* const* output_buffers,
    uint32_t output_frames
);
int32_t sonalloy_stretch_flush(
    sonalloy_stretch* handle,
    float* const* output_buffers,
    uint32_t output_frames
);
int32_t sonalloy_stretch_input_latency(const sonalloy_stretch* handle);
int32_t sonalloy_stretch_output_latency(const sonalloy_stretch* handle);
int32_t sonalloy_stretch_interval_samples(const sonalloy_stretch* handle);

#ifdef SONALLOY_DSP_TEST_HOOKS
void sonalloy_dsp_test_arm_stretch_process_exception(sonalloy_stretch* handle);
#endif

#ifdef __cplusplus
}
#endif

#endif
