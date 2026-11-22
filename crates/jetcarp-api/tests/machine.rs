//! `machine.*` (the hardware, the score and the limits) and the memory limit every world must
//! fit. The process's limits are the reference machine's here (16 GB, so JETCARP's part is 8 GB
//! and the largest world 4 GB), as in every test: the numbers below are the same everywhere.

mod common;

use common::{fails, scratch};
use jetcarp_api::op::{NoInput, Op, Quiet, Report, WorldOp};
use jetcarp_api::ops::machine::*;
use jetcarp_api::ops::world::{
    NewWorldIn, OpenIn, WorldApply, WorldNew, WorldOpen, check_world_memory,
};
use jetcarp_api::world::{Host, Session};
use jetcarp_api::{Ctx, ErrorKind, Limits, WorldMemory};
use jetcarp_core::machine::{Id, MachineFile, Source};
use jetcarp_core::{CaSpec, Scene, SizeSpec, TilingSpec};
use serde_json::json;

fn square(w: u32, h: u32) -> TilingSpec {
    let mut t = TilingSpec::new("square");
    t.size = Some(SizeSpec::Rect { w, h });
    t
}

#[test]
fn limits_set_and_reset_in_the_settings_folder() {
    let dir = scratch("machine-limits");
    let cx = Ctx::new().with_settings_dir(Some(dir.clone()));
    let l = MachineLimits::run(&cx, NoInput {}, &Quiet).unwrap();
    assert_eq!(l.entries.len(), Id::ALL.len());
    assert!(l.entries.iter().all(|e| e.source == Source::Suggested));
    assert!(!dir.join("limits.json").exists());
    let set = |name: &str, value: &str| {
        MachineLimitsSet::run(
            &cx,
            LimitSetIn {
                name: name.into(),
                value: value.into(),
            },
            &Quiet,
        )
    };
    // An AI's share of a quarter, and HashLife's nodes, kept as the person's own.
    let out = set("ai-share", "25%").unwrap();
    let ai = out.entries.iter().find(|e| e.id == Id::AiShare).unwrap();
    assert_eq!((ai.value, ai.source), (0.25, Source::Own));
    set("hashlife_nodes", "100M").unwrap();
    let text = std::fs::read_to_string(dir.join("limits.json")).unwrap();
    assert!(text.contains("\"ai-share\": 0.25"), "{text}");
    assert!(text.contains("\"hashlife-nodes\": 100000000"), "{text}");
    // A text report says where each value came from.
    let t = MachineLimits::run(&cx, NoInput {}, &Quiet).unwrap().text();
    assert!(t.contains("your own value; suggested small"), "{t}");
    // Refused: no such limit, a bad value, past the ceiling.
    fails(set("speed", "1"), ErrorKind::Invalid);
    fails(set("world", "lots"), ErrorKind::Invalid);
    let e = fails(set("ai-share", "2"), ErrorKind::Invalid);
    assert!(e.to_string().contains("must be small to all of it"), "{e}");
    // One back to the suggestion, then all.
    let one = MachineLimitsReset::run(
        &cx,
        LimitResetIn {
            name: Some("ai-share".into()),
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!(
        one.entries
            .iter()
            .find(|e| e.id == Id::AiShare)
            .unwrap()
            .source,
        Source::Suggested
    );
    assert_eq!(
        one.entries
            .iter()
            .find(|e| e.id == Id::HashlifeNodes)
            .unwrap()
            .value,
        1e8
    );
    MachineLimitsReset::run(&cx, LimitResetIn::default(), &Quiet).unwrap();
    assert!(!dir.join("limits.json").exists());
    // With no settings folder there is nowhere to keep them.
    let none = Ctx::new().with_settings_dir(None);
    if none.paths().settings.is_none() {
        fails(
            MachineLimitsSet::run(
                &none,
                LimitSetIn {
                    name: "world".into(),
                    value: "1GB".into(),
                },
                &Quiet,
            ),
            ErrorKind::Io,
        );
    }
}

/// A short run of the benchmark on the processor: every answer right, the score kept in
/// machine.json, a report in Markdown and JSON, and `machine.info` showing it.
#[test]
fn a_short_score_is_kept_and_reported() {
    let dir = scratch("machine-score");
    // Every folder of the test's own: the benchmark's scratch files go in the data folder, so
    // without one chosen here it would make the user's own (CI's default-folders step catches it).
    std::fs::create_dir_all(dir.join("settings")).unwrap();
    jetcarp_core::paths::set_chosen_data_dir(&dir.join("settings"), Some(&dir.join("data")))
        .unwrap();
    let cx = Ctx::new()
        .with_settings_dir(Some(dir.join("settings")))
        .with_out_root(Some(dir.join("out")));
    // `JETCARP_DATA_DIR`, when it is set (CI's main test step, a local run with scratch
    // folders), comes before the folder chosen here; either way, never the user's own.
    let want = std::env::var_os("JETCARP_DATA_DIR")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| dir.join("data"));
    assert_eq!(cx.paths().data, want);
    let info = MachineInfo::run(&cx, NoInput {}, &Quiet).unwrap();
    assert!(info.score.is_none());
    assert!(info.text().contains("not measured yet"), "{}", info.text());
    let out = MachineScore::run(
        &cx,
        ScoreIn {
            quick: true,
            gpu: Some(false),
            report: Some(dir.join("report").display().to_string()),
            save: None,
        },
        &Quiet,
    )
    .unwrap();
    assert!(out.check().is_ok());
    assert!(out.score.problems.is_empty(), "{:?}", out.score.problems);
    assert!(out.score.score > 0.0 && out.score.quick);
    assert_eq!(out.score.parts.iter().filter(|p| p.scored).count(), 5);
    let f = out.machine_file.clone().unwrap();
    assert!(f.ends_with("machine.json"));
    let kept = MachineFile::load(Some(&dir.join("settings"))).unwrap();
    assert_eq!(kept.score.as_ref().unwrap().score, out.score.score);
    assert!(!kept.fingerprint.is_empty());
    let md = std::fs::read_to_string(dir.join("report.md")).unwrap();
    assert!(md.starts_with("# JETCARP Hardware Score"), "{md}");
    assert!(md.contains("| Life soups on the hat (scored) |"), "{md}");
    assert!(md.contains("## The limits"), "{md}");
    let js: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("report.json")).unwrap()).unwrap();
    assert_eq!(js["score"]["benchmark"], 1);
    assert!(js["limits"]["entries"].as_array().unwrap().len() == Id::ALL.len());
    // machine.info and doctor show it.
    let info = MachineInfo::run(&cx, NoInput {}, &Quiet).unwrap();
    assert_eq!(info.score.as_ref().unwrap().score, out.score.score);
    assert!(info.stale.is_none());
    let doc = jetcarp_api::ops::system::SystemDoctor::run(&cx, NoInput {}, &Quiet).unwrap();
    let t = doc.text();
    assert!(t.contains("This computer\n  system:"), "{t}");
    assert!(t.contains("JETCARP Hardware Score: "), "{t}");
    assert!(t.contains("every limit: `jetcarp limits`"), "{t}");
}

