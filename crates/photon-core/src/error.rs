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
    #[error("this album comes from Picasa; change it in Picasa")]
    PicasaAlbum(i64),
    #[error("a tag needs a name")]
    EmptyTagName,
    #[error("a saved search needs a name")]
    EmptySearchName,
    #[error("a saved search needs a query")]
    EmptySearchQuery,
    /// A crop that is inverted, or too small to be anything but a slip of the pointer.
    #[error("that crop is not a usable rectangle")]
    InvalidCrop,
    /// An edit, rotate or clipboard copy asked of a video: photon never decodes one, so it
    /// cannot render one turned, cropped or copied.
    #[error("This is a video; that only works on photos.")]
    NotAPhoto(i64),
    /// An export aimed inside a watched folder. Copies written there would be scanned back
    /// in as new photos: one click would double the library and fill the duplicate finder.
    #[error(
        "that folder is inside the watched folder {existing} - photon would index the copies as new photos"
    )]
    ExportIntoLibrary { existing: String },
    /// The one write photon makes inside a watched folder: a star into a Picasa INI. Carries
    /// the path so the message names the file the user has to look at.
    #[error("could not write {}: {source}", path.display())]
    IniWrite {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
