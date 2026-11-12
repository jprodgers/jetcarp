//! First-run setup through the operations: `setup.status`, `setup.set_data_dir` (with
//! `paths.json`), and `setup.apply` for every profile on a scratch setup, with golden packs as
//! the packs that came with the program and Quick Start builds of a second or two; and
//! `quickstart.build`, stopped and carried on to its end.

mod common;

use common::{fails, scratch};
use jetcarp_api::ops::setup::{
    self, ApplyIn, DataDirIn, PackChoice, Profile, SetupApply, SetupSetDataDir,
};
use jetcarp_api::{Ctx, ErrorKind, Op, Quiet};
use jetcarp_search::jobs::{self, Budget, RunOptions, Runner};
use jetcarp_store::{JobStatus, Library};
use serde_json::json;
use std::path::{Path, PathBuf};

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../jetcarp-pack/tests/golden")
        .join(name)
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &to.join(e.file_name()));
        } else {
            std::fs::copy(&p, to.join(e.file_name())).unwrap();
        }
    }
}

/// A scratch setup: its own settings folder, library, output folder, and a bundled packs folder
/// holding the golden `minimal` pack as the starter pack and `media` beside it.
struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = scratch(&format!("setup-{name}"));
        copy_dir(&golden("minimal"), &dir.join("bundled/jetcarp-starter"));
        copy_dir(&golden("media"), &dir.join("bundled/extras"));
        Scratch { dir }
    }
    fn settings(&self) -> PathBuf {
        self.dir.join("settings")
    }
    fn library(&self) -> PathBuf {
        self.dir.join("data/library.sqlite")
    }
    fn ctx(&self) -> Ctx {
        Ctx::new()
            .with_settings_dir(Some(self.settings()))
            .with_library(Some(self.library()))
            .with_out_root(Some(self.dir.join("out")))
            .with_bundled_packs(Some(self.dir.join("bundled")))
    }
}

/// A Quick Start build of a moment: a few tilings and families, small grids.
fn tiny_search() -> jetcarp_api::ops::search::ExploreIn {
    serde_json::from_value(json!({
        "tilings": "square, hex", "families": ["life_like", "generations"],
        "screen_cells": 400, "screen_gens": 48,
        "confirm_cells": 900, "confirm_gens": 64, "confirm_seeds": 2, "seed": 5,
    }))
    .unwrap()
}

fn apply_in(profile: &str) -> ApplyIn {
    ApplyIn {
        profile: profile.into(),
        quickstart_minutes: Some(0.03),
        search: Some(tiny_search()),
        verify: Some("none".into()),
        threads: Some(2),
        gpu: true,
        ..Default::default()
    }
}