/// Over the machine's memory limit, a world is refused before anything is built, naming what
/// it needs, the limit and where the limit came from.
#[test]
fn worlds_over_the_memory_limit_are_refused() {
    let cx = Ctx::new().with_limits(Limits::for_this_machine());
    let mut h = Session::new();
    // 30,000 x 30,000 Life cells take about 6.7 GB; the reference machine's largest world is 4 GB.
    let e = fails(
        WorldNew::run(
            &cx,
            &mut h,
            serde_json::from_value::<NewWorldIn>(json!({
                "tiling": "square", "width": 30000, "height": 30000, "cas": ["Life"]
            }))
            .unwrap(),
            &Quiet,
        ),
        ErrorKind::Limit,
    );
    assert_eq!(
        e.message,
        "this world needs 6.7 GB; this machine's limit is 4 GB (half of the 8 GB reserved for \
         JETCARP, from its hardware: Tools → This computer…, or `jetcarp limits`)"
    );
    // A scene the same size.
    let mut sc = Scene::new(square(30000, 30000));
    sc.cas.push(CaSpec::new("B3/S23"));
    fails(
        WorldOpen::run(
            &cx,
            &mut h,
            serde_json::from_value::<OpenIn>(json!({"scene": sc})).unwrap(),
            &Quiet,
        ),
        ErrorKind::Limit,
    );
    // Growing the world to it, and adding a CA that takes it over.
    let apply = |h: &mut Session, cmd: serde_json::Value| {
        WorldApply::run(
            &cx,
            h,
            serde_json::from_value(json!({"commands": [cmd]})).unwrap(),
            &Quiet,
        )
    };
    let big = serde_json::to_value(square(30000, 30000)).unwrap();
    fails(
        apply(&mut h, json!({"op": "set_tiling", "tiling": big})),
        ErrorKind::Limit,
    );
    let before = h.world().topo.len();
    assert!(before < 100_000, "nothing big was built");
    // Within the limit as it stands: 12,000 x 12,000 Life cells, 1.1 GB, is not built here; the
    // check alone passes it, and a Lenia CA on top (20 bytes a cell more) does not.
    let l = Limits::for_this_machine();
    let mid = square(12_000, 12_000);
    assert!(check_world_memory(&mid, &[CaSpec::new("B3/S23")], &l).is_ok());
    let e = check_world_memory(
        &mid,
        &[
            CaSpec::new("B3/S23"),
            CaSpec::new("Lenia"),
            CaSpec::new("Lenia"),
        ],
        &l,
    )
    .unwrap_err();
    assert!(e.message.starts_with("this world needs 6.4 GB"), "{e}");
    // Unlimited (tests, library users): only the engine's ceilings.
    assert!(check_world_memory(&square(30000, 30000), &[], &Limits::NONE).is_ok());
}

