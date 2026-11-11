//! A closed pipe on the way out (`jetcarp --json quickstart | head`) ends the program quietly, as
//! it ends other command-line tools, instead of with a panic or an error message.
//!
//! Rust ignores SIGPIPE, so writing to a pipe whose reader has gone fails with a broken-pipe
//! error: `println!` panics with it and a `write!` returns it. Both are caught here once for the
//! whole program, rather than in each command: [`install`] adds a panic hook that turns the
//! printing panic into a quiet exit, and `main` passes its error through [`is_broken`]. SIGPIPE
//! itself is left ignored, since the same program is also the app, the background host and the MCP
//! server, whose pipes and sockets must not end the process.

use std::io::ErrorKind;
use std::process;

/// What a shell reports for a program ended by SIGPIPE (128 + 13), the usual way a command-line
/// tool ends when its output is closed. Windows has no such convention; the same number is used.
pub const STATUS: i32 = 141;

/// The texts of this system's broken-pipe errors, as a failed `println!` quotes them: EPIPE on
/// Unix, and on Windows ERROR_BROKEN_PIPE and ERROR_NO_DATA ("The pipe is being closed").
fn broken_texts() -> Vec<String> {
    [32, 109, 232]
        .into_iter()
        .map(std::io::Error::from_raw_os_error)
        .filter(|e| e.kind() == ErrorKind::BrokenPipe)
        .map(|e| e.to_string())
        .collect()
}

/// Whether a panic message is `println!`'s (or `eprintln!`'s) failure on a closed pipe.
fn is_print_failure(msg: &str) -> bool {
    msg.starts_with("failed printing to std") && broken_texts().iter().any(|t| msg.ends_with(t))
}

/// Ends the program quietly once a printing panic turns out to be a closed pipe; every other
/// panic goes to the default hook as before.
pub fn install() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let p = info.payload();
        let msg = p
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| p.downcast_ref::<&str>().copied())
            .unwrap_or("");
        if is_print_failure(msg) {
            process::exit(STATUS);
        }
        default(info);
    }));
}

/// Whether `e` (or anything it was caused by) is a broken-pipe error.
pub fn is_broken(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        c.downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == ErrorKind::BrokenPipe)
            || c.downcast_ref::<serde_json::Error>()
                .and_then(|j| j.io_error_kind())
                .is_some_and(|k| k == ErrorKind::BrokenPipe)
    })
}

/// Ends the program as a closed pipe ends it.
pub fn exit() -> ! {
    process::exit(STATUS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_pipe_is_told_from_other_failures() {
        let epipe = std::io::Error::from(ErrorKind::BrokenPipe);
        let texts = broken_texts();
        assert!(!texts.is_empty(), "this system has a broken-pipe error");
        for t in &texts {
            assert!(is_print_failure(&format!("failed printing to stdout: {t}")));
        }
        assert!(!is_print_failure(
            "failed printing to stdout: No space left on device (os error 28)"
        ));
        assert!(!is_print_failure("index out of bounds"));
        let e = anyhow::Error::new(epipe).context("writing the report");
        assert!(is_broken(&e));
        assert!(!is_broken(&anyhow::anyhow!("no such file")));
        let disk = std::io::Error::other("disk full");
        assert!(!is_broken(&anyhow::Error::new(disk)));
    }
}
