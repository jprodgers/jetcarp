//! Runs the real host program with its own folders, and drives it over its link.

use serde_json::{Value, json};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Host {
    child: Child,
    home: PathBuf,
}

fn home(name: &str) -> PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-host-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("config/jetcarp/rotations")).unwrap();
    std::fs::create_dir_all(dir.join("run")).unwrap();
    dir
}

/// Points every folder the host uses into `home`. The `JETCARP_` ones are what Windows and macOS go
/// by; the XDG ones keep anything else on Linux in there too.
fn with_env(c: &mut Command, home: &Path) {
    c.env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("JETCARP_CONFIG_DIR", home.join("config/jetcarp"))
        .env("JETCARP_RUNTIME_DIR", home.join("run/jetcarp"))
        .env("JETCARP_LIBRARY", home.join("library"))
        .env("HOME", home)
        .env("JETCARP_HOST_NO_PAUSE_RULES", "1")
        .env_remove("JETCARP_GPU");
}

fn spawn(home: &Path, extra: &[&str]) -> Child {
    let mut c = Command::new(env!("CARGO_BIN_EXE_jetcarp"));
    c.args(["host", "serve", "--headless", "--canvas", "160x120"])
        .args(extra)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    with_env(&mut c, home);
    c.spawn().expect("the host starts")
}

