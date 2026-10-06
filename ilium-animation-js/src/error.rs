//! Package and JavaScript host failures; errors never execute package code.
#[derive(Debug, thiserror::Error)]
pub enum AnimationError {
    #[error("invalid package: {0}")]
    InvalidPackage(String),
    #[error("package budget exceeded: {0}")]
    Budget(String),
    #[error("package integrity failed: {0}")]
    Integrity(String),
    #[error("unsupported animation API version: {0}")]
    ApiVersion(u32),
    #[error("native animation preparing: {0}")]
    Preparing(&'static str),
    #[error("JavaScript runtime: {0}")]
    Runtime(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),
}
pub type Result<T> = std::result::Result<T, AnimationError>;
