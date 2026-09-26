use serde::Serialize;
use std::path::Path;

/// What kind of media a library item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Image,
    /// Played and poster-framed by the webview, never decoded by photon
    /// (spec `2026-09-26-photon-video-design.md`).
    Video,
}

impl MediaKind {
    /// Classifies a file by its extension; `None` means photon ignores the file.
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            // A multi-page TIFF shows its first page: photon has no notion of one file
            // holding several photos.
            "jpg" | "jpeg" | "jpe" | "png" | "gif" | "webp" | "tif" | "tiff" | "bmp" | "avif" => {
                Some(Self::Image)
            }
            _ => None,
        }
    }

    pub fn to_db(self) -> i64 {
        match self {
            Self::Image => 0,
            Self::Video => 1,
        }
    }

    pub fn from_db(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::Image),
            1 => Some(Self::Video),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThumbState {
    Pending,
    Ready,
    Failed,
}

impl ThumbState {
    pub fn to_db(self) -> i64 {
        match self {
            Self::Pending => 0,
            Self::Ready => 1,
            Self::Failed => 2,
        }
    }

    pub fn from_db(value: i64) -> Self {
        match value {
            1 => Self::Ready,
            2 => Self::Failed,
            _ => Self::Pending,
        }
    }
}

/// Content fingerprint used as the thumbnail cache key.
pub fn fingerprint(path: &str, size: i64, mtime_ms: i64) -> u64 {
    let mut buf = Vec::with_capacity(path.len() + 16);
    buf.extend_from_slice(path.as_bytes());
    buf.extend_from_slice(&size.to_le_bytes());
    buf.extend_from_slice(&mtime_ms.to_le_bytes());
    xxhash_rust::xxh3::xxh3_64(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn recognises_common_image_extensions_case_insensitively() {
        for name in [
            "a.jpg", "a.JPEG", "a.jpe", "a.png", "a.gif", "a.WebP", "a.tif", "a.TIFF", "a.bmp",
            "a.avif", "a.AVIF",
        ] {
            assert_eq!(
                MediaKind::from_path(Path::new(name)),
                Some(MediaKind::Image),
                "{name}"
            );
        }
        for name in ["a.txt", "a.heic", "a", ".jpg"] {
            assert_eq!(MediaKind::from_path(Path::new(name)), None, "{name}");
        }
    }

    #[test]
    fn db_round_trips() {
        assert_eq!(
            MediaKind::from_db(MediaKind::Image.to_db()),
            Some(MediaKind::Image)
        );
        assert_eq!(MediaKind::from_db(42), None);
        for s in [ThumbState::Pending, ThumbState::Ready, ThumbState::Failed] {
            assert_eq!(ThumbState::from_db(s.to_db()), s);
        }
    }

    #[test]
    fn video_round_trips_through_the_database_value() {
        assert_eq!(MediaKind::Video.to_db(), 1);
        assert_eq!(MediaKind::from_db(1), Some(MediaKind::Video));
        assert_eq!(MediaKind::from_db(0), Some(MediaKind::Image));
        assert_eq!(MediaKind::from_db(2), None);
    }

    #[test]
    fn video_serialises_as_the_ui_expects() {
        assert_eq!(
            serde_json::to_string(&MediaKind::Video).unwrap(),
            "\"video\""
        );
    }

    #[test]
    fn fingerprint_changes_with_every_input() {
        let base = fingerprint("/p/a.jpg", 100, 1_000);
        assert_eq!(base, fingerprint("/p/a.jpg", 100, 1_000));
        assert_ne!(base, fingerprint("/p/b.jpg", 100, 1_000));
        assert_ne!(base, fingerprint("/p/a.jpg", 101, 1_000));
        assert_ne!(base, fingerprint("/p/a.jpg", 100, 1_001));
    }
}
