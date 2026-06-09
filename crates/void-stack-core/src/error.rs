use thiserror::Error;

#[derive(Debug, Error)]
pub enum VoidStackError {
    #[error("Project not found: {0}")]
    ProjectNotFound(String),

    #[error("Service not found: {service} in project {project}")]
    ServiceNotFound { project: String, service: String },

    #[error("Config file not found: {0}")]
    ConfigNotFound(String),

    #[error("Invalid config: {0}")]
    InvalidConfig(String),

    #[error("Runner error: {0}")]
    RunnerError(String),

    #[error("Process failed to start: {0}")]
    ProcessStartFailed(String),

    #[error("Hook failed: {hook} — {reason}")]
    HookFailed { hook: String, reason: String },

    #[error(transparent)]
    Index(#[from] IndexError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    TomlParse(#[from] toml::de::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, VoidStackError>;

/// Errors from the semantic/vector index subsystem (`vector_index`).
///
/// Converts into [`VoidStackError`] via `#[from]`, so index functions can be
/// used with `?` in code returning [`Result`].
#[derive(Debug, Error)]
pub enum IndexError {
    #[error("no semantic index found for project '{0}' — run `index_project` first")]
    IndexNotFound(String),

    #[error("embedding failed: {0}")]
    EmbeddingFailed(String),

    #[error("HNSW index error: {0}")]
    HnswIo(String),

    #[error("index metadata DB error: {0}")]
    MetaDb(String),

    #[error("{0}")]
    Other(String),
}

impl From<rusqlite::Error> for IndexError {
    fn from(e: rusqlite::Error) -> Self {
        IndexError::MetaDb(e.to_string())
    }
}

impl From<std::io::Error> for IndexError {
    fn from(e: std::io::Error) -> Self {
        IndexError::Other(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_error_messages() {
        assert_eq!(
            IndexError::IndexNotFound("myproj".into()).to_string(),
            "no semantic index found for project 'myproj' — run `index_project` first"
        );
        assert!(
            IndexError::EmbeddingFailed("boom".into())
                .to_string()
                .contains("embedding failed")
        );
        assert!(IndexError::HnswIo("x".into()).to_string().contains("HNSW"));
    }

    #[test]
    fn test_index_error_converts_into_voidstack_error() {
        let e: VoidStackError = IndexError::MetaDb("locked".into()).into();
        assert!(e.to_string().contains("index metadata DB error: locked"));
    }

    #[test]
    fn test_rusqlite_error_maps_to_metadb() {
        let e: IndexError = rusqlite::Error::InvalidQuery.into();
        assert!(matches!(e, IndexError::MetaDb(_)));
    }
}
