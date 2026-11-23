//! Windows: a sign-out, a shutdown and `taskkill` without `/F` stop a command and the background
//! host as SIGTERM does on Linux and macOS (#319): a foreground `jetcarp search` at its next
//! checkpoint, paused and resumable, and the host as `jetcarp host stop` stops it, each removing
//! its endpoint file and its record in the memory pool. Before, Windows ended them at once.
//!
//! `jetcarp.exe` loads `user32.dll`, so Windows tells it of a sign-out or a shutdown only through
//! its windows (`WM_QUERYENDSESSION`, then `WM_ENDSESSION`), as it tells every program; a hidden
//! window of its own hears them (`jetcarp_core::quit_signal`). These tests find that window, as
//! Windows does, among the top-level windows of the process they started, and send it what
//! Windows sends at a sign-out. `taskkill` without `/F` sends a program's windows `WM_CLOSE`;
//! the tests run the real `taskkill`. Closing a console (`CTRL_CLOSE_EVENT`) cannot be sent to
//! another process: the `jetcarp` program's own tests hand it to a search as Windows does
//! (`stop_signal.rs`).
//!
//! Each test runs the real program, with every folder a scratch one under the target folder, and
//! sends its messages only to the process it started. The host runs headless.
#![cfg(windows)]

use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    ENDSESSION_LOGOFF, FindWindowExW, GetWindowThreadProcessId, SMTO_NORMAL, SendMessageTimeoutW,
    WM_ENDSESSION, WM_QUERYENDSESSION,
};

/// A scratch home for one test, its photosensitive seizure warning answered (#96), as on a
/// computer where JETCARP has been set up. Removed when dropped.
struct Home(PathBuf);

