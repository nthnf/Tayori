use std::path::PathBuf;

#[allow(dead_code)]
pub const DEFAULT_WHISPER_MODEL_NAME: &str = "large-v3-turbo-q5_0";
pub const DEFAULT_WHISPER_MODEL_FILE: &str = "ggml-large-v3-turbo-q5_0.bin";

pub fn default_whisper_model_path() -> PathBuf {
    whisper_model_path(DEFAULT_WHISPER_MODEL_FILE)
}

pub fn whisper_model_path(filename: &str) -> PathBuf {
    app_data_dir().join("models").join("whisper").join(filename)
}

fn app_data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").expect("HOME is not set");
            PathBuf::from(home).join(".local/share")
        })
        .join("tayori")
}
