use std::fmt;

pub type Result<T> = std::result::Result<T, AssetError>;

/// Bounded diagnostics. Paths and untrusted source text are never written to the
/// terminal by this module; a UI must display `summary`, not raw downloaded JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetError {
    Cancelled,
    InvalidId(String),
    InvalidPath(String),
    InvalidMetadata(String),
    InvalidImage(String),
    InvalidReview(String),
    Unsupported(String),
    Integrity {
        expected: String,
        actual: String,
    },
    Limit {
        resource: &'static str,
        requested: u64,
        limit: u64,
    },
    Allocation,
    Duplicate(String),
}

pub fn summary(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(240)
        .collect()
}

impl fmt::Display for AssetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("asset request cancelled or superseded"),
            Self::InvalidId(s) => write!(f, "invalid resource identifier: {}", summary(s)),
            Self::InvalidPath(s) => write!(f, "invalid pack path: {}", summary(s)),
            Self::InvalidMetadata(s) => write!(f, "invalid texture metadata: {}", summary(s)),
            Self::InvalidImage(s) => write!(f, "invalid texture image: {}", summary(s)),
            Self::InvalidReview(s) => write!(f, "invalid full-pack review: {}", summary(s)),
            Self::Unsupported(s) => write!(f, "unsupported asset feature: {}", summary(s)),
            Self::Integrity { expected, actual } => {
                write!(f, "asset hash mismatch: expected {expected}, got {actual}")
            }
            Self::Limit {
                resource,
                requested,
                limit,
            } => write!(f, "{resource} budget exceeded: {requested} > {limit}"),
            Self::Allocation => f.write_str("asset allocation failed"),
            Self::Duplicate(s) => write!(f, "duplicate resolved texture: {}", summary(s)),
        }
    }
}
impl std::error::Error for AssetError {}
