//! `jetcarp uninstall` tells from the program's own path how it was installed, and names the one
//! command that removes it (#103). The paths of package managers are only read; the installer's
//! marker, a Python environment and cargo's root are made in scratch folders.

mod common;

use common::scratch;
use jetcarp_api::op::Report;
use jetcarp_api::ops::system::{InstalledWith, UninstallReport, installed_with};
use std::path::{Path, PathBuf};

const SH: &str = "curl -LsSf https://github.com/jprodgers/jetcarp/releases/latest/download/jetcarp-installer.sh | sh -s -- --uninstall";

/// What installed `exe`, and its commands.
fn by(exe: &str, windows: bool) -> Option<(String, Vec<String>)> {
    installed_with(Path::new(exe), windows).map(|w| (w.by, w.remove))
}

fn one(by: &str, cmd: &str) -> Option<(String, Vec<String>)> {
    Some((by.into(), vec![cmd.into()]))
}

#[test]
fn package_managers_by_their_folders() {
    for exe in [
        "/opt/homebrew/Cellar/jetcarp/0.4.2/bin/jetcarp",
        "/usr/local/Cellar/jetcarp/0.4.2/bin/jetcarp",
        "/home/linuxbrew/.linuxbrew/Cellar/jetcarp/0.4.2/bin/jetcarp",
    ] {
        assert_eq!(
            by(exe, false),
            one("homebrew", "brew uninstall jetcarp"),
            "{exe}"
        );
    }
    assert_eq!(
        by(r"C:\Users\ada\scoop\apps\jetcarp\0.4.2\jetcarp.exe", true),
        one("scoop", "scoop uninstall jetcarp")
    );
    assert_eq!(
        by(
            r"C:\Users\ada\AppData\Local\Microsoft\WinGet\Packages\JimmieRodgers.JETCARP_Microsoft.Winget.Source_8wekyb3d8bbwe\jetcarp-0.4.2-x86_64-pc-windows-msvc\jetcarp.exe",
            true
        ),
        one("winget", "winget uninstall JimmieRodgers.JETCARP")
    );
    for exe in [
        "/home/ada/.local/share/pipx/venvs/jetcarp/bin/jetcarp",
        "/Users/ada/.local/pipx/venvs/jetcarp/bin/jetcarp",
        r"C:\Users\ada\pipx\venvs\jetcarp\Scripts\jetcarp.exe",
    ] {
        assert_eq!(
            by(exe, exe.contains('\\')),
            one("pipx", "pipx uninstall jetcarp"),
            "{exe}"
        );
    }
    for exe in [
        "/home/ada/.local/share/uv/tools/jetcarp/bin/jetcarp",
        r"C:\Users\ada\AppData\Roaming\uv\data\tools\jetcarp\Scripts\jetcarp.exe",
    ] {
        assert_eq!(
            by(exe, exe.contains('\\')),
            one("uv", "uv tool uninstall jetcarp"),
            "{exe}"
        );
    }
    let cargo = installed_with(Path::new("/home/ada/.cargo/bin/jetcarp"), false).unwrap();
    assert_eq!(cargo.by, "cargo");
    assert!(
        cargo.remove[0].starts_with("cargo uninstall jetcarp"),
        "{cargo:?}"
    );
    assert_eq!(cargo.folder, PathBuf::from("/home/ada/.cargo/bin"));
    // Nothing to tell by: a release archive unpacked somewhere, a program copied onto the PATH.
    for exe in [
        "/home/ada/Downloads/jetcarp-0.4.2-x86_64-unknown-linux-gnu/jetcarp",
        "/usr/local/bin/jetcarp",
        "jetcarp",
    ] {
        assert_eq!(by(exe, false), None, "{exe}");
    }
}