/// Asks the host, from a helper process whose environment points at this test's folders, since
/// the link finds the host through those folders.
fn call_in(home: &Path, args: &Value) -> Result<Value, String> {
    let file = home.join("run/jetcarp/host-endpoint.json");
    let info: Value =
        serde_json::from_str(&std::fs::read_to_string(file).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let info = jetcarp_mcp::link::EndpointInfo {
        port: info["port"].as_u64().ok_or("no port")? as u16,
        token: info["token"].as_str().ok_or("no token")?.to_string(),
    };
    let mut c = jetcarp_mcp::link::AppClient::connect(Some(info))?;
    let r = c.call(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": jetcarp_mcp::link::HOST_TOOL, "arguments": args}}))?;
    let text = r["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string();
    if r["result"]["isError"] == true {
        return Err(text);
    }
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

impl Host {
    fn start(name: &str, rotation: Option<&str>) -> Host {
        let home = home(name);
        let extra: Vec<&str> = rotation.map_or(vec![], |r| vec!["--rotation", r]);
        let child = spawn(&home, &extra);
        let host = Host { child, home };
        host.wait(|v| v["running"] == true, "the host to answer");
        host
    }

    fn call(&self, args: Value) -> Result<Value, String> {
        call_in(&self.home, &args)
    }

    fn wait(&self, ok: impl Fn(&Value) -> bool, what: &str) -> Value {
        let t0 = Instant::now();
        let mut last = Value::Null;
        while t0.elapsed() < Duration::from_secs(60) {
            if let Ok(v) = self.call(json!({"action": "status"})) {
                if ok(&v) {
                    return v;
                }
                last = v;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("timed out waiting for {what}; last status {last}");
    }

    fn stage(v: &Value) -> &Value {
        &v["screens"][0]["stage"]
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

#[test]
fn runs_scenes_and_answers_its_link() {
    let mut h = Host::start("run", Some("Wolfram series"));
    let v = h.wait(
        |v| Host::stage(v)["generation"].as_u64() > Some(2),
        "generations",
    );
    let s = Host::stage(&v);
    assert_eq!(s["rotation"], "Wolfram series");
    assert_eq!(s["number"], 1);
    assert_eq!(s["count"], 256);
    assert_eq!(s["caption"][0], "Rule 0");
    assert!(
        s["caption"][1].as_str().unwrap().contains("Square grid"),
        "{s}"
    );

    let v = h.call(json!({"action": "next"})).unwrap();
    assert_eq!(Host::stage(&v)["building"], true);
    let v = h.wait(
        |v| Host::stage(v)["caption"][0] == "Rule 1",
        "the next rule",
    );
    assert_eq!(Host::stage(&v)["number"], 2);

    // Pausing freezes generations.
    let v = h.call(json!({"action": "pause"})).unwrap();
    assert_eq!(v["paused"], true);
    std::thread::sleep(Duration::from_millis(300));
    let a = Host::stage(&h.call(json!({"action": "status"})).unwrap())["generation"].clone();
    std::thread::sleep(Duration::from_millis(500));
    let b = Host::stage(&h.call(json!({"action": "status"})).unwrap())["generation"].clone();
    assert_eq!(a, b, "generations while paused");
    h.call(json!({"action": "resume"})).unwrap();
    h.wait(
        |v| Host::stage(v)["generation"].as_u64() > b.as_u64(),
        "generations after resuming",
    );

    assert!(
        h.call(json!({"action": "rotation", "name": "no such"}))
            .unwrap_err()
            .contains("no rotation")
    );
    assert!(
        h.call(json!({"action": "nonsense"}))
            .unwrap_err()
            .contains("unknown action")
    );

    // Changing the rotation is remembered, and quitting removes the endpoint.
    let v = h
        .call(json!({"action": "rotation", "name": "Classics, hourly"}))
        .unwrap();
    assert_eq!(Host::stage(&v)["rotation"], "Classics, hourly");
    let saved = std::fs::read_to_string(h.home.join("config/jetcarp/host.json")).unwrap();
    assert!(saved.contains("Classics, hourly"), "{saved}");
    // The answer to "quit" is written back before the host goes, rather than the caller seeing
    // the connection reset (as `jetcarp host stop` sometimes did on Windows).
    let bye = h.call(json!({"action": "quit"}));
    let ok = bye.as_ref().is_ok_and(|v| v["running"] == true);
    let status = h.child.wait().unwrap();
    // On failure, say how the host ended and what it printed, which a CI log otherwise lacks.
    let mut err = String::new();
    if let Some(mut e) = h.child.stderr.take() {
        let _ = e.read_to_string(&mut err);
    }
    assert!(
        ok,
        "answer to quit: {bye:?}; the host ended with {status:?}; its stderr:\n{err}"
    );
    assert!(status.success(), "{status:?}; its stderr:\n{err}");
    assert!(!h.home.join("run/jetcarp/host-endpoint.json").exists());
}

#[test]
fn only_one_host_runs() {
    let h = Host::start("single", Some("Wolfram series"));
    let second = spawn(&h.home, &[]).wait_with_output().unwrap();
    assert_eq!(second.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&second.stderr).contains("already running"));
    // The first is unharmed.
    assert!(h.call(json!({"action": "status"})).is_ok());
}

#[test]
fn a_crash_does_not_lock_the_next_host_out() {
    let mut h = Host::start("crash", Some("Wolfram series"));
    h.child.kill().unwrap();
    h.child.wait().unwrap();
    h.child = spawn(&h.home, &["--rotation", "Wolfram series"]);
    // The old endpoint file is still there until the new host replaces it.
    h.wait(|v| v["pid"] == h.child.id(), "the new host");
}

#[test]
fn bad_settings_fall_back_and_say_so() {
    let dir = home("bad");
    std::fs::write(dir.join("config/jetcarp/host.json"), "{ not json").unwrap();
    let child = spawn(&dir, &[]);
    let h = Host { child, home: dir };
    let v = h.wait(|v| v["running"] == true, "the host");
    assert!(v["log"].to_string().contains("using the defaults"), "{v}");
    assert_eq!(Host::stage(&v)["rotation"], "A classic a day");
}

#[test]
fn an_oversized_world_is_refused_and_the_rotation_moves_on() {
    let dir = home("big");
    let settings = json!({"max_cells": 500, "screens": [{"rotation": "Wolfram series"}]});
    std::fs::write(dir.join("config/jetcarp/host.json"), settings.to_string()).unwrap();
    let child = spawn(&dir, &[]);
    let h = Host { child, home: dir };
    let v = h.wait(|v| v["paused"] == true, "the host to give up");
    assert!(v["log"].to_string().contains("over the limit"), "{v}");
    assert!(v["log"].to_string().contains("can be shown"), "{v}");
    assert_eq!(Host::stage(&v)["cells"], 0);
}

#[test]
fn each_screen_runs_its_own_rotation_and_moves_alone() {
    let dir = home("two");
    let settings = json!({"screens": [
        {"rotation": "Wolfram series"},
        {"rotation": "Wolfram series"},
    ]});
    std::fs::write(dir.join("config/jetcarp/host.json"), settings.to_string()).unwrap();
    let child = spawn(&dir, &[]);
    let h = Host { child, home: dir };
    let both = |v: &Value| {
        v["screens"].as_array().is_some_and(|s| {
            s.len() == 2
                && s.iter()
                    .all(|x| x["stage"]["generation"].as_u64() > Some(0))
        })
    };
    h.wait(both, "both screens to run");
    h.call(json!({"action": "next", "screen": 1})).unwrap();
    let v = h.wait(
        |v| v["screens"][1]["stage"]["caption"][0] == "Rule 1",
        "the second screen to move",
    );
    assert_eq!(v["screens"][0]["stage"]["caption"][0], "Rule 0");
}

#[test]
fn the_extension_can_hold_the_scene_and_a_wallpaper_is_remembered() {
    let h = Host::start("hold", Some("Wolfram series"));
    let v = h.wait(|v| v["running"] == true, "the host");
    // Windows and macOS need no helper to show a wallpaper, so there it counts as present.
    assert_eq!(v["extension"], cfg!(any(windows, target_os = "macos")));
    assert_eq!(v["wallpaper"], false);

    // A heartbeat says nothing need be drawn: paused, but not by the user.
    let reply = h
        .call(json!({"action": "extension", "holds": {"covered": true, "dark": false}}))
        .unwrap();
    assert_eq!(reply["ok"], true);
    let v = h.wait(|v| Host::stage(v)["paused"] == true, "the hold to pause it");
    assert_eq!(v["held"][0], "covered");
    assert_eq!(v["paused"], false);
    assert_eq!(v["extension"], true);

    // Pausing by hand outlasts the hold.
    h.call(json!({"action": "pause"})).unwrap();
    h.call(json!({"action": "extension", "holds": {"covered": false}}))
        .unwrap();
    let v = h.call(json!({"action": "status"})).unwrap();
    assert_eq!(Host::stage(&v)["paused"], true);
    h.call(json!({"action": "resume"})).unwrap();
    h.wait(|v| Host::stage(v)["paused"] == false, "it to carry on");

    h.call(json!({"action": "wallpaper", "on": true})).unwrap();
    let v = h.call(json!({"action": "status"})).unwrap();
    assert_eq!(v["wallpaper"], true);
    assert_eq!(v["screens"][0]["place"], "wallpaper");
    let saved: Value = serde_json::from_str(
        &std::fs::read_to_string(h.home.join("config/jetcarp/host.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(saved["screens"][0]["place"], "wallpaper");
}

/// `jetcarp host start` with its output captured, as `| tee`, `$(...)` and CI steps do: the output
/// ends when the command does, not when the host it started stops, because the host gets none of
/// the command's handles.
#[test]
fn host_start_can_be_captured_and_returns() {
    use std::io::Read;
    let dir = home("start");
    let command = |args: &[&str]| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_jetcarp"));
        c.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        with_env(&mut c, &dir);
        c
    };
    /// Stops the host however the test ends.
    struct Stop(Command);
    impl Drop for Stop {
        fn drop(&mut self) {
            let _ = self.0.output();
        }
    }
    let mut start = command(&["host", "start", "--headless"]).spawn().unwrap();
    let _stop = Stop(command(&["host", "stop"]));
    let (mut out, mut err) = (start.stdout.take().unwrap(), start.stderr.take().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (mut o, mut e) = (String::new(), String::new());
        let _ = out.read_to_string(&mut o);
        let _ = err.read_to_string(&mut e);
        let _ = tx.send((o, e));
    });
    let (o, e) = rx
        .recv_timeout(Duration::from_secs(60))
        .expect("the output of `host start` ends with it (the host holds none of it)");
    assert!(start.wait().unwrap().success(), "{e}");
    assert!(o.starts_with("running, pid"), "{o}");
    // And the host is still running. On failure, say whether its process is alive (CI logs show
    // nothing else of a detached host).
    let v = call_in(&dir, &json!({"action": "status"})).unwrap_or_else(|e| {
        let pid = std::fs::read_to_string(dir.join("run/jetcarp/host-endpoint.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v["pid"].as_u64());
        let alive = pid.map(|p| {
            if cfg!(windows) {
                let out = Command::new("tasklist")
                    .args(["/FI", &format!("PID eq {p}"), "/NH"])
                    .output();
                out.map(|o| String::from_utf8_lossy(&o.stdout).contains(&p.to_string()))
                    .unwrap_or(false)
            } else {
                Command::new("kill")
                    .args(["-0", &p.to_string()])
                    .status()
                    .is_ok_and(|s| s.success())
            }
        });
        panic!("status after host start: {e}; host pid {pid:?}, alive: {alive:?}")
    });
    assert_eq!(v["running"], true);
}

/// `jetcarp host stop` returns once the host has gone: the next command finds it stopped (it
/// used to answer one more command while closing), and `host start` starts a new one.
#[test]
fn host_stop_returns_once_the_host_has_gone() {
    let dir = home("stop");
    let run = |args: &[&str]| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_jetcarp"));
        c.args(args).stdin(Stdio::null());
        with_env(&mut c, &dir);
        c.output().unwrap()
    };
    /// Stops the host however the test ends.
    struct Stop<F: Fn()>(F);
    impl<F: Fn()> Drop for Stop<F> {
        fn drop(&mut self) {
            (self.0)();
        }
    }
    let _stop = Stop(|| {
        let _ = run(&["host", "stop"]);
    });
    for round in 0..2 {
        let o = run(&["host", "start", "--headless"]);
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(o.status.success(), "start, round {round}: {err}");
        let o = run(&["host", "stop"]);
        let (out, err) = (
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr),
        );
        assert!(o.status.success(), "stop, round {round}: {out}{err}");
        assert_eq!(out.trim(), "stopped");
        let o = run(&["host", "status"]);
        assert!(!o.status.success(), "status right after stop answered");
        assert!(
            String::from_utf8_lossy(&o.stderr).contains("not running"),
            "{}",
            String::from_utf8_lossy(&o.stderr)
        );
    }
}

/// Started at login (`host serve --login`), the host shows and runs only the screens placed on the
/// desktop; one in a window waits unseen.
#[test]
fn a_host_started_at_login_leaves_window_screens_unseen() {
    let dir = home("login");
    let settings = json!({"screens": [
        {"rotation": "Wolfram series", "place": "window"},
        {"rotation": "Wolfram series", "place": "wallpaper"},
    ]});
    std::fs::write(dir.join("config/jetcarp/host.json"), settings.to_string()).unwrap();
    let child = spawn(&dir, &["--login"]);
    let h = Host { child, home: dir };
    let v = h.wait(
        |v| v["screens"][1]["stage"]["generation"].as_u64() > Some(2),
        "the desktop screen to run",
    );
    assert_eq!(v["login"], true);
    assert_eq!(v["screens"][0]["hidden"], true);
    assert_eq!(v["screens"][1]["hidden"], false);
    assert_eq!(
        v["screens"][0]["stage"]["cells"], 0,
        "the window screen never ran"
    );
}

/// The login entry runs `host serve --login`, and `host login off` removes it, an entry written
/// before `--login` existed too. (Not on Windows, whose entry is in the user's real registry.)
#[cfg(not(windows))]
#[test]
fn the_login_entry_starts_the_host_for_the_desktop_and_comes_off() {
    let dir = home("login-entry");
    let run = |args: &[&str]| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_jetcarp"));
        c.args(args);
        with_env(&mut c, &dir);
        let out = c.output().unwrap();
        assert!(out.status.success(), "{out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let entry = if cfg!(target_os = "macos") {
        dir.join("Library/LaunchAgents/com.jimmieprodgers.jetcarp.host.plist")
    } else {
        dir.join("config/autostart/jetcarp-host.desktop")
    };
    let said = run(&["host", "login", "on"]);
    assert!(said.contains("host serve --login"), "{said}");
    let text = std::fs::read_to_string(&entry).unwrap();
    assert!(text.contains("--login"), "{text}");
    run(&["host", "login", "off"]);
    assert!(!entry.exists());
    // One written by 0.4.0-rc.1, without --login.
    let old = text
        .replace(" \"--login\"", "")
        .replace("<string>--login</string>", "");
    assert!(!old.contains("--login"));
    std::fs::write(&entry, old).unwrap();
    run(&["host", "login", "off"]);
    assert!(!entry.exists(), "the old entry is removed too");
    let _ = std::fs::remove_dir_all(&dir);
}
