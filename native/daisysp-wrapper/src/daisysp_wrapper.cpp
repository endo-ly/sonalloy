#include "sonalloy_dsp.h"

#include "Synthesis/oscillator.h"

#include <cmath>
#include <cstdint>
#include <limits>
#include <new>
#include <stdexcept>

struct sonalloy_dsp_oscillator {
    daisysp::Oscillator oscillator;
    float sample_rate = 0.0f;
    bool prepared = false;
#ifdef SONALLOY_DSP_TEST_HOOKS
    bool throw_on_process = false;
#endif
};

namespace {

constexpr const char* kBackendVersion =
    "DaisySP V1.0.0 (a0494a3adb67f549e18dfd71a35fa656f65b38b6)";

bool valid_sample_rate(double sample_rate) {
    return std::isfinite(sample_rate) && sample_rate > 0.0 &&
           sample_rate <= static_cast<double>(std::numeric_limits<float>::max());
}

bool valid_frequency(float frequency_hz, float sample_rate) {
    return std::isfinite(frequency_hz) && frequency_hz >= 0.0f &&
           frequency_hz <= sample_rate * 0.5f;
}

}  // namespace

extern "C" const char* sonalloy_dsp_backend_version(void) {
    return kBackendVersion;
}

extern "C" uint32_t sonalloy_dsp_capabilities(void) {
    return SONALLOY_DSP_CAPABILITY_SINE | SONALLOY_DSP_CAPABILITY_SAW;
}

extern "C" sonalloy_dsp_oscillator* sonalloy_dsp_oscillator_create(void) {
    try {
        return new sonalloy_dsp_oscillator();
    } catch (...) {
        return nullptr;
    }
}

extern "C" void sonalloy_dsp_oscillator_destroy(sonalloy_dsp_oscillator* handle) {
    try {
        delete handle;
    } catch (...) {
    }
}

extern "C" int32_t sonalloy_dsp_oscillator_prepare(
    sonalloy_dsp_oscillator* handle,
    double sample_rate,
    int32_t waveform
) {
    if (handle == nullptr) {
        return SONALLOY_DSP_NULL_HANDLE;
    }
    handle->prepared = false;
    handle->sample_rate = 0.0f;
#ifdef SONALLOY_DSP_TEST_HOOKS
    handle->throw_on_process = false;
#endif
    if (!valid_sample_rate(sample_rate)) {
        return SONALLOY_DSP_INVALID_ARGUMENT;
    }
    if (waveform != SONALLOY_DSP_WAVEFORM_SINE &&
        waveform != SONALLOY_DSP_WAVEFORM_SAW) {
        return SONALLOY_DSP_UNSUPPORTED_WAVEFORM;
    }

    try {
        handle->sample_rate = static_cast<float>(sample_rate);
        handle->oscillator.Init(handle->sample_rate);
        handle->oscillator.SetAmp(1.0f);
        handle->oscillator.SetWaveform(
            waveform == SONALLOY_DSP_WAVEFORM_SINE
                ? daisysp::Oscillator::WAVE_SIN
                : daisysp::Oscillator::WAVE_POLYBLEP_SAW
        );
        handle->prepared = true;
        return SONALLOY_DSP_OK;
    } catch (...) {
        handle->prepared = false;
        return SONALLOY_DSP_NATIVE_EXCEPTION;
    }
}

extern "C" int32_t sonalloy_dsp_oscillator_reset(sonalloy_dsp_oscillator* handle) {
    if (handle == nullptr) {
        return SONALLOY_DSP_NULL_HANDLE;
    }
    if (!handle->prepared) {
        return SONALLOY_DSP_NOT_PREPARED;
    }

    try {
        handle->oscillator.Reset(0.0f);
        return SONALLOY_DSP_OK;
    } catch (...) {
        return SONALLOY_DSP_NATIVE_EXCEPTION;
    }
}

extern "C" int32_t sonalloy_dsp_oscillator_process(
    sonalloy_dsp_oscillator* handle,
    float frequency_hz,
    float* output,
    uint32_t frames
) {
    if (handle == nullptr) {
        return SONALLOY_DSP_NULL_HANDLE;
    }
    if (frames > 0u && output == nullptr) {
        return SONALLOY_DSP_INVALID_ARGUMENT;
    }
    if (!handle->prepared) {
        if (output != nullptr) {
            for (uint32_t index = 0; index < frames; ++index) {
                output[index] = 0.0f;
            }
        }
        return SONALLOY_DSP_NOT_PREPARED;
    }
    if (!valid_frequency(frequency_hz, handle->sample_rate)) {
        if (output != nullptr) {
            for (uint32_t index = 0; index < frames; ++index) {
                output[index] = 0.0f;
            }
        }
        return SONALLOY_DSP_INVALID_ARGUMENT;
    }

    try {
#ifdef SONALLOY_DSP_TEST_HOOKS
        if (handle->throw_on_process) {
            handle->throw_on_process = false;
            throw std::runtime_error("native process test exception");
        }
#endif
        handle->oscillator.SetFreq(frequency_hz);
        for (uint32_t index = 0; index < frames; ++index) {
            output[index] = handle->oscillator.Process();
        }
        return SONALLOY_DSP_OK;
    } catch (...) {
        if (output != nullptr) {
            for (uint32_t index = 0; index < frames; ++index) {
                output[index] = 0.0f;
            }
        }
        return SONALLOY_DSP_NATIVE_EXCEPTION;
    }
}

#ifdef SONALLOY_DSP_TEST_HOOKS
extern "C" void sonalloy_dsp_test_arm_process_exception(
    sonalloy_dsp_oscillator* handle
) {
    if (handle != nullptr) {
        handle->throw_on_process = true;
    }
}
#endif
