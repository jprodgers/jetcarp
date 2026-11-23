//! The signals that stop a command that runs until it is stopped (`jetcarp search`, `jetcarp jobs
//! resume`, `jetcarp script`): SIGTERM, SIGHUP and Ctrl-C each stop it as Ctrl-C did, at its next
//! checkpoint or step, and its record in the memory pool goes as the program ends (#278). Before,
//! SIGTERM and SIGHUP ended it at once, a search's job left marked running with nothing running
//! it. Every system goes through `jetcarp_core::quit_signal`, as the app, the host and the MCP
//! server do. On Windows that takes Ctrl-C and Ctrl-Break (as SIGINT), the console closing (as
//! SIGHUP), and a sign-out, a shutdown or `taskkill` without `/F` (as SIGTERM), waiting for the
//! command to stop within the time Windows allows (#319); before, only Ctrl-C and Ctrl-Break
//! did, through the `ctrlc` crate, and closing the console ended a search at once.

/// From now on, the first of SIGTERM, SIGHUP and Ctrl-C (on Windows, the events that come as
/// them) runs `stop` with the signal's name (`"SIGTERM"`; `"SIGINT"` for Ctrl-C). A second ends
/// the program at once, with 128 + its number (130 for Ctrl-C). Once per process: a later call
/// does nothing and returns `false`.
pub(crate) fn on_stop(stop: impl FnOnce(&'static str) + Send + 'static) -> bool {
    jetcarp_core::quit_signal::on_signal(stop)
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
    use jetcarp_api::op::{Op, Quiet};
    use jetcarp_api::ops::jobs::{JobsStatus, StatusIn};
    use serde_json::{Value, json};
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    #[test]
    fn a_stopped_command_ends_as_a_shell_says_the_signal_ended_it() {
        assert_eq!(status("SIGINT"), 130);
        assert_eq!(status("SIGTERM"), 143);
        assert_eq!(status("SIGHUP"), 129);
        assert_eq!(shown("SIGINT"), "Ctrl-C");
        assert_eq!(shown("SIGTERM"), "SIGTERM");
    }

    /// Set in the twin's process by the test.
    const CHILD: &str = "JETCARP_STOP_SIGNAL_TEST_CHILD";
    /// What the twin ends with when the console's handler returns: Windows ends the process then.
    const ENDED_BY_WINDOWS: i32 = 58;

    /// Job `job` in the library `library` as `jetcarp jobs status JOB --json` shows it.
    fn job_view(library: Option<&Path>, job: i64) -> Option<Value> {
        let cx = crate::platform::ctx().with_library(library.map(Path::to_path_buf));
        let i: StatusIn = serde_json::from_value(json!({ "job": job })).unwrap();
        let out = JobsStatus::run(&cx, i, &Quiet).ok()?;
        serde_json::to_value(out).ok()
    }

    /// Windows' console closing (#319) stops a foreground `jetcarp search` as Ctrl-C and SIGHUP
    /// do: its job stops at its next checkpoint, paused and resumable, and the program ends with
    /// 0, its memory record gone. The close comes as Windows sends it, on a thread of its own,
    /// through the console's handler (`jetcarp_core::quit_signal::console_event`), and Windows
    /// ends the process as soon as that handler returns: before, nothing took the close, and
    /// Windows ended the search at once, its job left marked running and its record left behind.
    #[test]
    fn a_closed_console_stops_a_search_at_its_checkpoint() {
        let root = jetcarp_core::scratch::ScratchDir::new("closed-console");
        std::fs::create_dir_all(root.join("config")).unwrap();
        let library = root.join("data").join("library.sqlite");
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args([
            "stop_signal::tests::child_searches_until_its_console_closes",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ]);
        for (var, sub) in [
            ("JETCARP_CONFIG_DIR", "config"),
            ("JETCARP_DATA_DIR", "data"),
            ("JETCARP_OUT", "out"),
            ("JETCARP_RUNTIME_DIR", "run"),
        ] {
            cmd.env(var, root.join(sub));
        }
        let out = cmd
            .env("JETCARP_LIBRARY", &library)
            .env(CHILD, "1")
            .env("JETCARP_GPU", "off")
            .env_remove("JETCARP_REQUIRE_GPU")
            .stdin(Stdio::null())
            .output()
            .expect("start the twin");
        let said = format!(
            "its stdout:\n{}\nits stderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let pid = String::from_utf8_lossy(&out.stdout)
            .lines()
            .find_map(|l| l.split_once("twin pid: ")?.1.trim().parse::<u32>().ok())
            .unwrap_or_else(|| panic!("the twin did not start; {said}"));
        assert!(
            said.contains("SIGHUP: stopping at the next checkpoint"),
            "the search did not say it was stopping; {said}"
        );
        let record = root
            .join("run")
            .join(jetcarp_core::machine::pool::DIR)
            .join(format!("{pid}.json"));
        assert!(
            !record.exists(),
            "the search left its memory record {}; {said}",
            record.display()
        );
        let v = job_view(Some(&library), 1).expect("job 1");
        assert_eq!(v["status"], "paused", "the job is paused: {v}");
        assert_eq!(v["stale"], false, "{v}");
        assert_eq!(v["resumable"], true, "{v}");
        assert!(v["done"].as_u64().unwrap_or(0) > 0, "{v}");
        assert!(
            !said.contains("the handler returned"),
            "the console's handler returned before the search had stopped, so Windows ended it; \
             {said}"
        );
        assert_eq!(
            out.status.code(),
            Some(0),
            "the search ends as Ctrl-C ends it, with 0; {said}"
        );
    }

    /// The twin: `jetcarp search hunt --forever`, run as `main` runs it, in folders of the
    /// test's own; once its job has made a checkpoint, its console closes, as Windows says so.
    #[test]
    #[ignore = "started by a_closed_console_stops_a_search_at_its_checkpoint"]
    fn child_searches_until_its_console_closes() {
        use clap::Parser;
        use std::io::Write;
        if std::env::var_os(CHILD).is_none() {
            return;
        }
        let settings = jetcarp_core::paths::config_dir().expect("JETCARP_CONFIG_DIR is set");
        jetcarp_core::photosensitivity::record_in(
            &settings,
            jetcarp_core::photosensitivity::Answer::NoDisorder,
            "test",
        )
        .unwrap();
        println!("twin pid: {}", std::process::id());
        let _ = std::io::stdout().flush();
        jetcarp_core::machine::pool::end_with(crate::end);
        std::thread::spawn(|| {
            let t0 = Instant::now();
            while job_view(None, 1).is_none_or(|v| v["done"].as_u64().unwrap_or(0) == 0) {
                if t0.elapsed() > Duration::from_secs(120) {
                    println!("no checkpoint in two minutes");
                    std::process::exit(3);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let t0 = Instant::now();
            jetcarp_core::quit_signal::console_event(jetcarp_core::quit_signal::CTRL_CLOSE_EVENT);
            println!(
                "the handler returned after ms: {}",
                t0.elapsed().as_millis()
            );
            let _ = std::io::stdout().flush();
            std::process::exit(ENDED_BY_WINDOWS);
        });
        let cli = crate::Cli::parse_from([
            "jetcarp",
            "search",
            "hunt",
            "--forever",
            "--label",
            "until-closed",
            "--threads",
            "2",
        ]);
        let r = crate::finish(crate::run_cli(cli));
        // As `main` ends, with the status it gives.
        std::process::exit(if r.is_ok() { 0 } else { 1 });
    }
}
