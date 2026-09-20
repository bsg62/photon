//! photon-core: headless library, scanning and thumbnail engine for photon.

pub mod decode;
pub mod duplicates;
pub mod edit;
pub mod error;
pub mod export;
pub mod grid;
pub mod iptc;
pub mod keywords;
pub mod library;
pub mod media;
pub mod metadata;
pub mod paths;
pub mod picasa;
pub mod scanner;
pub mod search;
pub mod thumbs;
pub mod watcher;
pub mod xmp;

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
