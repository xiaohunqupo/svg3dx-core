use thiserror::Error;

/// Fatal document-level errors (invalid XML, missing root, etc.).
#[derive(Debug, Error)]
pub enum Error {
    #[error("XML parse error: {0}")]
    Xml(String),
    #[error("document is not an SVG root element")]
    NotSvg,
    #[error("svg3d is not enabled (missing svg3d=\"true\")")]
    Svg3dDisabled,
    #[error("internal error: {0}")]
    Internal(String),
    #[error("package error: {0}")]
    Package(String),
}

/// Non-fatal evaluation warning: subtree was skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub message: String,
    pub path: Option<String>,
}

impl Warning {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            path: None,
        }
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }
}

pub type Result<T> = std::result::Result<T, Error>;
