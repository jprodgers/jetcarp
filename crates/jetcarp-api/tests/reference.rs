//! `reference.*`, `system.*`, and `files.list`: happy paths and error kinds.

mod common;

use common::{fails, keys, scratch};
use jetcarp_api::ctx::{Platform, UndoStep};
use jetcarp_api::op::{NoInput, Op, Quiet, Report, to_value};
use jetcarp_api::ops::files::{FilesIn, FilesList, FilesOut};
use jetcarp_api::ops::reference::*;
use jetcarp_api::ops::system::{SystemDoctor, SystemPaths, SystemUninstall, UninstallIn};
use jetcarp_api::{Ctx, ErrorKind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn rules_by_family() {
    let cx = Ctx::new();
    let all = ReferenceRules::run(&cx, RulesIn::default(), &Quiet).unwrap();
    assert_eq!(all.rules.len(), jetcarp_core::rules::catalog().len());
    assert!(all.families.contains(&"Life-like".to_string()));
    let t = all.text();
    assert!(
        t.starts_with(&format!("{:30} {:15} ", "Life", "B3/S23")),
        "{t}"
    );
    assert!(t.trim_end().ends_with("See the README for every family."));
    let gens = ReferenceRules::run(
        &cx,
        RulesIn {
            family: Some("GENERATIONS".into()),
        },
        &Quiet,
    )
    .unwrap();
    assert!(gens.rules.iter().any(|r| r.notation == "B2/S/C3"));
    assert!(gens.rules.iter().all(|r| r.family == "Generations"));
    assert_eq!(gens.families, all.families);
    let e = fails(
        ReferenceRules::run(
            &cx,
            RulesIn {
                family: Some("Q7".into()),
            },
            &Quiet,
        ),
        ErrorKind::NotFound,
    );
    assert!(e.message.contains("Families: Life-like"), "{e}");
}

#[test]
fn tilings_families_and_search() {
    let cx = Ctx::new();
    let t = ReferenceTilings::run(&cx, TilingsIn::default(), &Quiet).unwrap();
    assert_eq!(t.families.len(), jetcarp_core::families::FAMILIES.len());
    let sq = t.families.iter().find(|f| f.id == "square").unwrap();
    assert_eq!((sq.edge.clone(), sq.touch.clone()), (vec![4], vec![8]));
    assert_eq!(t.catalog.k_uniform, 1248);
    assert_eq!(t.catalog.isohedral, 81);
    assert!(t.text().lines().any(|l| l.starts_with("square ")));
    let s = ReferenceTilings::run(
        &cx,
        TilingsIn {
            search: Some(" K2- ".into()),
            limit: None,
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!(s.total_matches, Some(20));
    assert!(s.matches.as_ref().unwrap().iter().any(|m| m.id == "k2-005"));
    assert!(s.families.is_empty());
    assert!(
        s.text().starts_with("20 tilings match \"k2-\":"),
        "{}",
        s.text()
    );
    let few = ReferenceTilings::run(
        &cx,
        TilingsIn {
            search: Some("k2-".into()),
            limit: Some(3),
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!(few.matches.as_ref().unwrap().len(), 3);
    assert!(few.text().contains("(first 3 shown; "), "{}", few.text());
    let none = ReferenceTilings::run(
        &cx,
        TilingsIn {
            search: Some("zzzz".into()),
            limit: None,
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!(none.total_matches, Some(0));
}

#[test]
fn isohedral_types_keep_their_ids() {
    let t = ReferenceIsohedral::run(&Ctx::new(), NoInput {}, &Quiet).unwrap();
    assert_eq!(t.types.len(), 81);
    // Rhai's isohedral_tilings() lists these ids; they were "ih" + the type number.
    let ids: Vec<String> = jetcarp_core::isohedral::IH_TYPES
        .iter()
        .map(|t| format!("ih{:02}", t.num))
        .collect();
    assert_eq!(
        t.types.iter().map(|x| x.id.clone()).collect::<Vec<_>>(),
        ids
    );
    assert!(t.types.iter().all(|x| x.error.is_none()));
    assert!(t.text().starts_with("ih01 "), "{}", t.text());
}

#[test]
fn palettes_and_seeds() {
    let cx = Ctx::new();
    let p = ReferencePalettes::run(&cx, NoInput {}, &Quiet).unwrap();
    let names: Vec<&str> = jetcarp_core::render::palette_names().collect();
    assert_eq!(p.palettes.len(), names.len());
    let paper = p.palettes.iter().find(|x| x.name == "Paper").unwrap();
    assert!(paper.background.starts_with('#') && paper.background.len() == 7);
    assert!(!paper.stops.is_empty());
    assert_eq!(p.text().lines().count(), names.len());
    let s = ReferenceSeeds::run(&cx, NoInput {}, &Quiet).unwrap();
    assert!(s.forms.len() > 10);
    // Every pattern shape the list names is one the seed reader takes.
    for shape in &s.shapes {
        jetcarp_api::args::parse_seed(&serde_json::json!(format!("pattern:{shape}")))
            .unwrap_or_else(|e| panic!("{shape}: {e}"));
    }
    assert!(s.text().contains("Pattern shapes: all, disc"));
}

#[test]
fn paths_and_doctor() {
    let cx = Ctx::new();
    let p = SystemPaths::run(&cx, NoInput {}, &Quiet).unwrap();
    assert_eq!(p.data, jetcarp_core::paths::data_dir());
    assert_eq!(p.output, jetcarp_core::paths::output_dir());
    let t = p.text();
    let lines: Vec<&str> = t.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].starts_with("settings: ") && lines[1].starts_with("data:     "));
    // `jetcarp paths --json` prints these three keys, as it always has.
    assert_eq!(keys(&to_value(&p)), ["data", "output", "settings"]);
    // Without a platform there is no GPU and no desktop integration.
    let d = SystemDoctor::run(&cx, NoInput {}, &Quiet).unwrap();
    assert!(d.gpu.is_empty());
    assert_eq!(
        d.folders.library,
        jetcarp_core::library::Library::default_path()
    );
    let t = d.text();
    assert!(t.starts_with("JETCARP "), "{t}");
    assert!(t.contains("\nFolders\n  settings: "), "{t}");
    assert!(
        t.contains("  none found: simulation runs on the CPU\n"),
        "{t}"
    );
    assert!(
        t.contains("\nDesktop\n  background host: not running\n"),
        "{t}"
    );
    let v = to_value(&d);
    assert_eq!(
        keys(&v),
        [
            "arch",
            "desktop",
            "environment",
            "folders",
            "gpu",
            "os",
            "threads",
            "version"
        ]
    );
    assert!(v["folders"].get("library_bytes").is_none());
}

/// A platform whose undo steps count their runs; the second one fails.
struct Fake(Arc<AtomicUsize>);

impl Platform for Fake {
    fn undo_steps(&self) -> Vec<UndoStep> {
        let (a, b) = (self.0.clone(), self.0.clone());
        vec![
            UndoStep {
                what: "remove the test hook".into(),
                run: Box::new(move || {
                    a.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }),
            },
            UndoStep {
                what: "stop the test daemon".into(),
                run: Box::new(move || {
                    b.fetch_add(1, Ordering::SeqCst);
                    Err("nope".into())
                }),
            },
        ]
    }
}

#[test]
fn uninstall_lists_then_undoes() {
    let runs = Arc::new(AtomicUsize::new(0));
    let cx = Ctx::new().with_platform(Arc::new(Fake(runs.clone())));
    let plan = SystemUninstall::run(&cx, UninstallIn::default(), &Quiet).unwrap();
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    assert!(!plan.done);
    let t = plan.text();
    assert!(
        t.starts_with(
            "Would undo (run again with --yes):\n  remove the test hook\n  stop the test daemon\n"
        ),
        "{t}"
    );
    assert!(t.contains("are never deleted."));
    // Running the steps deletes nothing of the user's unless asked (settings, data), but it
    // forgets a recorded host command, so only do it in the tests' own settings folder.
    if std::env::var_os("JETCARP_CONFIG_DIR").is_none() {
        return;
    }
    let done = SystemUninstall::run(
        &cx,
        UninstallIn {
            yes: true,
            ..Default::default()
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!(runs.load(Ordering::SeqCst), 2);
    let t = done.text();
    assert!(
        t.contains("Undoing:\n  done: remove the test hook\n"),
        "{t}"
    );
    assert!(t.contains("  FAILED: stop the test daemon: nope\n"), "{t}");
    assert_eq!(done.steps[1].ok, Some(false));
}

#[test]
fn files_list_reads_inside_its_roots_only() {
    let dir = scratch("files");
    std::fs::write(dir.join("a.json"), "{\"x\": 1}").unwrap();
    std::fs::write(dir.join("b.png"), [0u8; 8]).unwrap();
    std::fs::create_dir_all(dir.join("sweep")).unwrap();
    std::fs::write(dir.join("sweep/manifest.json"), "{}").unwrap();
    let cx = Ctx::new()
        .with_out_root(Some(dir.clone()))
        .with_library(Some(dir.join("catalog")));
    let list = |a: FilesIn| FilesList::run(&cx, a, &Quiet);
    let FilesOut::Listing(l) = list(FilesIn {
        path: Some(dir.display().to_string()),
        ..Default::default()
    })
    .unwrap() else {
        panic!("not a listing")
    };
    assert_eq!(l.entries, 3);
    let sweep = l.newest_first.iter().find(|e| e.name == "sweep").unwrap();
    assert_eq!(sweep.kind, "dir");
    assert_eq!(sweep.has, Some(vec!["manifest.json".to_string()]));
    let a = l.newest_first.iter().find(|e| e.name == "a.json").unwrap();
    assert_eq!(a.bytes, Some(8));
    let FilesOut::Listing(l) = list(FilesIn {
        path: Some(dir.display().to_string()),
        limit: Some(1),
        ..Default::default()
    })
    .unwrap() else {
        panic!()
    };
    assert_eq!((l.entries, l.shown), (3, 1));
    let f = list(FilesIn {
        file: Some(dir.join("a.json").display().to_string()),
        ..Default::default()
    })
    .unwrap();
    assert!(f.text().ends_with(":\n{\"x\": 1}"), "{}", f.text());
    let FilesOut::File(ft) = f else { panic!() };
    assert_eq!((ft.bytes, ft.truncated), (8, false));
    // Errors: not text, not a folder, outside the roots, missing, a bad root.
    let file = |p: &std::path::Path| FilesIn {
        file: Some(p.display().to_string()),
        ..Default::default()
    };
    fails(list(file(&dir.join("b.png"))), ErrorKind::Invalid);
    fails(list(file(&dir.join("sweep"))), ErrorKind::Invalid);
    fails(
        list(FilesIn {
            path: Some(dir.join("a.json").display().to_string()),
            ..Default::default()
        }),
        ErrorKind::Invalid,
    );
    let outside = std::env::current_dir().unwrap();
    if !outside.starts_with(cx.output_root()) && !outside.starts_with(cx.library().folder()) {
        fails(
            list(FilesIn {
                path: Some(outside.display().to_string()),
                ..Default::default()
            }),
            ErrorKind::Io,
        );
    }
    fails(
        list(FilesIn {
            path: Some(dir.join("nope").display().to_string()),
            ..Default::default()
        }),
        ErrorKind::NotFound,
    );
    fails(
        list(FilesIn {
            root: Some("home".into()),
            ..Default::default()
        }),
        ErrorKind::Invalid,
    );
    let _ = std::fs::remove_dir_all(dir);
}
