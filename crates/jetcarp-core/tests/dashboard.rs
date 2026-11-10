//! The hunt dashboard's and the Quick Start menu's logic (Phase 4, deliverables 7 and 8): job
//! summaries, stop and resume across catalog handles (through the job runner's kinds and
//! handles), the heat map's binning, finds, ratings, the opening rows, search and filters, and an
//! entry's ratings and opens.

use jetcarp_core::dashboard::{self, HeatColour, Target};
use jetcarp_core::hunt::job::{self as hunt, HuntSpec};
use jetcarp_core::jobs::{self, RunOptions, Runner};
use jetcarp_core::library::{
    CurationState, HeatQuery, HeatRow, JobStatus, Library, QsStatus, QuickStartRow, UsageRow,
};
use jetcarp_core::qsmenu::{self, Filter, Origin, Sort};
use jetcarp_core::topology::{Nb, TilingSpec};
use std::collections::HashMap;

fn temp_lib(name: &str) -> Library {
    let dir = std::env::temp_dir().join(format!("jetcarp-dash-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Library::open(dir)
}

fn small_hunt(lib: &Library, soups: u64) -> i64 {
    let spec = HuntSpec {
        rules: vec!["B3/S23".into(), "B36/S23".into()],
        tilings: vec![TilingSpec::new("square")],
        soups: Some(soups),
        ..Default::default()
    };
    let job = hunt::start(lib, &spec).unwrap();
    let o = RunOptions {
        threads: 2,
        ..Default::default()
    };
    Runner::new(lib.clone()).run(job, &o).unwrap();
    job
}

#[test]
fn utc_round_trip_and_durations() {
    assert_eq!(dashboard::utc_seconds("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(
        dashboard::utc_seconds("2026-11-06T12:34:56Z"),
        Some(1_793_968_496)
    );
    let now = jetcarp_core::library::now_utc();
    let t = dashboard::utc_seconds(&now).unwrap();
    assert!((t - dashboard::now_seconds()).abs() <= 2);
    assert_eq!(dashboard::duration(3725.0), "1 h 2 min");
    assert_eq!(dashboard::duration(90061.0), "1 d 1 h");
    assert_eq!(dashboard::digits(1234567), "1,234,567");
}

#[test]
fn gray_order_puts_rule_neighbours_side_by_side() {
    for n in 0..512 {
        assert_eq!(dashboard::gray_inverse(dashboard::gray(n)), n);
    }
    // Neighbouring columns differ in exactly one birth count, rows in one survival count.
    for x in 0..255 {
        let (a, _) = dashboard::heat_masks(x, 0, 8);
        let (b, _) = dashboard::heat_masks(x + 1, 0, 8);
        assert_eq!((a ^ b).count_ones(), 1);
        assert_eq!((a | b) & 1, 0, "B0 is never shown");
    }
    for y in 0..511 {
        let (_, a) = dashboard::heat_masks(0, y, 8);
        let (_, b) = dashboard::heat_masks(0, y + 1, 8);
        assert_eq!((a ^ b).count_ones(), 1);
    }
    // Every square rule lands on its own cell, and the cell gives its masks back.
    let life = (1 << 3, (1 << 2) | (1 << 3));
    let (x, y) = dashboard::heat_cell(life.0, life.1, 8);
    assert_eq!(dashboard::heat_masks(x, y, 8), life);
}

#[test]
fn heat_map_bins_sum_finds_and_fold_high_counts() {
    let row = |b: u32, s: u32, finds: u64, ships: u64| HeatRow {
        birth: b,
        survive: s,
        notation: format!("{b}/{s}"),
        finds,
        ships,
        copies: finds,
    };
    let rows = vec![row(1 << 3, 0b1100, 10, 2), row(1 << 2, 0b0100, 5, 1)];
    let m = dashboard::heat_map(&rows, 8, HeatColour::Finds);
    assert_eq!((m.w, m.h, m.folded), (256, 512, false));
    let (x, y) = dashboard::heat_cell(1 << 3, 0b1100, 8);
    assert_eq!(m.at(x, y), 10.0);
    assert_eq!(m.rules_at(x, y), &[0]);
    assert_eq!(m.max, 10.0);
    // Hexagons: 6 neighbours, B1..B6 by S0..S6.
    let h = dashboard::heat_map(&rows, 6, HeatColour::Ships);
    assert_eq!((h.w, h.h), (64, 128));
    assert_eq!(h.values.iter().sum::<f64>(), 3.0);
    // Twelve neighbours fold B9+ and S9+ onto the first eight counts: these two rules share a
    // cell, which sums their finds.
    let tri = vec![
        row((1 << 3) | (1 << 10), 1 << 2, 1, 0),
        row(1 << 3, (1 << 2) | (1 << 11), 2, 0),
    ];
    let t = dashboard::heat_map(&tri, 12, HeatColour::Finds);
    assert!(t.folded);
    let (x, y) = dashboard::heat_cell(1 << 3, 1 << 2, 12);
    assert_eq!(t.at(x, y), 3.0);
    assert_eq!(t.rules_at(x, y).len(), 2);
    assert_eq!(
        dashboard::tiling_degree(&TilingSpec::new("hex"), Nb::Touch, &[]),
        6
    );
    assert_eq!(
        dashboard::tiling_degree(&TilingSpec::new("square"), Nb::Edge, &[]),
        4
    );
}

#[test]
fn hunt_jobs_summarise_stop_resume_and_feed_the_heat_map() {
    let lib = temp_lib("hunt");
    let job = small_hunt(&lib, 60);
    let rows = lib.jobs().unwrap();
    let sums = dashboard::summaries(&rows, dashboard::now_seconds());
    assert_eq!(sums.len(), 1, "job 0 (manual) is left out");
    let s = &sums[0];
    assert_eq!(
        (s.id, s.kind.as_str(), s.status.as_str()),
        (job, "hunt", "done")
    );
    assert_eq!((s.done, s.total), (120, Some(120)), "2 groups x 60 soups");
    assert!(s.title.contains("B3/S23"), "{}", s.title);
    assert!(s.bars.iter().any(|b| b.0 == "still life" && b.1 > 0));
    assert!(!s.can_resume());
    // A running job whose progress is old is stalled, and can be resumed here.
    let mut stale = rows.clone();
    for r in &mut stale {
        r.status = JobStatus::Running;
        r.updated = "2020-01-01T00:00:00Z".into();
    }
    let st = dashboard::summaries(&stale, dashboard::now_seconds());
    assert!(st[0].stale && st[0].can_resume() && !st[0].is_running());

    // Heat map: both rules on squares.
    let k = jobs::kind("hunt");
    let row = lib.job(job).unwrap();
    let spaces = k.heat_spaces(&row);
    assert_eq!(spaces.len(), 1);
    assert_eq!(spaces[0].query, HeatQuery::Patterns { job: Some(job) });
    let (map, heat) = dashboard::load_heat(&lib, &spaces[0], HeatColour::Finds).unwrap();
    assert_eq!(map.degree, 8);
    let mut rules: Vec<&str> = heat.iter().map(|r| r.notation.as_str()).collect();
    rules.sort();
    assert_eq!(rules, ["B3/S23", "B36/S23"]);
    let life = heat.iter().find(|r| r.notation == "B3/S23").unwrap();
    let (x, y) = dashboard::heat_cell(life.birth, life.survive, 8);
    assert_eq!(map.at(x, y), life.finds as f64);
    // Every job: the same rules.
    let all = lib
        .heat_rows(
            &HeatQuery::Patterns { job: None },
            &jetcarp_core::library::tiling_key(&TilingSpec::new("square")),
            Nb::Touch,
            2,
        )
        .unwrap();
    assert_eq!(all.len(), 2);

    // Finds: newest and rarest, filtered by rule.
    let f = k.finds(&lib, &row, &[], 50).unwrap();
    assert!(!f.newest.is_empty() && !f.other.is_empty());
    let seen = |t: &Target| match t {
        Target::Pattern(id) => lib.get(id).unwrap().seen,
    };
    let rare: Vec<u64> = f.other.iter().map(|i| seen(&i.target)).collect();
    assert!(
        rare.windows(2).all(|w| w[0] <= w[1]),
        "rarest first: {rare:?}"
    );
    let only = k.finds(&lib, &row, &["B36/S23".to_string()], 50).unwrap();
    assert!(only.newest.iter().all(|i| i.rule == "B36/S23"));

    // Rating and rejecting a find.
    let t = f.other[0].target.clone();
    dashboard::rate(&lib, &t, 5).unwrap();
    let Target::Pattern(id) = &t;
    assert_eq!(lib.get(id).unwrap().rating, Some(5));
    let r = lib.ratings(Some("pattern")).unwrap();
    assert_eq!(
        (r.len(), r[0].rater.as_str(), r[0].rating),
        (1, "user", 5.0)
    );
    dashboard::reject(&lib, &t).unwrap();
    assert_eq!(lib.get(id).unwrap().state, CurationState::Rejected);
    let after = k.finds(&lib, &row, &[], 500).unwrap();
    assert!(
        after.other.iter().all(|i| i.target != t),
        "rejected finds are hidden"
    );

    // Stop and resume through the kind, from another catalog handle (another process would
    // do the same through the file).
    let lib2 = Library::open(lib.path());
    let spec = HuntSpec {
        rules: vec!["B3/S23".into()],
        tilings: vec![TilingSpec::new("square")],
        soups: None,
        ..Default::default()
    };
    let forever = hunt::start(&lib, &spec).unwrap();
    let run = Runner::new(lib.clone())
        .spawn(forever, RunOptions::default())
        .unwrap();
    assert!(run.summary().here && run.summary().resumable);
    let t0 = std::time::Instant::now();
    while lib2.job(forever).unwrap().status != JobStatus::Running && t0.elapsed().as_secs() < 30 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    k.request_stop(&lib2, forever).unwrap();
    run.wait().unwrap();
    let j = lib2.job(forever).unwrap();
    assert_eq!(j.status, JobStatus::Paused);
    let s = dashboard::summaries(&[j], dashboard::now_seconds());
    assert!(s[0].can_resume() && s[0].total.is_none() && s[0].eta_s.is_none());
}

fn scene_row(id: &str, title: &str, status: QsStatus) -> QuickStartRow {
    let e = jetcarp_core::quickstart::entries()
        .into_iter()
        .find(|e| e.id == "rule:B3/S23")
        .unwrap();
    let mut q = QuickStartRow::for_scene(id, "scene", e.scene);
    q.title = title.into();
    q.status = status;
    q.behaviour = "soup".into();
    q
}

#[test]
fn menu_has_every_source_categories_and_search() {
    let lib = temp_lib("menu");
    lib.put_quickstart(&scene_row("my-life", "My Life soup", QsStatus::Approved))
        .unwrap();
    lib.put_quickstart(&scene_row("waiting", "Waiting", QsStatus::Candidate))
        .unwrap();
    let mut items = qsmenu::load_with(&lib, None);
    let n_built = jetcarp_core::quickstart::entries().len();
    // One approved scene and the built-ins; the candidate is hidden.
    assert_eq!(items.len(), n_built + 1);
    assert!(items.iter().all(|i| i.q.id != "find:waiting"));
    assert!(items.iter().all(|i| i.q.id.strip_prefix("find:").is_none_or(|x| x == "my-life")));
    let mine = items.iter().find(|i| i.q.id == "find:my-life").unwrap();
    assert_eq!(
        (mine.origin, mine.family.as_str(), mine.grid_group.as_str()),
        (Origin::MyFinds, "Life-like", "Squares")
    );
    // Built-ins get a category and a grid group.
    let ant = items.iter().find(|i| i.q.id == "langtons-ant").unwrap();
    assert_eq!(ant.behaviour, "Ants & turmites");
    let r30 = items.iter().find(|i| i.q.id == "rule:W30").unwrap();
    assert_eq!(r30.behaviour, "Fractals & rows");
    assert!(items.iter().any(|i| i.grid_group == "Aperiodic"));
    assert!(items.iter().any(|i| i.grid_group == "Archimedean"));
    assert!(items.iter().all(|i| i.origin != Origin::Atlas));

    // Search: every word must match, in any field.
    let f = |text: &str| Filter {
        text: text.into(),
        ..Default::default()
    };
    let hits = qsmenu::select(&items, &f("my life"), Sort::Featured);
    assert_eq!(hits.len(), 1);
    assert_eq!(items[hits[0]].q.id, "find:my-life");
    assert!(qsmenu::select(&items, &f("zzz-nothing"), Sort::Featured).is_empty());
    // Categories.
    let fam = Filter {
        family: "Life-like".into(),
        grid: "Squares".into(),
        origin: Some(Origin::MyFinds),
        ..Default::default()
    };
    let v = qsmenu::select(&items, &fam, Sort::Rule);
    assert_eq!(v.len(), 1);
    assert_eq!(items[v[0]].q.id, "find:my-life");
    let beh = qsmenu::facets(&items, |i| &i.behaviour);
    assert!(beh.iter().any(|(b, _)| b == "Classics"));
    // Sorts: rules ascend; a shuffle is reproducible and keeps every entry.
    let all = Filter::default();
    let rules: Vec<String> = qsmenu::select(&items, &all, Sort::Rule)
        .iter()
        .map(|&i| items[i].q.rule.clone())
        .collect();
    assert!(rules.windows(2).all(|w| w[0] <= w[1]));
    qsmenu::shuffle(&mut items, 7);
    let feat = qsmenu::select(&items, &all, Sort::Featured);
    assert_eq!(feat.len(), items.len());
    qsmenu::shuffle(&mut items, 7);
    assert_eq!(feat, qsmenu::select(&items, &all, Sort::Featured));

    // A user rating of an entry and its opens show in the next load.
    let row = lib.get_quickstart("my-life").unwrap();
    qsmenu::rate_entry(&lib, &row, 5).unwrap();
    assert!(qsmenu::rate_entry(&lib, &row, 6).is_err());
    let r = lib.ratings(Some("quickstart")).unwrap();
    assert_eq!(
        (r.len(), r[0].target_id.as_str(), r[0].rating, r[0].rater.as_str()),
        (1, "my-life", 5.0, "user")
    );
    lib.note_quickstart_open("find:my-life").unwrap();
    lib.note_quickstart_open("find:my-life").unwrap();
    let items = qsmenu::load_with(&lib, None);
    let top = &items[qsmenu::popular(&items, 1)[0]];
    assert_eq!(
        (top.q.id.as_str(), top.opens, top.rating),
        ("find:my-life", 2, Some(5.0))
    );
}

fn usage(id: &str, opens: u64, last: &str) -> UsageRow {
    UsageRow {
        entry_id: id.into(),
        opens,
        first_opened: Some("2026-01-01T00:00:00Z".into()),
        last_opened: Some(last.into()),
    }
}

#[test]
fn popular_recent_and_random_rows() {
    let mut items = qsmenu::built_in_items();
    let usage = vec![
        usage("life", 3, "2026-11-01T10:00:00Z"),
        usage("rule:B3/S23", 3, "2026-11-02T10:00:00Z"),
        usage("wireworld", 5, "2026-10-01T10:00:00Z"),
        usage("brians-brain", 1, "2026-11-05T10:00:00Z"),
    ];
    // A rating breaks the tie between the two entries opened three times.
    let mut ratings = HashMap::new();
    ratings.insert(("quickstart".to_string(), "life".to_string()), 5.0);
    qsmenu::apply_usage(&mut items, &usage, &ratings);
    fn ids_of(items: &[qsmenu::Item], v: Vec<usize>) -> Vec<String> {
        v.into_iter().map(|i| items[i].q.id.clone()).collect()
    }
    let ids = |v: Vec<usize>| ids_of(&items, v);
    assert_eq!(
        ids(qsmenu::popular(&items, 10)),
        ["wireworld", "life", "rule:B3/S23", "brians-brain"]
    );
    assert_eq!(
        ids(qsmenu::recent(&items, 2)),
        ["brians-brain", "rule:B3/S23"]
    );
    // Without the rating, the later open wins the tie.
    let mut plain = qsmenu::built_in_items();
    qsmenu::apply_usage(&mut plain, &usage, &HashMap::new());
    let p: Vec<String> = qsmenu::popular(&plain, 3)
        .into_iter()
        .map(|i| plain[i].q.id.clone())
        .collect();
    assert_eq!(p, ["wireworld", "rule:B3/S23", "life"]);
    // Opening counts at once.
    qsmenu::note_open(&mut items, "hex-wolfram", "2026-11-06T00:00:00Z");
    assert_eq!(items[qsmenu::recent(&items, 1)[0]].q.id, "hex-wolfram");
    let ids = |v: Vec<usize>| ids_of(&items, v);

    // Random: the same seed gives the same draw, another seed another; no repeats; the order
    // of the menu does not matter.
    let a = ids(qsmenu::random(&items, 42, 12));
    let b = ids(qsmenu::random(&items, 42, 12));
    let c = ids(qsmenu::random(&items, 43, 12));
    assert_eq!(a, b);
    assert_ne!(a, c);
    let mut d = a.clone();
    d.sort();
    d.dedup();
    assert_eq!(d.len(), 12);
    let mut rev = items.clone();
    rev.reverse();
    let r: Vec<String> = qsmenu::random(&rev, 42, 12)
        .into_iter()
        .map(|i| rev[i].q.id.clone())
        .collect();
    assert_eq!(r, a);
    assert_eq!(qsmenu::random(&items, 1, 100_000).len(), items.len());
}

#[test]
fn ants_pile_mover_and_evolve_jobs_have_their_own_kinds() {
    use jetcarp_core::ants::job::{self as antjob, AntSpec};
    use jetcarp_core::pile::job::{self as pilejob, PileJobSpec};
    let lib = temp_lib("kinds");
    let now = dashboard::now_seconds();

    // Ants: run to the end through the kind.
    let spec = AntSpec {
        tilings: vec![TilingSpec::new("square")],
        rules: vec!["Ant RL".into(), "Ant LLRR".into()],
        strings: None,
        steps: 20_000,
        ..Default::default()
    };
    let job = antjob::start(&lib, &spec).unwrap();
    let k = jobs::kind(antjob::KIND);
    assert!(k.runnable(&lib.job(job).unwrap()) && k.deterministic());
    let run = Runner::new(lib.clone())
        .spawn(job, RunOptions::default())
        .unwrap();
    run.wait().unwrap();
    let s = &dashboard::summaries(&[lib.job(job).unwrap()], now)[0];
    assert_eq!(s.kind_label, "Ant search");
    assert_eq!((s.unit.as_str(), s.done, s.total), ("runs", 2, Some(2)));
    assert!(
        s.title.contains("Ant RL") && s.title.contains("Square"),
        "{}",
        s.title
    );
    assert!(!s.bars.is_empty(), "outcomes");

    // A sandpile: run, then summarised from the row.
    let dir = std::env::temp_dir().join(format!("jetcarp-dash-pile-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let pspec = PileJobSpec {
        pile: jetcarp_core::pile::PileSpec {
            tiling: TilingSpec::new("square"),
            nb: Nb::Edge,
            grains: 3000,
            centre: jetcarp_core::pile::Centre::Cell,
        },
        folder: dir.clone(),
        px_per_cell: 2.0,
        margin: 0.0,
        palette: "Ember".into(),
        reverse: false,
        preview_side: 64,
        png: false,
        gpu: false,
        extra: serde_json::Value::Null,
    };
    let pj = pilejob::start(&lib, &pspec).unwrap();
    let run = Runner::new(lib.clone())
        .spawn(pj, RunOptions::default())
        .unwrap();
    run.wait().unwrap();
    let s = &dashboard::summaries(&[lib.job(pj).unwrap()], now)[0];
    assert_eq!(s.kind_label, "Sandpile");
    assert_eq!(s.status, "done");
    assert_eq!(s.title, "3,000 grains on Square");
    assert!(s.total.is_some() && s.done == s.total.unwrap());
    assert!(s.counters.iter().any(|c| c.0 == "topplings"));

    // A mover and an evolve search, from their saved progress.
    let mp = jetcarp_core::window::job::MoverProgress {
        job: 0,
        status: "paused".into(),
        generation: 2000,
        max_gens: 1_000_000,
        rings: 1000,
        target_rings: Some(10_000),
        level: 9,
        tiles: 900,
        rings_per_s: 5000.0,
        ..Default::default()
    };
    let mj = lib.create_job("mover", &serde_json::json!({})).unwrap();
    lib.update_job(
        mj,
        Some(JobStatus::Paused),
        Some(&serde_json::to_value(&mp).unwrap()),
        None,
    )
    .unwrap();
    let s = &dashboard::summaries(&[lib.job(mj).unwrap()], now)[0];
    assert_eq!(s.kind_label, "Mover (sliding window)");
    assert_eq!(
        (s.unit.as_str(), s.done, s.total),
        ("rings", 1000, Some(10_000))
    );
    assert!(s.can_resume());

    let ej = lib
        .create_job(
            "evolve",
            &serde_json::to_value(jetcarp_core::evolve::EvolveSpec::default()).unwrap(),
        )
        .unwrap();
    lib.update_job(
        ej,
        Some(JobStatus::Running),
        Some(
            &serde_json::json!({"generation": 12, "generations": 40, "best": 3.5,
            "best_ever": 4.25, "elapsed_s": 6.0}),
        ),
        None,
    )
    .unwrap();
    let s = &dashboard::summaries(&[lib.job(ej).unwrap()], now)[0];
    assert_eq!(s.kind_label, "Evolutionary search");
    assert_eq!((s.done, s.total), (12, Some(40)));
    assert!(
        s.counters
            .iter()
            .any(|c| c.0 == "best ever" && c.1 == "4.25")
    );
    // Evolve searches are a strategy now: resumable from their last generation.
    assert!(s.resumable);
    jobs::kind("evolve").request_stop(&lib, ej).unwrap();
    assert_eq!(lib.job(ej).unwrap().status, JobStatus::Paused);
    let _ = std::fs::remove_dir_all(&dir);
}
