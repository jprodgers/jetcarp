//! Scratch folders for JETCARP's own tests. Not part of its API: hidden from the documentation,
//! and free to change in any release.
//!
//! A test that makes a folder in the system's temp folder removes it again when it ends, also when
//! it fails, through [`ScratchDir`]'s `Drop`. Windows can't remove a file that is still open, such
//! as a library's database, so a `ScratchDir` is declared before anything that opens files in it:
//! Rust drops a scope's variables in the reverse of their order, so the folder goes last (#121).

use std::path::{Path, PathBuf};

/// A folder `jetcarp-<name>-<process id>` in the system's temp folder, emptied of anything an
/// earlier run left there, and removed with everything in it when this is dropped.
pub struct ScratchDir(PathBuf);

impl ScratchDir {
    /// The folder's path, after removing whatever was there. The folder itself is not made: some
    /// tests check what happens when it is missing, and most make it by opening something there.
    pub fn new(name: &str) -> ScratchDir {
        let path = std::env::temp_dir().join(format!("jetcarp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        ScratchDir(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl std::ops::Deref for ScratchDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for ScratchDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        // A file another thread or program has only just closed (SQLite's, a virus scanner's
        // look at a new file) can hold the folder a moment longer on Windows: try for a while.
        for _ in 0..50 {
            match std::fs::remove_dir_all(&self.0) {
                Ok(()) => return,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
            }
        }
        eprintln!("could not remove the test folder {}", self.0.display());
    }
}

#[cfg(test)]
mod tests {
    use super::ScratchDir;

    #[test]
    fn the_folder_goes_when_the_test_ends_and_when_it_fails() {
        let path = {
            let d = ScratchDir::new("scratch-self");
            std::fs::create_dir_all(d.join("a/b")).unwrap();
            std::fs::write(d.join("a/b/c.txt"), "x").unwrap();
            d.to_path_buf()
        };
        assert!(!path.exists(), "removed at the end of its scope");

        let failed = std::panic::catch_unwind(|| {
            let d = ScratchDir::new("scratch-panic");
            std::fs::create_dir_all(&*d).unwrap();
            std::fs::write(d.join("f"), "x").unwrap();
            panic!("a failing test");
        });
        assert!(failed.is_err());
        let left =
            std::env::temp_dir().join(format!("jetcarp-scratch-panic-{}", std::process::id()));
        assert!(!left.exists(), "removed although the test failed");
    }
}
