use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const APP_DIR: &str = "wemi-coder";

/// User-owned paths for persistent application data.
///
/// The base directory comes from the platform's standard config directory:
/// Linux: ~/.config, Windows: %APPDATA%, macOS: ~/Library/Application Support.
#[derive(Debug, Clone)]
pub struct UserDataDirs {
    root: PathBuf,
    config: PathBuf,
    api: PathBuf,
    history: PathBuf,
}

impl UserDataDirs {
    pub fn discover() -> Result<Self> {
        let root = dirs::config_dir()
            .context("unable to determine the platform config directory")?
            .join(APP_DIR);
        Ok(Self {
            config: root.join("config"),
            api: root.join("api"),
            history: root.join("history"),
            root,
        })
    }

    pub fn ensure_exists(&self) -> Result<()> {
        for directory in [self.root(), self.config(), self.api(), self.history()] {
            std::fs::create_dir_all(directory)
                .with_context(|| format!("unable to create data directory {}", directory.display()))?;
        }
        Ok(())
    }

    pub fn root(&self) -> &Path { &self.root }
    pub fn config(&self) -> &Path { &self.config }
    pub fn api(&self) -> &Path { &self.api }
    pub fn history(&self) -> &Path { &self.history }

    #[allow(dead_code)]
    pub fn config_file(&self) -> PathBuf { self.config.join("config.json") }
    #[allow(dead_code)]
    pub fn api_settings_file(&self) -> PathBuf { self.api.join("settings.json") }
    #[allow(dead_code)]
    pub fn history_file(&self) -> PathBuf { self.history.join("sessions.jsonl") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_categorized_under_app_root() {
        let dirs = UserDataDirs::discover().unwrap();
        assert_eq!(dirs.config_file().parent(), Some(dirs.config()));
        assert_eq!(dirs.api_settings_file().parent(), Some(dirs.api()));
        assert_eq!(dirs.history_file().parent(), Some(dirs.history()));
        assert!(dirs.root().ends_with(APP_DIR));
    }
}
