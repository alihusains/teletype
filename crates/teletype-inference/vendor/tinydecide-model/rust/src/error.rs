use std::fmt;

/// Everything that can go wrong when loading a model or answering a request.
#[derive(Debug)]
pub enum Error {
    /// Reading `meta.json` or `model.bin` failed.
    Io(std::io::Error),
    /// `meta.json` is not valid JSON or misses a field.
    Json(serde_json::Error),
    /// The model files are inconsistent (missing tensor, wrong shape, offset past the end, ...).
    Model(String),
    /// The request cannot be answered, for example a question longer than the model accepts.
    Request(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Json(e) => write!(f, "meta.json: {e}"),
            Error::Model(m) | Error::Request(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
