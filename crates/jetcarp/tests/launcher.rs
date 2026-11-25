//! `jetcarp launcher` puts JETCARP among the system's apps, with its icon, and takes it out again
//! (#105); `jetcarp uninstall` lists it and removes it too, and `jetcarp doctor` says where it is.
//! Each test has a scratch home: on Linux the menu entry and the icons go in its `share` folder
//! (`XDG_DATA_HOME`), on macOS `JETCARP.app` in its `Applications`, on Windows the shortcut in its
//! Start menu (`APPDATA`) and the launcher's copy in its `LOCALAPPDATA`.
//!
//! The menu entry is checked by the Desktop Entry spec's rules here, and by
//! `desktop-file-validate` as well wherever it is installed.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn home(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "jetcarp-launcher-test-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `jetcarp` with every folder in `home`, and no way to reach the desktop session (its bus, its
/// display), so `jetcarp uninstall` sees no GNOME extension and changes nothing of the session's.
fn jetcarp(home: &Path, args: &[&str]) -> (bool, String, String) {
    run_in(Path::new(env!("CARGO_BIN_EXE_jetcarp")), home, args, None)
}

/// [`jetcarp`] with another program (a copy of it, or the launcher), and the `PATH` given.
fn run_in(exe: &Path, home: &Path, args: &[&str], path: Option<&OsStr>) -> (bool, String, String) {
    let mut c = Command::new(exe);
    if let Some(p) = path {
        c.env("PATH", p);
    }
    c.args(args)
        .stdin(Stdio::null())
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
        .env("GSETTINGS_BACKEND", "memory")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("XDG_CURRENT_DESKTOP");
    let out = c.output().unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The program the launcher should start: this test's, its links followed.
fn program() -> PathBuf {
    real(Path::new(env!("CARGO_BIN_EXE_jetcarp")))
}

fn real(p: &Path) -> PathBuf {
    jetcarp_core::paths::plain_path(std::fs::canonicalize(p).unwrap())
}

/// The Desktop Entry spec's rules (1.5) that apply to JETCARP's entry: a comment or the group
/// header first; `Key=Value` lines with keys of letters, digits and `-`, each once; the keys an
/// application needs; lists ending in `;`; an icon named by its theme name (no extension); a
/// `true`/`false` boolean; and an `Exec` whose quoted program is escaped as the spec says.
fn check_by_the_spec(text: &str) {
    let mut lines = text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty());
    assert_eq!(lines.next(), Some("[Desktop Entry]"), "{text}");
    let mut keys = std::collections::BTreeMap::new();
    for l in lines {
        assert!(!l.starts_with('['), "one group only: {l}");
        let (k, v) = l
            .split_once('=')
            .unwrap_or_else(|| panic!("not Key=Value: {l}"));
        assert!(
            !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "key {k:?}"
        );
        assert!(
            keys.insert(k.to_string(), v.to_string()).is_none(),
            "{k} twice"
        );
    }
    for k in ["Type", "Name", "Exec", "Icon"] {
        assert!(keys.contains_key(k), "{k} missing: {text}");
    }
    assert_eq!(keys["Type"], "Application");
    assert_eq!(keys["Terminal"], "false");
    assert_eq!(keys["StartupNotify"], "true");
    for list in ["Categories", "Keywords"] {
        assert!(keys[list].ends_with(';'), "{list}");
    }
    // A main category, and Math with the main category it belongs under.
    assert_eq!(keys["Categories"], "Science;Math;");
    assert!(!keys["Icon"].contains('.') && !keys["Icon"].contains('/'));
    let exec = &keys["Exec"];
    assert!(exec.starts_with('"') && exec.ends_with('"'), "{exec}");
    let inner = &exec[1..exec.len() - 1];
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => assert!(matches!(chars.next(), Some('"' | '`' | '$' | '\\'))),
            '"' | '`' | '$' => panic!("{c} unescaped in {exec}"),
            _ => {}
        }
    }
}