#[test]
fn the_installer_by_its_marker() {
    let d = scratch("installed-with-installer");
    let lib = d.join("my lib").join("it's jetcarp");
    std::fs::create_dir_all(&lib).unwrap();
    let exe = lib.join("jetcarp");
    std::fs::write(&exe, b"").unwrap();
    // Without the marker, a folder of its own is not enough to tell.
    assert_eq!(installed_with(&exe, false), None);
    std::fs::write(
        lib.join(".jetcarp-installer"),
        "JETCARP 0.4.2 x86_64-unknown-linux-gnu\n",
    )
    .unwrap();
    let w = installed_with(&exe, false).unwrap();
    assert_eq!(
        (w.by.as_str(), w.folder.as_path()),
        ("installer", lib.as_path())
    );
    // Not the installer's default folder, so it is named, quoted for the shell.
    let quoted = format!("'{}'", lib.display().to_string().replace('\'', r"'\''"));
    assert_eq!(w.remove, [format!("{SH} --dir {quoted}")]);
    // The PowerShell installer is saved first, then run.
    let w = installed_with(&exe, true).unwrap();
    assert_eq!(w.remove.len(), 2, "{w:?}");
    assert_eq!(
        w.remove[0],
        "Invoke-WebRequest https://github.com/jprodgers/jetcarp/releases/latest/download/jetcarp-installer.ps1 -OutFile jetcarp-installer.ps1"
    );
    let ps_quoted = format!("'{}'", lib.display().to_string().replace('\'', "''"));
    assert_eq!(
        w.remove[1],
        format!(
            "powershell -ExecutionPolicy Bypass -File .\\jetcarp-installer.ps1 -Uninstall -InstallDir {ps_quoted}"
        )
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_python_environment_and_a_cargo_root() {
    let d = scratch("installed-with-roots");
    let venv = d.join("venv");
    std::fs::create_dir_all(venv.join("bin")).unwrap();
    std::fs::write(venv.join("bin").join("jetcarp"), b"").unwrap();
    assert_eq!(
        installed_with(&venv.join("bin").join("jetcarp"), false),
        None
    );
    std::fs::write(venv.join("pyvenv.cfg"), "home = /usr/bin\n").unwrap();
    assert_eq!(
        by(
            &venv.join("bin").join("jetcarp").display().to_string(),
            false
        ),
        one("pip", "pip uninstall jetcarp")
    );

    // `cargo install --root DIR`: named, since it is not cargo's own folder.
    let root = d.join("tools");
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::write(root.join(".crates.toml"), "[v1]\n").unwrap();
    let exe = root.join("bin").join("jetcarp");
    assert_eq!(
        by(&exe.display().to_string(), false),
        one(
            "cargo",
            &format!("cargo uninstall jetcarp --root {}", root.display())
        )
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_report_ends_with_the_command_that_applies() {
    let report = |installed_with: Option<InstalledWith>| UninstallReport {
        done: true,
        steps: vec![],
        kept_data: None,
        kept_settings: None,
        kept_publisher: None,
        output: PathBuf::from("/home/ada/Pictures/JETCARP"),
        installed_with,
    };
    let t = report(Some(InstalledWith {
        by: "homebrew".into(),
        folder: PathBuf::from("/opt/homebrew/Cellar/jetcarp/0.4.2/bin"),
        remove: vec!["brew uninstall jetcarp".into()],
    }))
    .text();
    assert!(
        t.ends_with(
            "\nTo remove the program itself (in /opt/homebrew/Cellar/jetcarp/0.4.2/bin, installed \
             with Homebrew), run this:\n\n    brew uninstall jetcarp\n\nIf you connected JETCARP \
             to an AI assistant, remove it there too: `claude mcp remove jetcarp` for Claude \
             Code, or the \"jetcarp\" entry in Claude Desktop's claude_desktop_config.json.\n"
        ),
        "{t}"
    );
    assert!(!t.contains("pipx"), "{t}");
    let t = report(Some(InstalledWith {
        by: "pip".into(),
        folder: PathBuf::from("/home/ada/venv/bin"),
        remove: vec!["pip uninstall jetcarp".into()],
    }))
    .text();
    assert!(
        t.contains(
            "(in the Python environment /home/ada/venv, installed with pip), run this with that \
             environment active:\n\n    pip uninstall jetcarp\n"
        ),
        "{t}"
    );
    let t = report(None).text();
    assert!(
        t.contains("\nTo remove the program itself, use what installed it: the installer again"),
        "{t}"
    );
}
