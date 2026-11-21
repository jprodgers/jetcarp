//! `jetcarp launcher` puts JETCARP among the system's apps, with its icon, and takes it out again
//! (#105); `jetcarp uninstall` lists it and removes it too. Each test has a scratch home: on Linux
//! the menu entry and the icons go in its `share` folder (`XDG_DATA_HOME`), on macOS `JETCARP.app`
//! in its `Applications`, on Windows the shortcut in its Start menu (`APPDATA`).
//!
//! The menu entry is checked by the Desktop Entry spec's rules here, and by
//! `desktop-file-validate` as well wherever it is installed.

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
    let mut c = Command::new(env!("CARGO_BIN_EXE_jetcarp"));
    c.args(args)
        .stdin(Stdio::null())
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_DATA_HOME", home.join("share"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("APPDATA", home.join("AppData/Roaming"))
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

/// The program the launcher should start: this test's, its links followed; on Windows the
/// windowless launcher beside it when it was built (`--features windows-launcher`).
fn program() -> PathBuf {
    let p = jetcarp_core::paths::plain_path(
        std::fs::canonicalize(env!("CARGO_BIN_EXE_jetcarp")).unwrap(),
    );
    let w = p.with_file_name("jetcarpw.exe");
    if cfg!(windows) && w.exists() { w } else { p }
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

    let (ok, said, err) = jetcarp(&h, &["launcher", "install"]);
    assert!(ok, "{said}{err}");
    let program = program();
    assert!(
        said.contains(&program.display().to_string()),
        "names the program it starts: {said}"
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
        vec![lnk]
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
    let (ok, said, _) = jetcarp(&h, &["launcher", "remove"]);
    assert!(ok && said.contains("nothing to remove"), "{said}");
    let (ok, plan, _) = jetcarp(&h, &["uninstall"]);
    assert!(ok && !plan.contains("remove JETCARP"), "{plan}");
    let _ = std::fs::remove_dir_all(&h);
}

/// An app of the same name that JETCARP did not make is neither replaced nor removed (macOS), and
/// on Linux a menu entry JETCARP did not write is left alone.
#[cfg(not(windows))]
#[test]
fn someone_else_s_launcher_is_left_alone() {
    let h = home("theirs");
    let theirs = if cfg!(target_os = "macos") {
        let p = h.join("Applications/JETCARP.app/Contents/Info.plist");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        p
    } else {
        let p = h.join("share/applications/jetcarp.desktop");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        p
    };
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