#[test]
fn the_launcher_goes_in_with_its_icon_and_comes_out_with_uninstall() {
    let h = home("cycle");
    let (ok, said, err) = jetcarp(&h, &["launcher", "status"]);
    assert!(ok, "{err}");
    assert!(said.contains("is not in"), "{said}");
    let (ok, doctor, err) = jetcarp(&h, &["doctor"]);
    assert!(ok, "{err}");
    assert!(
        doctor.contains("\n  launcher: JETCARP is not in"),
        "the doctor says it is not there: {doctor}"
    );

    let (ok, said, err) = jetcarp(&h, &["launcher", "install"]);
    assert!(ok, "{said}{err}");
    let program = program();
    assert!(
        said.contains(&program.display().to_string()),
        "names the program it starts: {said}"
    );
    let (ok, doctor, err) = jetcarp(&h, &["doctor"]);
    assert!(ok, "{err}");
    assert!(
        doctor.contains("\n  launcher: JETCARP is in"),
        "the doctor says where it is: {doctor}"
    );
    let icons = Path::new(env!("CARGO_MANIFEST_DIR")).join("../jetcarp-display/icons");
    let made: Vec<PathBuf> = if cfg!(target_os = "macos") {
        let app = h.join("Applications/JETCARP.app");
        let plist = std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap();
        assert!(plist.contains("<string>com.jimmieprodgers.jetcarp</string>"));
        let script = std::fs::read_to_string(app.join("Contents/MacOS/JETCARP")).unwrap();
        assert!(script.contains(&program.display().to_string()), "{script}");
        assert_eq!(
            std::fs::read(app.join("Contents/Resources/jetcarp.icns")).unwrap(),
            std::fs::read(icons.join("jetcarp.icns")).unwrap()
        );
        // Signed on this Mac, ad hoc.
        let v = Command::new("codesign")
            .args(["--verify", "--verbose"])
            .arg(&app)
            .output()
            .unwrap();
        assert!(v.status.success(), "{v:?}");
        vec![app]
    } else if cfg!(windows) {
        let lnk = h.join("AppData/Roaming/Microsoft/Windows/Start Menu/Programs/JETCARP.lnk");
        let bytes = std::fs::read(&lnk).unwrap();
        // A shell link: its header's size, then the class of shell links.
        assert_eq!(&bytes[..4], &[0x4C, 0, 0, 0]);
        // With the AppUserModelID the app's window has too, so a pin made from the taskbar is of
        // this entry (#347).
        assert_eq!(
            jetcarp_display::launcher::shortcut_app_id(&lnk).as_deref(),
            Some(jetcarp_display::launcher::APP_USER_MODEL_ID),
            "the entry's AppUserModelID"
        );
        // Doctor names it, as JETCARP's own entry (#348); and beside another entry named
        // JETCARP (Scoop makes one in *Scoop Apps*), it names both.
        let (ok, json, err) = jetcarp(&h, &["--json", "doctor"]);
        assert!(ok, "{err}");
        let d: serde_json::Value = serde_json::from_str(&json).unwrap();
        let entries = d["desktop"]["start_menu"].as_array().unwrap().clone();
        let ours: Vec<&serde_json::Value> = entries.iter().filter(|e| e["ours"] == true).collect();
        assert_eq!(ours.len(), 1, "{entries:?}");
        let scoop = lnk.parent().unwrap().join("Scoop Apps");
        std::fs::create_dir_all(&scoop).unwrap();
        std::fs::copy(&lnk, scoop.join("JETCARP.lnk")).unwrap();
        let (ok, doctor, err) = jetcarp(&h, &["doctor"]);
        assert!(ok, "{err}");
        assert!(
            doctor.contains("\n  Start menu entries named JETCARP:\n")
                && doctor.contains(&scoop.join("JETCARP.lnk").display().to_string()),
            "{doctor}"
        );
        std::fs::remove_dir_all(&scoop).unwrap();
        let mut made = vec![lnk];
        // With the windowless launcher built, the entry starts a copy of it in a folder of
        // JETCARP's own, beside a record of this program.
        if program.with_file_name("jetcarpw.exe").exists() {
            let stable = h.join("AppData/Local/jetcarp/launcher");
            let record = std::fs::read_to_string(stable.join("jetcarp-program")).unwrap();
            assert_eq!(record.trim(), program.display().to_string());
            assert_eq!(
                ours[0]["then_starts"].as_str(),
                Some(program.display().to_string().as_str()),
                "{entries:?}"
            );
            made.push(stable.join("jetcarpw.exe"));
            made.push(stable.join("jetcarp-program"));
        }
        made
    } else {
        let entry = h.join("share/applications/jetcarp.desktop");
        let text = std::fs::read_to_string(&entry).unwrap();
        check_by_the_spec(&text);
        assert!(
            text.contains(&format!("\nExec=\"{}\"\n", program.display())),
            "{text}"
        );
        if let Ok(v) = Command::new("desktop-file-validate").arg(&entry).output() {
            let out = format!(
                "{}{}",
                String::from_utf8_lossy(&v.stdout),
                String::from_utf8_lossy(&v.stderr)
            );
            assert!(
                v.status.success() && out.trim().is_empty(),
                "desktop-file-validate: {out}"
            );
        }
        let mut files = vec![entry];
        for n in [16, 24, 32, 48, 64, 128, 256, 512] {
            let p = h.join(format!("share/icons/hicolor/{n}x{n}/apps/jetcarp.png"));
            assert_eq!(
                std::fs::read(&p).unwrap(),
                std::fs::read(icons.join(format!("jetcarp-{n}.png"))).unwrap(),
                "{}",
                p.display()
            );
            files.push(p);
        }
        files
    };
    let (ok, said, _) = jetcarp(&h, &["launcher", "status"]);
    assert!(ok && said.contains("JETCARP is in"), "{said}");
    // Twice, as an installer run again does.
    let (ok, said, err) = jetcarp(&h, &["launcher", "install"]);
    assert!(ok, "{said}{err}");

    // `jetcarp uninstall` takes it out with the rest. (Not on Windows, where it would undo the
    // rest in the user's real registry and folders.)
    if !cfg!(windows) {
        let (ok, plan, err) = jetcarp(&h, &["uninstall"]);
        assert!(ok, "{err}");
        assert!(plan.contains("Would undo"), "{plan}");
        assert!(plan.contains("  remove JETCARP"), "{plan}");
        assert!(made.iter().all(|f| f.exists()), "only listed");
        let (ok, done, err) = jetcarp(&h, &["uninstall", "--yes"]);
        assert!(ok, "{err}");
        assert!(done.contains("  done: remove JETCARP"), "{done}");
    } else {
        let (ok, said, err) = jetcarp(&h, &["launcher", "remove"]);
        assert!(ok && said.contains("removed"), "{said}{err}");
    }
    for f in &made {
        assert!(!f.exists(), "{} is still there", f.display());
    }
    // Neither `launcher` nor `uninstall` records itself in the memory pool, so they make no
    // runtime folder: the installers' `--uninstall` runs `launcher` after `jetcarp uninstall` has
    // removed that folder, and left an empty `memory/` in it (#135). `doctor` above does record
    // itself, and on Windows no `uninstall --yes` has removed what it made, so from none here.
    let run = h.join("run/jetcarp");
    let _ = std::fs::remove_dir_all(&run);
    let (ok, said, _) = jetcarp(&h, &["launcher", "remove"]);
    assert!(ok && said.contains("nothing to remove"), "{said}");
    let (ok, plan, _) = jetcarp(&h, &["uninstall"]);
    assert!(ok && !plan.contains("remove JETCARP"), "{plan}");
    assert!(!run.exists(), "{} was made", run.display());
    let _ = std::fs::remove_dir_all(&h);
}