#[test]
fn status_before_setup() {
    let s = Scratch::new("status");
    let st = setup::status(&s.ctx());
    assert!(!st.done);
    assert!(st.record.is_none());
    assert_eq!(st.folders.settings.as_deref(), Some(s.settings().as_path()));
    assert_eq!(Path::new(&st.library), s.library());
    assert!(
        st.free_bytes.is_some_and(|b| b > 0),
        "free space on the disk"
    );
    // The bundled packs, with their feeds, titles and sizes; the starter pack by its name.
    let ids: Vec<String> = st.bundled.iter().map(|e| e.id()).collect();
    assert_eq!(ids, ["extras/golden-m", "jetcarp-starter/golden-a"]);
    let starter: Vec<bool> = st.bundled.iter().map(|e| e.starter).collect();
    assert_eq!(starter, [false, true]);
    let a = &st.bundled[1];
    assert_eq!(
        (a.title.as_str(), a.patterns, a.quickstart),
        ("minimal", 3, 1)
    );
    assert!(
        a.bytes > 10_000 && !a.imported && a.error.is_none(),
        "{a:?}"
    );
    assert!(a.manifest.starts_with("sha256:"));
    // No GPU in a test context: the build runs on the CPU, and the status says why.
    assert!(!st.gpu.usable);
    assert!(!st.gpu.note.is_empty());
    assert_eq!(st.counts.patterns, 0);
    // The five profiles, with what each takes here.
    let names: Vec<&str> = st.profiles.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "desktop-art",
            "explorer",
            "researcher",
            "everything",
            "blank-slate"
        ]
    );
    let explorer = st.info(Profile::Explorer).unwrap();
    assert_eq!(explorer.quickstart_minutes, 10.0);
    assert_eq!(explorer.packs, PackChoice::Starter);
    assert!(explorer.build_optional && !explorer.chooses_data_dir);
    assert!(explorer.bytes > 0 && explorer.cost.starts_with("about "));
    let blank = st.info(Profile::BlankSlate).unwrap();
    assert_eq!((blank.bytes, blank.minutes), (0, 0.0));
    assert_eq!(blank.cost, "no extra disk space or time");
    let every = st.info(Profile::Everything).unwrap();
    assert!(every.desktop_offers && every.chooses_data_dir && every.chooses_packs);
    assert!(every.bytes > explorer.bytes);
    // The text names them all.
    let t = jetcarp_api::Report::text(&st);
    assert!(t.contains("setup: not run yet"), "{t}");
    assert!(t.contains("the starter pack"), "{t}");
    // With no starter named, every bundled pack is the starter pack.
    std::fs::rename(
        s.dir.join("bundled/jetcarp-starter"),
        s.dir.join("bundled/a"),
    )
    .unwrap();
    let st = setup::status(&s.ctx());
    assert!(st.bundled.iter().all(|e| e.starter));
    // No bundled packs at all (as after `cargo install`).
    let st = setup::status(&s.ctx().with_bundled_packs(Some(s.dir.join("nowhere"))));
    assert!(st.bundled.is_empty());
}

