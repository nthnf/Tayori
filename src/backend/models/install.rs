use crate::backend::path::app_data_dir;
use anyhow::Result;
use reqwest::Client;
use std::fs::{self};
use std::path::{Path, PathBuf};

const SILERO_URL: &str =
    "https://huggingface.co/onnx-community/silero-vad/resolve/main/onnx/model_quantized.onnx";

pub fn moonshine_url(size: &str) -> String {
    format!(
        "https://blob.handy.computer/moonshine-{}-streaming-en.tar.gz",
        size
    )
}

fn default_model_path() -> PathBuf {
    app_data_dir().join("models")
}

pub fn default_silero_path(base_path: Option<&Path>) -> PathBuf {
    let default_path = default_model_path();
    let base = base_path.unwrap_or(&default_path);
    base.join("silero_vad.onnx")
}

pub fn moonshine_path(size: &str, base_path: Option<&Path>) -> PathBuf {
    let default_path = default_model_path();
    let base = base_path.unwrap_or(&default_path);
    base.join(format!("moonshine/{}", size))
}

pub async fn install_moonshine(size: &str, base_path: Option<&Path>) -> Result<PathBuf> {
    let url = moonshine_url(size);
    let target_dir = moonshine_path(size, base_path);
    fs::create_dir_all(&target_dir)?;

    // Download and extract
    let response = Client::new().get(url).send().await?;
    let bytes = response.bytes().await?;
    let tar = flate2::read::GzDecoder::new(std::io::Cursor::new(bytes));
    let mut archive = tar::Archive::new(tar);
    archive.unpack(&target_dir)?;

    // Find the nested directory (there should only be one)
    let nested_dir = fs::read_dir(&target_dir)?
        .filter_map(Result::ok)
        .find(|entry| entry.path().is_dir());

    if let Some(dir) = nested_dir {
        let nested_path = dir.path();
        // Move contents from nested to target
        for entry in fs::read_dir(&nested_path)? {
            let entry = entry?;
            let from = entry.path();
            let to = target_dir.join(entry.file_name());
            fs::rename(from, to)?;
        }
        // Remove the now-empty nested directory
        fs::remove_dir(nested_path)?;
    }

    Ok(target_dir)
}

pub async fn install_silero(base_path: Option<&Path>) -> Result<PathBuf> {
    let url = SILERO_URL;
    let target = default_silero_path(base_path);

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }

    let response = Client::new().get(url).send().await?;
    let bytes = response.bytes().await?;
    fs::write(&target, bytes)?;

    Ok(target)
}

pub fn list_installed_models(base_path: Option<&Path>) -> Result<Vec<String>> {
    let default_path = default_model_path();
    let models_dir = base_path.unwrap_or(&default_path).join("moonshine");

    let mut installed = Vec::new();

    if !models_dir.exists() {
        return Ok(installed);
    }

    for entry in fs::read_dir(models_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir()
            && let Some(name) = path.file_name().and_then(|n| n.to_str())
        {
            installed.push(name.to_string());
        }
    }

    Ok(installed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_resolution() {
        let moonshine = moonshine_path("tiny", None);
        assert!(moonshine.ends_with("moonshine/tiny"));
    }
}

#[test]
fn test_list_installed_models() {
    let temp_dir = tempfile::tempdir().unwrap();
    let base_path = temp_dir.path();
    let moonshine_dir = base_path.join("moonshine");

    // Create some mock installed model directories
    fs::create_dir_all(moonshine_dir.join("tiny")).unwrap();
    fs::create_dir_all(moonshine_dir.join("small")).unwrap();
    fs::write(moonshine_dir.join("not-a-dir"), "test").unwrap(); // Should be ignored

    let mut models = list_installed_models(Some(base_path)).unwrap();
    models.sort();

    assert_eq!(models, vec!["small", "tiny"]);
}