/// An app of the same name that JETCARP did not make is neither replaced nor removed (macOS); on
/// Linux a menu entry JETCARP did not write is left alone, and on Windows a `JETCARP.lnk` that
/// does not start a JETCARP program (this one can't be read as a shortcut at all).
#[test]
fn someone_else_s_launcher_is_left_alone() {
    let h = home("theirs");
    let theirs = if cfg!(target_os = "macos") {
        h.join("Applications/JETCARP.app/Contents/Info.plist")
    } else if cfg!(windows) {
        h.join("AppData/Roaming/Microsoft/Windows/Start Menu/Programs/JETCARP.lnk")
    } else {
        h.join("share/applications/jetcarp.desktop")
    };
    std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
    std::fs::write(&theirs, "not JETCARP's\n").unwrap();
    let (ok, said, err) = jetcarp(&h, &["launcher", "install"]);
    assert!(!ok, "{said}");
    assert!(err.contains("JETCARP did not make it"), "{err}");
    let (ok, _, err) = jetcarp(&h, &["launcher", "remove"]);
    assert!(!ok);
    assert!(err.contains("not made by JETCARP"), "{err}");
    assert_eq!(std::fs::read_to_string(&theirs).unwrap(), "not JETCARP's\n");
    let _ = std::fs::remove_dir_all(&h);
}

