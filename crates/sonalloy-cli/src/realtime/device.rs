use std::str::FromStr;

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{
    BufferSize, DeviceId, SampleFormat, StreamConfig, SupportedBufferSize, SupportedStreamConfig,
    SupportedStreamConfigRange,
};
use midir::{MidiInput, MidiInputPort};
use serde::Serialize;
use sonalloy_core::{Diagnostic, DiagnosticCode};

#[derive(Debug)]
pub(crate) struct DeviceError {
    pub(crate) diagnostic: Diagnostic,
}

impl DeviceError {
    fn audio(message: impl Into<String>) -> Self {
        Self {
            diagnostic: Diagnostic::error(DiagnosticCode::AudioDeviceError, message),
        }
    }

    fn audio_detail(message: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            diagnostic: Diagnostic::error(DiagnosticCode::AudioDeviceError, message)
                .with_detail(detail),
        }
    }

    fn midi(message: impl Into<String>) -> Self {
        Self {
            diagnostic: Diagnostic::error(DiagnosticCode::MidiError, message),
        }
    }

    fn midi_detail(message: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            diagnostic: Diagnostic::error(DiagnosticCode::MidiError, message).with_detail(detail),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct DeviceInventoryReport {
    pub(crate) audio_inputs: Vec<AudioDeviceReport>,
    pub(crate) audio_outputs: Vec<AudioDeviceReport>,
    pub(crate) midi_inputs: Vec<MidiDeviceReport>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AudioDeviceReport {
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(rename = "default")]
    pub(crate) is_default: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) default_config: Option<AudioConfigReport>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MidiDeviceReport {
    pub(crate) id: String,
    pub(crate) name: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct AudioConfigReport {
    pub(crate) sample_rate: u32,
    pub(crate) channels: usize,
    pub(crate) sample_format: String,
    pub(crate) buffer_size: Option<BufferSizeReport>,
}

#[derive(Debug, Serialize)]
pub(crate) struct BufferSizeReport {
    pub(crate) min: u32,
    pub(crate) max: u32,
}

pub(crate) struct SelectedAudioDevice {
    pub(crate) device: cpal::Device,
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) config: SupportedStreamConfig,
    pub(crate) stream_config: StreamConfig,
}

pub(crate) struct SelectedAudioInputDevice {
    pub(crate) device: cpal::Device,
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) config: SupportedStreamConfig,
    pub(crate) stream_config: StreamConfig,
}

pub(crate) struct SelectedMidiDevice {
    pub(crate) input: MidiInput,
    pub(crate) port: MidiInputPort,
    pub(crate) id: String,
    pub(crate) name: String,
}

pub(crate) fn inventory() -> Result<DeviceInventoryReport, DeviceError> {
    let host = cpal::default_host();
    let default_input_id = host
        .default_input_device()
        .and_then(|device| device.id().ok());
    let default_id = host
        .default_output_device()
        .and_then(|device| device.id().ok());
    let mut audio_inputs = Vec::new();
    let devices = host.input_devices().map_err(|error| {
        DeviceError::audio_detail("could not enumerate audio inputs", error.to_string())
    })?;
    for device in devices {
        let id = device.id().map_err(|error| {
            DeviceError::audio_detail("could not identify an audio input", error.to_string())
        })?;
        let default_config = device
            .default_input_config()
            .ok()
            .map(|config| audio_config_report(&config));
        audio_inputs.push(AudioDeviceReport {
            id: id.to_string(),
            name: device.to_string(),
            is_default: default_input_id.as_ref() == Some(&id),
            default_config,
        });
    }
    let mut audio_outputs = Vec::new();
    let devices = host.output_devices().map_err(|error| {
        DeviceError::audio_detail("could not enumerate audio outputs", error.to_string())
    })?;
    for device in devices {
        let id = device.id().map_err(|error| {
            DeviceError::audio_detail("could not identify an audio output", error.to_string())
        })?;
        let default_config = device
            .default_output_config()
            .ok()
            .map(|config| audio_config_report(&config));
        audio_outputs.push(AudioDeviceReport {
            id: id.to_string(),
            name: device.to_string(),
            is_default: default_id.as_ref() == Some(&id),
            default_config,
        });
    }

    let midi_input = MidiInput::new("sonalloy device list").map_err(|error| {
        DeviceError::midi_detail("could not initialize MIDI input", error.to_string())
    })?;
    let mut midi_inputs = Vec::new();
    for port in midi_input.ports() {
        let id = port.id();
        let name = midi_input.port_name(&port).map_err(|error| {
            DeviceError::midi_detail("could not identify a MIDI input", error.to_string())
        })?;
        midi_inputs.push(MidiDeviceReport { id, name });
    }

    Ok(DeviceInventoryReport {
        audio_inputs,
        audio_outputs,
        midi_inputs,
    })
}

