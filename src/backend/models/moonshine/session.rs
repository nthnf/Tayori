use ort::ep::CPU;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use std::path::Path;

/// Build the execution provider list.
/// Only using CPU as GPU accelerators have been removed.
fn execution_providers() -> Vec<ort::ep::ExecutionProviderDispatch> {
    vec![CPU::default().build()]
}

/// Internal session builder with full control over threading and EP selection.
fn build_session(
    path: &Path,
    intra_threads: Option<usize>,
    parallel_execution: bool,
) -> Result<Session, ort::Error> {
    let mut builder =
        Session::builder()?.with_optimization_level(GraphOptimizationLevel::Level3)?;

    if let Some(n) = intra_threads.filter(|&n| n > 0) {
        builder = builder.with_intra_threads(n)?;
    }

    builder = builder.with_parallel_execution(parallel_execution)?;

    let session = builder
        .with_execution_providers(execution_providers())?
        .commit_from_file(path)?;

    for input in session.inputs() {
        log::info!(
            "Model input: name={}, type={:?}",
            input.name(),
            input.dtype()
        );
    }
    for output in session.outputs() {
        log::info!(
            "Model output: name={}, type={:?}",
            output.name(),
            output.dtype()
        );
    }

    Ok(session)
}

/// Create an ONNX session with standard settings.
pub fn create_session(path: &Path) -> Result<Session, ort::Error> {
    build_session(path, None, true)
}

/// Create an ONNX session with configurable thread count.
pub fn create_session_with_threads(path: &Path, num_threads: usize) -> Result<Session, ort::Error> {
    build_session(path, Some(num_threads), true)
}

/// Resolve a model file path for the requested quantization level.
///
/// Looks for `{name}.{suffix}.onnx` based on the quantization variant,
/// falling back to `{name}.onnx` (FP32) if the requested file doesn't exist.
pub fn resolve_model_path(
    dir: &Path,
    name: &str,
    quantization: &super::Quantization,
) -> std::path::PathBuf {
    let suffix = match quantization {
        super::Quantization::FP32 => None,
        super::Quantization::FP16 => Some("fp16"),
        super::Quantization::Int8 => Some("int8"),
        super::Quantization::Int4 => Some("int4"),
    };

    if let Some(suffix) = suffix {
        let path = dir.join(format!("{}.{}.onnx", name, suffix));
        if path.exists() {
            log::info!("Loading {} model: {}", suffix, path.display());
            return path;
        }
        log::warn!(
            "{} model not found at {}, falling back to {}.onnx",
            suffix,
            path.display(),
            name
        );
    }

    dir.join(format!("{}.onnx", name))
}

/// Read a custom metadata string from an ONNX session.
pub fn read_metadata_str(session: &Session, key: &str) -> Result<Option<String>, ort::Error> {
    let meta = session.metadata()?;
    Ok(meta.custom(key).filter(|s| !s.is_empty()))
}

/// Read a custom metadata i32 value, with optional default.
pub fn read_metadata_i32(
    session: &Session,
    key: &str,
    default: Option<i32>,
) -> Result<Option<i32>, crate::backend::models::moonshine::TranscribeError> {
    let str_val = read_metadata_str(session, key).map_err(|e| {
        crate::backend::models::moonshine::TranscribeError::Config(format!(
            "failed to read metadata '{}': {}",
            key, e
        ))
    })?;
    match str_val {
        Some(v) => Ok(Some(v.parse::<i32>().map_err(|e| {
            crate::backend::models::moonshine::TranscribeError::Config(format!(
                "failed to parse '{}': {}",
                key, e
            ))
        })?)),
        None => Ok(default),
    }
}

/// Read a comma-separated float vector from metadata.
pub fn read_metadata_float_vec(
    session: &Session,
    key: &str,
) -> Result<Option<Vec<f32>>, crate::backend::models::moonshine::TranscribeError> {
    let str_val = read_metadata_str(session, key).map_err(|e| {
        crate::backend::models::moonshine::TranscribeError::Config(format!(
            "failed to read metadata '{}': {}",
            key, e
        ))
    })?;
    match str_val {
        Some(v) => {
            let floats: Result<Vec<f32>, _> =
                v.split(',').map(|s| s.trim().parse::<f32>()).collect();
            Ok(Some(floats.map_err(|e| {
                crate::backend::models::moonshine::TranscribeError::Config(format!(
                    "failed to parse floats in '{}': {}",
                    key, e
                ))
            })?))
        }
        None => Ok(None),
    }
}
