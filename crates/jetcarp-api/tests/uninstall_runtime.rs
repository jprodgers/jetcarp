//! `jetcarp uninstall` lists and removes what JETCARP's programs leave in the runtime folder once
//! none of them is running from it (#135): the host's and the tuner's locks, the app's, the
//! host's and the HTTP endpoints' files (one of them left half-written), the memory pool's
//! records, and then the folder itself when that empties it. A file that a running program still
//! uses is kept, and the report says which and why; a file JETCARP did not put there is never
//! touched. On Windows the runtime folder is also the default data folder: there the runtime
//! files go and the library stays, unless `--data` deletes the whole folder.
//!
//! On the Mac, an `app-endpoint.json` the app left (it had been ended with SIGTERM) was still in
//! `~/.cache/jetcarp` after `jetcarp uninstall --settings --data --yes`, which neither listed nor
//! removed it.
//!
//! Every folder is a scratch folder, checked to be one before anything runs; the platform is the
//! headless one, which has nothing of its own to undo. This file is its own test program with one
//! test, because the folders come from environment variables, which it sets.

use jetcarp_api::op::Report;
use jetcarp_api::ops::system::{SystemUninstall, UninstallIn, UninstallReport};
use jetcarp_api::{Ctx, Op, Quiet};
use std::net::TcpListener;
use std::path::{Path, PathBuf};

