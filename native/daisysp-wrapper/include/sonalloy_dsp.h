#ifndef SONALLOY_DSP_H
#define SONALLOY_DSP_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct sonalloy_dsp_oscillator sonalloy_dsp_oscillator;
typedef struct sonalloy_dsp_variable_oscillator sonalloy_dsp_variable_oscillator;
typedef struct sonalloy_dsp_filter sonalloy_dsp_filter;
typedef struct sonalloy_dsp_wavefolder sonalloy_dsp_wavefolder;
typedef struct sonalloy_dsp_modal_resonator sonalloy_dsp_modal_resonator;

enum sonalloy_dsp_result {
    SONALLOY_DSP_OK = 0,
    SONALLOY_DSP_INVALID_ARGUMENT = 1,
    SONALLOY_DSP_NULL_HANDLE = 2,
    SONALLOY_DSP_NOT_PREPARED = 3,
    SONALLOY_DSP_UNSUPPORTED_WAVEFORM = 4,
    SONALLOY_DSP_NATIVE_EXCEPTION = 5,
    SONALLOY_DSP_NON_FINITE = 6
};

enum sonalloy_dsp_waveform {
    SONALLOY_DSP_WAVEFORM_SINE = 0,
    SONALLOY_DSP_WAVEFORM_SAW = 1,
    SONALLOY_DSP_WAVEFORM_TRIANGLE = 2,
    SONALLOY_DSP_WAVEFORM_SQUARE = 3,
    SONALLOY_DSP_WAVEFORM_PULSE = 4
};

enum sonalloy_dsp_filter_mode {
    SONALLOY_DSP_FILTER_LOW_PASS = 0,
    SONALLOY_DSP_FILTER_HIGH_PASS = 1,
    SONALLOY_DSP_FILTER_BAND_PASS = 2,
    SONALLOY_DSP_FILTER_NOTCH = 3
};

enum sonalloy_dsp_capability {
    SONALLOY_DSP_CAPABILITY_SINE = 1u << 0,
    SONALLOY_DSP_CAPABILITY_SAW = 1u << 1,
    SONALLOY_DSP_CAPABILITY_TRIANGLE = 1u << 2,
    SONALLOY_DSP_CAPABILITY_SQUARE = 1u << 3,
    SONALLOY_DSP_CAPABILITY_PULSE = 1u << 4
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
int32_t sonalloy_dsp_oscillator_reset_phase(
    sonalloy_dsp_oscillator* handle,
    float phase
);
int32_t sonalloy_dsp_oscillator_process(
    sonalloy_dsp_oscillator* handle,
    float frequency_hz,
    float* output,
    uint32_t frames
);
int32_t sonalloy_dsp_oscillator_process_with_pulse_width(
    sonalloy_dsp_oscillator* handle,
    float frequency_hz,
    float pulse_width,
    float* output,
    uint32_t frames
);
int32_t sonalloy_dsp_oscillator_process_ramp(
    sonalloy_dsp_oscillator* handle,
    float start_frequency_hz,
    float end_frequency_hz,
    float* output,
    uint32_t frames
);
int32_t sonalloy_dsp_oscillator_process_ramp_with_pulse_width(
    sonalloy_dsp_oscillator* handle,
    float start_frequency_hz,
    float end_frequency_hz,
    float start_pulse_width,
    float end_pulse_width,
    float* output,
    uint32_t frames
);

sonalloy_dsp_variable_oscillator* sonalloy_dsp_variable_oscillator_create(void);
void sonalloy_dsp_variable_oscillator_destroy(sonalloy_dsp_variable_oscillator* handle);
int32_t sonalloy_dsp_variable_oscillator_prepare(
    sonalloy_dsp_variable_oscillator* handle,
    double sample_rate,
    int32_t waveform
);
int32_t sonalloy_dsp_variable_oscillator_reset(
    sonalloy_dsp_variable_oscillator* handle
);
int32_t sonalloy_dsp_variable_oscillator_process(
    sonalloy_dsp_variable_oscillator* handle,
    float master_frequency_hz,
    float slave_frequency_hz,
    float pulse_width,
    float* output,
    uint32_t frames
);
int32_t sonalloy_dsp_variable_oscillator_process_ramp(
    sonalloy_dsp_variable_oscillator* handle,
    float start_master_frequency_hz,
    float end_master_frequency_hz,
    float start_slave_frequency_hz,
    float end_slave_frequency_hz,
    float start_pulse_width,
    float end_pulse_width,
    float* output,
    uint32_t frames
);

sonalloy_dsp_filter* sonalloy_dsp_filter_create(void);
void sonalloy_dsp_filter_destroy(sonalloy_dsp_filter* handle);
int32_t sonalloy_dsp_filter_prepare(
    sonalloy_dsp_filter* handle,
    double sample_rate
);
int32_t sonalloy_dsp_filter_reset(sonalloy_dsp_filter* handle);
int32_t sonalloy_dsp_filter_process(
    sonalloy_dsp_filter* handle,
    int32_t mode,
    float cutoff_hz,
    float resonance,
    float* buffer,
    uint32_t frames
);
int32_t sonalloy_dsp_filter_process_ramp(
    sonalloy_dsp_filter* handle,
    int32_t mode,
    float start_cutoff_hz,
    float end_cutoff_hz,
    float resonance,
    float* buffer,
    uint32_t frames
);
int32_t sonalloy_dsp_filter_process_ramp_with_resonance(
    sonalloy_dsp_filter* handle,
    int32_t mode,
    float start_cutoff_hz,
    float end_cutoff_hz,
    float start_resonance,
    float end_resonance,
    float* buffer,
    uint32_t frames
);

sonalloy_dsp_wavefolder* sonalloy_dsp_wavefolder_create(void);
void sonalloy_dsp_wavefolder_destroy(sonalloy_dsp_wavefolder* handle);
int32_t sonalloy_dsp_wavefolder_prepare(
    sonalloy_dsp_wavefolder* handle,
    double sample_rate
);
int32_t sonalloy_dsp_wavefolder_reset(sonalloy_dsp_wavefolder* handle);
int32_t sonalloy_dsp_wavefolder_process(
    sonalloy_dsp_wavefolder* handle,
    float drive,
    float mix,
    float* buffer,
    uint32_t frames
);
int32_t sonalloy_dsp_wavefolder_process_ramp(
    sonalloy_dsp_wavefolder* handle,
    float start_drive,
    float end_drive,
    float start_mix,
    float end_mix,
    float* buffer,
    uint32_t frames
);

sonalloy_dsp_modal_resonator* sonalloy_dsp_modal_resonator_create(void);
void sonalloy_dsp_modal_resonator_destroy(sonalloy_dsp_modal_resonator* handle);
int32_t sonalloy_dsp_modal_resonator_prepare(
    sonalloy_dsp_modal_resonator* handle,
    double sample_rate,
    int32_t mode_count
);
int32_t sonalloy_dsp_modal_resonator_reset(sonalloy_dsp_modal_resonator* handle);
int32_t sonalloy_dsp_modal_resonator_process_ramp(
    sonalloy_dsp_modal_resonator* handle,
    float start_frequency_hz,
    float end_frequency_hz,
    float start_structure,
    float end_structure,
    float start_brightness,
    float end_brightness,
    float start_decay,
    float end_decay,
    float* buffer,
    uint32_t frames
);

#ifdef SONALLOY_DSP_TEST_HOOKS
void sonalloy_dsp_test_arm_modal_process_exception(
    sonalloy_dsp_modal_resonator* handle
);
#endif

#ifdef __cplusplus
}
#endif

#endif
