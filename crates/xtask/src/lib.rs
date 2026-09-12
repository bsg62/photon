pub mod checks;

// Re-export for binary usage
pub use checks::{
    Versions, check_versions, parse_cargo_version, parse_pkg_version, parse_tauri_version,
};
