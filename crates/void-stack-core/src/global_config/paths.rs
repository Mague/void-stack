use std::path::PathBuf;

use crate::error::{Result, VoidStackError};

pub const GLOBAL_CONFIG_FILENAME: &str = "config.toml";
pub const APP_DIR_NAME: &str = "void-stack";
/// Overrides the base data directory (the `data_local_dir` parent of
/// `void-stack/`). Tests point it at a tempdir so fixtures never write
/// into the user's real data dir (indexes, config, briefings, stats).
pub const DATA_DIR_ENV: &str = "VOID_STACK_DATA_DIR";

/// Base directory every void-stack artifact hangs from. Honors
/// [`DATA_DIR_ENV`]; falls back to the OS local-data dir.
pub fn data_base_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var(DATA_DIR_ENV)
        && !dir.trim().is_empty()
    {
        return Some(PathBuf::from(dir));
    }
    dirs::data_local_dir()
}

/// Get the global config directory (%LOCALAPPDATA\void-stack\ on Windows).
pub fn global_config_dir() -> Result<PathBuf> {
    let base = data_base_dir().ok_or_else(|| {
        VoidStackError::ConfigNotFound("Cannot determine local data directory".into())
    })?;
    Ok(base.join(APP_DIR_NAME))
}

/// Full path to the global config file.
pub fn global_config_path() -> Result<PathBuf> {
    Ok(global_config_dir()?.join(GLOBAL_CONFIG_FILENAME))
}
