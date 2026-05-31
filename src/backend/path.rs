use std::path::PathBuf;

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
