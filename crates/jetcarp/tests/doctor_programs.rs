//! `jetcarp doctor` names every program named `jetcarp` on the PATH, in the PATH's order, and
//! says which one `jetcarp` runs, when there is more than one JETCARP (#348). On the Windows
//! laptop of rc.1's sitting, Scoop's 0.4.1 came first on the PATH and the installer's rc.1 second,
//! so `jetcarp` ran 0.4.1, and nothing JETCARP said mentioned the other copy.
//!
//! Each run has a scratch home with every folder in it, and two stand-ins for other copies (a
//! script named `jetcarp`, `jetcarp.cmd` on Windows) in folders of their own put first on the
//! PATH. Doctor never runs them; it only reads where they are.

use serde_json::Value;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn home(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "jetcarp-doctor-programs-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A stand-in for another copy of JETCARP in `dir`: a program named `jetcarp` that is not
/// JETCARP.
fn stand_in(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    if cfg!(windows) {
        let p = dir.join("jetcarp.cmd");
        std::fs::write(&p, "@echo a stand-in for the doctor test\r\n").unwrap();
        p
    } else {
        let p = dir.join("jetcarp");
        std::fs::write(&p, "#!/bin/sh\necho a stand-in for the doctor test\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    }
}

/// The PATH this test was given, without its folders holding a `jetcarp`: so a JETCARP
/// installed on this machine is not counted.
fn path_without_jetcarps() -> Vec<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["jetcarp.exe", "jetcarp.cmd", "jetcarp.bat", "jetcarp.com"]
    } else {
        &["jetcarp"]
    };
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter(|d| !names.iter().any(|n| d.join(n).exists()))
        .collect()
}

/// `jetcarp doctor` (with `--json` first when `json`) with every folder in `home` and the PATH
/// `path`.
fn doctor(home: &Path, path: &[PathBuf], json: bool) -> String {
    let path: OsString = std::env::join_paths(path).unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_jetcarp"));
    if json {
        c.arg("--json");
    }
    let out = c
        .arg("doctor")
        .stdin(Stdio::null())
        .env("PATH", path)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_DATA_HOME", home.join("share"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("APPDATA", home.join("AppData/Roaming"))
        .env("LOCALAPPDATA", home.join("AppData/Local"))
        .env("JETCARP_CONFIG_DIR", home.join("config/jetcarp"))
        .env("JETCARP_DATA_DIR", home.join("data"))
        .env("JETCARP_RUNTIME_DIR", home.join("run/jetcarp"))
        .env("JETCARP_OUT", home.join("out"))
        .env("JETCARP_LIBRARY", home.join("library"))
        .env("JETCARP_GPU", "off")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn same_file(a: &Path, b: &Path) -> bool {
    let c = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    c(a) == c(b)
}

#[test]
fn doctor_names_every_jetcarp_on_the_path_and_which_one_runs() {
    let h = home("two");
    let first = stand_in(&h.join("first/bin"));
    let second = stand_in(&h.join("second/bin"));
    let mut path = vec![h.join("first/bin"), h.join("second/bin")];
    path.extend(path_without_jetcarps());

    let text = doctor(&h, &path, false);
    let (a, b) = (first.display().to_string(), second.display().to_string());
    let (at_a, at_b) = (text.find(&a), text.find(&b));
    assert!(
        at_a.is_some() && at_b.is_some(),
        "doctor names both programs on the PATH: {text}"
    );
    assert!(at_a < at_b, "in the PATH's order: {text}");
    let line_a = text.lines().find(|l| l.contains(&a)).unwrap();
    assert!(
        line_a.contains("runs when you type `jetcarp`"),
        "says the first is the one `jetcarp` runs: {line_a}"
    );
    let line_b = text.lines().find(|l| l.contains(&b)).unwrap();
    assert!(!line_b.contains("runs when you type"), "{line_b}");
    // This program is not on that PATH, and is named too.
    let me = Path::new(env!("CARGO_BIN_EXE_jetcarp"));
    assert!(text.contains("this program: "), "{text}");

    let d: Value = serde_json::from_str(&doctor(&h, &path, true)).unwrap();
    let on_path = d["programs"]["on_path"].as_array().unwrap();
    assert_eq!(on_path.len(), 2, "{on_path:?}");
    assert!(same_file(
        Path::new(on_path[0]["path"].as_str().unwrap()),
        &first
    ));
    assert!(same_file(
        Path::new(on_path[1]["path"].as_str().unwrap()),
        &second
    ));
    assert!(on_path.iter().all(|p| p["this"] == false), "{on_path:?}");
    assert!(same_file(
        Path::new(d["programs"]["this"].as_str().unwrap()),
        me
    ));
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn doctor_says_nothing_of_other_copies_when_there_is_one() {
    let h = home("one");
    let me = Path::new(env!("CARGO_BIN_EXE_jetcarp"));
    let mut path = vec![me.parent().unwrap().to_path_buf()];
    path.extend(path_without_jetcarps());
    let text = doctor(&h, &path, false);
    assert!(
        !text.contains("More than one JETCARP"),
        "one JETCARP, this one, on the PATH: {text}"
    );
    let d: Value = serde_json::from_str(&doctor(&h, &path, true)).unwrap();
    let on_path = d["programs"]["on_path"].as_array().unwrap();
    assert_eq!(on_path.len(), 1, "{on_path:?}");
    assert_eq!(on_path[0]["this"], true, "{on_path:?}");
    let _ = std::fs::remove_dir_all(&h);
}
