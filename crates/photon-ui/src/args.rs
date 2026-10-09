//! The command line: where the library is, and how the window opens.

use crate::dirs::{self, Dirs};
use std::path::PathBuf;

pub const USAGE: &str = "usage: photon-native [--data-dir DIR] [--cache-dir DIR] [--fullscreen]

  --data-dir DIR    the directory that holds library.db
  --cache-dir DIR   the directory that holds thumbs/
  --fullscreen      open fullscreen

Without --data-dir and --cache-dir the library the Tauri photon opens is opened.";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub data_dir: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
    pub fullscreen: bool,
}

impl Args {
    /// The arguments after the program's name. An error is the line to print before the
    /// usage.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut parsed = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let mut value = |name: &str| {
                args.next()
                    .map(PathBuf::from)
                    .ok_or_else(|| format!("{name} needs a directory"))
            };
            match arg.as_str() {
                "--data-dir" => parsed.data_dir = Some(value("--data-dir")?),
                "--cache-dir" => parsed.cache_dir = Some(value("--cache-dir")?),
                "--fullscreen" => parsed.fullscreen = true,
                other => return Err(format!("unknown argument {other}")),
            }
        }
        // One without the other would open one application's library with another's
        // thumbnails: every key would miss, and the cache would fill with a second copy.
        if parsed.data_dir.is_some() != parsed.cache_dir.is_some() {
            return Err("--data-dir and --cache-dir go together".to_owned());
        }
        Ok(parsed)
    }

    /// Where the library is: the two directories named, or the standard ones.
    pub fn dirs(&self) -> Option<Dirs> {
        match (&self.data_dir, &self.cache_dir) {
            (Some(data), Some(cache)) => Some(dirs::within(data, cache)),
            _ => dirs::standard(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn parse(args: &[&str]) -> Result<Args, String> {
        Args::parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn no_arguments_is_the_standard_library_in_a_window() {
        let args = parse(&[]).unwrap();
        assert_eq!(args, Args::default());
        assert_eq!(args.dirs(), dirs::standard());
    }

    #[test]
    fn the_two_directories_name_another_library() {
        let args = parse(&[
            "--data-dir",
            "/x/data",
            "--cache-dir",
            "/x/cache",
            "--fullscreen",
        ])
        .unwrap();
        assert!(args.fullscreen);
        let dirs = args.dirs().unwrap();
        assert_eq!(dirs.db_path, Path::new("/x/data/library.db"));
        assert_eq!(dirs.cache_dir, Path::new("/x/cache/thumbs"));
    }

    #[test]
    fn one_directory_without_the_other_is_refused() {
        assert_eq!(
            parse(&["--data-dir", "/x/data"]),
            Err("--data-dir and --cache-dir go together".to_owned())
        );
        assert!(parse(&["--cache-dir", "/x/cache"]).is_err());
    }

    #[test]
    fn a_directory_left_out_and_an_unknown_argument_are_errors() {
        assert_eq!(
            parse(&["--data-dir"]),
            Err("--data-dir needs a directory".to_owned())
        );
        assert_eq!(
            parse(&["--frobnicate"]),
            Err("unknown argument --frobnicate".to_owned())
        );
    }
}
