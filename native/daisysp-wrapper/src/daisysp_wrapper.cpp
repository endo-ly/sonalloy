#include "sonalloy_dsp.h"

#include "Synthesis/oscillator.h"
#include "Synthesis/variableshapeosc.h"
#include "Filters/svf.h"
#include "Effects/wavefolder.h"

#include <cmath>
#include <cstdint>
#include <limits>
#include <new>
#include <stdexcept>

struct sonalloy_dsp_oscillator {
    daisysp::Oscillator oscillator;
    float sample_rate = 0.0f;
    // DaisySP's PolyBLEP Triangle retains an integrator state that Reset does not clear.
    int32_t waveform = SONALLOY_DSP_WAVEFORM_SINE;
    float triangle_phase = 0.0f;
    float triangle_phase_inc = 0.0f;
    float triangle_last_out = 0.0f;
    bool prepared = false;
#ifdef SONALLOY_DSP_TEST_HOOKS
    bool throw_on_process = false;
#endif
};

struct sonalloy_dsp_variable_oscillator {
    daisysp::VariableShapeOscillator oscillator;
    float sample_rate = 0.0f;
    float waveshape = 0.0f;
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

struct sonalloy_dsp_wavefolder {
    daisysp::Wavefolder wavefolder;
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

bool valid_variable_frequency(float frequency_hz, float sample_rate) {
    return std::isfinite(frequency_hz) && frequency_hz > 0.0f &&
           frequency_hz <= sample_rate * 0.5f;
}

bool valid_phase(float phase) {
    return std::isfinite(phase) && phase >= 0.0f && phase <= 1.0f;
}

bool valid_pulse_width(float pulse_width) {
    return std::isfinite(pulse_width) && pulse_width >= 0.0f && pulse_width <= 1.0f;
}

bool valid_cutoff(float cutoff_hz, float sample_rate) {
    return std::isfinite(cutoff_hz) && cutoff_hz > 0.0f &&
           cutoff_hz <= sample_rate * 0.45f;
}

bool valid_resonance(float resonance) {
    return std::isfinite(resonance) && resonance >= 0.0f && resonance <= 1.0f;
}

bool valid_wavefolder_drive(float drive) {
    return std::isfinite(drive) && drive >= 1.0f && drive <= 8.0f;
}

bool valid_wavefolder_mix(float mix) {
    return std::isfinite(mix) && mix >= 0.0f && mix <= 1.0f;
}

bool valid_wavefolder_input(const float* buffer, uint32_t frames) {
    for (uint32_t index = 0; index < frames; ++index) {
        if (!std::isfinite(buffer[index])) {
            return false;
        }
    }
    return true;
}

}  // namespace

namespace {

bool variable_waveshape(int32_t waveform, float* waveshape) {
    switch (waveform) {
        case SONALLOY_DSP_WAVEFORM_SAW:
            *waveshape = 0.5f;
            return true;
        case SONALLOY_DSP_WAVEFORM_TRIANGLE:
            *waveshape = 0.0f;
            return true;
        case SONALLOY_DSP_WAVEFORM_SQUARE:
        case SONALLOY_DSP_WAVEFORM_PULSE:
            *waveshape = 1.0f;
            return true;
        default:
            return false;
    }
}

void clear_variable_output(float* output, uint32_t frames) {
    if (output != nullptr) {
        for (uint32_t index = 0; index < frames; ++index) {
            output[index] = 0.0f;
        }
    }
}

float triangle_polyblep(float phase_inc, float phase) {
    if (phase_inc <= 0.0f) {
        return 0.0f;
    }
    if (phase < phase_inc) {
        phase /= phase_inc;
        return phase + phase - phase * phase - 1.0f;
    }
    if (phase > 1.0f - phase_inc) {
        phase = (phase - 1.0f) / phase_inc;
        return phase * phase + phase + phase + 1.0f;
    }
    return 0.0f;
}

float triangle_fastmod1f(float phase) {
    return phase >= 1.0f ? phase - 1.0f : phase;
}

float process_triangle_sample(sonalloy_dsp_oscillator* handle) {
    const float phase = handle->triangle_phase;
    float output = phase < 0.5f ? 1.0f : -1.0f;
    output += triangle_polyblep(handle->triangle_phase_inc, phase);
    output -= triangle_polyblep(
        handle->triangle_phase_inc,
        triangle_fastmod1f(phase + 0.5f)
    );
    output = handle->triangle_phase_inc * output +
        (1.0f - handle->triangle_phase_inc) * handle->triangle_last_out;
    handle->triangle_last_out = output;
    output *= 4.0f;

    handle->triangle_phase += handle->triangle_phase_inc;
    if (handle->triangle_phase > 1.0f) {
        handle->triangle_phase -= 1.0f;
    }
    return output;
}

void set_oscillator_frequency(sonalloy_dsp_oscillator* handle, float frequency_hz) {
    if (handle->waveform == SONALLOY_DSP_WAVEFORM_TRIANGLE) {
        handle->triangle_phase_inc = frequency_hz / handle->sample_rate;
    } else {
        handle->oscillator.SetFreq(frequency_hz);
    }
}

float process_oscillator_sample(sonalloy_dsp_oscillator* handle) {
    if (handle->waveform == SONALLOY_DSP_WAVEFORM_TRIANGLE) {
        return process_triangle_sample(handle);
    }
    return handle->oscillator.Process();
}

}  // namespace

extern "C" const char* sonalloy_dsp_backend_version(void) {
    return kBackendVersion;
}

extern "C" uint32_t sonalloy_dsp_capabilities(void) {
    return SONALLOY_DSP_CAPABILITY_SINE |
           SONALLOY_DSP_CAPABILITY_SAW |
           SONALLOY_DSP_CAPABILITY_TRIANGLE |
           SONALLOY_DSP_CAPABILITY_SQUARE |
           SONALLOY_DSP_CAPABILITY_PULSE;
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
#ifdef SONALLOY_DSP_TEST_HOOKS
    handle->throw_on_process = false;
#endif
    if (!valid_sample_rate(sample_rate)) {
        return SONALLOY_DSP_INVALID_ARGUMENT;
    }
    try {
        uint8_t native_waveform = daisysp::Oscillator::WAVE_SIN;
        switch (waveform) {
            case SONALLOY_DSP_WAVEFORM_SINE:
                native_waveform = daisysp::Oscillator::WAVE_SIN;
                break;
            case SONALLOY_DSP_WAVEFORM_SAW:
                native_waveform = daisysp::Oscillator::WAVE_POLYBLEP_SAW;
                break;
            case SONALLOY_DSP_WAVEFORM_TRIANGLE:
                native_waveform = daisysp::Oscillator::WAVE_POLYBLEP_TRI;
                break;
            case SONALLOY_DSP_WAVEFORM_SQUARE:
            case SONALLOY_DSP_WAVEFORM_PULSE:
                native_waveform = daisysp::Oscillator::WAVE_POLYBLEP_SQUARE;
                break;
            default:
                return SONALLOY_DSP_UNSUPPORTED_WAVEFORM;
        }
        handle->sample_rate = static_cast<float>(sample_rate);
        handle->oscillator.Init(handle->sample_rate);
        handle->oscillator.SetAmp(1.0f);
        handle->oscillator.SetWaveform(native_waveform);
        handle->waveform = waveform;
        handle->triangle_phase = 0.0f;
        handle->triangle_phase_inc = 100.0f / handle->sample_rate;
        handle->triangle_last_out = 0.0f;
        handle->prepared = true;
        return SONALLOY_DSP_OK;
    } catch (...) {
        handle->prepared = false;
        return SONALLOY_DSP_NATIVE_EXCEPTION;
    }
}

extern "C" int32_t sonalloy_dsp_oscillator_reset(sonalloy_dsp_oscillator* handle) {
    return sonalloy_dsp_oscillator_reset_phase(handle, 0.0f);
}

extern "C" int32_t sonalloy_dsp_oscillator_reset_phase(
    sonalloy_dsp_oscillator* handle,
    float phase
) {
    if (handle == nullptr) {
        return SONALLOY_DSP_NULL_HANDLE;
    }
    if (!handle->prepared) {
        return SONALLOY_DSP_NOT_PREPARED;
    }
    if (!valid_phase(phase)) {
        return SONALLOY_DSP_INVALID_ARGUMENT;
    }

    try {
        handle->oscillator.Reset(phase);
        if (handle->waveform == SONALLOY_DSP_WAVEFORM_TRIANGLE) {
            handle->triangle_phase = phase;
            handle->triangle_last_out = 0.0f;
        }
        return SONALLOY_DSP_OK;
    } catch (...) {
        return SONALLOY_DSP_NATIVE_EXCEPTION;
    }
}

extern "C" int32_t sonalloy_dsp_oscillator_process_with_pulse_width(
    sonalloy_dsp_oscillator* handle,
    float frequency_hz,
    float pulse_width,
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
    if (!valid_frequency(frequency_hz, handle->sample_rate) ||
        !valid_pulse_width(pulse_width)) {
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
        set_oscillator_frequency(handle, frequency_hz);
        handle->oscillator.SetPw(pulse_width);
        for (uint32_t index = 0; index < frames; ++index) {
            output[index] = process_oscillator_sample(handle);
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
        set_oscillator_frequency(handle, frequency_hz);
        for (uint32_t index = 0; index < frames; ++index) {
            output[index] = process_oscillator_sample(handle);
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
#ifdef SONALLOY_DSP_TEST_HOOKS
        if (handle->throw_on_process) {
            handle->throw_on_process = false;
            throw std::runtime_error("native ramp process test exception");
        }
#endif
        if (start_frequency_hz > 0.0f && end_frequency_hz > 0.0f) {
            const float frequency_step = frames == 0u
                ? 1.0f
                : std::exp(
                    std::log(end_frequency_hz / start_frequency_hz) /
                    static_cast<float>(frames));
            float frequency_hz = start_frequency_hz;
            for (uint32_t index = 0; index < frames; ++index) {
                set_oscillator_frequency(handle, frequency_hz);
                output[index] = process_oscillator_sample(handle);
                frequency_hz *= frequency_step;
            }
        } else {
            for (uint32_t index = 0; index < frames; ++index) {
                const float position = frames <= 1u
                    ? 0.0f
                    : static_cast<float>(index) / static_cast<float>(frames);
                const float frequency_hz = start_frequency_hz +
                    (end_frequency_hz - start_frequency_hz) * position;
                set_oscillator_frequency(handle, frequency_hz);
                output[index] = process_oscillator_sample(handle);
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

extern "C" int32_t sonalloy_dsp_oscillator_process_ramp_with_pulse_width(
    sonalloy_dsp_oscillator* handle,
    float start_frequency_hz,
    float end_frequency_hz,
    float start_pulse_width,
    float end_pulse_width,
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
        !valid_frequency(end_frequency_hz, handle->sample_rate) ||
        !valid_pulse_width(start_pulse_width) ||
        !valid_pulse_width(end_pulse_width)) {
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
            throw std::runtime_error("native ramp process test exception");
        }
#endif
        const float frequency_step = (frames == 0u ||
                                      start_frequency_hz <= 0.0f ||
                                      end_frequency_hz <= 0.0f)
            ? 0.0f
            : std::exp(
                std::log(end_frequency_hz / start_frequency_hz) /
                static_cast<float>(frames));
        float frequency_hz = start_frequency_hz;
        for (uint32_t index = 0; index < frames; ++index) {
            const float position = static_cast<float>(index) /
                static_cast<float>(frames);
            const float pulse_width = start_pulse_width +
                (end_pulse_width - start_pulse_width) * position;
            if (start_frequency_hz > 0.0f && end_frequency_hz > 0.0f) {
                set_oscillator_frequency(handle, frequency_hz);
                frequency_hz *= frequency_step;
            } else {
                set_oscillator_frequency(handle, start_frequency_hz +
                    (end_frequency_hz - start_frequency_hz) * position);
            }
            handle->oscillator.SetPw(pulse_width);
            output[index] = process_oscillator_sample(handle);
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

extern "C" sonalloy_dsp_variable_oscillator*
sonalloy_dsp_variable_oscillator_create(void) {
    try {
        return new sonalloy_dsp_variable_oscillator();
    } catch (...) {
        return nullptr;
    }
}

extern "C" void sonalloy_dsp_variable_oscillator_destroy(
    sonalloy_dsp_variable_oscillator* handle
) {
    try {
        delete handle;
    } catch (...) {
    }
}

extern "C" int32_t sonalloy_dsp_variable_oscillator_prepare(
    sonalloy_dsp_variable_oscillator* handle,
    double sample_rate,
    int32_t waveform
) {
    if (handle == nullptr) {
        return SONALLOY_DSP_NULL_HANDLE;
    }
    handle->prepared = false;
#ifdef SONALLOY_DSP_TEST_HOOKS
    handle->throw_on_process = false;
#endif
    if (!valid_sample_rate(sample_rate)) {
        return SONALLOY_DSP_INVALID_ARGUMENT;
    }
    if (!variable_waveshape(waveform, &handle->waveshape)) {
        return SONALLOY_DSP_UNSUPPORTED_WAVEFORM;
    }
    try {
        handle->sample_rate = static_cast<float>(sample_rate);
        handle->oscillator.Init(handle->sample_rate);
        handle->oscillator.SetWaveshape(handle->waveshape);
        handle->oscillator.SetSync(true);
        handle->prepared = true;
        return SONALLOY_DSP_OK;
    } catch (...) {
        handle->prepared = false;
        return SONALLOY_DSP_NATIVE_EXCEPTION;
    }
}

extern "C" int32_t sonalloy_dsp_variable_oscillator_reset(
    sonalloy_dsp_variable_oscillator* handle
) {
    if (handle == nullptr) {
        return SONALLOY_DSP_NULL_HANDLE;
    }
    if (!handle->prepared) {
        return SONALLOY_DSP_NOT_PREPARED;
    }
    try {
        handle->oscillator.Init(handle->sample_rate);
        handle->oscillator.SetWaveshape(handle->waveshape);
        handle->oscillator.SetSync(true);
        return SONALLOY_DSP_OK;
    } catch (...) {
        return SONALLOY_DSP_NATIVE_EXCEPTION;
    }
}

extern "C" int32_t sonalloy_dsp_variable_oscillator_process(
    sonalloy_dsp_variable_oscillator* handle,
    float master_frequency_hz,
    float slave_frequency_hz,
    float pulse_width,
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
        clear_variable_output(output, frames);
        return SONALLOY_DSP_NOT_PREPARED;
    }
    if (!valid_variable_frequency(master_frequency_hz, handle->sample_rate) ||
        !valid_variable_frequency(slave_frequency_hz, handle->sample_rate) ||
        !valid_pulse_width(pulse_width)) {
        clear_variable_output(output, frames);
        return SONALLOY_DSP_INVALID_ARGUMENT;
    }
    try {
#ifdef SONALLOY_DSP_TEST_HOOKS
        if (handle->throw_on_process) {
            handle->throw_on_process = false;
            throw std::runtime_error("native variable oscillator test exception");
        }
#endif
        handle->oscillator.SetFreq(master_frequency_hz);
        handle->oscillator.SetSyncFreq(slave_frequency_hz);
        handle->oscillator.SetPW(pulse_width);
        for (uint32_t index = 0; index < frames; ++index) {
            output[index] = handle->oscillator.Process();
        }
        return SONALLOY_DSP_OK;
    } catch (...) {
        clear_variable_output(output, frames);
        return SONALLOY_DSP_NATIVE_EXCEPTION;
    }
}

extern "C" int32_t sonalloy_dsp_variable_oscillator_process_ramp(
    sonalloy_dsp_variable_oscillator* handle,
    float start_master_frequency_hz,
    float end_master_frequency_hz,
    float start_slave_frequency_hz,
    float end_slave_frequency_hz,
    float start_pulse_width,
    float end_pulse_width,
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
        clear_variable_output(output, frames);
        return SONALLOY_DSP_NOT_PREPARED;
    }
    if (!valid_variable_frequency(start_master_frequency_hz, handle->sample_rate) ||
        !valid_variable_frequency(end_master_frequency_hz, handle->sample_rate) ||
        !valid_variable_frequency(start_slave_frequency_hz, handle->sample_rate) ||
        !valid_variable_frequency(end_slave_frequency_hz, handle->sample_rate) ||
        !valid_pulse_width(start_pulse_width) ||
        !valid_pulse_width(end_pulse_width)) {
        clear_variable_output(output, frames);
        return SONALLOY_DSP_INVALID_ARGUMENT;
    }
    try {
#ifdef SONALLOY_DSP_TEST_HOOKS
        if (handle->throw_on_process) {
            handle->throw_on_process = false;
            throw std::runtime_error("native variable oscillator ramp test exception");
        }
#endif
        const float master_step = std::exp(
            std::log(end_master_frequency_hz / start_master_frequency_hz) /
            static_cast<float>(frames == 0u ? 1u : frames));
        const float slave_step = std::exp(
            std::log(end_slave_frequency_hz / start_slave_frequency_hz) /
            static_cast<float>(frames == 0u ? 1u : frames));
        float master_frequency_hz = start_master_frequency_hz;
        float slave_frequency_hz = start_slave_frequency_hz;
        for (uint32_t index = 0; index < frames; ++index) {
            const float position = static_cast<float>(index) /
                static_cast<float>(frames);
            const float pulse_width = start_pulse_width +
                (end_pulse_width - start_pulse_width) * position;
            handle->oscillator.SetFreq(master_frequency_hz);
            handle->oscillator.SetSyncFreq(slave_frequency_hz);
            handle->oscillator.SetPW(pulse_width);
            output[index] = handle->oscillator.Process();
            master_frequency_hz *= master_step;
            slave_frequency_hz *= slave_step;
        }
        return SONALLOY_DSP_OK;
    } catch (...) {
        clear_variable_output(output, frames);
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
                : static_cast<float>(index) / static_cast<float>(frames);
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

extern "C" sonalloy_dsp_wavefolder* sonalloy_dsp_wavefolder_create(void) {
    try {
        return new sonalloy_dsp_wavefolder();
    } catch (...) {
        return nullptr;
    }
}

extern "C" void sonalloy_dsp_wavefolder_destroy(sonalloy_dsp_wavefolder* handle) {
    try {
        delete handle;
    } catch (...) {
    }
}

extern "C" int32_t sonalloy_dsp_wavefolder_prepare(
    sonalloy_dsp_wavefolder* handle,
    double sample_rate
) {
    if (handle == nullptr) {
        return SONALLOY_DSP_NULL_HANDLE;
    }
    handle->prepared = false;
#ifdef SONALLOY_DSP_TEST_HOOKS
    handle->throw_on_process = false;
#endif
    if (!valid_sample_rate(sample_rate)) {
        return SONALLOY_DSP_INVALID_ARGUMENT;
    }
    try {
        handle->wavefolder.Init();
        handle->prepared = true;
        return SONALLOY_DSP_OK;
    } catch (...) {
        handle->prepared = false;
        return SONALLOY_DSP_NATIVE_EXCEPTION;
    }
}

extern "C" int32_t sonalloy_dsp_wavefolder_reset(sonalloy_dsp_wavefolder* handle) {
    if (handle == nullptr) {
        return SONALLOY_DSP_NULL_HANDLE;
    }
    if (!handle->prepared) {
        return SONALLOY_DSP_NOT_PREPARED;
    }
    try {
        handle->wavefolder.Init();
        return SONALLOY_DSP_OK;
    } catch (...) {
        return SONALLOY_DSP_NATIVE_EXCEPTION;
    }
}

extern "C" int32_t sonalloy_dsp_wavefolder_process(
    sonalloy_dsp_wavefolder* handle,
    float drive,
    float mix,
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
        clear_variable_output(buffer, frames);
        return SONALLOY_DSP_NOT_PREPARED;
    }
    if (!valid_wavefolder_drive(drive) || !valid_wavefolder_mix(mix) ||
        !valid_wavefolder_input(buffer, frames)) {
        clear_variable_output(buffer, frames);
        return SONALLOY_DSP_INVALID_ARGUMENT;
    }

    try {
#ifdef SONALLOY_DSP_TEST_HOOKS
        if (handle->throw_on_process) {
            handle->throw_on_process = false;
            throw std::runtime_error("native wavefolder process test exception");
        }
#endif
        for (uint32_t index = 0; index < frames; ++index) {
            handle->wavefolder.SetGain(drive);
            const float folded = handle->wavefolder.Process(buffer[index]);
            buffer[index] += (folded - buffer[index]) * mix;
        }
        return SONALLOY_DSP_OK;
    } catch (...) {
        clear_variable_output(buffer, frames);
        return SONALLOY_DSP_NATIVE_EXCEPTION;
    }
}

extern "C" int32_t sonalloy_dsp_wavefolder_process_ramp(
    sonalloy_dsp_wavefolder* handle,
    float start_drive,
    float end_drive,
    float start_mix,
    float end_mix,
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
        clear_variable_output(buffer, frames);
        return SONALLOY_DSP_NOT_PREPARED;
    }
    if (!valid_wavefolder_drive(start_drive) || !valid_wavefolder_drive(end_drive) ||
        !valid_wavefolder_mix(start_mix) || !valid_wavefolder_mix(end_mix) ||
        !valid_wavefolder_input(buffer, frames)) {
        clear_variable_output(buffer, frames);
        return SONALLOY_DSP_INVALID_ARGUMENT;
    }

    try {
#ifdef SONALLOY_DSP_TEST_HOOKS
        if (handle->throw_on_process) {
            handle->throw_on_process = false;
            throw std::runtime_error("native wavefolder ramp test exception");
        }
#endif
        for (uint32_t index = 0; index < frames; ++index) {
            const float position = frames <= 1u
                ? 0.0f
                : static_cast<float>(index) / static_cast<float>(frames);
            const float drive = start_drive + (end_drive - start_drive) * position;
            const float mix = start_mix + (end_mix - start_mix) * position;
            handle->wavefolder.SetGain(drive);
            const float folded = handle->wavefolder.Process(buffer[index]);
            buffer[index] += (folded - buffer[index]) * mix;
        }
        return SONALLOY_DSP_OK;
    } catch (...) {
        clear_variable_output(buffer, frames);
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

extern "C" void sonalloy_dsp_test_arm_variable_process_exception(
    sonalloy_dsp_variable_oscillator* handle
) {
    if (handle != nullptr) {
        handle->throw_on_process = true;
    }
}

extern "C" void sonalloy_dsp_test_arm_wavefolder_process_exception(
    sonalloy_dsp_wavefolder* handle
) {
    if (handle != nullptr) {
        handle->throw_on_process = true;
    }
}
#endif