#[test]
fn a_data_folder_is_chosen_in_paths_json() {
    let s = Scratch::new("data-dir");
    let cx = s.ctx();
    let env_set = std::env::var_os("JETCARP_DATA_DIR").is_some_and(|v| !v.is_empty());
    let big = s.dir.join("big disk/jetcarp");
    // A check alone records nothing.
    let o = SetupSetDataDir::run(
        &cx,
        DataDirIn {
            path: Some(big.display().to_string()),
            dry_run: true,
        },
        &Quiet,
    )
    .unwrap();
    assert!(o.dry_run && o.file.is_none());
    assert!(!s.settings().join("paths.json").exists());
    let o = SetupSetDataDir::run(
        &cx,
        DataDirIn {
            path: Some(big.display().to_string()),
            dry_run: false,
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!(Path::new(&o.data), big);
    assert!(!o.has_library);
    assert_eq!(o.effective, !env_set);
    assert_eq!(
        o.file.as_deref().map(Path::new),
        Some(s.settings().join("paths.json").as_path())
    );
    assert!(big.is_dir(), "the folder is made");
    assert_eq!(
        jetcarp_core::paths::chosen_data_dir(&s.settings()),
        Some(big.clone())
    );
    // Every folder lookup honours it (unless JETCARP_DATA_DIR is set, which wins).
    if !env_set {
        assert_eq!(cx.paths().data, big);
        let free = Ctx::new().with_settings_dir(Some(s.settings()));
        assert_eq!(free.library_path_now(), big.join("library.sqlite"));
    }
    // A folder that is not empty and holds no library is refused, and nothing changes.
    let full = s.dir.join("photos");
    std::fs::create_dir_all(&full).unwrap();
    std::fs::write(full.join("holiday.jpg"), b"jpeg").unwrap();
    std::fs::write(full.join(".DS_Store"), b"").unwrap();
    let e = fails(
        SetupSetDataDir::run(
            &cx,
            DataDirIn {
                path: Some(full.display().to_string()),
                dry_run: false,
            },
            &Quiet,
        ),
        ErrorKind::Conflict,
    );
    assert!(e.message.contains("holiday.jpg"), "{e}");
    assert!(
        e.hint.as_deref().unwrap_or("").contains("never moves"),
        "{e:?}"
    );
    assert_eq!(
        jetcarp_core::paths::chosen_data_dir(&s.settings()),
        Some(big.clone())
    );
    // A folder that holds a JETCARP library is taken as it is.
    let other = s.dir.join("other data");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("library.sqlite"), b"").unwrap();
    std::fs::write(other.join("notes.txt"), b"mine").unwrap();
    let o = SetupSetDataDir::run(
        &cx,
        DataDirIn {
            path: Some(other.display().to_string()),
            dry_run: false,
        },
        &Quiet,
    )
    .unwrap();
    assert!(o.has_library);
    // A file is not a folder.
    fails(
        SetupSetDataDir::run(
            &cx,
            DataDirIn {
                path: Some(full.join("holiday.jpg").display().to_string()),
                dry_run: false,
            },
            &Quiet,
        ),
        ErrorKind::Invalid,
    );
    // Back to the default: paths.json goes.
    let o = SetupSetDataDir::run(&cx, DataDirIn::default(), &Quiet).unwrap();
    assert!(o.file.is_none());
    assert!(!s.settings().join("paths.json").exists());
    assert_eq!(Path::new(&o.data), jetcarp_core::paths::default_data_dir());
}

/// The jobs of a library by kind.
fn kinds(lib: &Library) -> Vec<String> {
    let mut v: Vec<String> = lib.jobs().unwrap().into_iter().map(|j| j.kind).collect();
    v.sort();
    v
}

#[test]
fn a_dry_run_says_what_it_would_do_and_does_nothing() {
    let s = Scratch::new("dry");
    let a = SetupApply::run(
        &s.ctx(),
        ApplyIn {
            dry_run: true,
            ..apply_in("everything")
        },
        &Quiet,
    )
    .unwrap();
    assert!(a.dry_run && a.jobs.is_empty() && a.record.is_none());
    let kinds_of: Vec<&str> = a.steps.iter().map(|s| s.kind.as_str()).collect();
    assert_eq!(
        kinds_of,
        [
            "record",
            "pack_import",
            "pack_import",
            "quickstart_build",
            "desktop"
        ]
    );
    assert!(a.steps[3].what.contains("on the CPU"), "{:?}", a.steps[3]);
    assert!(!s.settings().join(setup::RECORD_FILE).exists());
    assert!(!s.library().exists(), "nothing written to the library");
    // A bad profile, a bad pack folder.
    fails(
        SetupApply::run(&s.ctx(), apply_in("gardener"), &Quiet),
        ErrorKind::Invalid,
    );
    let empty = s.dir.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    fails(
        SetupApply::run(
            &s.ctx(),
            ApplyIn {
                packs: vec![empty.display().to_string()],
                dry_run: true,
                ..apply_in("researcher")
            },
            &Quiet,
        ),
        ErrorKind::NotFound,
    );
}

#[test]
fn every_profile_applies() {
    for p in Profile::ALL {
        let s = Scratch::new(p.name());
        let cx = s.ctx();
        let mut input = apply_in(p.name());
        if p.quickstart_minutes() == 0.0 {
            // Its own: none (a minutes given would build all the same).
            input.quickstart_minutes = None;
        }
        let data = s.dir.join("big disk");
        let own = s.dir.join("my packs");
        if p.chooses_packs() {
            copy_dir(&golden("searches"), &own.join("searches"));
            input.packs = vec![own.display().to_string()];
        }
        if p.chooses_data_dir() {
            input.data_dir = Some(data.display().to_string());
        }
        let a = SetupApply::run(&cx, input, &Quiet).unwrap();
        assert_eq!(a.profile, p.name());
        assert_eq!(a.desktop_offers, p.desktop_offers(), "{p:?}");
        let rec = setup::record(&s.settings()).expect("recorded");
        assert_eq!(rec.profile, p.name());
        assert_eq!(rec.jobs, a.jobs.iter().map(|j| j.job).collect::<Vec<_>>());
        let lib = Library::open(s.library());
        let want: Vec<&str> = match p {
            Profile::DesktopArt | Profile::BlankSlate => vec![],
            Profile::Explorer => vec!["explore", "pack_import"],
            Profile::Researcher => vec!["explore", "pack_import", "pack_import"],
            Profile::Everything => vec!["explore", "pack_import", "pack_import", "pack_import"],
        };
        // (A pack's search comes in as a paused explore job of its own.)
        let mut got = kinds(&lib);
        if p.chooses_packs() {
            let i = got.iter().position(|k| k == "explore").unwrap();
            got.remove(i);
        }
        let mut setup_kinds: Vec<String> = a.jobs.iter().map(|j| j.kind.clone()).collect();
        setup_kinds.sort();
        assert_eq!(setup_kinds, want, "{p:?}");
        assert!(got.len() >= want.len(), "{p:?}: {got:?}");
        for j in &a.jobs {
            assert_eq!(j.status, "done", "{p:?}: {j:?}");
        }
        let qs = lib.quickstart_counts().unwrap();
        if p.packs() != PackChoice::None {
            assert!(qs.get("pack").is_some_and(|n| *n >= 1), "{p:?}: {qs:?}");
            // The build ended and published its atlas (perhaps empty, so short a search).
            let build = a.jobs.iter().find(|j| j.kind == "explore").unwrap();
            let row = lib.job(build.job).unwrap();
            assert_eq!(row.status, JobStatus::Done);
            assert_eq!(row.label(), Some(setup::BUILD_LABEL));
            assert!(row.progress["published"].is_object(), "{}", row.progress);
            assert!(
                build.line.starts_with("Quick Start build"),
                "{}",
                build.line
            );
            assert_eq!(build.fraction, Some(1.0));
        } else {
            assert!(qs.get("pack").is_none(), "{p:?}");
        }
        if p.chooses_data_dir() {
            assert_eq!(
                jetcarp_core::paths::chosen_data_dir(&s.settings()),
                Some(data.clone())
            );
            assert!(a.steps[0].what.contains("big disk"), "{:?}", a.steps[0]);
        } else {
            assert!(!s.settings().join("paths.json").exists(), "{p:?}");
        }
        // The status reads the record and the jobs back.
        let st = setup::status(&cx);
        assert!(st.done);
        assert_eq!(st.profile(), Some(p));
        assert_eq!(st.jobs.len(), a.jobs.len());
        if p == Profile::Explorer {
            // Again: the starter pack is in the library already, so only a build runs.
            let b = SetupApply::run(&cx, apply_in("explorer"), &Quiet).unwrap();
            assert!(
                b.steps
                    .iter()
                    .any(|s| s.kind == "skip" && s.what.contains("in the library already")),
                "{:?}",
                b.steps
            );
            let k: Vec<&str> = b.jobs.iter().map(|j| j.kind.as_str()).collect();
            assert_eq!(k, ["explore"]);
            assert!(st.bundled.iter().any(|e| e.starter && e.imported));
        }
    }
}

#[test]
fn setup_jobs_run_in_the_background() {
    let s = Scratch::new("background");
    let cx = s.ctx();
    let a = SetupApply::run(
        &cx,
        ApplyIn {
            background: true,
            ..apply_in("explorer")
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!(a.jobs.len(), 2);
    let lib = Library::open(s.library());
    for j in &a.jobs {
        let out = cx.registry().wait(&lib, j.job).expect("a run here");
        assert_eq!(out.unwrap().status, JobStatus::Done, "{j:?}");
        let now = setup::setup_job(&cx, &lib, j.job).unwrap();
        assert!(now.settled() && now.status == "done", "{now:?}");
    }
}

#[test]
fn a_quick_start_build_stops_and_carries_on_to_its_end() {
    use jetcarp_api::ops::quickstart::BuildIn;
    let dir = scratch("qs-build");
    let lib = Library::open(dir.join("lib"));
    let b = BuildIn {
        minutes: Some(0.04),
        search: tiny_search(),
        ..Default::default()
    };
    let js = b.job_spec().unwrap();
    assert_eq!(js.label.as_deref(), Some(setup::BUILD_LABEL));
    assert_eq!(js.spec["build"]["minutes"], 0.04);
    assert_eq!(js.spec["gpu"], true);
    let job = jobs::create(&lib, &js).unwrap();
    // Stopped by an item budget after a few runs: paused, its minutes not yet spent.
    let o = RunOptions {
        threads: 2,
        budget: Budget::items(20),
        ..Default::default()
    };
    let out = Runner::new(lib.clone()).run(job, &o).unwrap();
    assert_eq!(out.status, JobStatus::Paused);
    assert!(out.progress["published"].is_null());
    let first = out.progress["elapsed_s"].as_f64().unwrap();
    assert!(first < 0.04 * 60.0, "{first}");
    // Carried on: to its minutes over both sessions, then published.
    let out = Runner::new(lib.clone())
        .run(
            job,
            &RunOptions {
                threads: 2,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(out.status, JobStatus::Done);
    let elapsed = out.progress["elapsed_s"].as_f64().unwrap();
    assert!(elapsed >= 0.04 * 60.0 - 0.01, "{elapsed}");
    let p = &out.progress["published"];
    assert!(p["entries"].as_u64().is_some(), "{}", out.progress);
    let atlas = lib
        .quickstart_counts()
        .unwrap()
        .get("atlas")
        .copied()
        .unwrap_or(0);
    assert_eq!(atlas, p["entries"].as_u64().unwrap());
    // The dashboard calls it what it is.
    let row = lib.job(job).unwrap();
    let sum = jobs::kind("explore").summary(&row);
    assert_eq!(sum.kind_label, "Quick Start build");
    // Bad minutes.
    for m in [0.0, -1.0, f64::NAN] {
        let bad = BuildIn {
            minutes: Some(m),
            ..Default::default()
        };
        assert!(bad.job_spec().is_err(), "{m}");
    }
}

#[test]
fn a_pack_import_job_reports_and_runs_again_harmlessly() {
    let s = Scratch::new("import-job");
    let lib = Library::open(s.library());
    let spec = json!({
        "path": s.dir.join("bundled/jetcarp-starter").display().to_string(),
        "title": "Starter",
        "verify": "none",
    });
    let js = jobs::JobSpec {
        strategy: "pack_import".into(),
        spec,
        label: Some("setup: Starter".into()),
        budget: Default::default(),
    };
    // Registered when a context is made.
    let _ = Ctx::new();
    let job = jobs::create(&lib, &js).unwrap();
    let out = Runner::new(lib.clone())
        .run(job, &RunOptions::default())
        .unwrap();
    assert_eq!(out.status, JobStatus::Done);
    let r = &out.progress["report"];
    assert_eq!(r["patterns_added"], 3, "{}", out.progress);
    assert_eq!(r["quickstart"], 1);
    assert!(
        out.progress["line"]
            .as_str()
            .unwrap()
            .contains("Starter imported: 3 patterns new"),
        "{}",
        out.progress
    );
    let k = jobs::kind("pack_import");
    assert_eq!(k.label(), "Pack import");
    let sum = k.summary(&lib.job(job).unwrap());
    assert_eq!((sum.done, sum.total), (1, Some(1)));
    // A second job for the same pack adds nothing.
    let job2 = jobs::create(&lib, &js).unwrap();
    let out = Runner::new(lib.clone())
        .run(job2, &RunOptions::default())
        .unwrap();
    assert_eq!(out.progress["report"]["patterns_added"], 0);
    assert_eq!(out.progress["report"]["patterns_merged"], 3);
    // A relative path is refused; a missing pack fails the job with the reason.
    let bad = |spec| {
        jobs::create(
            &lib,
            &jobs::JobSpec {
                strategy: "pack_import".into(),
                spec,
                ..Default::default()
            },
        )
    };
    assert!(bad(json!({"path": "relative/pack"})).is_err());
    let gone = bad(json!({"path": s.dir.join("gone").display().to_string()})).unwrap();
    let r = Runner::new(lib.clone()).run(gone, &RunOptions::default());
    assert!(r.is_err());
    assert_eq!(lib.job(gone).unwrap().status, JobStatus::Failed);
}
