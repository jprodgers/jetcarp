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
        "Invoke-WebRequest https://github.com/jprodgers/jetcarp/releases/latest/download/jetcarp-installer.ps1 -OutFile \"$env:TEMP\\jetcarp-installer.ps1\""
    );
    let ps_quoted = format!("'{}'", lib.display().to_string().replace('\'', "''"));
    assert_eq!(
        w.remove[1],
        format!(
            "powershell -ExecutionPolicy Bypass -File \"$env:TEMP\\jetcarp-installer.ps1\" -Uninstall -InstallDir {ps_quoted}"
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
        already_gone: vec![],
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

/// `jetcarp doctor`'s section on more than one JETCARP (#348): every program named `jetcarp` on
/// the PATH in its order, what a Scoop shim runs, how each was installed, which `jetcarp` runs,
/// the Start menu entries named JETCARP, and the commands that remove the others. The programs
/// are stand-ins in a scratch folder: one in cargo's `bin`, one Scoop shim beside the `.shim`
/// file that names its program.
#[test]
fn doctor_names_each_jetcarp_on_the_path_whose_it_is_and_how_to_remove_the_others() {
    use jetcarp_api::ctx::StartMenuEntry;
    use jetcarp_api::ops::system::doctor_programs;
    let d = scratch("doctor-programs");
    let cargo_bin = d.join(".cargo").join("bin");
    let shims = d.join("scoop").join("shims");
    let installer = d.join("lib").join("jetcarp");
    for dir in [&cargo_bin, &shims, &installer] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let exe = |dir: &Path| {
        let p = dir.join("jetcarp");
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    };
    let scoop_program = d.join("scoop/apps/jetcarp/current/jetcarp");
    std::fs::write(
        shims.join("jetcarp.shim"),
        format!("path = \"{}\"\n", scoop_program.display()),
    )
    .unwrap();
    let shim = exe(&shims);
    let cargo = exe(&cargo_bin);
    std::fs::write(cargo_bin.parent().unwrap().join(".crates.toml"), "").unwrap();
    let mine = exe(&installer);
    std::fs::write(installer.join(".jetcarp-installer"), "JETCARP 0.4.3\n").unwrap();
    // A folder named twice, and one with no program, count once and not at all.
    let path = std::env::join_paths([&shims, &cargo_bin, &shims, &d, &installer]).unwrap();

    let p = doctor_programs(Some(mine.clone()), &path, false, None);
    let found: Vec<&Path> = p.on_path.iter().map(|x| x.path.as_path()).collect();
    assert_eq!(found, [shim.as_path(), cargo.as_path(), mine.as_path()]);
    assert_eq!(p.on_path[0].runs.as_deref(), Some(scoop_program.as_path()));
    let whose: Vec<Option<&str>> = p
        .on_path
        .iter()
        .map(|x| x.installed_with.as_ref().map(|w| w.by.as_str()))
        .collect();
    assert_eq!(whose, [Some("scoop"), Some("cargo"), Some("installer")]);
    assert_eq!(
        p.on_path.iter().map(|x| x.this).collect::<Vec<_>>(),
        [false, false, true]
    );
    let start = [
        StartMenuEntry {
            shortcut: d.join("Start Menu/Programs/JETCARP.lnk"),
            starts: Some(d.join("AppData/Local/jetcarp/launcher/jetcarpw.exe")),
            ours: true,
            then_starts: Some(mine.clone()),
        },
        StartMenuEntry {
            shortcut: d.join("Start Menu/Programs/Scoop Apps/JETCARP.lnk"),
            starts: Some(d.join("scoop/apps/jetcarp/current/jetcarpw.exe")),
            ours: false,
            then_starts: None,
        },
    ];
    let t = p.text(&start);
    assert!(t.starts_with("\nMore than one JETCARP\n"), "{t}");
    let lines: Vec<&str> = t.lines().collect();
    let at = |what: &Path| {
        lines
            .iter()
            .position(|l| l.starts_with(&format!("    {}", what.display())))
            .unwrap_or_else(|| panic!("{} is not named: {t}", what.display()))
    };
    assert!(at(&shim) < at(&cargo) && at(&cargo) < at(&mine), "{t}");
    assert_eq!(
        lines[at(&shim)],
        format!(
            "    {}, which runs {} (installed with Scoop): it runs when you type `jetcarp` here",
            shim.display(),
            scoop_program.display()
        )
    );
    assert!(lines[at(&mine)].ends_with("(installed with JETCARP's installer), this program"));
    assert!(
        t.contains("JETCARP's own entry, from `jetcarp launcher install`"),
        "{t}"
    );
    assert!(
        t.contains(&format!(
            "  note: `jetcarp` here runs {}, not this program\n",
            scoop_program.display()
        )),
        "{t}"
    );
    assert!(t.contains("\n    scoop uninstall jetcarp    ("), "{t}");
    assert!(t.contains("\n    cargo uninstall jetcarp"), "{t}");
    // Scoop's, named by the shim and by the Start menu entry, once.
    assert_eq!(t.matches("scoop uninstall jetcarp").count(), 1, "{t}");

    // One JETCARP, this one: nothing to say.
    let alone = doctor_programs(
        Some(mine.clone()),
        &std::env::join_paths([&installer]).unwrap(),
        false,
        None,
    );
    assert!(alone.text(&start[..1]).is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

/// On Windows the PATH's folders are searched for `jetcarp` with each of `PATHEXT`'s
/// extensions, in its order, as the command prompt and PowerShell do.
#[test]
fn on_windows_each_pathext_extension_is_looked_for_in_its_order() {
    use jetcarp_api::ops::system::jetcarps_on_path;
    let d = scratch("doctor-pathext");
    let (a, b) = (d.join("a"), d.join("b"));
    for dir in [&a, &b] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(a.join("jetcarp.cmd"), "").unwrap();
    std::fs::write(a.join("jetcarp.exe"), "").unwrap();
    std::fs::write(b.join("jetcarp.ps1"), "").unwrap();
    let path = std::env::join_paths([&a, &b]).unwrap();
    let names = |found: Vec<PathBuf>| -> Vec<String> {
        found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    };
    // .exe before .cmd, as PATHEXT has them by default; .ps1 is not one of them.
    assert_eq!(names(jetcarps_on_path(&path, true, None)), ["jetcarp.exe"]);
    assert_eq!(
        names(jetcarps_on_path(&path, true, Some(".CMD;.EXE".as_ref()))),
        ["jetcarp.cmd"]
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// Doctor shows the home folder as `~` (#348): at the start of a path, on a folder's boundary,
/// in the system's own separators; on Windows whatever the case.
#[test]
fn the_home_folder_is_shown_as_a_tilde() {
    use jetcarp_api::ops::system::home_as_tilde;
    let t = |p: &str, h: Option<&str>| home_as_tilde(Path::new(p), h.map(Path::new));
    assert_eq!(
        t("/home/ada/.local/bin/jetcarp", Some("/home/ada")),
        "~/.local/bin/jetcarp"
    );
    assert_eq!(
        t("/home/ada/.local/bin/jetcarp", Some("/home/ada/")),
        "~/.local/bin/jetcarp"
    );
    assert_eq!(t("/home/ada", Some("/home/ada")), "~");
    // Another user's folder whose name begins with the same letters, and no home: as it is.
    assert_eq!(
        t("/home/adam/jetcarp", Some("/home/ada")),
        "/home/adam/jetcarp"
    );
    assert_eq!(
        t("/opt/jetcarp/jetcarp", Some("/home/ada")),
        "/opt/jetcarp/jetcarp"
    );
    assert_eq!(t("/home/ada/jetcarp", None), "/home/ada/jetcarp");
    assert_eq!(t("/home/ada/jetcarp", Some("")), "/home/ada/jetcarp");
    assert_eq!(
        t(
            r"C:\Users\ada\AppData\Local\Programs\jetcarp\jetcarp.exe",
            Some(r"C:\Users\ada")
        ),
        r"~\AppData\Local\Programs\jetcarp\jetcarp.exe"
    );
    if cfg!(windows) {
        assert_eq!(
            t(r"c:\users\ADA\jetcarp.exe", Some(r"C:\Users\ada")),
            r"~\jetcarp.exe"
        );
    }
}
