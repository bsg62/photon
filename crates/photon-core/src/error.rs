use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("path is not valid UTF-8: {0:?}")]
    NonUtf8Path(PathBuf),
    #[error("library schema version {found} is newer than supported version {supported}")]
    SchemaTooNew { found: i64, supported: i64 },
    #[error("item {0} not found")]
    NotFound(i64),
    #[error("thumbnail generation failed: {0}")]
    ThumbFailed(String),
    #[error("folder not found: {0:?}")]
    FolderNotFound(PathBuf),
    #[error("folder overlaps the watched folder {existing}")]
    FolderOverlap { existing: String },
    #[error("{path} is used by photon itself and cannot be watched")]
    FolderExcluded { path: String },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