pub(crate) fn select_audio(
    requested_id: Option<&str>,
    requested_sample_rate: Option<u32>,
    requested_buffer_size: usize,
) -> Result<SelectedAudioDevice, DeviceError> {
    let requested_buffer_size = u32::try_from(requested_buffer_size)
        .map_err(|_| DeviceError::audio("the requested audio buffer size is too large"))?;
    if requested_buffer_size == 0 {
        return Err(DeviceError::audio(
            "the requested audio buffer size must be positive",
        ));
    }

    let host = cpal::default_host();
    let device = match requested_id {
        Some(text) => {
            let id = DeviceId::from_str(text).map_err(|error| {
                DeviceError::audio_detail("the audio device ID is invalid", error.to_string())
            })?;
            host.device_by_id(&id).ok_or_else(|| {
                DeviceError::audio_detail(
                    "the requested audio output is not available",
                    format!("device ID: {text}"),
                )
            })?
        }
        None => host
            .default_output_device()
            .ok_or_else(|| DeviceError::audio("no default audio output is available"))?,
    };
    let id = device.id().map_err(|error| {
        DeviceError::audio_detail("could not identify the audio output", error.to_string())
    })?;
    let config = choose_config(&device, requested_sample_rate, requested_buffer_size)?;
    let stream_config = StreamConfig {
        channels: config.channels(),
        sample_rate: config.sample_rate(),
        buffer_size: BufferSize::Fixed(requested_buffer_size),
    };
    Ok(SelectedAudioDevice {
        id: id.to_string(),
        name: device.to_string(),
        device,
        config,
        stream_config,
    })
}

pub(crate) fn select_audio_input(
    requested_id: Option<&str>,
    sample_rate: u32,
    requested_buffer_size: usize,
    required_channels: usize,
) -> Result<SelectedAudioInputDevice, DeviceError> {
    let requested_buffer_size = u32::try_from(requested_buffer_size)
        .map_err(|_| DeviceError::audio("the requested audio buffer size is too large"))?;
    if requested_buffer_size == 0 {
        return Err(DeviceError::audio(
            "the requested audio buffer size must be positive",
        ));
    }
    if !(1..=2).contains(&required_channels) {
        return Err(DeviceError::audio(
            "the external audio input channel count must be one or two",
        ));
    }

    let host = cpal::default_host();
    let device = match requested_id {
        Some(text) => {
            let id = DeviceId::from_str(text).map_err(|error| {
                DeviceError::audio_detail("the audio input device ID is invalid", error.to_string())
            })?;
            host.device_by_id(&id).ok_or_else(|| {
                DeviceError::audio_detail(
                    "the requested audio input is not available",
                    format!("device ID: {text}"),
                )
            })?
        }
        None => host
            .default_input_device()
            .ok_or_else(|| DeviceError::audio("no default audio input is available"))?,
    };
    let config = choose_input_config(
        &device,
        sample_rate,
        requested_buffer_size,
        required_channels,
    )?;
    let stream_config = StreamConfig {
        channels: config.channels(),
        sample_rate: config.sample_rate(),
        buffer_size: BufferSize::Fixed(requested_buffer_size),
    };
    let id = device.id().map_err(|error| {
        DeviceError::audio_detail("could not identify the audio input", error.to_string())
    })?;
    let name = device.to_string();
    Ok(SelectedAudioInputDevice {
        device,
        id: id.to_string(),
        name,
        config,
        stream_config,
    })
}

