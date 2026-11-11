#[derive(Debug, thiserror::Error)]
pub enum WorkError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid: {0}")]
    Invalid(String),
    /// The CLI isn't installed or couldn't start.
    #[error("{0}")]
    Provider(String),
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

impl WorkError {
    /// Short machine-readable code for JSON error bodies (same codes as `aos-tools`).
    pub fn code(&self) -> &'static str {
        match self {
            WorkError::NotFound(_) => "not_found",
            WorkError::Invalid(_) => "invalid_args",
            WorkError::Provider(_) => "provider",
            _ => "internal",
        }
    }
}

pub type Result<T, E = WorkError> = std::result::Result<T, E>;