fn write(p: &Path, text: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// A port nothing listens on: one bound and let go again.
fn closed_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// An endpoint file as the app, the host and the HTTP endpoints write it.
fn endpoint(port: u16) -> String {
    format!(r#"{{"port": {port}, "token": "0123456789abcdef", "pid": 4242}}"#)
}

/// A memory pool record (`jetcarp_core::machine::pool::Use`), recorded `age_ms` ago.
fn record(pid: u32, age_ms: u64) -> String {
    format!(
        r#"{{"pid": {pid}, "program": "the app", "bytes": 1000000, "at_ms": {}}}"#,
        now_ms() - age_ms
    )
}

/// Every entry under `d`, as paths inside it (folders end in `/`), sorted.
fn tree(d: &Path) -> Vec<String> {
    fn walk(root: &Path, d: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            let rel = p.strip_prefix(root).unwrap().display().to_string();
            if e.file_type().unwrap().is_dir() {
                out.push(format!("{rel}/"));
                walk(root, &p, out);
            } else {
                out.push(rel);
            }
        }
    }
    let mut out = Vec::new();
    walk(d, d, &mut out);
    out.sort();
    out.iter().map(|s| s.replace('\\', "/")).collect::<Vec<_>>()
}

#[test]
fn the_runtime_folder_is_emptied_and_removed_once_nothing_runs_from_it() {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("uninstall-runtime");
    let _ = std::fs::remove_dir_all(&d);
    // Named `jetcarp`, as the runtime folder is (`$XDG_RUNTIME_DIR/jetcarp`, `~/.cache/jetcarp`).
    let runtime = d.join("run").join("jetcarp");
    let (settings, data, out) = (d.join("config"), d.join("data"), d.join("out"));
    for f in [&runtime, &settings, &data, &out] {
        std::fs::create_dir_all(f).unwrap();
    }
    // SAFETY: this test program runs this one test, which sets them before each Ctx it makes.
    let set = |config: &Path, data: &Path, runtime: &Path| unsafe {
        std::env::set_var("JETCARP_CONFIG_DIR", config);
        std::env::set_var("JETCARP_DATA_DIR", data);
        std::env::set_var("JETCARP_RUNTIME_DIR", runtime);
        std::env::set_var("JETCARP_OUT", &out);
        std::env::remove_var("JETCARP_LIBRARY");
    };
    let run = |yes: bool, data: bool| -> UninstallReport {
        let cx = Ctx::new();
        let p = cx.paths();
        for f in p.settings.iter().chain([&p.data]).chain(p.runtime.iter()) {
            assert!(f.starts_with(&d), "not a scratch folder: {}", f.display());
        }
        SystemUninstall::run(
            &cx,
            UninstallIn {
                yes,
                data,
                ..Default::default()
            },
            &Quiet,
        )
        .unwrap()
    };
    // The step that names the runtime folder.
    let runtime_step = |r: &UninstallReport, folder: &Path| -> usize {
        let shown = folder.display().to_string();
        let found: Vec<usize> = (0..r.steps.len())
            .filter(|&i| r.steps[i].what.contains(&shown))
            .collect();
        assert_eq!(found.len(), 1, "one step for {shown}:\n{}", r.text());
        found[0]
    };

    // ---- What JETCARP's programs leave there once they have gone.
    set(&settings, &data, &runtime);
    let left = [
        "host.lock",
        "tune.lock",
        "app-endpoint.json",
        "host-endpoint.json",
        "mcp-http-endpoint.json",
        "app-http-endpoint.json",
        ".app-endpoint.json.4242.tmp",
        "memory/4242.json",
        "memory/4243.json.tmp",
    ];
    write(&runtime.join("host.lock"), "");
    write(&runtime.join("tune.lock"), "");
    write(&runtime.join("app-endpoint.json"), &endpoint(closed_port()));
    write(
        &runtime.join("host-endpoint.json"),
        &endpoint(closed_port()),
    );
    write(
        &runtime.join("mcp-http-endpoint.json"),
        &endpoint(closed_port()),
    );
    // A record with no port: nothing can be reached through it.
    write(&runtime.join("app-http-endpoint.json"), "{}");
    write(&runtime.join(".app-endpoint.json.4242.tmp"), "{\"po");
    // A record not refreshed for two minutes: its program has gone.
    write(&runtime.join("memory/4242.json"), &record(4242, 120_000));
    write(&runtime.join("memory/4243.json.tmp"), "");
    // Not JETCARP's: never touched.
    write(&runtime.join("notes.txt"), "mine");

    // Listed, with every file, and nothing done without `yes`.
    let plan = run(false, false);
    let i = runtime_step(&plan, &runtime);
    let what = &plan.steps[i].what;
    for name in [
        "host.lock",
        "tune.lock",
        "app-endpoint.json",
        "host-endpoint.json",
        "mcp-http-endpoint.json",
        "app-http-endpoint.json",
        "memory",
    ] {
        assert!(what.contains(name), "{name} in {what:?}");
    }
    assert!(!what.contains("notes.txt"), "{what:?}");
    assert_eq!(plan.steps[i].ok, None);
    for f in left {
        assert!(runtime.join(f).exists(), "only listed: {f}");
    }

    // ---- The app and a host are running: what they use is kept, and said to be. The app is a
    // new one (process 4244), so the file that process 4242 left half-written goes.
    let app = TcpListener::bind("127.0.0.1:0").unwrap();
    write(
        &runtime.join("app-endpoint.json"),
        &endpoint(app.local_addr().unwrap().port()).replace("4242", "4244"),
    );
    write(&runtime.join("memory/4244.json"), &record(4244, 0));
    let held = std::fs::OpenOptions::new()
        .write(true)
        .open(runtime.join("host.lock"))
        .unwrap();
    held.try_lock().unwrap();
    let r = run(true, false);
    let i = runtime_step(&r, &runtime);
    assert_eq!(r.steps[i].ok, Some(true), "{:?}", r.steps[i]);
    let note = r.steps[i].note.clone().unwrap_or_default();
    for kept in ["app-endpoint.json", "host.lock", "memory/4244.json"] {
        assert!(note.contains(kept), "{kept} in the note {note:?}");
    }
    assert!(note.contains("running"), "{note:?}");
    assert_eq!(
        tree(&runtime),
        [
            "app-endpoint.json",
            "host.lock",
            "memory/",
            "memory/4244.json",
            "notes.txt"
        ],
        "{}",
        r.text()
    );

    // ---- They have gone: the rest goes, and the folder stays only for what is not JETCARP's.
    drop(held);
    drop(app);
    write(&runtime.join("memory/4244.json"), &record(4244, 120_000));
    let r = run(true, false);
    let i = runtime_step(&r, &runtime);
    assert_eq!(r.steps[i].ok, Some(true), "{:?}", r.steps[i]);
    assert_eq!(tree(&runtime), ["notes.txt"], "{}", r.text());
    assert_eq!(
        std::fs::read_to_string(runtime.join("notes.txt")).unwrap(),
        "mine"
    );

    // ---- With that gone too, the empty folder goes, and then there is nothing to undo.
    std::fs::remove_file(runtime.join("notes.txt")).unwrap();
    let plan = run(false, false);
    runtime_step(&plan, &runtime);
    let r = run(true, false);
    let i = runtime_step(&r, &runtime);
    assert_eq!(r.steps[i].ok, Some(true), "{:?}", r.steps[i]);
    assert!(!runtime.exists(), "{}", r.text());
    assert!(
        run(false, false).steps.is_empty(),
        "{}",
        run(false, false).text()
    );
    // A runtime folder not named `jetcarp` (one `JETCARP_RUNTIME_DIR` names) is emptied, not
    // removed.
    let scratch_run = d.join("scratch-run");
    write(
        &scratch_run.join("app-endpoint.json"),
        &endpoint(closed_port()),
    );
    set(&settings, &data, &scratch_run);
    let r = run(true, false);
    runtime_step(&r, &scratch_run);
    assert!(
        scratch_run.is_dir() && tree(&scratch_run).is_empty(),
        "{}",
        r.text()
    );
    assert!(
        run(false, false).steps.is_empty(),
        "{}",
        run(false, false).text()
    );

    // ---- Windows: the runtime folder is the default data folder (`%LOCALAPPDATA%\jetcarp`), with
    // the screen saver and the Start menu entry's launcher beside the library.
    let win = d.join("win").join("jetcarp");
    let win_settings = d.join("win-config");
    std::fs::create_dir_all(&win_settings).unwrap();
    set(&win_settings, &win, &win);
    let library = [
        "library.sqlite",
        "media/ab/ab12cd.png",
        "JETCARP.scr",
        "launcher/jetcarpw.exe",
        "launcher/jetcarp-program",
    ];
    for f in library {
        write(&win.join(f), "x");
    }
    let runtime_files = |w: &Path| {
        write(&w.join("host.lock"), "");
        write(&w.join("app-endpoint.json"), &endpoint(closed_port()));
        write(&w.join("memory/4242.json"), &record(4242, 120_000));
    };
    runtime_files(&win);
    // Without --data: the runtime files go; the library, its media, the screen saver and the
    // launcher stay, and so does the folder.
    let r = run(true, false);
    runtime_step(&r, &win);
    assert!(r.steps.iter().all(|s| s.ok == Some(true)), "{:?}", r.steps);
    assert_eq!(
        tree(&win),
        [
            "JETCARP.scr",
            "launcher/",
            "launcher/jetcarp-program",
            "launcher/jetcarpw.exe",
            "library.sqlite",
            "media/",
            "media/ab/",
            "media/ab/ab12cd.png",
        ],
        "{}",
        r.text()
    );
    assert!(r.kept_data.is_some(), "{}", r.text());
    // With --data: the runtime files go first, then the whole folder.
    runtime_files(&win);
    let r = run(true, true);
    assert!(r.steps.iter().all(|s| s.ok == Some(true)), "{:?}", r.steps);
    assert!(!win.exists(), "{}", r.text());
    let _ = std::fs::remove_dir_all(&d);
}
