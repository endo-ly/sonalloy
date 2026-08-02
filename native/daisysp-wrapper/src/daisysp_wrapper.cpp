#include "sonalloy_dsp.h"

#include "Synthesis/oscillator.h"
#include "Filters/svf.h"

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

struct sonalloy_dsp_filter {
    daisysp::Svf filter;
    float sample_rate = 0.0f;
    bool prepared = false;
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

bool valid_cutoff(float cutoff_hz, float sample_rate) {
    return std::isfinite(cutoff_hz) && cutoff_hz > 0.0f &&
           cutoff_hz <= sample_rate * 0.45f;
}

bool valid_resonance(float resonance) {
    return std::isfinite(resonance) && resonance >= 0.0f && resonance <= 1.0f;
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

extern "C" int32_t sonalloy_dsp_oscillator_process_ramp(
    sonalloy_dsp_oscillator* handle,
    float start_frequency_hz,
    float end_frequency_hz,
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
    if (!valid_frequency(start_frequency_hz, handle->sample_rate) ||
        !valid_frequency(end_frequency_hz, handle->sample_rate)) {
        if (output != nullptr) {
            for (uint32_t index = 0; index < frames; ++index) {
                output[index] = 0.0f;
            }
        }
        return SONALLOY_DSP_INVALID_ARGUMENT;
    }

    try {
        if (start_frequency_hz > 0.0f && end_frequency_hz > 0.0f) {
            const float frequency_step = frames == 0u
                ? 1.0f
                : std::exp(
                    std::log(end_frequency_hz / start_frequency_hz) /
                    static_cast<float>(frames));
            float frequency_hz = start_frequency_hz;
            for (uint32_t index = 0; index < frames; ++index) {
                handle->oscillator.SetFreq(frequency_hz);
                output[index] = handle->oscillator.Process();
                frequency_hz *= frequency_step;
            }
        } else {
            for (uint32_t index = 0; index < frames; ++index) {
                const float position = frames <= 1u
                    ? 0.0f
                    : static_cast<float>(index) / static_cast<float>(frames);
                const float frequency_hz = start_frequency_hz +
                    (end_frequency_hz - start_frequency_hz) * position;
                handle->oscillator.SetFreq(frequency_hz);
                output[index] = handle->oscillator.Process();
            }
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

extern "C" sonalloy_dsp_filter* sonalloy_dsp_filter_create(void) {
    try {
        return new sonalloy_dsp_filter();
    } catch (...) {
        return nullptr;
    }
}

extern "C" void sonalloy_dsp_filter_destroy(sonalloy_dsp_filter* handle) {
    try {
        delete handle;
    } catch (...) {
    }
}

extern "C" int32_t sonalloy_dsp_filter_prepare(
    sonalloy_dsp_filter* handle,
    double sample_rate
) {
    if (handle == nullptr) {
        return SONALLOY_DSP_NULL_HANDLE;
    }
    handle->prepared = false;
    handle->sample_rate = 0.0f;
    if (!valid_sample_rate(sample_rate)) {
        return SONALLOY_DSP_INVALID_ARGUMENT;
    }
    try {
        handle->sample_rate = static_cast<float>(sample_rate);
        handle->filter.Init(handle->sample_rate);
        handle->prepared = true;
        return SONALLOY_DSP_OK;
    } catch (...) {
        handle->prepared = false;
        return SONALLOY_DSP_NATIVE_EXCEPTION;
    }
}

extern "C" int32_t sonalloy_dsp_filter_reset(sonalloy_dsp_filter* handle) {
    if (handle == nullptr) {
        return SONALLOY_DSP_NULL_HANDLE;
    }
    if (!handle->prepared) {
        return SONALLOY_DSP_NOT_PREPARED;
    }
    try {
        handle->filter.Init(handle->sample_rate);
        return SONALLOY_DSP_OK;
    } catch (...) {
        return SONALLOY_DSP_NATIVE_EXCEPTION;
    }
}

extern "C" int32_t sonalloy_dsp_filter_process(
    sonalloy_dsp_filter* handle,
    float cutoff_hz,
    float resonance,
    float* buffer,
    uint32_t frames
) {
    if (handle == nullptr) {
        return SONALLOY_DSP_NULL_HANDLE;
    }
    if (frames > 0u && buffer == nullptr) {
        return SONALLOY_DSP_INVALID_ARGUMENT;
    }
    if (!handle->prepared) {
        if (buffer != nullptr) {
            for (uint32_t index = 0; index < frames; ++index) {
                buffer[index] = 0.0f;
            }
        }
        return SONALLOY_DSP_NOT_PREPARED;
    }
    if (!valid_cutoff(cutoff_hz, handle->sample_rate) ||
        !valid_resonance(resonance)) {
        if (buffer != nullptr) {
            for (uint32_t index = 0; index < frames; ++index) {
                buffer[index] = 0.0f;
            }
        }
        return SONALLOY_DSP_INVALID_ARGUMENT;
    }
    try {
        handle->filter.SetFreq(cutoff_hz);
        handle->filter.SetRes(resonance);
        for (uint32_t index = 0; index < frames; ++index) {
            handle->filter.Process(buffer[index]);
            buffer[index] = handle->filter.Low();
        }
        return SONALLOY_DSP_OK;
    } catch (...) {
        if (buffer != nullptr) {
            for (uint32_t index = 0; index < frames; ++index) {
                buffer[index] = 0.0f;
            }
        }
        return SONALLOY_DSP_NATIVE_EXCEPTION;
    }
}

extern "C" int32_t sonalloy_dsp_filter_process_ramp(
    sonalloy_dsp_filter* handle,
    float start_cutoff_hz,
    float end_cutoff_hz,
    float resonance,
    float* buffer,
    uint32_t frames
) {
    if (handle == nullptr) {
        return SONALLOY_DSP_NULL_HANDLE;
    }
    if (frames > 0u && buffer == nullptr) {
        return SONALLOY_DSP_INVALID_ARGUMENT;
    }
    if (!handle->prepared) {
        if (buffer != nullptr) {
            for (uint32_t index = 0; index < frames; ++index) {
                buffer[index] = 0.0f;
            }
        }
        return SONALLOY_DSP_NOT_PREPARED;
    }
    if (!valid_cutoff(start_cutoff_hz, handle->sample_rate) ||
        !valid_cutoff(end_cutoff_hz, handle->sample_rate) ||
        !valid_resonance(resonance)) {
        if (buffer != nullptr) {
            for (uint32_t index = 0; index < frames; ++index) {
                buffer[index] = 0.0f;
            }
        }
        return SONALLOY_DSP_INVALID_ARGUMENT;
    }
    try {
        handle->filter.SetRes(resonance);
        for (uint32_t index = 0; index < frames; ++index) {
            const float position = frames <= 1u
                ? 0.0f
                : static_cast<float>(index) / static_cast<float>(frames - 1u);
            const float cutoff_hz = start_cutoff_hz +
                (end_cutoff_hz - start_cutoff_hz) * position;
            handle->filter.SetFreq(cutoff_hz);
            handle->filter.Process(buffer[index]);
            buffer[index] = handle->filter.Low();
        }
        return SONALLOY_DSP_OK;
    } catch (...) {
        if (buffer != nullptr) {
            for (uint32_t index = 0; index < frames; ++index) {
                buffer[index] = 0.0f;
            }
        }
        return SONALLOY_DSP_NATIVE_EXCEPTION;
    }
}

extern "C" int32_t sonalloy_dsp_filter_process_ramp_with_resonance(
    sonalloy_dsp_filter* handle,
    float start_cutoff_hz,
    float end_cutoff_hz,
    float start_resonance,
    float end_resonance,
    float* buffer,
    uint32_t frames
) {
    if (handle == nullptr) {
        return SONALLOY_DSP_NULL_HANDLE;
    }
    if (frames > 0u && buffer == nullptr) {
        return SONALLOY_DSP_INVALID_ARGUMENT;
    }
    if (!handle->prepared) {
        if (buffer != nullptr) {
            for (uint32_t index = 0; index < frames; ++index) {
                buffer[index] = 0.0f;
            }
        }
        return SONALLOY_DSP_NOT_PREPARED;
    }
    if (!valid_cutoff(start_cutoff_hz, handle->sample_rate) ||
        !valid_cutoff(end_cutoff_hz, handle->sample_rate) ||
        !valid_resonance(start_resonance) ||
        !valid_resonance(end_resonance)) {
        if (buffer != nullptr) {
            for (uint32_t index = 0; index < frames; ++index) {
                buffer[index] = 0.0f;
            }
        }
        return SONALLOY_DSP_INVALID_ARGUMENT;
    }
    try {
        for (uint32_t index = 0; index < frames; ++index) {
            const float position = frames <= 1u
                ? 0.0f
                : static_cast<float>(index) / static_cast<float>(frames);
            const float cutoff_hz = start_cutoff_hz +
                (end_cutoff_hz - start_cutoff_hz) * position;
            const float resonance = start_resonance +
                (end_resonance - start_resonance) * position;
            handle->filter.SetFreq(cutoff_hz);
            handle->filter.SetRes(resonance);
            handle->filter.Process(buffer[index]);
            buffer[index] = handle->filter.Low();
        }
        return SONALLOY_DSP_OK;
    } catch (...) {
        if (buffer != nullptr) {
            for (uint32_t index = 0; index < frames; ++index) {
                buffer[index] = 0.0f;
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
