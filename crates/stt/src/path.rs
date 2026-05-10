use std::path::PathBuf;

/// Default whisper model name used by Tayori.
#[allow(dead_code)]
pub const DEFAULT_WHISPER_MODEL_NAME: &str = "small-q8_0";

/// Build the default model path using the bundled default model name.
pub fn default_model_path() -> PathBuf {
    model_path(DEFAULT_WHISPER_MODEL_NAME)
}

/// Build the full on-disk path for a named whisper model.
pub fn model_path(model_name: &str) -> PathBuf {
    // Keep model storage under Tayori's app data dir so callers only pass a
    // logical model name like `small-q8_0`, not a platform-specific path.
    app_data_dir()
        .join("models")
        .join("whisper")
        .join(model_filename(model_name))
}

fn model_filename(filename: &str) -> String {
    // whisper.cpp model files follow the `ggml-<name>.bin` convention.
    format!("ggml-{}.bin", filename)
}

fn app_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        // Prefer roaming profile storage, then local app data, then user home.
        // Final fallback keeps tests/dev runs from failing on odd environments.
        return std::env::var_os("APPDATA")
            .or_else(|| std::env::var_os("LOCALAPPDATA"))
            .or_else(|| {
                std::env::var_os("USERPROFILE").map(|home| {
                    PathBuf::from(home)
                        .join("AppData")
                        .join("Roaming")
                        .into_os_string()
                })
            })
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".").join("AppData").join("Roaming"))
            .join("tayori");
    }

    #[cfg(target_os = "linux")]
    {
        // Follow XDG first; fall back to the common ~/.local/share location.
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME").expect("HOME is not set");
                PathBuf::from(home).join(".local/share")
            })
            .join("tayori")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_path_appends_expected_filename() {
        let path = model_path("small-q8_0");

        assert!(path.ends_with("tayori/models/whisper/ggml-small-q8_0.bin"));
    }

    #[test]
    fn model_filename_prefixes_ggml() {
        assert_eq!(model_filename("medium"), "ggml-medium.bin");
    }
}
