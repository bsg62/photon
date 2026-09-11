//! photon-core: headless library, scanning and thumbnail engine for photon.

pub mod decode;
pub mod error;
pub mod library;
pub mod media;
pub mod metadata;
pub mod thumbs;

#[cfg(test)]
mod testutil;

pub use error::{Error, Result};

/// Current wall-clock time in milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
