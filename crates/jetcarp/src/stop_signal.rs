//! The signals that stop a command that runs until it is stopped (`jetcarp search`, `jetcarp jobs
//! resume`, `jetcarp script`): SIGTERM, SIGHUP and Ctrl-C each stop it as Ctrl-C did, at its next
//! checkpoint or step, and its record in the memory pool goes as the program ends (#278). Before,
//! SIGTERM and SIGHUP ended it at once, a search's job left marked running with nothing running
//! it. Linux and macOS go through `jetcarp_core::quit_signal`, as the app, the host and the MCP
//! server do; Windows has only Ctrl-C, through `ctrlc`. Never both in one process.

/// From now on, the first of SIGTERM, SIGHUP and Ctrl-C runs `stop` with the signal's name
/// (`"SIGTERM"`; `"SIGINT"` for Ctrl-C). A second ends the program at once: with 128 + its
/// number on Linux and macOS, and with 130 (its memory record removed) on Windows. Once per
/// process: a later call does nothing and returns `false`.
pub(crate) fn on_stop(stop: impl FnOnce(&'static str) + Send + 'static) -> bool {
    #[cfg(unix)]
    {
        jetcarp_core::quit_signal::on_signal(stop)
    }
    #[cfg(windows)]
    {
        use std::sync::atomic::{AtomicU32, Ordering};
        let presses = AtomicU32::new(0);
        let stop = std::sync::Mutex::new(Some(stop));
        ctrlc::set_handler(move || {
            if presses.fetch_add(1, Ordering::SeqCst) == 0 {
                if let Some(f) = stop.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    f("SIGINT");
                }
            } else {
                jetcarp_core::machine::pool::exit(130);
            }
        })
        .is_ok()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = stop;
        false
    }
}

/// The status a command a signal stopped ends with, as a shell gives a program the signal ended:
/// 128 + its number (130 for Ctrl-C).
pub(crate) fn status(sig: &str) -> i32 {
    match sig {
        "SIGHUP" => 128 + 1,
        "SIGTERM" => 128 + 15,
        _ => 128 + 2,
    }
}

/// The signal as the person knows it: `Ctrl-C` for SIGINT.
pub(crate) fn shown(sig: &str) -> &str {
    if sig == "SIGINT" { "Ctrl-C" } else { sig }
}

/// What quits at once, while the command stops.
pub(crate) fn again(sig: &str) -> &'static str {
    if sig == "SIGINT" {
        "press Ctrl-C again to quit now"
    } else {
        "a second signal quits now"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stopped_command_ends_as_a_shell_says_the_signal_ended_it() {
        assert_eq!(status("SIGINT"), 130);
        assert_eq!(status("SIGTERM"), 143);
        assert_eq!(status("SIGHUP"), 129);
        assert_eq!(shown("SIGINT"), "Ctrl-C");
        assert_eq!(shown("SIGTERM"), "SIGTERM");
    }
}