impl Home {
    fn new(name: &str) -> Home {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("jetcarp-session-end-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("config")).unwrap();
        jetcarp_core::photosensitivity::record_in(
            &dir.join("config"),
            jetcarp_core::photosensitivity::Answer::NoDisorder,
            "test",
        )
        .unwrap();
        Home(dir)
    }

    fn runtime(&self) -> PathBuf {
        self.0.join("run")
    }

    /// Process `pid`'s record in the memory pool.
    fn record(&self, pid: u32) -> PathBuf {
        self.runtime()
            .join(jetcarp_core::machine::pool::DIR)
            .join(format!("{pid}.json"))
    }

    /// `jetcarp args`, every folder it uses in here.
    fn jetcarp(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_jetcarp"));
        c.args(args)
            .env("JETCARP_CONFIG_DIR", self.0.join("config"))
            .env("JETCARP_DATA_DIR", self.0.join("data"))
            .env("JETCARP_LIBRARY", self.0.join("data/library.sqlite"))
            .env("JETCARP_OUT", self.0.join("out"))
            .env("JETCARP_RUNTIME_DIR", self.runtime())
            .env("JETCARP_HOST_NO_PAUSE_RULES", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        c
    }

    /// `jetcarp search hunt --forever`, on the processor.
    fn search(&self) -> Running {
        let mut c = self.jetcarp(&[
            "search",
            "hunt",
            "--forever",
            "--label",
            "until-signed-out",
            "--threads",
            "2",
        ]);
        c.env("JETCARP_GPU", "off");
        Running::start(c)
    }

    /// The headless host, on a small canvas, following its own GPU setting.
    fn host(&self) -> Running {
        let mut c = self.jetcarp(&["host", "serve", "--headless", "--canvas", "160x120"]);
        c.env_remove("JETCARP_GPU");
        Running::start(c)
    }

    /// Job `job` as `jetcarp jobs status JOB --json` shows it.
    fn job(&self, job: i64) -> Value {
        self.job_if_there(job)
            .unwrap_or_else(|e| panic!("jobs status {job}: {e}"))
    }

    /// The same, or what `jetcarp jobs status` said instead (before the job is made).
    fn job_if_there(&self, job: i64) -> Result<Value, String> {
        let out = self
            .jetcarp(&["jobs", "status", &job.to_string(), "--json"])
            .stdout(Stdio::piped())
            .output()
            .unwrap();
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).into_owned());
        }
        serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A program a test started: what it says on standard error as it comes, and killed (by the
/// process id it was started with) if the test ends first.
struct Running {
    child: Child,
    lines: Receiver<String>,
    said: Vec<String>,
}

impl Running {
    fn start(mut c: Command) -> Running {
        let mut child = c.spawn().expect("jetcarp starts");
        let err = child.stderr.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for l in BufReader::new(err).lines().map_while(Result::ok) {
                let _ = tx.send(l);
            }
        });
        Running {
            child,
            lines,
            said: vec![],
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Everything it has said on standard error so far.
    fn stderr(&mut self) -> String {
        while let Ok(l) = self.lines.try_recv() {
            self.said.push(l);
        }
        self.said.join("\n")
    }

    /// Waits up to a minute for `ready`, failing with what it said if it does not come.
    fn wait_until(&mut self, what: &str, ready: impl Fn() -> bool) {
        let t0 = Instant::now();
        while !ready() {
            if let Ok(Some(s)) = self.child.try_wait() {
                panic!(
                    "it ended ({s:?}) before {what}; its stderr:\n{}",
                    self.stderr()
                );
            }
            if t0.elapsed() > Duration::from_secs(60) {
                panic!("no {what} after a minute; its stderr:\n{}", self.stderr());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Its hidden session window, as Windows finds it among the process's top-level windows.
    fn session_window(&mut self) -> HWND {
        let class: Vec<u16> = jetcarp_core::quit_signal::SESSION_WINDOW_CLASS
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let pid = self.pid();
        let found = std::cell::Cell::new(0isize);
        self.wait_until("session window", || {
            let mut w: HWND = std::ptr::null_mut();
            loop {
                // SAFETY: plain calls; `class` outlives them.
                w = unsafe {
                    FindWindowExW(std::ptr::null_mut(), w, class.as_ptr(), std::ptr::null())
                };
                if w.is_null() {
                    return false;
                }
                let mut owner = 0u32;
                unsafe { GetWindowThreadProcessId(w, &mut owner) };
                if owner == pid {
                    found.set(w as isize);
                    return true;
                }
            }
        });
        found.get() as HWND
    }

    /// Tells it the session ends, as Windows tells every program's windows at a sign-out: may
    /// the session end (it must say yes), then that it does, which its window answers only once
    /// the program has ended itself (on a thread of its own, here).
    fn sign_out(&mut self) {
        let w = self.session_window() as isize;
        let logoff = ENDSESSION_LOGOFF as isize;
        let mut may = 0usize;
        // SAFETY: plain calls, to a window of the process this test started.
        let sent = unsafe {
            SendMessageTimeoutW(
                w as HWND,
                WM_QUERYENDSESSION,
                0,
                logoff,
                SMTO_NORMAL,
                5_000,
                &mut may,
            )
        };
        assert_ne!(sent, 0, "WM_QUERYENDSESSION was not answered");
        assert_eq!(may, 1, "the program would not let the session end");
        std::thread::spawn(move || {
            let mut r = 0usize;
            unsafe {
                SendMessageTimeoutW(
                    w as HWND,
                    WM_ENDSESSION,
                    1,
                    logoff,
                    SMTO_NORMAL,
                    60_000,
                    &mut r,
                )
            };
        });
    }

    /// `taskkill /PID pid`, without `/F`: Windows' own tool asks its windows to close.
    fn taskkill(&mut self) {
        let _ = self.session_window();
        let out = Command::new("taskkill")
            .args(["/PID", &self.pid().to_string()])
            .output()
            .expect("taskkill runs");
        assert!(
            out.status.success(),
            "taskkill /PID {}: {}{}",
            self.pid(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Waits up to half a minute for it to end; how it ended, and what it said.
    fn ended(&mut self) -> (ExitStatus, String) {
        let t0 = Instant::now();
        let status = loop {
            if let Some(s) = self.child.try_wait().unwrap() {
                break Some(s);
            }
            if t0.elapsed() > Duration::from_secs(30) {
                break None;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        // What it said as it went, once its end of the pipe has closed.
        std::thread::sleep(Duration::from_millis(100));
        let err = self.stderr();
        let status =
            status.unwrap_or_else(|| panic!("still running after 30 s; its stderr:\n{err}"));
        (status, err)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// A search started in `home`, once its job has made a checkpoint.
fn search_at_a_checkpoint(home: &Home) -> (Running, PathBuf) {
    let mut search = home.search();
    let record = home.record(search.pid());
    search.wait_until("memory record", || record.exists());
    search.wait_until("a checkpoint", || {
        home.job_if_there(1)
            .is_ok_and(|v| v["done"].as_u64().unwrap_or(0) > 0)
    });
    (search, record)
}

/// What a search stopped at its checkpoint leaves: its job paused and resumable, its memory
/// record gone, and an end with 0, as after Ctrl-C.
fn stopped_at_its_checkpoint(home: &Home, record: &Path, status: ExitStatus, err: &str) {
    assert!(
        err.contains("SIGTERM: stopping at the next checkpoint"),
        "it says it is stopping; its stderr:\n{err}"
    );
    assert!(
        status.success(),
        "the search ends as Ctrl-C ends it, with 0: {status:?}; its stderr:\n{err}"
    );
    assert!(
        !record.exists(),
        "the search left its memory record {}; its stderr:\n{err}",
        record.display()
    );
    let v = home.job(1);
    assert_eq!(v["status"], "paused", "the job is paused: {v}");
    assert_eq!(v["stale"], false, "{v}");
    assert_eq!(v["resumable"], true, "{v}");
}

/// A sign-out stops a foreground `jetcarp search` at its next checkpoint, as SIGTERM does on
/// Linux and macOS.
#[test]
fn signing_out_stops_a_search_at_its_checkpoint() {
    let home = Home::new("search");
    let (mut search, record) = search_at_a_checkpoint(&home);
    search.sign_out();
    let (status, err) = search.ended();
    stopped_at_its_checkpoint(&home, &record, status, &err);
}

/// `taskkill` without `/F` stops a foreground `jetcarp search` at its next checkpoint, as `kill`
/// does on Linux and macOS. Before, it refused: a console program with no window could only be
/// ended with `/F`.
#[test]
fn taskkill_stops_a_search_at_its_checkpoint() {
    let home = Home::new("taskkill");
    let (mut search, record) = search_at_a_checkpoint(&home);
    search.taskkill();
    let (status, err) = search.ended();
    stopped_at_its_checkpoint(&home, &record, status, &err);
}

/// A sign-out stops the background host as `jetcarp host stop` does: it ends with 0, its link
/// closed, so `host-endpoint.json` is gone from the runtime folder, and its record in the memory
/// pool with it.
#[test]
fn signing_out_stops_the_host_as_host_stop_does() {
    let home = Home::new("host");
    let file = home.runtime().join("host-endpoint.json");
    let mut host = home.host();
    let record = home.record(host.pid());
    host.wait_until("endpoint file and memory record", || {
        file.exists() && record.exists()
    });
    host.sign_out();
    let (status, err) = host.ended();
    assert!(
        !file.exists(),
        "the sign-out left {} behind (the host ended with {status:?}); its stderr:\n{err}",
        file.display()
    );
    assert!(
        status.success(),
        "the host ends as `jetcarp host stop` ends it, with 0: {status:?}; its stderr:\n{err}"
    );
    assert!(
        !record.exists(),
        "the host left its memory record {}; its stderr:\n{err}",
        record.display()
    );
}