pub(crate) fn select_midi(requested_id: Option<&str>) -> Result<SelectedMidiDevice, DeviceError> {
    let input = MidiInput::new("sonalloy live input").map_err(|error| {
        DeviceError::midi_detail("could not initialize MIDI input", error.to_string())
    })?;
    let ports = input.ports();
    let port = match requested_id {
        Some(id) => input.find_port_by_id(id).ok_or_else(|| {
            DeviceError::midi_detail(
                "the requested MIDI input is not available",
                format!("device ID: {id}"),
            )
        })?,
        None => match ports.as_slice() {
            [] => {
                return Err(DeviceError::midi(
                    "no MIDI input is available for live performance",
                ));
            }
            [port] => port.clone(),
            _ => {
                let names = ports
                    .iter()
                    .filter_map(|port| input.port_name(port).ok())
                    .collect::<Vec<_>>();
                return Err(DeviceError::midi_detail(
                    "multiple MIDI inputs are available; specify --midi-device",
                    names.join(", "),
                ));
            }
        },
    };
    let id = port.id();
    let name = input.port_name(&port).map_err(|error| {
        DeviceError::midi_detail("could not identify the MIDI input", error.to_string())
    })?;
    Ok(SelectedMidiDevice {
        input,
        port,
        id,
        name,
    })
}

fn choose_config(
    device: &cpal::Device,
    requested_sample_rate: Option<u32>,
    requested_buffer_size: u32,
) -> Result<SupportedStreamConfig, DeviceError> {
    if let Ok(default) = device.default_output_config() {
        if is_supported_config(&default)
            && requested_sample_rate.is_none_or(|rate| rate == default.sample_rate())
            && buffer_is_supported(default.buffer_size(), requested_buffer_size)
        {
            return Ok(default);
        }
    }

    let ranges = device.supported_output_configs().map_err(|error| {
        DeviceError::audio_detail("could not query audio output formats", error.to_string())
    })?;
    let mut candidates = Vec::new();
    for range in ranges {
        if !is_supported_range(&range)
            || !buffer_is_supported(range.buffer_size(), requested_buffer_size)
        {
            continue;
        }
        let Some(sample_rate) = choose_sample_rate(&range, requested_sample_rate) else {
            continue;
        };
        let Some(config) = range.try_with_sample_rate(sample_rate) else {
            continue;
        };
        candidates.push((config_score(&config), config));
    }
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
    candidates
        .into_iter()
        .next()
        .map(|(_, config)| config)
        .ok_or_else(|| {
            DeviceError::audio_detail(
                "the requested audio configuration is not supported",
                format!(
                    "sample rate: {}; buffer size: {}",
                    requested_sample_rate
                        .map_or_else(|| "device default".to_owned(), |rate| rate.to_string()),
                    requested_buffer_size
                ),
            )
        })
}

fn choose_input_config(
    device: &cpal::Device,
    requested_sample_rate: u32,
    requested_buffer_size: u32,
    required_channels: usize,
) -> Result<SupportedStreamConfig, DeviceError> {
    if let Ok(default) = device.default_input_config() {
        if is_supported_input_config(&default)
            && usize::from(default.channels()) >= required_channels
            && default.sample_rate() == requested_sample_rate
            && buffer_is_supported(default.buffer_size(), requested_buffer_size)
        {
            return Ok(default);
        }
    }

    let ranges = device.supported_input_configs().map_err(|error| {
        DeviceError::audio_detail("could not query audio input formats", error.to_string())
    })?;
    let mut candidates = Vec::new();
    for range in ranges {
        if !is_supported_input_range(&range)
            || usize::from(range.channels()) < required_channels
            || !buffer_is_supported(range.buffer_size(), requested_buffer_size)
            || !(range.min_sample_rate()..=range.max_sample_rate()).contains(&requested_sample_rate)
        {
            continue;
        }
        let Some(config) = range.try_with_sample_rate(requested_sample_rate) else {
            continue;
        };
        candidates.push((input_config_score(&config, required_channels), config));
    }
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
    candidates
        .into_iter()
        .next()
        .map(|(_, config)| config)
        .ok_or_else(|| {
            DeviceError::audio_detail(
                "the requested audio input configuration is not supported",
                format!(
                    "sample rate: {requested_sample_rate}; buffer size: {requested_buffer_size}; channels: {required_channels}"
                ),
            )
        })
}

fn choose_sample_rate(
    range: &SupportedStreamConfigRange,
    requested_sample_rate: Option<u32>,
) -> Option<u32> {
    if let Some(sample_rate) = requested_sample_rate {
        return (range.min_sample_rate()..=range.max_sample_rate())
            .contains(&sample_rate)
            .then_some(sample_rate);
    }
    [48_000, 44_100, range.min_sample_rate()]
        .into_iter()
        .find(|sample_rate| {
            (range.min_sample_rate()..=range.max_sample_rate()).contains(sample_rate)
        })
}

