#include "sonalloy_stretch.h"

#include "signalsmith-stretch.h"

#include <cmath>
#include <cstdint>
#include <exception>
#include <limits>
#include <new>

struct sonalloy_stretch {
    signalsmith::stretch::SignalsmithStretch<float> processor{0};
    int32_t channels = 0;
    uint32_t max_input_frames = 0;
    uint32_t max_output_frames = 0;
    bool prepared = false;
#ifdef SONALLOY_DSP_TEST_HOOKS
    bool fail_next_process = false;
#endif
};

namespace {

bool valid_sample_rate(double sample_rate) {
    return std::isfinite(sample_rate) && sample_rate > 0.0 &&
           sample_rate <= static_cast<double>(std::numeric_limits<int32_t>::max());
}

bool valid_frame_limit(uint32_t frames) {
    return frames > 0 && frames <= static_cast<uint32_t>(std::numeric_limits<int32_t>::max());
}

bool valid_playback_rate(double playback_rate) {
    return std::isfinite(playback_rate) && playback_rate > 0.0 &&
           playback_rate <= static_cast<double>(std::numeric_limits<float>::max());
}

template <typename Callback>
int32_t invoke(const Callback& callback) {
    try {
        callback();
        return SONALLOY_STRETCH_OK;
    } catch (const std::exception&) {
        return SONALLOY_STRETCH_NATIVE_EXCEPTION;
    } catch (...) {
        return SONALLOY_STRETCH_NATIVE_EXCEPTION;
    }
}

} // namespace

extern "C" const char* sonalloy_stretch_backend_version(void) {
    static const char version[] = "signalsmith-stretch-1.3.2+linear-0.3.1";
    return version;
}

extern "C" sonalloy_stretch* sonalloy_stretch_create(void) {
    try {
        return new sonalloy_stretch();
    } catch (...) {
        return nullptr;
    }
}

extern "C" void sonalloy_stretch_destroy(sonalloy_stretch* handle) {
    delete handle;
}

extern "C" int32_t sonalloy_stretch_prepare(
    sonalloy_stretch* handle,
    int32_t channels,
    double sample_rate,
    uint32_t max_input_frames,
    uint32_t max_output_frames
) {
    if (handle == nullptr) {
        return SONALLOY_STRETCH_NULL_HANDLE;
    }
    handle->prepared = false;
    if ((channels != 1 && channels != 2) || !valid_sample_rate(sample_rate) ||
        !valid_frame_limit(max_input_frames) || !valid_frame_limit(max_output_frames)) {
        return SONALLOY_STRETCH_INVALID_ARGUMENT;
    }
    const auto status = invoke([&] {
        handle->processor.presetDefault(channels, static_cast<float>(sample_rate), true);
        handle->channels = channels;
        handle->max_input_frames = max_input_frames;
        handle->max_output_frames = max_output_frames;
        handle->prepared = true;
    });
    if (status != SONALLOY_STRETCH_OK) {
        handle->prepared = false;
    }
    return status;
}

extern "C" int32_t sonalloy_stretch_reset(sonalloy_stretch* handle) {
    if (handle == nullptr) {
        return SONALLOY_STRETCH_NULL_HANDLE;
    }
    if (!handle->prepared) {
        return SONALLOY_STRETCH_NOT_PREPARED;
    }
    return invoke([&] { handle->processor.reset(); });
}

extern "C" int32_t sonalloy_stretch_set_pitch(sonalloy_stretch* handle, double semitones) {
    if (handle == nullptr) {
        return SONALLOY_STRETCH_NULL_HANDLE;
    }
    if (!handle->prepared) {
        return SONALLOY_STRETCH_NOT_PREPARED;
    }
    if (!std::isfinite(semitones)) {
        return SONALLOY_STRETCH_INVALID_ARGUMENT;
    }
    return invoke([&] { handle->processor.setTransposeSemitones(static_cast<float>(semitones)); });
}

extern "C" int32_t sonalloy_stretch_seek(
    sonalloy_stretch* handle,
    const float* const* input_buffers,
    uint32_t input_frames,
    double playback_rate
) {
    if (handle == nullptr) {
        return SONALLOY_STRETCH_NULL_HANDLE;
    }
    if (!handle->prepared) {
        return SONALLOY_STRETCH_NOT_PREPARED;
    }
    if (input_buffers == nullptr || input_frames > handle->max_input_frames ||
        !valid_playback_rate(playback_rate)) {
        return SONALLOY_STRETCH_INVALID_ARGUMENT;
    }
    for (int32_t channel = 0; channel < handle->channels; ++channel) {
        if (input_frames > 0 && input_buffers[channel] == nullptr) {
            return SONALLOY_STRETCH_INVALID_ARGUMENT;
        }
        for (uint32_t frame = 0; frame < input_frames; ++frame) {
            if (!std::isfinite(input_buffers[channel][frame])) {
                return SONALLOY_STRETCH_NON_FINITE;
            }
        }
    }
    const auto status = invoke([&] {
        handle->processor.seek(
            input_buffers,
            static_cast<int>(input_frames),
            static_cast<float>(playback_rate)
        );
    });
    return status;
}