/// An AI's worlds must fit its share: small by default, memory enough for every world the MCP
/// tools allowed before (the cell limit decides there).
#[test]
fn an_ais_worlds_fit_its_share() {
    let ai = Limits::for_an_ai();
    assert_eq!(ai.world_memory, WorldMemory::Ai);
    assert_eq!((ai.max_cells, ai.max_grid_side), (4_000_000, 4096));
    // 4 million cells of the costliest kind, and a level-7 Spectre patch, fit.
    let lenia = [CaSpec::new("Lenia")];
    assert!(check_world_memory(&square(2000, 2000), &lenia, &ai).is_ok());
    let mut patch = TilingSpec::new("spectre");
    patch.size = Some(SizeSpec::Level(7));
    assert!(check_world_memory(&patch, &[CaSpec::new("B3/S23")], &ai).is_ok());
    // A world past the share's memory is refused, naming the share.
    let e = check_world_memory(&square(9000, 9000), &[CaSpec::new("B3/S23")], &ai).unwrap_err();
    assert!(
        e.message
            .starts_with("this world needs 618 MB; this machine's limit is 512 MB (an AI's share, small, of the machine's 4 GB"),
        "{e}"
    );
}

/// A world over the limit is refused with the reason, never made smaller to fit (#300): 100,000
/// x 100,000 squares (about 75 GB) from `world.new` and `set_tiling`, on the command line's
/// limits (the reference machine's 4 GB) and on an AI's (its share of them); nothing is built.
/// Within the limit, a world larger than any view draws cell by cell is built at the size asked.
#[test]
fn worlds_over_the_limit_are_refused_with_the_reason_never_made_smaller() {
    let new = |cx: &Ctx, h: &mut Session, w: u32, hh: u32| {
        WorldNew::run(
            cx,
            h,
            serde_json::from_value::<NewWorldIn>(json!({
                "tiling": "square", "width": w, "height": hh, "cas": ["Life"]
            }))
            .unwrap(),
            &Quiet,
        )
    };
    let set_tiling = |cx: &Ctx, h: &mut Session, w: u32, hh: u32| {
        let t = serde_json::to_value(square(w, hh)).unwrap();
        WorldApply::run(
            cx,
            h,
            serde_json::from_value(json!({"commands": [{"op": "set_tiling", "tiling": t}]}))
                .unwrap(),
            &Quiet,
        )
    };
    let machine = Ctx::new().with_limits(Limits::for_this_machine());
    let ai = Ctx::new().with_limits(Limits::for_an_ai());
    let mut h = Session::new();
    new(&machine, &mut h, 64, 64).unwrap();
    for cx in [&machine, &ai] {
        let e = fails(new(cx, &mut h, 100_000, 100_000), ErrorKind::Limit);
        let reason = |m: &str| {
            if cx.limits.world_memory == WorldMemory::Ai {
                m.contains("an AI's share, small, of this machine's limits")
                    && m.contains("Tools → This computer…, or `jetcarp limits`")
            } else {
                m.contains("this world needs 74.5 GB; this machine's limit is 4 GB (half of")
            }
        };
        assert!(reason(&e.message), "world.new: {e}");
        let e = fails(set_tiling(cx, &mut h, 100_000, 100_000), ErrorKind::Limit);
        assert!(reason(&e.message), "set_tiling: {e}");
        assert_eq!(h.world().topo.len(), 64 * 64, "nothing was built");
    }
    // An AI's grid over its cells, not its sides, says where that limit comes from too.
    let e = fails(new(&ai, &mut h, 3000, 3000), ErrorKind::Limit);
    assert!(
        e.message.starts_with(
            "3000 x 3000 on square is 9,000,000 cells, over the limit of 4,000,000 cells (try \
             2000 x 2000). It is an AI's share, small, of this machine's limits"
        ),
        "{e}"
    );
    // Within the limit: 20,000 x 1,000 squares, 20 million cells (about 150 MB), built as asked.
    let out = new(&machine, &mut h, 20_000, 1_000).unwrap();
    assert_eq!(out.status.cells, 20_000_000);
    let g = h.world().topo.lattice().unwrap();
    assert_eq!((g.w, g.h), (20_000, 1_000));
}
