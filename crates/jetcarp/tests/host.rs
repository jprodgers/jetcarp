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

/// A scratch home for one test, its photosensitive seizure warning accepted (#96), as on a
/// computer where JETCARP has been set up: the host runs its scenes.
fn home(name: &str) -> PathBuf {
    let dir = unwarned_home(name);
    jetcarp_core::photosensitivity::record_in(
        &dir.join("config/jetcarp"),
        jetcarp_core::photosensitivity::Answer::NoDisorder,
        "test",
    )
    .unwrap();
    dir
}

/// A scratch home where the photosensitive seizure warning has not been answered.
fn unwarned_home(name: &str) -> PathBuf {
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
    spawn_with_env(home, extra, &[])
}

fn spawn_with_env(home: &Path, extra: &[&str], env: &[(&str, &str)]) -> Child {
    let mut c = Command::new(env!("CARGO_BIN_EXE_jetcarp"));
    c.args(["host", "serve", "--headless", "--canvas", "160x120"])
        .args(extra)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    with_env(&mut c, home);
    c.envs(env.iter().copied());
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
        Host::start_with_env(name, rotation, &[])
    }

    /// As [`Host::start`], with these variables set in the host's environment.
    fn start_with_env(name: &str, rotation: Option<&str>, env: &[(&str, &str)]) -> Host {
        let home = home(name);
        let extra: Vec<&str> = rotation.map_or(vec![], |r| vec!["--rotation", r]);
        let child = spawn_with_env(&home, &extra, env);
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
    assert_eq!(v["held_why"][0], "windows cover the desktop");
    assert_eq!(
        (&v["paused"], &v["paused_why"]),
        (&json!(false), &json!([]))
    );
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

/// `status` says what each screen's world steps on now, and follows the GPU setting when it
/// changes: the desktop background's own choice, off, puts it on the processor and says whose
/// choice that was, and the log's GPU line is the new one, not the one from start-up (#75).
#[test]
fn status_follows_the_gpu_setting() {
    gpu_setting_off_is_the_reason(Host::start("simulation", Some("Wolfram series")));
}

/// The same with no GPU at all, as on a machine without one, or a session whose GPU was taken
/// away (#138): the setting is still the reason given, since it is the user's own choice and holds
/// whether or not there is a GPU. Vulkan with no driver to load is how the host is shown no GPU.
#[test]
fn status_follows_the_gpu_setting_with_no_gpu() {
    let h = Host::start_with_env(
        "simulation-nogpu",
        Some("Wolfram series"),
        &[
            ("WGPU_BACKEND", "vulkan"),
            ("VK_ICD_FILENAMES", "no-such-driver.json"),
            ("VK_DRIVER_FILES", "no-such-driver.json"),
        ],
    );
    let v = h.wait(
        |v| v["screens"][0]["simulation"]["on"].is_string(),
        "the screen's simulation",
    );
    assert!(v["gpu"].is_null(), "the host was meant to see no GPU: {v}");
    assert_eq!(
        v["screens"][0]["simulation"]["why"], "GPU simulation unavailable: no GPU",
        "{v}"
    );
    gpu_setting_off_is_the_reason(h);
}

fn gpu_setting_off_is_the_reason(h: Host) {
    let v = h.wait(
        |v| v["screens"][0]["simulation"]["on"].is_string(),
        "the screen's simulation",
    );
    let gpu = v["gpu"].as_str().map(str::to_string);
    let settings = json!({"gpu": "off", "screens": [{"rotation": "Wolfram series"}]});
    std::fs::write(
        h.home.join("config/jetcarp/host.json"),
        settings.to_string(),
    )
    .unwrap();
    let v = h.call(json!({"action": "reload"})).unwrap();
    let sim = &v["screens"][0]["simulation"];
    assert_eq!(sim["on"], "cpu", "{v}");
    // The user's own choice first, with a GPU or without (`SimBackend::report`).
    assert_eq!(
        sim["why"],
        "GPU simulation off, from the desktop background's own setting"
    );
    if gpu.is_some() {
        let log = v["log"].to_string();
        assert!(
            log.contains("GPU simulation off (the desktop background's own setting)"),
            "{log}"
        );
        assert_eq!(
            log.matches("GPU simulation").count(),
            1,
            "the start-up line is taken back: {log}"
        );
    }
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

/// Until the photosensitive seizure warning has been accepted, nothing on the command line starts
/// the desktop background or the screen saver: each says why, with the warning, and changes
/// nothing. Once `jetcarp setup` has accepted it, the host starts (#96).
#[test]
fn nothing_starts_on_screen_before_the_warning_is_accepted() {
    let dir = unwarned_home("unwarned");
    let run = |args: &[&str]| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_jetcarp"));
        c.args(args).stdin(Stdio::null());
        with_env(&mut c, &dir);
        c.output().unwrap()
    };
    /// Stops a host however the test ends.
    struct Stop<F: Fn()>(F);
    impl<F: Fn()> Drop for Stop<F> {
        fn drop(&mut self) {
            (self.0)();
        }
    }
    let _stop = Stop(|| {
        let _ = run(&["host", "stop"]);
    });
    for (args, what) in [
        (
            &["host", "start", "--headless"][..],
            "start the background host",
        ),
        (&["host", "wallpaper", "on"], "show the desktop background"),
        (
            &["host", "login", "on"],
            "start the background host at login",
        ),
        (&["host", "saver", "install"], "install the screen saver"),
        (
            &["host", "serve", "--saver", "--seconds", "1"],
            "run the screen saver",
        ),
    ] {
        let o = run(args);
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(!o.status.success(), "{args:?} was not refused: {err}");
        assert!(
            err.contains(&format!(
                "JETCARP won't {what} until its photosensitive seizure warning has been accepted"
            )),
            "{args:?}: {err}"
        );
        assert!(err.contains("Hodgepodge"), "the warning itself: {err}");
    }
    let o = run(&["host", "status"]);
    assert!(String::from_utf8_lossy(&o.stderr).contains("not running"));
    assert!(
        !dir.join("config/jetcarp/host.json").exists(),
        "nothing saved"
    );
    assert!(!dir.join("config/autostart/jetcarp-host.desktop").exists());
    assert!(
        !dir.join("Library/LaunchAgents/com.jimmieprodgers.jetcarp.host.plist")
            .exists()
    );
    // Reading and turning off are not refused.
    assert!(run(&["host", "wallpaper", "off"]).status.success());
    // Accepted with `jetcarp setup`, the host starts.
    let o = run(&["setup", "--yes", "--seizure-warning", "no-disorder"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let o = run(&["host", "start", "--headless"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

/// A host started before the photosensitive seizure warning was accepted (at login, say, after an
/// upgrade) holds every screen at its first frame and says why in its status; resuming by hand or
/// a sweep on screen does not get round it. Once the warning is accepted it starts by itself
/// (#96).
#[test]
fn a_host_holds_its_screens_until_the_warning_is_accepted() {
    let dir = unwarned_home("held");
    let child = spawn(&dir, &["--rotation", "Wolfram series"]);
    let h = Host { child, home: dir };
    let v = h.wait(
        |v| Host::stage(v)["cells"].as_u64() > Some(0),
        "the first frame",
    );
    assert_eq!(v["held"], json!(["warning"]), "{v}");
    assert_eq!(v["paused"], false, "held, not paused: {v}");
    let why = v["held_why"][0].as_str().unwrap();
    assert_eq!(why, jetcarp_core::photosensitivity::HOST_HELD);
    let e = h
        .call(json!({"action": "tune", "seconds": 0.5}))
        .unwrap_err();
    if jetcarp_core::fit::FIT {
        assert!(e.contains("photosensitive seizure warning"), "{e}");
    } else {
        // No sweep at all while Fit to this computer is switched off (#112).
        assert_eq!(e, jetcarp_core::fit::HELD_BACK);
    }
    h.call(json!({"action": "resume"})).unwrap();
    std::thread::sleep(Duration::from_millis(800));
    let v = h.call(json!({"action": "status"})).unwrap();
    assert_eq!(Host::stage(&v)["generation"], 0, "nothing animated: {v}");
    jetcarp_core::photosensitivity::record_in(
        &h.home.join("config/jetcarp"),
        jetcarp_core::photosensitivity::Answer::NoDisorder,
        "test",
    )
    .unwrap();
    let v = h.wait(
        |v| Host::stage(v)["generation"].as_u64() > Some(2),
        "the scenes to start once the warning was accepted",
    );
    assert_eq!(v["held"], json!([]), "{v}");
}

/// A screen tuned for this computer builds a tuned scene at the size found, and the scene it shows
/// (`scene`) records that grid, so it replays as shown; a scene the tuning does not name keeps its
/// own size. A tuner's hold pauses the screens until it lets go.
#[test]
fn a_tuned_size_is_applied_and_recorded() {
    let dir = home("tuned");
    let settings = json!({"screens": [{"rotation": "Wolfram series", "tuning": {
        "rotation": "Wolfram series", "canvas": [160, 120], "scale": 1.0,
        "scenes": [{"scene": "W0", "design_px": 4.0, "cell_px": 8.0, "cells": 300,
                    "fits": true}]}}]});
    std::fs::write(dir.join("config/jetcarp/host.json"), settings.to_string()).unwrap();
    let child = spawn(&dir, &[]);
    let h = Host { child, home: dir };
    let v = h.wait(
        |v| Host::stage(v)["generation"].as_u64() > Some(0),
        "the tuned scene to run",
    );
    let s = Host::stage(&v);
    assert_eq!(
        (s["label"].as_str(), s["caption"][0].as_str()),
        (Some("W0"), Some("Rule 0"))
    );
    assert_eq!(s["cell_px"], 8.0, "{s}");
    assert_eq!(s["tuned"], true);
    assert_eq!(s["grid"], json!([20, 15]));
    assert_eq!(s["cells"], 300);
    // The scene it shows, as `jetcarp host scene` saves it, builds the same grid.
    let shown = h.call(json!({"action": "scene"})).unwrap();
    assert_eq!(shown["tuned"], true);
    let scene: jetcarp_core::Scene = serde_json::from_value(shown["scene"].clone()).unwrap();
    let w = jetcarp_core::World::from_scene(&scene).unwrap();
    assert_eq!(w.topo.len(), 300);
    // W1 (Rule 1) was not tuned: its own 4 px.
    h.call(json!({"action": "next"})).unwrap();
    let v = h.wait(
        |v| Host::stage(v)["caption"][0] == "Rule 1" && Host::stage(v)["cells"].as_u64() > Some(0),
        "the next rule",
    );
    let s = Host::stage(&v);
    assert_eq!(
        (s["cell_px"].as_f64(), s["tuned"].as_bool()),
        (Some(4.0), Some(false))
    );
    assert_eq!(s["grid"], json!([40, 30]));
    // A tuner holds the screens, and lets go.
    let reply = h.call(json!({"action": "tuning", "on": true})).unwrap();
    assert_eq!(reply["ok"], true);
    let v = h.wait(|v| Host::stage(v)["paused"] == true, "the tuning hold");
    assert!(v["held"].to_string().contains("tuning"), "{v}");
    // A hold, not a pause: `paused` and `paused_why` agree that it isn't paused (#76).
    assert!(
        v["held_why"]
            .to_string()
            .contains("tuning the desktop background"),
        "{v}"
    );
    assert_eq!(
        (&v["paused"], &v["paused_why"]),
        (&json!(false), &json!([]))
    );
    assert_eq!(v["tuning"]["running"], true);
    h.call(json!({"action": "tuning", "on": false})).unwrap();
    let v = h.wait(|v| Host::stage(v)["paused"] == false, "it to carry on");
    assert_eq!(v["tuning"]["running"], false);
}

/// `jetcarp` with this test's folders: its exit status, standard output and standard error.
fn jetcarp_in(home: &Path, args: &[&str]) -> (bool, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_jetcarp"));
    c.args(args).stdin(Stdio::null());
    with_env(&mut c, home);
    let out = c.output().unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// *Fit to this computer* is switched off in 0.4.2 (#112): `jetcarp host tune` is not in the help
/// and refuses, with or without a host; the host refuses every sweep and a tuner's hold, says
/// nothing of fitting in its status, and shows each scene at its own size, as 0.4.1 did, though
/// `host.json` holds the sizes a release candidate's Fit saved. Those stay in the file as they
/// were, for 0.4.3, whatever else is saved. (With Fit back on, the tests above cover it.)
#[test]
fn fit_is_switched_off_and_a_saved_fit_is_kept_but_not_applied() {
    use jetcarp_core::fit::{FIT, HELD_BACK};
    if FIT {
        return;
    }
    let dir = home("nofit");
    let tuning = json!({"rotation": "Wolfram series", "canvas": [160, 120], "scale": 1.0,
        "limits": {"cpu_percent": 5.0, "gpu_percent": 40.0, "auto": true}, "fps": 30.0,
        "scenes": [{"scene": "W0", "design_px": 4.0, "cell_px": 8.0, "chosen": true,
                    "recommended": {"px": 8.0}}]});
    let settings = json!({"screens": [{"rotation": "Wolfram series", "tuning": tuning}],
        "tune": {"cpu_percent": 5.0, "gpu_percent": 40.0, "auto": true}});
    let file = dir.join("config/jetcarp/host.json");
    std::fs::write(&file, settings.to_string()).unwrap();
    let saved =
        || -> Value { serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap() };
    let saved_tuning = || -> jetcarp_display::host_settings::Tuning {
        serde_json::from_value(saved()["screens"][0]["tuning"].clone()).unwrap()
    };
    let want: jetcarp_display::host_settings::Tuning =
        serde_json::from_value(tuning.clone()).unwrap();

    // Not in the help, and refused with no host running.
    let (ok, help, _) = jetcarp_in(&dir, &["host", "--help"]);
    assert!(ok, "{help}");
    assert!(help.contains("  pacing"), "{help}");
    assert!(
        !help.lines().any(|l| l.trim_start().starts_with("tune")),
        "{help}"
    );
    let (ok, out, err) = jetcarp_in(&dir, &["host", "tune"]);
    assert!(!ok, "refused: {out}{err}");
    assert!(err.contains(HELD_BACK), "{err}");

    // The host shows W0 (Rule 0) at its own 4 px, not the 8 px saved.
    let child = spawn(&dir, &[]);
    let h = Host {
        child,
        home: dir.clone(),
    };
    let v = h.wait(
        |v| Host::stage(v)["generation"].as_u64() > Some(0),
        "the scene to run",
    );
    let s = Host::stage(&v);
    assert_eq!(s["label"], "W0", "{s}");
    assert_eq!(s["cell_px"], 4.0, "{s}");
    assert_eq!(s["grid"], json!([40, 30]), "{s}");
    assert_eq!(s["cells"], 1200, "{s}");
    // Nothing of fitting in the status.
    for k in ["tune", "tuning"] {
        assert!(v.get(k).is_none(), "{k} in {v}");
    }
    assert!(s.get("tuned").is_none(), "{s}");
    let shown = h.call(json!({"action": "scene"})).unwrap();
    assert_eq!(shown["grid"], json!([40, 30]));
    assert!(shown.get("tuned").is_none(), "{shown}");
    // Every way of asking for a sweep, its news or a tuner's hold is refused.
    for a in [
        json!({"action": "tune", "seconds": 0.5}),
        json!({"action": "tune", "on": false}),
        json!({"action": "tune_stop"}),
        json!({"action": "tune_events"}),
        json!({"action": "tuning", "on": true}),
    ] {
        assert_eq!(h.call(a.clone()).unwrap_err(), HELD_BACK, "{a}");
    }
    let v = h.call(json!({"action": "status"})).unwrap();
    assert_eq!(v["held"], json!([]), "no tuner's hold: {v}");
    // The command line, with the host running, refuses too (as JSON lines with --json).
    let (ok, out, err) = jetcarp_in(&dir, &["host", "tune", "--json"]);
    assert!(!ok, "refused: {out}{err}");
    let last: Value = serde_json::from_str(out.lines().last().unwrap()).unwrap();
    assert_eq!(
        (last["event"].as_str(), last["error"].as_str()),
        (Some("failed"), Some(HELD_BACK)),
        "{out}"
    );
    // A setting saved and read again keeps the saved fit as it was, unused.
    let (ok, out, err) = jetcarp_in(&dir, &["host", "pacing", "low-power"]);
    assert!(ok, "{out}{err}");
    h.wait(|v| v["pacing"] == "low_power", "low power");
    assert_eq!(saved_tuning(), want);
    assert_eq!(saved()["tune"]["cpu_percent"], 5.0);
    let v = h.call(json!({"action": "status"})).unwrap();
    assert_eq!(Host::stage(&v)["cell_px"], 4.0, "{v}");
}

/// Generations stepped, frames drawn and seconds passed on screen 0 over about a second and a
/// half, with the status at the end.
fn paced(h: &Host) -> (u64, u64, f64, Value) {
    let count = |v: &Value| {
        (
            Host::stage(v)["generation"].as_u64().unwrap(),
            v["screens"][0]["frames"].as_u64().unwrap(),
        )
    };
    let a = h.call(json!({"action": "status"})).unwrap();
    let t = Instant::now();
    std::thread::sleep(Duration::from_millis(1500));
    let b = h.call(json!({"action": "status"})).unwrap();
    let secs = t.elapsed().as_secs_f64();
    let ((g0, f0), (g1, f1)) = (count(&a), count(&b));
    (g1.saturating_sub(g0), f1.saturating_sub(f0), secs, b)
}

/// A scene of 60 generations a second draws 60 frames a second, a generation each (#42). The host
/// drew 30, so every frame showed every second generation, and Life's blinkers stood still. In
/// low power (`jetcarp host pacing low-power`, which tells the running host) it draws 30 frames
/// a second, and the scene runs at 30 generations, still one a frame. The frame rates are the
/// host's own figures; the counts allow for a slow machine (a frame that comes late steps no
/// more than one generation, and the scene falls behind instead).
#[test]
fn a_screen_draws_every_generation_or_in_low_power_caps_the_speed() {
    let dir = home("pace");
    let rotation = json!({"name": "Sixty", "look": {"speed": 60.0}, "default_minutes": null,
        "items": [{"source": {"kind": "series", "rules": "B3/S23"}}]});
    std::fs::write(
        dir.join("config/jetcarp/rotations/sixty.json"),
        rotation.to_string(),
    )
    .unwrap();
    let settings = json!({"screens": [{"rotation": "Sixty"}]});
    std::fs::write(dir.join("config/jetcarp/host.json"), settings.to_string()).unwrap();
    let child = spawn(&dir, &[]);
    let h = Host { child, home: dir };
    let v = h.wait(
        |v| Host::stage(v)["generation"].as_u64() > Some(10),
        "the scene to run",
    );
    assert_eq!(v["pacing"], "every_generation", "{v}");
    let (gens, frames, secs, v) = paced(&h);
    let s = Host::stage(&v);
    assert_eq!(
        (s["frame_rate"].as_f64(), s["speed"].as_f64()),
        (Some(60.0), Some(60.0)),
        "{s}"
    );
    let what = format!("{gens} generations in {frames} frames, {secs:.2} s");
    assert!(
        frames > 10 && gens <= frames + 1,
        "one a frame at most: {what}"
    );
    assert!(gens * 4 >= frames * 3, "nearly one every frame: {what}");

    // Low power, from the command line.
    let mut c = Command::new(env!("CARGO_BIN_EXE_jetcarp"));
    c.args(["host", "pacing", "low-power"]).stdin(Stdio::null());
    with_env(&mut c, &h.home);
    let out = c.output().unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{said}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(said.contains("pacing: low power, 30 fps"), "{said}");
    h.wait(
        |v| v["pacing"] == "low_power" && Host::stage(v)["frame_rate"].as_f64() == Some(30.0),
        "low power",
    );
    let (gens, frames, secs, v) = paced(&h);
    let s = Host::stage(&v);
    assert_eq!(
        (s["frame_rate"].as_f64(), s["speed"].as_f64()),
        (Some(30.0), Some(30.0)),
        "{s}"
    );
    let what = format!("{gens} generations in {frames} frames, {secs:.2} s");
    assert!(
        frames > 5 && gens <= frames + 1,
        "one a frame at most: {what}"
    );
    assert!(frames as f64 / secs < 45.0, "drawn at 30, not 60: {what}");
    assert!(gens as f64 / secs < 45.0, "at most 30 a second: {what}");
}

/// A rotation as the desktop background of the folders in `dir`, on `screens` screens, every one
/// on the desktop.
fn rotation_on(dir: &Path, rotation: Value, screens: usize) {
    let name = rotation["name"].as_str().unwrap().to_string();
    std::fs::write(
        dir.join(format!(
            "config/jetcarp/rotations/{}.json",
            name.to_lowercase().replace(' ', "-")
        )),
        rotation.to_string(),
    )
    .unwrap();
    let screens: Vec<Value> = (0..screens)
        .map(|_| json!({"rotation": name, "place": "wallpaper"}))
        .collect();
    let settings = json!({ "screens": screens });
    std::fs::write(dir.join("config/jetcarp/host.json"), settings.to_string()).unwrap();
}

/// A rotation of two rules (a series of them), as the desktop background of the folders in `dir`.
fn two_rules(dir: &Path) {
    rotation_on(
        dir,
        json!({"name": "Two rules",
            "items": [{"source": {"kind": "series", "rules": "B3/S23,B36/S23"}}]}),
        1,
    );
}

/// Three scenes: two Life-like rules on squares and one on triangles, which has more neighbours
/// a cell, so it is the one measured.
fn three_scenes(dir: &Path, screens: usize) {
    rotation_on(
        dir,
        json!({"name": "Three scenes", "look": {"cell_px": 4.0, "speed": 30.0, "refill": true},
        "items": [
            {"source": {"kind": "series", "rules": "B3/S23", "family": "square"}},
            {"source": {"kind": "series", "rules": "B36/S23", "family": "square"}},
            {"source": {"kind": "series", "rules": "B4/S345", "family": "triangle"}}
        ]}),
        screens,
    );
}

/// `jetcarp host tune --json`'s lines.
fn events_of(stdout: &str) -> Vec<Value> {
    stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}")))
        .collect()
}

/// The folders' host.json.
fn saved(dir: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join("config/jetcarp/host.json")).unwrap())
        .unwrap()
}

/// `jetcarp host tune --json` with these options, in the folders of `dir`: its events, and its
/// output for the message when it fails.
fn tune(dir: &Path, extra: &[&str]) -> (Vec<Value>, String) {
    let mut args = vec!["host", "tune", "--json", "--trial-seconds", "0.5"];
    args.extend_from_slice(extra);
    let (ok, out, err) = jetcarp_in(dir, &args);
    assert!(ok, "{out}\n{err}");
    (events_of(&out), format!("{out}\n{err}"))
}

fn count(events: &[Value], kind: &str) -> usize {
    events.iter().filter(|e| e["event"] == kind).count()
}

/// A fast fit (#113), off screen with no host running: one scene measured a screen, the
/// rotation's most demanding (Life-like on triangles, more neighbours a cell than on squares), by
/// a binary search of a few sizes rather than every scene at every size; every scene gets its
/// size, the measured one its cards; the fit says how long it took, and the pictures land in
/// `--crops` and beside the settings. A series' rules are one scene (#109).
#[test]
fn a_fit_measures_one_scene_a_screen_in_a_few_sizes() {
    let dir = home("tune");
    three_scenes(&dir, 1);
    let crops = dir.join("crops");
    let t0 = Instant::now();
    let (events, out) = tune(
        &dir,
        &[
            "--canvas",
            "640x360",
            "--cpu-limit",
            "50",
            "--crops",
            crops.to_str().unwrap(),
        ],
    );
    let took = t0.elapsed().as_secs_f64();
    assert_eq!(events[0]["event"], "start", "{out}");
    assert_eq!(events[0]["scenes"], 3, "{out}");
    let shares = &events[0]["shares"];
    assert_eq!(shares[0]["scene"], "B4/S345", "the most demanding: {out}");
    assert_eq!(shares[0]["share"], 1.0);
    // A few sizes of the one scene, not every scene at every size.
    let measured: Vec<&Value> = events.iter().filter(|e| e["event"] == "measured").collect();
    assert!(
        !measured.is_empty() && measured.len() <= 6,
        "{} sizes\n{out}",
        measured.len()
    );
    assert!(measured.iter().all(|e| e["scene"] == "B4/S345"), "{out}");
    assert_eq!(count(&events, "rung"), measured.len(), "{out}");
    assert_eq!(count(&events, "scene"), 3, "{out}");
    let done = events.last().unwrap();
    assert_eq!(done["event"], "done", "{out}");
    assert_eq!(done["trials"], measured.len() as u64);
    let secs = done["seconds"].as_f64().unwrap();
    assert!(
        secs < 30.0 && took < 60.0,
        "{secs} s ({took} s in all)\n{out}"
    );
    // Saved: the measured scene's cards, the others sized from it.
    let t = &saved(&dir)["screens"][0]["tuning"];
    assert_eq!(t["canvas"], json!([640, 360]));
    assert_eq!(t["share"], 1.0);
    let scenes = t["scenes"].as_array().unwrap();
    assert_eq!(scenes.len(), 3, "{t}");
    let tri = scenes.iter().find(|s| s["scene"] == "B4/S345").unwrap();
    assert_eq!(tri["measured"], true);
    assert_eq!(tri["rungs"].as_array().unwrap().len(), measured.len());
    for s in scenes.iter().filter(|s| s["scene"] != "B4/S345") {
        assert!(
            s.get("rungs").is_none() && s.get("measured").is_none(),
            "{s}"
        );
        let cost = s["cost"].as_f64().unwrap();
        assert!(cost > 0.0 && cost < 1.0, "cheaper than the triangles: {s}");
        // Never coarser than the measured scene's size.
        assert!(
            s["cell_px"].as_f64() <= tri["cell_px"].as_f64(),
            "{s} against {tri}"
        );
    }
    // Pictures, where there was a GPU to draw them.
    for e in &measured {
        if let Some(p) = e["rung"]["crop"].as_str() {
            assert!(crops.join(p).is_file(), "{p} in --crops");
            assert!(
                dir.join("config/jetcarp/tune").join(p).is_file(),
                "{p} beside the settings"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A rotation mixing scenes that step on the GPU with scenes that step on the processor is
/// measured on both: the most demanding of each, each by its own search (#113). Life steps on the
/// GPU; Wolfram's rule 30 from a soup steps on the processor. Each
/// other scene is sized from the one measured on its own device.
#[test]
fn a_rotation_mixing_devices_is_measured_on_both() {
    let dir = home("mixed");
    rotation_on(
        &dir,
        json!({"name": "Mixed devices", "look": {"cell_px": 4.0, "speed": 30.0, "refill": true},
        "items": [
            {"source": {"kind": "series", "rules": "B3/S23"}},
            {"source": {"kind": "series", "rules": "B36/S23"}},
            {"source": {"kind": "series", "rules": "W30", "seed": "random:0.3"}}
        ]}),
        1,
    );
    let (events, out) = tune(&dir, &["--canvas", "640x360"]);
    if events[0]["gpu"].is_null() {
        // No GPU: every scene steps on the processor, and one is measured.
        return;
    }
    let measured: std::collections::BTreeSet<&str> = events
        .iter()
        .filter(|e| e["event"] == "measured")
        .map(|e| e["scene"].as_str().unwrap())
        .collect();
    assert_eq!(
        measured.into_iter().collect::<Vec<_>>(),
        ["B3/S23", "W30"],
        "one scene on each device\n{out}"
    );
    let scenes = &saved(&dir)["screens"][0]["tuning"]["scenes"];
    let at = |name: &str| {
        scenes
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["scene"] == name)
            .unwrap()
            .clone()
    };
    let (life, rows, brian) = (at("B3/S23"), at("W30"), at("B36/S23"));
    assert_eq!(
        (life["measured"].as_bool(), life["gpu"].as_bool()),
        (Some(true), Some(true))
    );
    assert_eq!(
        (rows["measured"].as_bool(), rows["gpu"].as_bool()),
        (Some(true), None)
    );
    assert!(!rows["rungs"].as_array().unwrap().is_empty(), "{rows}");
    // B36/S23 is sized from Life, on its own device: no note about another device.
    assert!(brian.get("measured").is_none(), "{brian}");
    assert!(
        !brian.to_string().contains("the measured scene on"),
        "{brian}"
    );
    let done = events.last().unwrap();
    assert!(done["seconds"].as_f64().unwrap() < 30.0, "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A rule series is fitted as one scene (#109): the Wolfram series' first 41 rules are one
/// scene, measured by its first rule that draws (rule 1, #346), and the host shows every rule at
/// the size found.
#[test]
fn a_rule_series_is_fitted_as_one_scene() {
    let dir = home("series");
    rotation_on(
        &dir,
        json!({"name": "Some Wolfram", "look": {"cell_px": 4.0, "speed": 60.0, "refill": true},
            "items": [{"source": {"kind": "series", "rules": "W", "from": 0, "to": 40}}]}),
        1,
    );
    let (events, out) = tune(&dir, &["--canvas", "160x120", "--pick", "2"]);
    assert_eq!(events[0]["scenes"], 1, "{out}");
    assert!(count(&events, "measured") <= 6, "{out}");
    let scenes: Vec<&Value> = events.iter().filter(|e| e["event"] == "scene").collect();
    assert_eq!(scenes.len(), 1, "{out}");
    let r = &scenes[0]["result"];
    assert_eq!(r["scene"], "W1");
    assert_eq!(r["also"].as_array().unwrap().len(), 40, "{r}");
    assert_eq!(r["cell_px"], 2.0, "--pick 2: {r}");
    // The host shows a later rule of the series at that size too.
    let child = spawn_with_env(&dir, &[], &[]);
    let h = Host { child, home: dir };
    h.wait(|v| Host::stage(v)["label"] == "W0", "the series");
    for _ in 0..7 {
        h.call(json!({"action": "next"})).unwrap();
    }
    let v = h.wait(
        |v| Host::stage(v)["label"] == "W7" && Host::stage(v)["cells"].as_u64() > Some(0),
        "rule 7",
    );
    let s = Host::stage(&v);
    assert_eq!(
        (s["cell_px"].as_f64(), s["tuned"].as_bool()),
        (Some(2.0), Some(true)),
        "{s}"
    );
}

/// A rule series' cards show a picture (#346): the Wolfram series was measured and pictured by its
/// first rule, rule 0, which draws nothing from its one cell. Now by its first rule that changes
/// cells of its seed, rule 1, its rows drawn from the top of the screen: the picture is taken
/// where they are.
#[test]
fn a_rule_series_cards_show_a_picture() {
    let dir = home("series-picture");
    rotation_on(
        &dir,
        json!({"name": "Some Wolfram", "look": {"cell_px": 4.0, "speed": 60.0, "refill": true},
            "items": [{"source": {"kind": "series", "rules": "W", "from": 0, "to": 40}}]}),
        1,
    );
    let crops = dir.join("crops");
    let (events, out) = tune(
        &dir,
        &["--canvas", "640x360", "--crops", crops.to_str().unwrap()],
    );
    let measured: Vec<&Value> = events.iter().filter(|e| e["event"] == "measured").collect();
    assert!(!measured.is_empty(), "{out}");
    if events[0]["gpu"].is_null() {
        // No GPU: no pictures.
        return;
    }
    for e in &measured {
        let name = e["rung"]["crop"]
            .as_str()
            .unwrap_or_else(|| panic!("a picture: {e}"));
        let png = std::fs::read(crops.join(name)).unwrap();
        let img = jetcarp_core::media::encode::decode_png(&png).unwrap();
        let px: Vec<&[u8]> = img.rgba.chunks(4).collect();
        let mut counts = std::collections::HashMap::new();
        for p in &px {
            *counts.entry(*p).or_insert(0usize) += 1;
        }
        let most = counts.values().max().copied().unwrap_or(0);
        let drawn = (px.len() - most) as f64 / px.len() as f64;
        assert!(
            drawn > 0.05,
            "{name} ({} at {}): {:.1}% of its pixels drawn\n{out}",
            e["scene"],
            e["rung"]["size"],
            drawn * 100.0
        );
    }
    let r = &events.iter().find(|e| e["event"] == "scene").unwrap()["result"];
    assert_eq!(r["scene"], "W1", "measured and pictured by rule 1: {r}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--pick` places the sizes the last fit found while they are current, measuring nothing
/// (#77): each `--pick` on the Acer ran the whole 50 s sweep again. `--fresh` measures again, and
/// so does a pick once something it was found for has changed (here the processor limit).
#[test]
fn pick_places_the_last_fit_s_sizes_without_measuring() {
    let dir = home("pick");
    three_scenes(&dir, 1);
    let (first, out) = tune(&dir, &["--canvas", "640x360"]);
    assert!(count(&first, "measured") > 0, "{out}");
    let (events, out) = tune(&dir, &["--canvas", "640x360", "--pick", "own"]);
    assert_eq!(
        count(&events, "rung") + count(&events, "measured"),
        0,
        "{out}"
    );
    assert_eq!(events[0]["reused"], true, "{out}");
    assert_eq!(events.last().unwrap()["trials"], 0, "{out}");
    let scenes = saved(&dir)["screens"][0]["tuning"]["scenes"].clone();
    for s in scenes.as_array().unwrap() {
        assert_eq!(
            (s["cell_px"].as_f64(), s["chosen"].as_bool()),
            (Some(4.0), Some(true)),
            "{s}"
        );
    }
    // The cards are still there to pick from again.
    let (events, out) = tune(&dir, &["--canvas", "640x360", "--pick", "recommended"]);
    assert_eq!(count(&events, "measured"), 0, "{out}");
    let s = &saved(&dir)["screens"][0]["tuning"]["scenes"];
    assert!(
        s.as_array().unwrap().iter().all(|x| x["chosen"] != true),
        "{s}"
    );
    // --fresh measures again; so does a changed limit.
    let (events, out) = tune(&dir, &["--canvas", "640x360", "--pick", "own", "--fresh"]);
    assert!(count(&events, "measured") > 0, "{out}");
    let (events, out) = tune(
        &dir,
        &["--canvas", "640x360", "--pick", "own", "--cpu-limit", "30"],
    );
    assert!(count(&events, "measured") > 0, "{out}");
    let (ok, _, err) = jetcarp_in(&dir, &["host", "tune", "--fresh"]);
    assert!(!ok && err.contains("--fresh goes with --pick"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two screens are fitted at once, with one processor limit for the whole desktop shared by
/// their pixel area (#107): a 320 x 240 screen beside a 160 x 120 one gets four fifths of it.
/// The opening line says so, and each screen's share is saved with its fit.
#[test]
fn two_screens_are_fitted_at_once_sharing_the_limits_by_area() {
    let dir = home("fit-two");
    three_scenes(&dir, 2);
    // Each screen's size, as a fit with no host running reads it from the last one.
    let mut s = saved(&dir);
    for (i, canvas) in [[320, 240], [160, 120]].iter().enumerate() {
        s["screens"][i]["tuning"] =
            json!({"rotation": "Three scenes", "canvas": canvas, "scale": 1.0, "scenes": []});
    }
    std::fs::write(dir.join("config/jetcarp/host.json"), s.to_string()).unwrap();
    let (events, out) = tune(&dir, &["--cpu-limit", "10"]);
    let shares = events[0]["shares"].as_array().unwrap();
    assert_eq!(shares.len(), 2, "{out}");
    assert!(
        (shares[0]["share"].as_f64().unwrap() - 0.8).abs() < 1e-9,
        "{out}"
    );
    assert!(
        (shares[1]["share"].as_f64().unwrap() - 0.2).abs() < 1e-9,
        "{out}"
    );
    assert!(
        (shares[0]["cpu_percent"].as_f64().unwrap() - 8.0).abs() < 1e-4,
        "{out}"
    );
    assert!(
        (shares[1]["cpu_percent"].as_f64().unwrap() - 2.0).abs() < 1e-4,
        "{out}"
    );
    // Both screens in every round: their sizes come in pairs until one is settled.
    let screens: Vec<u64> = events
        .iter()
        .filter(|e| e["event"] == "rung")
        .map(|e| e["screen"].as_u64().unwrap())
        .collect();
    assert_eq!(screens[..2], [0, 1], "{out}");
    let s = saved(&dir);
    assert!((s["screens"][0]["tuning"]["share"].as_f64().unwrap() - 0.8).abs() < 1e-9);
    assert!((s["screens"][1]["tuning"]["share"].as_f64().unwrap() - 0.2).abs() < 1e-9);
    // For people: how the one limit is shared.
    let (ok, text, err) = jetcarp_in(
        &dir,
        &["host", "tune", "--trial-seconds", "0.5", "--dry-run"],
    );
    assert!(ok, "{text}{err}");
    assert!(
        text.contains("one for the whole desktop, shared between the screens by their pixel area")
            && text.contains("screen 0 (320x240, 80% of the area): processor 8.0%")
            && text.contains("screen 1 (160x120, 20% of the area): processor 2.0%"),
        "{text}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// With the host running, `jetcarp host tune` has the host fit on its own screen and follows it;
/// the series is one scene, left at the recommended size, which the host then shows. A fit can be
/// stopped part way, and nothing it found is saved.
#[test]
fn the_host_fits_its_own_screen_and_shows_the_recommended_size() {
    let dir = home("sweep");
    two_rules(&dir);
    let child = spawn(&dir, &[]);
    let h = Host { child, home: dir };
    h.wait(
        |v| Host::stage(v)["generation"].as_u64() > Some(0),
        "the host to run",
    );
    let (events, out) = tune(&h.home, &[]);
    assert_eq!(
        (
            count(&events, "start"),
            count(&events, "scene"),
            count(&events, "done")
        ),
        (1, 1, 1),
        "{out}"
    );
    assert!(count(&events, "measured") <= 6, "{out}");
    let s = saved(&h.home);
    let t = &s["screens"][0]["tuning"];
    assert_eq!(t["rotation"], "Two rules");
    assert_eq!(t["canvas"], json!([160, 120]));
    let scene = &t["scenes"][0];
    assert_eq!(
        (&scene["scene"], &scene["also"]),
        (&json!("B3/S23"), &json!(["B36/S23"]))
    );
    assert_ne!(scene["chosen"], true, "the recommended size, not a choice");
    let want = scene["cell_px"].as_f64().unwrap();
    // The host shows both rules at the size found.
    let v = h.wait(
        |v| {
            let s = Host::stage(v);
            s["label"] == "B3/S23"
                && s["cells"].as_u64() > Some(0)
                && s["cell_px"].as_f64() == Some(want)
        },
        "the host to show the size found",
    );
    assert_eq!(v["tuning"]["last"]["outcome"], "done");
    assert!(v["tuning"]["last"]["trials"].as_u64() <= Some(6), "{v}");
    // A pick with nothing changed is placed at once.
    let (events, out) = tune(&h.home, &["--pick", "own"]);
    assert_eq!(count(&events, "measured"), 0, "{out}");
    let v = h.wait(
        |v| Host::stage(v)["cell_px"].as_f64() == Some(4.0),
        "its own size",
    );
    assert_eq!(v["tuning"]["last"]["reused"], true, "{v}");
    // Started and stopped: the screen carries on as before.
    h.call(json!({"action": "tune", "seconds": 5.0})).unwrap();
    let v = h.call(json!({"action": "status"})).unwrap();
    assert_eq!(v["tuning"]["running"], true);
    let v = h.call(json!({"action": "tune", "on": false})).unwrap();
    assert_eq!(v["tuning"]["running"], false);
    assert_eq!(v["tuning"]["last"]["outcome"], "stopped");
    let e = h
        .call(json!({"action": "tune", "pick": "bigger"}))
        .unwrap_err();
    assert!(e.contains("--pick is"), "{e}");
}

/// While `jetcarp score` measures this computer, the desktop background holds its screens still
/// (its own load would lower the score, #301), and runs them again once the measuring has ended.
#[test]
fn the_desktop_background_holds_still_while_jetcarp_measures() {
    let h = Host::start("measuring", Some("Wolfram series"));
    h.wait(
        |v| Host::stage(v)["generation"].as_u64() > Some(2),
        "generations",
    );
    let mut c = Command::new(env!("CARGO_BIN_EXE_jetcarp"));
    c.args(["score", "--quick", "--no-gpu", "--no-save"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    with_env(&mut c, &h.home);
    c.env("JETCARP_DATA_DIR", h.home.join("data"))
        .env("JETCARP_OUT", h.home.join("out"));
    let mut score = c.spawn().expect("jetcarp score starts");
    let measuring = |v: &Value| {
        v["held"]
            .as_array()
            .is_some_and(|a| a.iter().any(|x| x == "measuring"))
    };
    let mut held = vec![];
    while score.try_wait().unwrap().is_none() {
        if let Ok(v) = h.call(json!({"action": "status"}))
            && measuring(&v)
        {
            held.push(Host::stage(&v)["generation"].as_u64());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(score.wait().unwrap().success());
    assert!(
        !held.is_empty(),
        "the desktop background was never held while JETCARP measured"
    );
    let v = h.wait(|v| !measuring(v), "the hold to end");
    assert!(
        v["held_why"].as_array().is_some_and(|a| a.is_empty()),
        "{v}"
    );
    let last = held.last().copied().flatten().unwrap_or(0);
    h.wait(
        |v| Host::stage(v)["generation"].as_u64() > Some(last),
        "generations after the measuring",
    );
}
