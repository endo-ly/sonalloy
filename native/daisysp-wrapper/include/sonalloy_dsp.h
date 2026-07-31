#ifndef SONALLOY_DSP_H
#define SONALLOY_DSP_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct sonalloy_dsp_oscillator sonalloy_dsp_oscillator;

enum sonalloy_dsp_result {
    SONALLOY_DSP_OK = 0,
    SONALLOY_DSP_INVALID_ARGUMENT = 1,
    SONALLOY_DSP_NULL_HANDLE = 2,
    SONALLOY_DSP_NOT_PREPARED = 3,
    SONALLOY_DSP_UNSUPPORTED_WAVEFORM = 4,
    SONALLOY_DSP_NATIVE_EXCEPTION = 5
};

enum sonalloy_dsp_waveform {
    SONALLOY_DSP_WAVEFORM_SINE = 0,
    SONALLOY_DSP_WAVEFORM_SAW = 1
};

enum sonalloy_dsp_capability {
    SONALLOY_DSP_CAPABILITY_SINE = 1u << 0,
    SONALLOY_DSP_CAPABILITY_SAW = 1u << 1
};

const char* sonalloy_dsp_backend_version(void);
uint32_t sonalloy_dsp_capabilities(void);

sonalloy_dsp_oscillator* sonalloy_dsp_oscillator_create(void);
void sonalloy_dsp_oscillator_destroy(sonalloy_dsp_oscillator* handle);
int32_t sonalloy_dsp_oscillator_prepare(
    sonalloy_dsp_oscillator* handle,
    double sample_rate,
    int32_t waveform
);
int32_t sonalloy_dsp_oscillator_reset(sonalloy_dsp_oscillator* handle);
int32_t sonalloy_dsp_oscillator_process(
    sonalloy_dsp_oscillator* handle,
    float frequency_hz,
    float* output,
    uint32_t frames
);

#ifdef __cplusplus
}
#endif

#endif
