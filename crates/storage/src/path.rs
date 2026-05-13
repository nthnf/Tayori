use anyhow::Result;
use std::path::{Path, PathBuf};

pub enum StorageKind {
    Sqlite,
    LanceDb,
}

pub fn default_storage_path(kind: StorageKind) -> Result<String> {
    let path = match kind {
        StorageKind::Sqlite => app_data_dir().join("db").join("sqlite").join("tayori.db"),

        StorageKind::LanceDb => app_data_dir().join("db").join("lancedb"),
    };

    path.into_os_string()
        .into_string()
        .map_err(|path| anyhow::anyhow!("storage path is not valid UTF-8: {:?}", path))
}

/// Ensure the directory needed for a storage path exists.
///
/// SQLite paths point at a database file, so this creates the parent directory.
/// LanceDB paths point at a database directory, so this creates the directory
/// itself. `create_dir_all` is cross-platform and handles Windows paths the
/// same way as Linux paths.
pub fn ensure_path_exists(kind: StorageKind, path: impl AsRef<Path>) -> Result<()> {
    let dir = match kind {
        StorageKind::Sqlite => path
            .as_ref()
            .parent()
            .ok_or_else(|| anyhow::anyhow!("sqlite path has no parent: {:?}", path.as_ref()))?,
        StorageKind::LanceDb => path.as_ref(),
    };

    std::fs::create_dir_all(dir)?;

    Ok(())
}

pub fn app_data_dir() -> PathBuf {
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
                let home = std::env::var_os("HOME").unwrap_or_else(|| ".".into());
                PathBuf::from(home).join(".local/share")
            })
            .join("tayori")
    }
}
