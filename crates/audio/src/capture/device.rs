use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait};

#[derive(Debug, Clone)]
pub enum CaptureDevice {
    /// Try to find a Linux monitor/source device.
    ///
    /// This is the best CPAL-level attempt at system/output audio.
    DefaultMonitor,

    /// Normal microphone/default input path.
    DefaultInput,

    /// Default output device.
    ///
    /// Warning: this usually does NOT mean monitor/loopback capture on Linux.
    DefaultOutput,

    /// Search by device display name.
    Named(String),
}

#[derive(Debug, Clone)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub is_default_input: bool,
    pub is_default_output: bool,
    pub has_default_input_config: bool,
    pub has_default_output_config: bool,
    pub looks_like_monitor: bool,
}

pub fn list_audio_devices() -> Result<Vec<AudioDeviceInfo>> {
    let host = cpal::default_host();

    let default_input_name = host
        .default_input_device()
        .and_then(|device| display_device_name(&device).ok());

    let default_output_name = host
        .default_output_device()
        .and_then(|device| display_device_name(&device).ok());

    let mut devices = Vec::new();

    for device in host.devices()? {
        let name = display_device_name(&device).unwrap_or_else(|_| "unknown device".to_string());

        let has_default_input_config = device.default_input_config().is_ok();
        let has_default_output_config = device.default_output_config().is_ok();

        devices.push(AudioDeviceInfo {
            looks_like_monitor: looks_like_monitor_name(&name),
            is_default_input: default_input_name.as_deref() == Some(name.as_str()),
            is_default_output: default_output_name.as_deref() == Some(name.as_str()),
            name,
            has_default_input_config,
            has_default_output_config,
        });
    }

    Ok(devices)
}

pub(crate) fn select_device(host: &cpal::Host, requested: &CaptureDevice) -> Result<cpal::Device> {
    match requested {
        CaptureDevice::DefaultMonitor => select_default_monitor_device(host),

        CaptureDevice::DefaultInput => host
            .default_input_device()
            .ok_or_else(|| anyhow::anyhow!("no default input device found")),

        CaptureDevice::DefaultOutput => host
            .default_output_device()
            .ok_or_else(|| anyhow::anyhow!("no default output device found")),

        CaptureDevice::Named(wanted_name) => {
            for device in host.devices()? {
                let name = display_device_name(&device).unwrap_or_default();

                if name == *wanted_name {
                    return Ok(device);
                }
            }

            Err(anyhow::anyhow!("audio device not found: {wanted_name}"))
        }
    }
}

fn select_default_monitor_device(host: &cpal::Host) -> Result<cpal::Device> {
    let mut monitor_candidates = Vec::new();

    for device in host.devices()? {
        let name = display_device_name(&device).unwrap_or_default();

        let has_input = device.default_input_config().is_ok();
        let looks_like_monitor = looks_like_monitor_name(&name);

        tracing::info!(
            name = %name,
            has_input,
            looks_like_monitor,
            "checking CPAL device for monitor capture"
        );

        if has_input && looks_like_monitor {
            monitor_candidates.push((name, device));
        }
    }

    if let Some((name, device)) = monitor_candidates.into_iter().next() {
        tracing::info!(
            device = %name,
            "selected monitor-like CPAL capture device"
        );

        return Ok(device);
    }

    Err(anyhow::anyhow!(
        "no CPAL monitor device found. CPAL is probably exposing microphone input only."
    ))
}

pub(crate) fn select_stream_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig> {
    // For capture, input config is the correct config.
    if let Ok(config) = device.default_input_config() {
        return Ok(config);
    }

    Err(anyhow::anyhow!(
        "selected capture device has no default input config"
    ))
}

pub(crate) fn display_device_name(device: &cpal::Device) -> Result<String> {
    // CPAL 0.17 prefers description() for user-facing display names.
    if let Ok(description) = device.description() {
        return Ok(description.to_string());
    }

    #[allow(deprecated)]
    {
        Ok(device.name()?)
    }
}

fn looks_like_monitor_name(name: &str) -> bool {
    let lower = name.to_lowercase();

    lower.contains("monitor")
        || lower.contains(".monitor")
        || lower.contains("sink")
        || lower.contains("output")
        || lower.contains("loopback")
}