fn config_score(config: &SupportedStreamConfig) -> u32 {
    let channels = u32::from(config.channels());
    let stereo = u32::from(config.channels() == 2);
    let format = u32::from(config.sample_format() == SampleFormat::F32);
    let preferred_rate = match config.sample_rate() {
        48_000 => 20,
        44_100 => 10,
        _ => 0,
    };
    stereo * 1_000 + format * 100 + preferred_rate + channels
}

fn input_config_score(config: &SupportedStreamConfig, required_channels: usize) -> u32 {
    let channels = usize::from(config.channels());
    let exact = u32::from(channels == required_channels);
    let format = u32::from(config.sample_format() == SampleFormat::F32);
    exact * 1_000 + format * 100 + u32::try_from(channels).unwrap_or(u32::MAX)
}

fn is_supported_config(config: &SupportedStreamConfig) -> bool {
    config.channels() >= 2 && is_pcm(config.sample_format())
}

fn is_supported_input_config(config: &SupportedStreamConfig) -> bool {
    config.channels() >= 1 && is_pcm(config.sample_format())
}

fn is_supported_range(range: &SupportedStreamConfigRange) -> bool {
    range.channels() >= 2 && is_pcm(range.sample_format())
}

fn is_supported_input_range(range: &SupportedStreamConfigRange) -> bool {
    range.channels() >= 1 && is_pcm(range.sample_format())
}

fn is_pcm(sample_format: SampleFormat) -> bool {
    matches!(
        sample_format,
        SampleFormat::I8
            | SampleFormat::I16
            | SampleFormat::I24
            | SampleFormat::I32
            | SampleFormat::I64
            | SampleFormat::U8
            | SampleFormat::U16
            | SampleFormat::U24
            | SampleFormat::U32
            | SampleFormat::U64
            | SampleFormat::F32
            | SampleFormat::F64
    )
}

fn buffer_is_supported(buffer_size: &SupportedBufferSize, requested: u32) -> bool {
    match buffer_size {
        SupportedBufferSize::Range { min, max } => (*min..=*max).contains(&requested),
        SupportedBufferSize::Unknown => true,
    }
}

fn audio_config_report(config: &SupportedStreamConfig) -> AudioConfigReport {
    AudioConfigReport {
        sample_rate: config.sample_rate(),
        channels: usize::from(config.channels()),
        sample_format: sample_format_name(config.sample_format()),
        buffer_size: match config.buffer_size() {
            SupportedBufferSize::Range { min, max } => Some(BufferSizeReport {
                min: *min,
                max: *max,
            }),
            SupportedBufferSize::Unknown => None,
        },
    }
}

pub(crate) fn sample_format_name(sample_format: SampleFormat) -> String {
    format!("{sample_format:?}").to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_selection_prefers_stereo_float_at_48_khz() {
        let stereo_float = SupportedStreamConfig::new(
            2,
            48_000,
            SupportedBufferSize::Range { min: 128, max: 512 },
            SampleFormat::F32,
        );
        let mono_float = SupportedStreamConfig::new(
            1,
            48_000,
            SupportedBufferSize::Range { min: 128, max: 512 },
            SampleFormat::F32,
        );
        let stereo_integer = SupportedStreamConfig::new(
            2,
            44_100,
            SupportedBufferSize::Range { min: 128, max: 512 },
            SampleFormat::I16,
        );

        assert!(config_score(&stereo_float) > config_score(&mono_float));
        assert!(config_score(&stereo_float) > config_score(&stereo_integer));
    }

    #[test]
    fn config_selection_rejects_mono_dsd_and_unsupported_buffer() {
        let mono = SupportedStreamConfigRange::new(
            1,
            44_100,
            48_000,
            SupportedBufferSize::Range {
                min: 128,
                max: 1_024,
            },
            SampleFormat::F32,
        );
        let dsd = SupportedStreamConfigRange::new(
            2,
            44_100,
            48_000,
            SupportedBufferSize::Unknown,
            SampleFormat::DsdU8,
        );
        let narrow_buffer = SupportedStreamConfigRange::new(
            2,
            44_100,
            48_000,
            SupportedBufferSize::Range {
                min: 512,
                max: 1_024,
            },
            SampleFormat::F32,
        );

        assert!(!is_supported_range(&mono));
        assert!(!is_supported_range(&dsd));
        assert!(!buffer_is_supported(
            narrow_buffer.buffer_size(),
            u32::try_from(super::super::DEFAULT_BUFFER_SIZE).expect("default fits u32")
        ));
    }
}
