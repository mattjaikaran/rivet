//! Shared scratch-directory fixture for `rivet-cli` tests.
//!
//! Tests that exercise the compiler write scratch projects into the system
//! temp dir — and some compile a full generated crate inside them (`rivet
//! /plan`, `rivet build`), hundreds of megabytes per fixture. Hand-rolled
//! `temp_dir().join(...)` helpers leak those directories: they unlink the
//! path only at the very end of the test body, so a panicking test leaves
//! the whole tree behind, and a per-process name adds a fresh stale copy on
//! every run.
//!
//! [`ScratchDir`] removes its directory in [`Drop`], so cleanup runs on the
//! success path and while a panic unwinds, and it names the directory
//! without the process id, so a rerun reuses the same path instead of
//! piling up new ones.

use std::ops::Deref;
use std::path::{Path, PathBuf};

/// A guard for a scratch directory under the system temp dir.
///
/// The directory lives at `<temp_dir>/rivet-<name>`. Pick a `name` unique
/// among tests that can run in parallel: each test must own its directory.
pub(crate) struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    /// Create a clean `<temp_dir>/rivet-<name>`, replacing any leftover from
    /// an earlier run.
    pub(crate) fn new(name: &str) -> ScratchDir {
        let path = std::env::temp_dir().join(format!("rivet-{name}"));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create scratch dir");
        ScratchDir { path }
    }

    /// The directory path.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Deref for ScratchDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for ScratchDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_a_clean_directory() {
        let dir = ScratchDir::new("test-support-clean");
        assert!(dir.is_dir(), "the guard creates its directory");
        std::fs::write(dir.join("stale"), "leftover").expect("write a leftover file");
        drop(dir);

        let dir = ScratchDir::new("test-support-clean");
        assert!(dir.is_dir(), "the guard recreates its directory");
        assert!(!dir.join("stale").exists(), "new() clears leftovers");
    }

    #[test]
    fn drop_removes_the_directory() {
        let path;
        {
            let dir = ScratchDir::new("test-support-drop");
            path = dir.path().to_path_buf();
            assert!(path.is_dir(), "the guard creates its directory");
        }
        assert!(!path.exists(), "dropping the guard removes the directory");
    }
}
