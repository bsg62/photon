use serde::Serialize;

/// Error returned to the UI: a machine-readable `kind` plus a human-readable message.
#[derive(Debug, Serialize)]
pub struct AppError {
    pub kind: &'static str,
    pub message: String,
}

impl AppError {
    pub fn internal(message: impl ToString) -> Self {
        Self {
            kind: "internal",
            message: message.to_string(),
        }
    }
}

impl From<photon_core::Error> for AppError {
    fn from(err: photon_core::Error) -> Self {
        use photon_core::Error::*;
        let kind = match &err {
            FolderNotFound(_) => "folderNotFound",
            FolderOverlap { .. } => "folderOverlap",
            FolderExcluded { .. } => "folderExcluded",
            NotFound(_) => "notFound",
            IniWrite { .. } => "iniWrite",
            EmptyAlbumName => "emptyAlbumName",
            EmptyTagName => "emptyTagName",
            _ => "internal",
        };
        Self {
            kind,
            message: err.to_string(),
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AppError {}