/// Windows, with the windowless launcher built: a winget upgrade, for real. JETCARP installed in a
/// versioned folder makes the Start menu entry; the upgrade replaces the folder with the next
/// version's, which is on the PATH; the copy of the launcher the entry starts still starts
/// JETCARP, and the new program's first run puts itself in the record.
#[cfg(windows)]
#[test]
fn the_start_menu_entry_survives_a_winget_upgrade() {
    let built = program();
    let launcher = built.with_file_name("jetcarpw.exe");
    if !launcher.exists() {
        eprintln!("skipped: jetcarpw.exe was not built (--features windows-launcher)");
        return;
    }
    let h = home("winget");
    let pkg = h.join("WinGet/Packages/JimmieRodgers.JETCARP_x");
    let version = |v: &str| {
        let d = pkg.join(format!("jetcarp-{v}-x86_64-pc-windows-msvc"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::copy(&built, d.join("jetcarp.exe")).unwrap();
        std::fs::copy(&launcher, d.join("jetcarpw.exe")).unwrap();
        real(&d.join("jetcarp.exe"))
    };
    let old = version("0.4.3");
    let (ok, said, err) = run_in(&old, &h, &["launcher", "install"], None);
    assert!(ok, "{said}{err}");
    let stable = h.join("AppData/Local/jetcarp/launcher");
    let copy = stable.join("jetcarpw.exe");
    let record = || std::fs::read_to_string(stable.join("jetcarp-program")).unwrap();
    assert_eq!(record().trim(), old.display().to_string());

    // The upgrade: the old folder goes, the new one comes, and the PATH leads to it.
    std::fs::remove_dir_all(old.parent().unwrap()).unwrap();
    let new = version("0.4.4");
    let path = std::env::join_paths([new.parent().unwrap()]).unwrap();
    let (ok, said, err) = run_in(&copy, &h, &["--version"], Some(&path));
    assert!(
        ok,
        "the entry's launcher starts the new program: {said}{err}"
    );
    assert!(said.contains("jetcarp"), "{said}");
    // That run was the new program's first, and it mended the record.
    assert_eq!(record().trim(), new.display().to_string());
    // From now on it is found without the PATH.
    let (ok, said, err) = run_in(&copy, &h, &["--version"], Some(OsStr::new("")));
    assert!(ok, "{said}{err}");
    let (ok, said, err) = run_in(&new, &h, &["launcher", "remove"], None);
    assert!(ok, "{said}{err}");
    assert!(!copy.exists() && !stable.join("jetcarp-program").exists());
    let _ = std::fs::remove_dir_all(&h);
}
