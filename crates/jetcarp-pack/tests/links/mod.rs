//! Links for the tests of the folder rules, on every system. On Unix a link is a symbolic link. On
//! Windows a link to a folder is a directory junction, which anyone may make, or a directory
//! symbolic link; a link to a file is a file symbolic link. Symbolic links need Developer Mode or
//! administrator rights on Windows.
//!
//! A case whose link cannot be made is skipped, and says so on standard error with the reason
//! (`skipped: …`); with `JETCARP_TEST_LINKS=all`, as on CI, it fails instead. A case that ran
//! says `ran: …`, so a run with `--nocapture` shows which cases ran.
//!
//! The same file is in `jetcarp-mcp/tests/links` and `jetcarp-pack/tests/links`: keep the two alike.

#![allow(dead_code)]

use std::path::Path;

/// A kind of link to a folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A symbolic link.
    Symlink,
    /// A directory junction (Windows only).
    Junction,
}

impl Kind {
    /// The kinds of link to a folder this system has.
    pub fn for_folders() -> &'static [Kind] {
        if cfg!(windows) {
            &[Kind::Junction, Kind::Symlink]
        } else {
            &[Kind::Symlink]
        }
    }

    /// A short name, for file names and messages.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Symlink => "symlink",
            Kind::Junction => "junction",
        }
    }
}

/// Take away whatever link is at `link` (a link to a file or to a folder), leaving its target.
fn clear(link: &Path) {
    let _ = std::fs::remove_file(link);
    if std::fs::symlink_metadata(link).is_ok_and(|m| m.file_type().is_symlink()) {
        // A Windows link to a folder is removed as a folder; the folder it leads to is not.
        let _ = std::fs::remove_dir(link);
    }
}

/// A link of `kind` at `link` to the folder `target`, which need not exist, made afresh.
pub fn folder(kind: Kind, target: &Path, link: &Path) -> Result<(), String> {
    clear(link);
    match kind {
        #[cfg(unix)]
        Kind::Symlink => std::os::unix::fs::symlink(target, link).map_err(|e| e.to_string()),
        #[cfg(windows)]
        Kind::Symlink => std::os::windows::fs::symlink_dir(target, link).map_err(|e| e.to_string()),
        #[cfg(windows)]
        Kind::Junction => {
            use std::os::windows::process::CommandExt;
            let out = std::process::Command::new("cmd")
                .raw_arg(format!(
                    "/C mklink /J \"{}\" \"{}\"",
                    link.display(),
                    target.display()
                ))
                .output()
                .map_err(|e| format!("cmd /C mklink /J: {e}"))?;
            if out.status.success() {
                Ok(())
            } else {
                Err(format!(
                    "mklink /J: {}{}",
                    String::from_utf8_lossy(&out.stdout).trim(),
                    String::from_utf8_lossy(&out.stderr).trim()
                ))
            }
        }
        #[allow(unreachable_patterns)]
        _ => Err(format!("no {} on this system", kind.name())),
    }
}

/// A symbolic link at `link` to the file `target`, which need not exist, made afresh.
pub fn file(target: &Path, link: &Path) -> Result<(), String> {
    clear(link);
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, link).map_err(|e| e.to_string());
    #[cfg(windows)]
    return std::os::windows::fs::symlink_file(target, link).map_err(|e| e.to_string());
    #[allow(unreachable_code)]
    Err("no symbolic links on this system".into())
}

/// The link for `case` could not be made (`why`): a skip, said on standard error, or under
/// `JETCARP_TEST_LINKS=all` a failure.
pub fn skip(case: &str, why: &str) {
    if std::env::var("JETCARP_TEST_LINKS").is_ok_and(|v| v == "all") {
        panic!("{case}: could not make the link ({why}), and JETCARP_TEST_LINKS=all");
    }
    eprintln!("skipped: {case}: could not make the link ({why})");
}

/// `case` ran to its end.
pub fn ran(case: &str) {
    eprintln!("ran: {case}");
}