extern "C" int32_t sonalloy_stretch_process(
    sonalloy_stretch* handle,
    const float* const* input_buffers,
    uint32_t input_frames,
    float* const* output_buffers,
    uint32_t output_frames
) {
    if (handle == nullptr) {
        return SONALLOY_STRETCH_NULL_HANDLE;
    }
    if (!handle->prepared) {
        return SONALLOY_STRETCH_NOT_PREPARED;
    }
    if (input_buffers == nullptr || output_buffers == nullptr ||
        input_frames > handle->max_input_frames || output_frames > handle->max_output_frames) {
        return SONALLOY_STRETCH_INVALID_ARGUMENT;
    }
    for (int32_t channel = 0; channel < handle->channels; ++channel) {
        if ((input_frames > 0 && input_buffers[channel] == nullptr) ||
            (output_frames > 0 && output_buffers[channel] == nullptr)) {
            return SONALLOY_STRETCH_INVALID_ARGUMENT;
        }
        for (uint32_t frame = 0; frame < input_frames; ++frame) {
            if (!std::isfinite(input_buffers[channel][frame])) {
                return SONALLOY_STRETCH_NON_FINITE;
            }
        }
    }
#ifdef SONALLOY_DSP_TEST_HOOKS
    if (handle->fail_next_process) {
        handle->fail_next_process = false;
        return SONALLOY_STRETCH_NATIVE_EXCEPTION;
    }
#endif
    const auto status = invoke([&] {
        handle->processor.process(input_buffers, static_cast<int>(input_frames), output_buffers,
                                  static_cast<int>(output_frames));
    });
    if (status != SONALLOY_STRETCH_OK) {
        return status;
    }
    for (int32_t channel = 0; channel < handle->channels; ++channel) {
        for (uint32_t frame = 0; frame < output_frames; ++frame) {
            if (!std::isfinite(output_buffers[channel][frame])) {
                for (int32_t clear_channel = 0; clear_channel < handle->channels; ++clear_channel) {
                    if (output_buffers[clear_channel] != nullptr) {
                        for (uint32_t clear_frame = 0; clear_frame < output_frames; ++clear_frame) {
                            output_buffers[clear_channel][clear_frame] = 0.0f;
                        }
                    }
                }
                return SONALLOY_STRETCH_NON_FINITE;
            }
        }
    }
    return SONALLOY_STRETCH_OK;
}

extern "C" int32_t sonalloy_stretch_flush(
    sonalloy_stretch* handle,
    float* const* output_buffers,
    uint32_t output_frames
) {
    if (handle == nullptr) {
        return SONALLOY_STRETCH_NULL_HANDLE;
    }
    if (!handle->prepared) {
        return SONALLOY_STRETCH_NOT_PREPARED;
    }
    if (output_buffers == nullptr || output_frames > handle->max_output_frames) {
        return SONALLOY_STRETCH_INVALID_ARGUMENT;
    }
    for (int32_t channel = 0; channel < handle->channels; ++channel) {
        if (output_frames > 0 && output_buffers[channel] == nullptr) {
            return SONALLOY_STRETCH_INVALID_ARGUMENT;
        }
    }
    const auto status = invoke([&] {
        handle->processor.flush(output_buffers, static_cast<int>(output_frames));
    });
    if (status != SONALLOY_STRETCH_OK) {
        for (int32_t channel = 0; channel < handle->channels; ++channel) {
            if (output_buffers[channel] != nullptr) {
                for (uint32_t frame = 0; frame < output_frames; ++frame) {
                    output_buffers[channel][frame] = 0.0f;
                }
            }
        }
        return status;
    }
    for (int32_t channel = 0; channel < handle->channels; ++channel) {
        for (uint32_t frame = 0; frame < output_frames; ++frame) {
            if (!std::isfinite(output_buffers[channel][frame])) {
                for (int32_t clear_channel = 0; clear_channel < handle->channels; ++clear_channel) {
                    if (output_buffers[clear_channel] != nullptr) {
                        for (uint32_t clear_frame = 0; clear_frame < output_frames; ++clear_frame) {
                            output_buffers[clear_channel][clear_frame] = 0.0f;
                        }
                    }
                }
                return SONALLOY_STRETCH_NON_FINITE;
            }
        }
    }
    return SONALLOY_STRETCH_OK;
}

extern "C" int32_t sonalloy_stretch_input_latency(const sonalloy_stretch* handle) {
    if (handle == nullptr) {
        return -1;
    }
    if (!handle->prepared) {
        return -1;
    }
    return handle->processor.inputLatency();
}

extern "C" int32_t sonalloy_stretch_output_latency(const sonalloy_stretch* handle) {
    if (handle == nullptr) {
        return -1;
    }
    if (!handle->prepared) {
        return -1;
    }
    return handle->processor.outputLatency();
}

extern "C" int32_t sonalloy_stretch_interval_samples(const sonalloy_stretch* handle) {
    if (handle == nullptr) {
        return -1;
    }
    if (!handle->prepared) {
        return -1;
    }
    return handle->processor.intervalSamples();
}

#ifdef SONALLOY_DSP_TEST_HOOKS
extern "C" void sonalloy_dsp_test_arm_stretch_process_exception(sonalloy_stretch* handle) {
    if (handle != nullptr) {
        handle->fail_next_process = true;
    }
}
#endif
