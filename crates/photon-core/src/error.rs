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
    #[error("thumbnail for item {0} timed out")]
    ThumbTimeout(i64),
    #[error("thumbnail for item {0} is temporarily unavailable")]
    ThumbUnavailable(i64),
    #[error("folder not found: {0:?}")]
    FolderNotFound(PathBuf),
    #[error("folder overlaps the watched folder {existing}")]
    FolderOverlap { existing: String },
    #[error("{path} is used by photon itself and cannot be watched")]
    FolderExcluded { path: String },
    #[error("an album needs a name")]
    EmptyAlbumName,
    #[error("a tag needs a name")]
    EmptyTagName,
    /// A crop that is inverted, or too small to be anything but a slip of the pointer.
    #[error("that crop is not a usable rectangle")]
    InvalidCrop,
    /// The one write photon makes inside a watched folder: a star into a Picasa INI. Carries
    /// the path so the message names the file the user has to look at.
    #[error("could not write {}: {source}", path.display())]
    IniWrite {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
