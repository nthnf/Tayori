use super::source::AudioSource;
use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait};

/// Selected CPAL device plus source kind.
pub struct AudioDevice {
    pub device: cpal::Device,
}

impl AudioDevice {
    pub fn from(source: AudioSource) -> Result<Self> {
        let host = Self::make_host();

        match source {
            AudioSource::Monitor => {
                let device = Self::resolve_monitor_device(&host)?;

                Ok(AudioDevice { device })
            }
            AudioSource::Mic => {
                let device = host
                    .default_input_device()
                    .ok_or_else(|| anyhow::anyhow!("no default input device found"))?;

                Ok(AudioDevice { device })
            }
        }
    }

    /// Query the device's default input config.
    pub fn default_input_config(&self) -> Result<cpal::SupportedStreamConfig> {
        Ok(self.device.default_input_config()?)
    }

    /// Prefer PipeWire on Linux, then fall back to CPAL default host.
    fn make_host() -> cpal::Host {
        cpal::host_from_id(cpal::HostId::PipeWire).unwrap_or_else(|_| cpal::default_host())
    }

    /// Resolve a monitor-style input device by name.
    fn resolve_monitor_device(host: &cpal::Host) -> Result<cpal::Device> {
        for device in host.devices()? {
            let name = Self::display_device_name(&device);
            let has_input = device.default_input_config().is_ok();
            let looks_like_monitor = Self::is_monitor_name(&name);

            tracing::info!(
                name = %name,
                has_input,
                looks_like_monitor,
                "checking CPAL device for monitor capture"
            );

            if has_input && looks_like_monitor {
                tracing::info!(device = %name, "selected monitor-like CPAL capture device");
                return Ok(device);
            }
        }

        let available_inputs = Self::available_device_names(host)?;
        Err(anyhow::anyhow!(
            "no monitor input device found; available input devices: {}",
            available_inputs.join(", ")
        ))
    }

    /// Check whether a device name looks like a monitor/loopback input.
    fn is_monitor_name(name: &str) -> bool {
        name.to_lowercase().contains("monitor")
            || name.to_lowercase().contains(".monitor")
            || name.to_lowercase().contains("sink")
            || name.to_lowercase().contains("output")
            || name.to_lowercase().contains("loopback")
    }

    /// Collect available device names for error messages.
    fn available_device_names(host: &cpal::Host) -> Result<Vec<String>> {
        let mut names = Vec::new();

        for device in host.devices()? {
            names.push(Self::display_device_name(&device));
        }

        Ok(names)
    }

    /// Get a stable display name for a CPAL device.
    fn display_device_name(device: &cpal::Device) -> String {
        if let Ok(description) = device.description() {
            return description.name().to_string();
        }

        "unknown device".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_prefers_pipewire_host() {
        let host = AudioDevice::make_host();

        assert_eq!(host.id(), cpal::HostId::PipeWire);
    }

    #[test]
    fn host_can_query_default_devices() {
        let host = AudioDevice::make_host();

        let _ = host.default_input_device();
        let _ = host.default_output_device();
    }

    #[test]
    fn monitor_name_match_is_case_insensitive() {
        assert!(AudioDevice::is_monitor_name("Built-in Audio Monitor"));
        assert!(AudioDevice::is_monitor_name("MONITOR MIX"));
        assert!(AudioDevice::is_monitor_name("sink_default"));
        assert!(!AudioDevice::is_monitor_name("Microphone"));
    }

    #[test]
    fn mic_source_resolves_to_an_input_device_when_available() {
        let Ok(device) = AudioDevice::from(AudioSource::Mic) else {
            return;
        };

        let _ = device
            .default_input_config()
            .expect("mic device has input config");
    }
}
