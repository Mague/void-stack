use std::path::PathBuf;

use crate::error::{Result, VoidStackError};

pub const GLOBAL_CONFIG_FILENAME: &str = "config.toml";
pub const APP_DIR_NAME: &str = "void-stack";

/// Get the global config directory (%LOCALAPPDATA\void-stack\ on Windows).
pub fn global_config_dir() -> Result<PathBuf> {
    let base = dirs::data_local_dir().ok_or_else(|| {
        VoidStackError::ConfigNotFound("Cannot determine local data directory".into())
    })?;
    Ok(base.join(APP_DIR_NAME))
}

/// Full path to the global config file.
pub fn global_config_path() -> Result<PathBuf> {
    Ok(global_config_dir()?.join(GLOBAL_CONFIG_FILENAME))
}
