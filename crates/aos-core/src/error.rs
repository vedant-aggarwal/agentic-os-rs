//! One error type for the whole core crate.

/// Errors the store and the domain rules can return.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("{kind} not found: {id}")]
    NotFound { kind: &'static str, id: String },

    #[error("'{id}' matches {count} {kind} rows; use more characters")]
    Ambiguous {
        kind: &'static str,
        id: String,
        count: usize,
    },

    #[error("invalid input: {0}")]
    Invalid(String),

    #[error("not authorized")]
    Unauthorized,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Shorthand for `Error::Invalid`.
pub fn invalid(msg: impl Into<String>) -> Error {
    Error::Invalid(msg.into())
}

pub(crate) fn not_found(kind: &'static str, id: impl Into<String>) -> Error {
    Error::NotFound {
        kind,
        id: id.into(),
    }
}
