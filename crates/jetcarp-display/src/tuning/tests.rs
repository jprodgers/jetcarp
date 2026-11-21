use super::*;

/// A made-up machine: the load grows with the cell count, and frames with it.
fn model(cpu_per_mcell: f64, gpu_per_mcell: f64) -> impl Fn(u64) -> Measure {
    move |cells: u64| {
        let mc = cells as f64 / 1e6;
        Measure {
            cpu: cpu_per_mcell * mc,
            gpu: Some(gpu_per_mcell * mc),
            p95: 0.002 + 0.005 * mc,
            fps: 30.0,
            gens_wanted: Some(60.0),
            gens_done: 60.0,
            on_gpu: true,
            gpu_timing: GpuTiming::Timestamps,
            cells,
        }
    }
}

fn limits() -> SceneLimits {
    SceneLimits::of(&TuneLimits::default(), 30.0, 1.0)
}

/// Runs a screen's search on a made-up machine; returns it and the sizes tried, in order.
fn search_on(
    canvas: [f64; 2],
    design: f32,
    m: impl Fn(u64) -> Measure,
    l: &SceneLimits,
    first: Option<Size>,
    keep: Option<Size>,
) -> (Search, Vec<f32>) {
    let mut s = Search::new(
        lattice_ladder(canvas, design, SWEEP_MAX_CELLS),
        Size::px(design),
        first,
        keep,
    );
    let mut order = vec![];
    while let Some(i) = s.next() {
        let size = s.sizes[i];
        order.push(size.px);
        s.record(i, size.px, &m(lattice_cells(canvas, size.px)), l);
    }
    (s, order)
}

/// The finest size within the limits, trying every size: what the search must find.
fn every_size(canvas: [f64; 2], design: f32, m: &dyn Fn(u64) -> Measure, l: &SceneLimits) -> Size {
    lattice_ladder(canvas, design, SWEEP_MAX_CELLS)
        .into_iter()
        .find(|s| fits(&m(lattice_cells(canvas, s.px)), l).is_ok())
        .unwrap_or_else(|| {
            *lattice_ladder(canvas, design, SWEEP_MAX_CELLS)
                .last()
                .unwrap()
        })
}

#[test]
fn the_ladder_is_in_physical_pixels_for_any_screen() {
    let px = |v: Vec<Size>| v.into_iter().map(|s| s.px).collect::<Vec<_>>();
    // A 2560 x 1440 screen at 1x: 1 px is 3.7 million cells, within the 4 million.
    assert_eq!(
        px(lattice_ladder([2560.0, 1440.0], 3.0, SWEEP_MAX_CELLS)),
        [1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0, 24.0]
    );
    // The same screen at 2x (5120 x 2880 physical): 1 and 2 px would be 14.7 and 3.7 million
    // cells; 2 px still fits.
    assert_eq!(
        px(lattice_ladder([5120.0, 2880.0], 6.0, SWEEP_MAX_CELLS))[..3],
        [2.0, 3.0, 4.0]
    );
    // A 3024 x 1964 Retina laptop: 1 px is 5.9 million; and its scene's own 2.5 px joins in.
    let mac = px(lattice_ladder([3024.0, 1964.0], 2.5, SWEEP_MAX_CELLS));
    assert_eq!(mac[..3], [2.0, 2.5, 3.0]);
    assert_eq!(
        *mac.last().unwrap(),
        32.0,
        "the coarsest keeps 60 cells across"
    );
    // A small screen stops early at the coarse end; a GPU with less room at the fine end.
    assert_eq!(
        px(lattice_ladder([1366.0, 768.0], 4.0, SWEEP_MAX_CELLS)).last(),
        Some(&12.0)
    );
    assert_eq!(
        px(lattice_ladder([2560.0, 1440.0], 3.0, 500_000))[0],
        3.0,
        "at most half a million cells: 3 px is the finest"
    );
    // A scene whose own size is beyond the cut keeps it.
    assert!(px(lattice_ladder([1280.0, 720.0], 40.0, SWEEP_MAX_CELLS)).contains(&40.0));
    assert_eq!(coarsest_px([1920.0, 1080.0]), 18.0);
    assert_eq!(coarsest_px([20000.0, 10000.0]), 64.0);
}

#[test]
fn a_patch_s_ladder_is_its_levels() {
    let v = patch_ladder("penrose-rhomb", [2560.0, 1440.0], Some(5), SWEEP_MAX_CELLS);
    let levels: Vec<u32> = v.iter().filter_map(|s| s.level).collect();
    // 4,000, 35,000 and 280,000 tiles; 2.24 million would be under a pixel a tile at 1440.
    assert_eq!(levels, [6, 5, 4], "finest first");
    assert!(v.windows(2).all(|w| w[0].px <= w[1].px));
    // A 4K screen has room for level 7's 1.3-pixel tiles.
    let k4 = patch_ladder("penrose-rhomb", [3840.0, 2160.0], None, SWEEP_MAX_CELLS);
    assert_eq!(k4[0].level, Some(7));
    assert!(k4[0].px > 1.0 && k4[0].px < 2.0, "{}", k4[0].px);
    // The scene's own level is always there, however small.
    let own = patch_ladder("spectre", [1280.0, 720.0], Some(2), SWEEP_MAX_CELLS);
    assert!(own.iter().any(|s| s.level == Some(2)));
    assert_eq!(
        cells_at("penrose-rhomb", [2560.0, 1440.0], &v[0]),
        jetcarp_core::aperiodic::tile_count("penrose-rhomb", 6).unwrap() as u64
    );
}

/// A fast fit (#113): a binary search finds the size trying every size would, in at most
/// [`MAX_TRIALS`] and usually four or five, on any machine from one with room for a cell per
/// pixel to one where nothing fits.
#[test]
fn a_binary_search_finds_the_finest_size_in_a_few_trials() {
    let canvas = [2560.0, 1440.0];
    let l = limits();
    let mut most = 0;
    for k in 0..60 {
        // From 0.005 to 40 of the machine per million cells.
        let per = 0.005 * 1.17f64.powi(k);
        let m = model(per, per / 2.0);
        let (s, order) = search_on(canvas, 3.0, &m, &l, None, None);
        let want = every_size(canvas, 3.0, &m, &l);
        assert!(
            s.recommended().0.same(&want),
            "{per}: {:?} against {want:?}, tried {order:?}",
            s.recommended()
        );
        assert!(order.len() <= 5, "{per}: {order:?}");
        most = most.max(order.len());
    }
    // Nine sizes: four or five trials, never nine.
    assert!(most <= 5, "{most}");
    // A machine where every size fits: 1 px, in four trials.
    let (s, order) = search_on(canvas, 3.0, model(0.001, 0.001), &l, None, None);
    assert_eq!(s.recommended().0, Size::px(1.0));
    assert!(order.len() <= 4, "{order:?}");
    // Nothing fits: the coarsest, with a card for it.
    let (s, _) = search_on(canvas, 3.0, model(1000.0, 1000.0), &l, None, None);
    let (size, card) = s.recommended();
    assert_eq!(size, Size::px(24.0));
    assert!(card.is_some_and(|c| !c.fits));
    assert!(!s.any_fit());
}

/// A good first guess (the size found last time, or one from the machine's score) settles it in
/// two trials: the guess, and the size one finer.
#[test]
fn a_good_guess_settles_in_two_trials() {
    let canvas = [2560.0, 1440.0];
    // 0.4 of the machine per million cells: under 0.2 million cells fit, so 6 px (102,400) is
    // the finest within the limits and 4 px (230,400) is over.
    let m = model(0.4, 0.1);
    let (s, order) = search_on(canvas, 3.0, &m, &limits(), Some(Size::px(6.0)), None);
    assert_eq!(order, [6.0, 4.0]);
    assert_eq!(s.recommended().0, Size::px(6.0));
    // A guess one size too fine: the next coarser settles it.
    let (s, order) = search_on(canvas, 3.0, &m, &limits(), Some(Size::px(4.0)), None);
    assert_eq!(order, [4.0, 6.0]);
    assert_eq!(s.recommended().0, Size::px(6.0));
    // A bad guess costs a few more, never a wrong answer.
    let (s, order) = search_on(canvas, 3.0, &m, &limits(), Some(Size::px(1.0)), None);
    assert_eq!(s.recommended().0, Size::px(6.0));
    assert!(order.len() <= 5, "{order:?}");
    // 1 px (3.7 million cells, 1.5 of the machine) is more than twice the limit: every finer
    // size counts as over without being shown.
    assert!(s.cards().iter().any(|r| r.size.px == 1.0 && !r.fits));
}

/// A size at the limit, measured either side of it from one fit to the next, keeps the size
/// recommended last time (#94): the Mac's 4 px measured 7.3 to 8.3% of the processor against a
/// line of 8.0%, so Recommended moved between 4 and 6 px with nothing changed.
#[test]
fn a_repeated_fit_keeps_its_recommendation_at_the_limit() {
    let canvas = [2560.0, 1440.0];
    let l = limits();
    // 4 px (230,400 cells) at 7.3% or 8.3% of the processor: the line is 8.0%.
    let noisy = |high: bool| {
        move |cells: u64| {
            let per = if high { 0.083 } else { 0.073 } / 0.2304;
            Measure {
                cpu: per * cells as f64 / 1e6,
                p95: 0.004,
                fps: 30.0,
                cells,
                ..Measure::default()
            }
        }
    };
    let mut kept = None;
    let mut seen = vec![];
    for k in 0..6 {
        let (s, order) = search_on(canvas, 3.0, noisy(k % 2 == 1), &l, kept, kept);
        let (rec, card) = s.recommended();
        seen.push(rec.px);
        if kept.is_some() {
            assert!(order.len() <= 2, "the kept size and one finer: {order:?}");
        }
        kept = Some(rec);
        // Marked as at the limit on its card.
        if rec.px == 4.0 {
            assert!(card.is_some_and(|c| c.at_limit), "{card:?}");
        }
    }
    assert!(
        seen.windows(2).skip(1).all(|w| w[0] == w[1]),
        "the same recommendation every time: {seen:?}"
    );
    // A finer size clearly within the limits still displaces it.
    let (s, _) = search_on(canvas, 3.0, model(0.01, 0.01), &l, kept, kept);
    assert_eq!(s.recommended().0, Size::px(1.0));
    // And a kept size clearly over gives way.
    let (s, _) = search_on(canvas, 3.0, model(0.8, 0.1), &l, kept, kept);
    assert!(
        s.recommended().0.px > kept.unwrap().px,
        "{:?}",
        s.recommended()
    );
    assert!(at_limit(1.05) && at_limit(0.95) && !at_limit(1.2) && !at_limit(0.8));
}

/// One processor limit for the whole desktop, shared between the screens by their pixel area
/// (#107): a 2560 x 1440 screen beside a 1920 x 1080 one gets 64% of it, and the two together
/// 10%, not 5% each.
#[test]
fn the_limits_are_one_for_the_desktop_shared_by_area() {
    let shares = area_shares(&[[2560.0, 1440.0], [1920.0, 1080.0]]);
    assert!(
        (shares[0] - 0.64).abs() < 1e-9 && (shares[1] - 0.36).abs() < 1e-9,
        "{shares:?}"
    );
    let l = TuneLimits::default();
    let a = SceneLimits::of(&l, 60.0, shares[0]);
    let b = SceneLimits::of(&l, 60.0, shares[1]);
    assert!((a.cpu - 0.064).abs() < 1e-9 && (b.cpu - 0.036).abs() < 1e-9);
    assert!(
        (a.cpu + b.cpu - 0.10).abs() < 1e-9,
        "the whole desktop's 10%"
    );
    assert!((a.gpu + b.gpu - 0.25).abs() < 1e-9);
    // One screen has all of it.
    assert_eq!(area_shares(&[[1280.0, 720.0]]), [1.0]);
    assert_eq!(SceneLimits::of(&l, 30.0, 1.0).cpu, 0.10);
    // Said plainly in the opening line.
    let share = |screen, canvas: [u32; 2], s: f64| ScreenShare {
        screen,
        canvas,
        share: s,
        cpu_percent: (10.0 * s) as f32,
        gpu_percent: (25.0 * s) as f32,
        scene: Some("Life".into()),
        scenes: 3,
    };
    let start = TuneEvent::Start {
        screens: vec![0, 1],
        scenes: 6,
        fps: 60.0,
        fps_max: None,
        gpu: None,
        shares: vec![share(0, [2560, 1440], 0.64), share(1, [1920, 1080], 0.36)],
        limits: Some(l),
        reused: false,
    };
    let line = start.describe();
    assert!(line.contains("on 2 screens at once"), "{line}");
    assert!(
        line.contains(
            "the 10% processor and 25% GPU limits are one for the whole desktop, shared between \
             the screens by their pixel area: screen 0 (2560x1440, 64% of the area): processor \
             6.4%, GPU 16%; screen 1 (1920x1080, 36% of the area): processor 3.6%, GPU 9.0%"
        ),
        "{line}"
    );
    assert!(line.contains("screen 1 measures Life, the most demanding of its 3 scenes"));
    let back: TuneEvent = serde_json::from_str(&serde_json::to_string(&start).unwrap()).unwrap();
    assert_eq!(back, start);
    // Measured together, the process's time is shared by each screen's frame times.
    assert_eq!(attribute(0.09, &[2.0, 1.0]), [0.06, 0.03]);
    assert_eq!(attribute(0.04, &[0.0, 0.0]), [0.02, 0.02]);
}

/// A slot of the rotation's item `series` (None: not a series), on `family`.
fn slot(label: &str, series: Option<usize>, family: &str) -> Slot {
    let scene = Scene::new(jetcarp_core::TilingSpec::rect(
        family,
        20,
        20,
        jetcarp_core::Boundary::Dead,
    ));
    Slot {
        label: label.into(),
        profile: crate::profiles::Profile {
            show: if series.is_some() {
                Show::Scene {
                    scene: Box::new(scene),
                }
            } else {
                Show::default()
            },
            ..Default::default()
        },
        stored: false,
        end: vec![],
        hold_s: 0.0,
        schedule: None,
        n: None,
        series,
    }
}

/// A rule series is one scene (#109): the Wolfram series' 256 rules were fitted rule by rule.
/// Its rules show at its first rule's size; another series item beside it, even of rules of the
/// same kind, is another scene, and so is anything that is not a series.
#[test]
fn a_rule_series_is_one_scene() {
    let slots = vec![
        slot("W0", Some(0), "square"),
        slot("W1", Some(0), "square"),
        slot("W2", Some(0), "square"),
        slot("profile Life", None, "square"),
        slot("B3/S23", Some(2), "hex"),
        slot("B36/S23", Some(2), "hex"),
        slot("B2/S", Some(3), "hex"),
        slot("profile Life too", None, "square"),
        slot("profile Brian", None, "square"),
    ];
    assert_eq!(
        scene_groups(&slots),
        [
            (0, vec![1, 2]),
            (3, vec![]),
            (4, vec![5]),
            (6, vec![]),
            (7, vec![]),
            (8, vec![])
        ]
    );
    // Its result names every rule, and the host finds each rule's size by it.
    let t = TunedScene {
        scene: "W0".into(),
        also: vec!["W1".into(), "W2".into()],
        design_px: 4.0,
        cell_px: 8.0,
        ..Default::default()
    };
    assert!(t.names("W2") && !t.names("W3"));
    let tuning = crate::host_settings::Tuning {
        rotation: "Wolfram series".into(),
        canvas: [160, 120],
        scenes: vec![t.clone()],
        ..Default::default()
    };
    assert_eq!(
        tuning.size_for("W1", 4.0, [160.0, 120.0]),
        Some(Size::px(8.0))
    );
    let line = scene_line(0, &t);
    assert!(line.contains("W0 and 2 more rules of its series"), "{line}");
}

fn planned(label: &str, cost: f64, design: f32) -> Planned {
    let canvas = [2560.0, 1440.0];
    Planned {
        label: label.into(),
        tiling: "square".into(),
        design,
        own: Size::px(design),
        sizes: lattice_ladder(canvas, design, SWEEP_MAX_CELLS),
        cost: Some(cost),
        fps: 30.0,
        ..Default::default()
    }
}

/// The rotation's most demanding scene is the one measured; the size found serves the whole
/// rotation, a cheaper scene going finer by its cost ratio; a placed pattern and a scene not
/// rebuilt to fill the screen keep their own sizes.
#[test]
fn the_size_found_serves_the_rotation_by_cost() {
    let canvas = [2560.0, 1440.0];
    let mut scenes = vec![
        planned("Life", 1000.0, 3.0),
        planned("Brian", 4000.0, 3.0),
        planned("cheap", 250.0, 3.0),
        Planned {
            placed: true,
            ..planned("glider", 9000.0, 4.0)
        },
        Planned {
            fixed: true,
            sizes: vec![Size::px(5.0)],
            ..planned("scene file", 9000.0, 5.0)
        },
        Planned {
            unpaced: true,
            ..planned("as fast as it can", 9000.0, 3.0)
        },
    ];
    assert_eq!(
        most_demanding(&scenes),
        Some(1),
        "Brian, not the fixed ones"
    );
    // The measured scene's search: 4 px (230,400 cells) the finest within the limits.
    let m = model(0.3, 0.1);
    let (s, _) = search_on(canvas, 3.0, &m, &limits(), None, None);
    assert_eq!(s.recommended().0, Size::px(4.0));
    let results = screen_results(
        &scenes,
        Some(1),
        Some(&s),
        canvas,
        GpuTiming::Kernel,
        None,
        &|_| None,
    );
    let at = |name: &str| results.iter().find(|r| r.scene == name).unwrap();
    assert!(at("Brian").measured && !at("Brian").rungs.is_empty());
    assert_eq!(at("Brian").size(), Size::px(4.0));
    // A quarter of the cost: four times the cells, 2 px (921,600).
    assert_eq!(at("Life").size(), Size::px(2.0));
    assert_eq!(at("Life").cost, 0.25);
    assert!(at("Life").rungs.is_empty() && !at("Life").measured);
    // A sixteenth: 1 px (3.7 million).
    assert_eq!(at("cheap").size(), Size::px(1.0));
    assert_eq!(at("glider").size(), Size::px(4.0), "its own");
    assert_eq!(at("scene file").size(), Size::px(5.0), "its own");
    assert_eq!(
        at("as fast as it can").size(),
        Size::px(6.0),
        "dearer: coarser"
    );
    // `--pick own` and a pixel size apply to every scene; recommended to the measured one.
    let picked = |p| {
        screen_results(
            &scenes,
            Some(1),
            Some(&s),
            canvas,
            GpuTiming::Kernel,
            Some(p),
            &|_| None,
        )
    };
    assert!(
        picked(Pick::Own)
            .iter()
            .all(|r| r.chosen && r.size().px == r.design_px)
    );
    assert!(
        picked(Pick::Px(6.0))
            .iter()
            .filter(|r| !r.placed && r.scene != "scene file")
            .all(|r| r.size() == Size::px(6.0) && r.chosen),
        "{:?}",
        picked(Pick::Px(6.0))
    );
    // A size chosen before is kept while it holds no more work than the measured scene's.
    let chose = |px: f32| {
        move |label: &str| {
            (label == "Life").then(|| {
                let mut t = TunedScene {
                    scene: "Life".into(),
                    design_px: 3.0,
                    chosen: true,
                    ..Default::default()
                };
                t.set_size(Size::px(px));
                t
            })
        }
    };
    let kept = screen_results(
        &scenes,
        Some(1),
        Some(&s),
        canvas,
        GpuTiming::Kernel,
        None,
        &chose(3.0),
    );
    let life = kept.iter().find(|r| r.scene == "Life").unwrap();
    assert_eq!((life.size(), life.chosen), (Size::px(3.0), true));
    let over = screen_results(
        &scenes,
        Some(1),
        Some(&s),
        canvas,
        GpuTiming::Kernel,
        None,
        &chose(1.0),
    );
    let life = over.iter().find(|r| r.scene == "Life").unwrap();
    assert_eq!((life.size(), life.chosen), (Size::px(2.0), false));
    assert!(
        life.note
            .as_deref()
            .unwrap()
            .contains("chosen before, 1 px")
    );
    // Nothing that can be sized: nothing measured, every scene at its own size.
    scenes.retain(|p| !p.sizable());
    assert_eq!(most_demanding(&scenes), None);
    let own = screen_results(&scenes, None, None, canvas, GpuTiming::None, None, &|_| {
        None
    });
    assert!(own.iter().all(|r| r.size().px == r.design_px && !r.chosen));
    // The costs: neighbour reads at the speed, and each frame's drawing.
    assert_eq!(
        cost_per_cell(8.0, 60.0, 60.0),
        8.0 * 60.0 + DRAW_READS * 60.0
    );
    assert_eq!(
        cost_per_cell(8.0, 0.0, 30.0),
        8.0 * 30.0 + DRAW_READS * 30.0
    );
}

/// The other scenes are sized from where the measured scene's loads cross the limits' line, not
/// from the sizes its own ladder happens to have: a Penrose patch whose finest level (280,000
/// tiles) used a third of the limits would otherwise hold Life on squares to 280,000 cells too.
/// A scene keeping its speed exactly is not at the limit for it.
#[test]
fn the_rest_are_sized_from_where_the_loads_cross_the_line() {
    let card = |cells: u64, load: f32| Rung {
        size: Size::px(1.0),
        cells,
        load,
        fits: load <= 1.0,
        ..Rung::default()
    };
    // Between the finest within the limits and the next finer over them.
    let c = line_cells(&[card(100_000, 0.5), card(200_000, 1.5), card(50_000, 0.25)]).unwrap();
    assert!((c - 150_000.0).abs() < 1.0, "{c}");
    // Nothing finer over them: scaled by its load, at most four times.
    assert_eq!(line_cells(&[card(280_000, 0.5)]), Some(560_000.0));
    assert_eq!(line_cells(&[card(280_000, 0.1)]), Some(1_120_000.0));
    // Nothing within them, or cards from before 0.4.3: none.
    assert_eq!(line_cells(&[card(280_000, 1.3)]), None);
    assert_eq!(line_cells(&[card(280_000, 0.0)]), None);
    // A linear load with a fixed cost of its own crosses the line later than the estimate.
    let affine = |cells: f64| 0.2 + cells / 1e6;
    let c = line_cells(&[card(300_000, affine(3e5) as f32)]).unwrap();
    assert!(affine(c) <= 1.0 + 1e-6, "{c}: {}", affine(c));
    // A scene that keeps its speed: no load from it, a generation of slack allowed.
    let kept = Measure {
        gens_wanted: Some(60.0),
        gens_done: 60.0,
        ..Measure::default()
    };
    assert_eq!(load(&kept, &limits()), 0.0);
    assert_eq!(
        load(
            &Measure {
                gens_done: 59.0,
                ..kept
            },
            &limits()
        ),
        0.0
    );
    let short = Measure {
        gens_done: 56.0,
        ..kept
    };
    assert!(
        (load(&short, &limits()) - 0.5).abs() < 1e-9,
        "5% short of a 10% allowance"
    );
    // A second's trial of a scene of 30 generations a second: 22 wanted, 21 done is no load,
    // and fits; the old count read it as 45% of the way to the line.
    let slow = Measure {
        gens_wanted: Some(22.5),
        gens_done: 21.0,
        ..Measure::default()
    };
    assert!(load(&slow, &limits()) < 0.25 && fits(&slow, &limits()).is_ok());
    let behind = Measure {
        gens_done: 18.0,
        ..slow
    };
    assert!(load(&behind, &limits()) > 1.0 && fits(&behind, &limits()).is_err());
    // Another scene's size is kept from one fit to the next while within the noise: 2 px holds
    // 921,600 cells at the same work a cell, so it stays for an allowance down to 838,000 or so,
    // and 1 px takes its place only when clearly allowed.
    let ladder = lattice_ladder([2560.0, 1440.0], 3.0, SWEEP_MAX_CELLS);
    let cells = |s: &Size| lattice_cells([2560.0, 1440.0], s.px);
    assert_eq!(derive(&ladder, cells, 1.0, 900_000), Size::px(3.0));
    assert_eq!(
        derive_kept(&ladder, cells, 1.0, 900_000, Some(Size::px(2.0))),
        Size::px(2.0)
    );
    assert_eq!(
        derive_kept(&ladder, cells, 1.0, 800_000, Some(Size::px(2.0))),
        Size::px(3.0)
    );
    assert_eq!(
        derive_kept(&ladder, cells, 1.0, 4_000_000, Some(Size::px(2.0))),
        Size::px(2.0)
    );
    assert_eq!(
        derive_kept(&ladder, cells, 1.0, 4_200_000, Some(Size::px(2.0))),
        Size::px(1.0)
    );
    // The base the rest come from: the line, or once chosen, the measured scene's size.
    let mut m = TunedScene {
        scene: "Penrose".into(),
        tiling: "penrose-rhomb".into(),
        measured: true,
        rungs: vec![Rung {
            size: Size {
                px: 2.6,
                level: Some(6),
            },
            ..card(280_000, 0.3)
        }],
        ..Default::default()
    };
    m.set_size(Size {
        px: 2.6,
        level: Some(6),
    });
    assert_eq!(base_cells(&m, [2560.0, 1440.0]), 933_333);
    m.chosen = true;
    assert_eq!(base_cells(&m, [2560.0, 1440.0]), 280_000);
}

/// A scene that steps on the other device from the measured one (the Wolfram series' row rules
/// on the processor beside Life on the GPU) has only the estimate of their costs to go by: it is
/// sized no finer than the measured scene's own size allows, and says so. The cost counts the GPU's
/// steps at its speed over the processor's.
#[test]
fn a_scene_on_the_other_device_is_not_sized_past_the_measured_one() {
    let canvas = [2560.0, 1440.0];
    let gpu = |p: Planned| Planned { gpu: true, ..p };
    // The measured scene: Life on the GPU, 4 px with room to spare (a third of the limits).
    let scenes = vec![
        gpu(planned("Life", 1000.0, 3.0)),
        gpu(planned("Brian", 250.0, 3.0)),
        planned("rows", 250.0, 3.0),
    ];
    let m = |cells: u64| Measure {
        cpu: 0.0267 * cells as f64 / 230_400.0,
        p95: 0.002,
        fps: 30.0,
        cells,
        ..Measure::default()
    };
    let (s, _) = search_on(canvas, 3.0, m, &limits(), Some(Size::px(4.0)), None);
    let results = screen_results(
        &scenes,
        Some(0),
        Some(&s),
        canvas,
        GpuTiming::Kernel,
        None,
        &|_| None,
    );
    let at = |name: &str| results.iter().find(|r| r.scene == name).unwrap();
    let life = at("Life").size();
    // On the GPU like Life, a quarter of its work: finer, from where the loads cross the line.
    assert!(
        at("Brian").size().px < life.px,
        "{:?} against {life:?}",
        at("Brian").size()
    );
    assert!(at("Brian").gpu && at("Brian").note.is_none());
    // On the processor: a quarter of the measured scene's work at its own size, no more.
    let rows = at("rows");
    let allowed = s.recommended().1.unwrap().cells as f64 / 0.25;
    assert!(
        lattice_cells(canvas, rows.size().px) as f64 <= allowed,
        "{rows:?}"
    );
    assert!(!rows.gpu);
    assert!(
        rows.note
            .as_deref()
            .unwrap()
            .contains("steps on the processor, the measured scene on the GPU"),
        "{rows:?}"
    );
    // The cost: the GPU's steps and the drawing at its speed over the processor's.
    assert_eq!(
        device_cost(8.0, 60.0, 60.0, false, 8.0),
        480.0 + DRAW_READS * 60.0 / 8.0
    );
    assert_eq!(
        device_cost(8.0, 60.0, 60.0, true, 8.0),
        60.0 + DRAW_READS * 60.0 / 8.0
    );
}

/// `--pick` places sizes from the last fit without measuring (#77): the measured scene's card,
/// and the rest from it by their costs, or every scene at its own size or at a cell size.
#[test]
fn a_pick_places_sizes_from_the_last_fit() {
    let canvas = [2560.0, 1440.0];
    let scenes = vec![planned("Brian", 4000.0, 3.0), planned("Life", 1000.0, 3.0)];
    let (s, _) = search_on(canvas, 3.0, model(0.3, 0.1), &limits(), None, None);
    let saved = screen_results(
        &scenes,
        Some(0),
        Some(&s),
        canvas,
        GpuTiming::Kernel,
        None,
        &|_| None,
    );
    let sizes = |t: &TunedScene| lattice_ladder(canvas, t.design_px, SWEEP_MAX_CELLS);
    let own = repick(&saved, &sizes, canvas, Pick::Own);
    assert!(
        own.iter().all(|r| r.size() == Size::px(3.0) && r.chosen),
        "{own:?}"
    );
    let px = repick(&saved, &sizes, canvas, Pick::Px(2.0));
    assert!(
        px.iter().all(|r| r.size() == Size::px(2.0) && r.chosen),
        "{px:?}"
    );
    let rec = repick(&px, &sizes, canvas, Pick::Recommended);
    assert_eq!(
        rec.iter()
            .map(|r| (r.size().px, r.chosen))
            .collect::<Vec<_>>(),
        [(4.0, false), (2.0, false)]
    );
    // The finest tried, and the rest from it.
    let finest = repick(&saved, &sizes, canvas, Pick::Finest);
    let tried_finest = s.cards()[0].size;
    assert_eq!(finest[0].size(), tried_finest);
    assert!(finest[1].size().px <= finest[0].size().px);
    // Whether the last fit still holds: the same JETCARP, rotation, scenes, monitor, limits and
    // GPU, with a scene measured.
    let t = crate::host_settings::Tuning {
        rotation: "r".into(),
        canvas: [2560, 1440],
        scale: 1.0,
        hz: Some(60.0),
        scenes: saved.clone(),
        version: "0.4.3".into(),
        gpu: Some("gpu".into()),
        ..Default::default()
    };
    let labels = ["Brian".to_string(), "Life".to_string()];
    let l = TuneLimits::default();
    let current = |labels: &[String], canvas, hz, l: &TuneLimits, gpu, v| {
        t.current("r", labels, canvas, 1.0, hz, l, gpu, v)
    };
    assert!(current(
        &labels,
        [2560.0, 1440.0],
        Some(60.0),
        &l,
        Some("gpu"),
        "0.4.3"
    ));
    assert!(!current(
        &labels,
        [1920.0, 1080.0],
        Some(60.0),
        &l,
        Some("gpu"),
        "0.4.3"
    ));
    assert!(!current(
        &labels,
        [2560.0, 1440.0],
        Some(120.0),
        &l,
        Some("gpu"),
        "0.4.3"
    ));
    assert!(!current(
        &labels,
        [2560.0, 1440.0],
        Some(60.0),
        &l,
        Some("other"),
        "0.4.3"
    ));
    assert!(!current(
        &labels,
        [2560.0, 1440.0],
        Some(60.0),
        &l,
        Some("gpu"),
        "0.4.4"
    ));
    let tighter = TuneLimits {
        cpu_percent: 5.0,
        ..l
    };
    assert!(!current(
        &labels,
        [2560.0, 1440.0],
        Some(60.0),
        &tighter,
        Some("gpu"),
        "0.4.3"
    ));
    let more = [labels[0].clone(), labels[1].clone(), "new".to_string()];
    assert!(!current(
        &more,
        [2560.0, 1440.0],
        Some(60.0),
        &l,
        Some("gpu"),
        "0.4.3"
    ));
}

/// A sweep that stopped at its coarsest size (the Acer's Fit, #38) can be undone: the scene's own
/// size and a cell size in pixels are placed though the sweep never showed them (#39).
#[test]
fn a_pick_places_sizes_the_sweep_never_showed() {
    let l = limits();
    let sizes: Vec<Size> = [1.0, 2.0, 3.0, 4.0, 8.0, 16.0].map(Size::px).to_vec();
    let mut s = Search::new(sizes, Size::px(3.0), Some(Size::px(16.0)), None);
    let far = Measure {
        cpu: 0.9,
        p95: 0.020,
        cells: 8040,
        ..Measure::default()
    };
    let i = s.next().unwrap();
    assert_eq!(s.sizes[i], Size::px(16.0));
    s.record(i, 16.0, &far, &l);
    assert!(s.next().is_none(), "nothing finer is worth showing");
    let p = Planned {
        label: "a".into(),
        tiling: "square".into(),
        design: 3.0,
        own: Size::px(3.0),
        ..Default::default()
    };
    let pick = |k| measured_scene(&p, &s, GpuTiming::None, Some(k), None);
    assert_eq!(pick(Pick::Recommended).size(), Size::px(16.0));
    assert_eq!(pick(Pick::Own).size(), Size::px(3.0));
    assert!(pick(Pick::Own).chosen);
    assert_eq!(pick(Pick::Px(3.0)).size(), Size::px(3.0));
    let t = measured_scene(&p, &s, GpuTiming::None, None, None);
    assert!(t.note.as_deref().unwrap().contains("no size was within"));
}

/// A scene is held to the frame rate it is drawn at (#42): Life at 60 generations a second
/// draws 60 frames a second, so its frames must take under 15 ms, not 30; in low power it draws
/// 30. A frame rate asked for still caps it.
#[test]
fn a_scene_is_judged_at_the_frame_rate_it_draws_at() {
    use crate::pace::Pacing;
    let every = Pacer {
        pacing: Pacing::EveryGeneration,
        fps: 30.0,
        refresh: Some(60.0),
        max_fps: None,
    };
    let low = Pacer {
        pacing: Pacing::LowPower,
        ..every
    };
    let l = TuneLimits::default();
    assert_eq!(scene_fps(&l, &every, 60.0), 60.0);
    assert_eq!(scene_fps(&l, &every, 240.0), 60.0, "at most the refresh");
    assert_eq!(scene_fps(&l, &every, 20.0), 30.0, "at least the frame rate");
    assert_eq!(scene_fps(&l, &every, 0.0), 30.0, "as fast as it can");
    assert_eq!(scene_fps(&l, &low, 60.0), 30.0);
    let fast = Pacer {
        refresh: Some(144.0),
        ..every
    };
    assert_eq!(scene_fps(&l, &fast, 240.0), 144.0);
    let capped = Pacer {
        max_fps: Some(60.0),
        ..fast
    };
    assert_eq!(scene_fps(&l, &capped, 240.0), 60.0, "the settings' maximum");
    let asked = TuneLimits {
        fps: Some(24.0),
        ..l
    };
    assert_eq!(scene_fps(&asked, &every, 60.0), 24.0);
    // A 20 ms frame keeps 30 frames a second, not 60.
    let m = Measure {
        p95: 0.020,
        ..Measure::default()
    };
    let at = |fps| SceneLimits::of(&l, fps, 1.0);
    assert!(fits(&m, &at(scene_fps(&l, &low, 60.0))).is_ok());
    let e = fits(&m, &at(scene_fps(&l, &every, 60.0))).unwrap_err();
    assert!(e.contains("at 60 fps, 16.7 ms"), "{e}");
    // The opening line gives the range.
    assert_eq!(fps_range([60.0, 30.0, 30.0]), (30.0, Some(60.0)));
    assert_eq!(fps_range([30.0, 30.0]), (30.0, None));
    let start = |fps, fps_max| TuneEvent::Start {
        screens: vec![0],
        scenes: 3,
        fps,
        fps_max,
        gpu: None,
        shares: vec![],
        limits: None,
        reused: false,
    };
    assert!(
        start(30.0, Some(59.94))
            .describe()
            .contains("at 30 to 59.94 frames a second")
    );
    assert!(
        start(30.0, None)
            .describe()
            .contains("at 30 frames a second")
    );
    // An older tuner's line, with no `fps_max`, still reads.
    let old: TuneEvent = serde_json::from_str(
        r#"{"event": "start", "screens": [0], "scenes": 3, "fps": 30.0, "gpu": null}"#,
    )
    .unwrap();
    assert_eq!(old, start(30.0, None));
    let text = serde_json::to_string(&old).unwrap();
    assert!(
        !text.contains("fps_max") && !text.contains("reused"),
        "{text}"
    );
}

/// A scene left at its own size is kept at the size it asks for (2.4 px) while its cells came
/// to another (2.6 px): its line names its own rung's size and cells, not two sizes and none.
#[test]
fn a_scene_at_its_own_size_reports_the_size_its_cells_show_at() {
    let own = Rung {
        size: Size::px(2.6),
        cells: 1234,
        fits: true,
        own: true,
        ..Rung::default()
    };
    let mut t = TunedScene {
        scene: "Penrose waves".into(),
        design_px: 2.4,
        recommended: Size::px(2.6),
        rungs: vec![own],
        measured: true,
        ..Default::default()
    };
    t.set_size(Size::px(2.4));
    assert_eq!(t.rung().map(|r| r.cells), Some(1234));
    let line = scene_line(0, &t);
    assert!(
        line.contains("recommended 2.6 px; shows at 2.6 px, 1,234 cells"),
        "{line}"
    );
}

/// The submission clock's GPU share is an upper bound: it never fails a size or stops the
/// sweep, while the GPU's own clocks do (#38).
#[test]
fn only_a_real_gpu_clock_judges_the_gpu_limit() {
    let l = SceneLimits {
        cpu: 0.10,
        gpu: 0.25,
        fps: 30.0,
    };
    let m = |gpu_timing| Measure {
        cpu: 0.01,
        gpu: Some(0.90),
        p95: 0.020,
        gpu_timing,
        ..Measure::default()
    };
    assert!(fits(&m(GpuTiming::Submission), &l).is_ok());
    assert!(load(&m(GpuTiming::Submission), &l) < 1.0);
    // Windows' engine counters are judged like Linux's count (#72).
    for t in [
        GpuTiming::Timestamps,
        GpuTiming::Kernel,
        GpuTiming::Counters,
    ] {
        assert!(fits(&m(t), &l).unwrap_err().contains("GPU 90%"));
        assert!(load(&m(t), &l) > FAR_OVER);
        let r = rung_of(Size::px(3.0), &m(t), &l, false);
        assert_eq!((r.gpu_percent, r.gpu_unmeasured), (Some(90.0), false));
    }
    // Slow frames still fail, whatever the clock.
    let slow = Measure {
        p95: 0.080,
        ..m(GpuTiming::Submission)
    };
    assert!(fits(&slow, &l).unwrap_err().contains("frames"));
}

/// Where the GPU share wasn't measured, the cards, the text and the JSON say so plainly, and
/// show the share as unknown, not as an unjudged number (#72).
#[test]
fn an_unmeasured_gpu_share_is_unknown_and_says_why() {
    let l = SceneLimits {
        cpu: 0.10,
        gpu: 0.25,
        fps: 60.0,
    };
    let m = Measure {
        cpu: 0.02,
        gpu: Some(0.90),
        p95: 0.010,
        fps: 60.0,
        on_gpu: true,
        gpu_timing: GpuTiming::Submission,
        cells: 230_400,
        ..Measure::default()
    };
    let r = rung_of(Size::px(3.0), &m, &l, true);
    assert_eq!(
        (r.gpu_percent, r.gpu_unmeasured, r.fits),
        (None, true, true)
    );
    let line = rung_line(&r);
    assert!(
        line.contains("processor 2.0%, GPU not measured, 60 fps"),
        "{line}"
    );
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains(r#""gpu_unmeasured":true"#), "{json}");
    assert!(!json.contains("gpu_percent"), "{json}");
    // The counters failing to give a share is unmeasured too; no GPU at all is not.
    let lost = Measure {
        gpu: None,
        gpu_timing: GpuTiming::Counters,
        ..m
    };
    assert!(lost.gpu_unmeasured());
    let cpu_only = Measure {
        gpu: None,
        gpu_timing: GpuTiming::None,
        on_gpu: false,
        ..m
    };
    assert!(!cpu_only.gpu_unmeasured());
    let line = rung_line(&rung_of(Size::px(3.0), &cpu_only, &l, true));
    assert!(!line.contains("GPU"), "{line}");
    // The fit's last line, for people and as JSON.
    let done = TuneEvent::done(Some("host.json".into()), 3, GpuTiming::Submission);
    assert_eq!(
        done.describe(),
        "fitted 3 scene(s); GPU share not measured on this adapter (it has no GPU timestamps): \
         the GPU limit isn't applied; saved to host.json"
    );
    let json = serde_json::to_string(&done).unwrap();
    assert!(
        json.contains(r#""gpu_note":"GPU share not measured"#),
        "{json}"
    );
    let counted = TuneEvent::done_in(None, 3, GpuTiming::Counters, 5, 7.25);
    assert_eq!(
        counted.describe(),
        "fitted 3 scene(s) in 7.2 s, 5 sizes tried (GPU time from Windows' GPU engine counters \
         for JETCARP's process); not saved"
    );
    assert!(
        !serde_json::to_string(&counted)
            .unwrap()
            .contains("gpu_note")
    );
    // A line from a tuner before 0.4.2, with no note, still reads.
    let old: TuneEvent = serde_json::from_str(
        r#"{"event": "done", "saved": null, "scenes": 1, "gpu_timing": "submission"}"#,
    )
    .unwrap();
    assert!(matches!(
        old,
        TuneEvent::Done {
            gpu_note: None,
            seconds: None,
            ..
        }
    ));
}

#[test]
fn a_choice_is_kept_while_it_fits_and_replaced_when_it_does_not() {
    let canvas = [2560.0, 1440.0];
    let p = planned("a", 1.0, 3.0);
    let m = model(0.2, 0.1);
    let (s, _) = search_on(canvas, 3.0, &m, &limits(), None, None);
    let first = measured_scene(&p, &s, GpuTiming::None, None, None);
    assert_eq!((first.size(), first.chosen), (Size::px(4.0), false));
    // Someone chooses 6 px, then the monitors change and it is measured again: the search tries
    // it first, and it is kept.
    let mut mine = first.clone();
    mine.set_size(Size::px(6.0));
    mine.chosen = true;
    let (s, order) = search_on(canvas, 3.0, &m, &limits(), Some(Size::px(6.0)), None);
    assert_eq!(order[0], 6.0);
    let again = measured_scene(&p, &s, GpuTiming::None, None, Some(&mine));
    assert_eq!(
        (again.size(), again.chosen, again.note.clone()),
        (Size::px(6.0), true, None)
    );
    // A choice over the limits on the new screen gives way to the recommended size, with a note.
    mine.set_size(Size::px(2.0));
    let (s, _) = search_on(canvas, 3.0, &m, &limits(), Some(Size::px(2.0)), None);
    let again = measured_scene(&p, &s, GpuTiming::None, None, Some(&mine));
    assert_eq!((again.size(), again.chosen), (Size::px(4.0), false));
    assert!(
        again
            .note
            .as_deref()
            .unwrap()
            .contains("chosen before, 2 px, is over")
    );
    assert_eq!(Pick::parse("12px").unwrap(), Pick::Px(12.0));
    assert_eq!(Pick::parse("Recommended").unwrap(), Pick::Recommended);
    assert!(Pick::parse("0.5").is_err() && Pick::parse("bigger").is_err());
}

#[test]
fn placed_patterns_are_told_from_soups() {
    use jetcarp_core::{Boundary, CaSpec, TilingSpec};
    let scene = |seed: SeedSpec| {
        let mut sc = Scene::new(TilingSpec::rect("square", 40, 30, Boundary::Wrap));
        sc.cas = vec![CaSpec::new("B3/S23").seed(seed)];
        sc
    };
    assert!(!is_placed(&scene(SeedSpec::Random { density: 0.3 })));
    assert!(!is_placed(&scene(SeedSpec::RandomStates)));
    assert!(is_placed(&scene(SeedSpec::Center)));
    assert!(is_placed(&scene(SeedSpec::Cells {
        cells: vec![[1, 1], [2, 1]]
    })));
    assert!(is_placed(&scene(SeedSpec::Ants { count: 1 })));
    assert!(!is_placed(&scene(SeedSpec::Ants { count: 5 })));
    // An empty field with a pasted pattern.
    let mut pasted = scene(SeedSpec::Empty);
    pasted.commands.push(Command::SetCells {
        id: 1,
        cells: vec![[5, 1]],
    });
    assert!(is_placed(&pasted));
    // Nothing placed at all, or a placed pattern kept going by reseeding: not.
    assert!(!is_placed(&scene(SeedSpec::Empty)));
    let mut reseeded = scene(SeedSpec::Center);
    let id = crate::reseed_presets::PRESETS[0].id;
    reseeded.reseed = crate::reseed_presets::preset_policies(id, 1200).unwrap();
    assert!(!reseeded.reseed.is_empty() && !is_placed(&reseeded));
}

#[test]
fn cards_name_their_pictures_and_crops_keep_the_screen_s_pixels() {
    assert_eq!(
        crop_name(0, "profile Life", &Size::px(3.0)),
        "s0-profile-life-3px.png"
    );
    assert_eq!(
        crop_name(
            1,
            "Penrose waves",
            &Size {
                px: 2.4,
                level: Some(6)
            }
        ),
        "s1-penrose-waves-l6.png"
    );
    assert_eq!(crop_name(0, "x", &Size::px(2.5)), "s0-x-2_5px.png");
    // Even sides on an even canvas, odd on an odd one, so the picture's pixel grid is the
    // screen's.
    assert_eq!(crop_size([2560.0, 1440.0]), [192, 120]);
    assert_eq!(crop_size([1365.0, 767.0]), [191, 119]);
    assert_eq!(crop_size([100.0, 50.0]), [100, 50]);
    assert_eq!(warmup(1.0), 0.25);
    assert_eq!(warmup(4.0), 0.5);
    let r = TuneRequest {
        cpu_percent: Some(5.0),
        pick: Some(Pick::Own),
        fresh: true,
        dry_run: true,
        ..Default::default()
    };
    assert_eq!(
        r.args(),
        ["--cpu-limit", "5", "--pick", "own", "--fresh", "--dry-run"]
    );
    let only = TuneRequest {
        screens: vec![1],
        ..Default::default()
    };
    assert!(only.wants(1) && !only.wants(0) && TuneRequest::default().wants(3));
    let e = TuneEvent::Rung {
        screen: 0,
        number: 2,
        of: 7,
        scene: "Life".into(),
        size: Size::px(3.0),
        rung: 1,
        rungs: 6,
    };
    // A patch's heading names its level only; its tiles' size is known once it is built.
    let patch = TuneEvent::Rung {
        screen: 0,
        number: 2,
        of: 7,
        scene: "Penrose waves".into(),
        size: Size {
            px: 20.2,
            level: Some(4),
        },
        rung: 1,
        rungs: 3,
    };
    assert!(
        patch.describe().contains("at level 4 (size 1 of up to 3)"),
        "{}",
        patch.describe()
    );
    let line = serde_json::to_string(&e).unwrap();
    assert!(line.contains(r#""event":"rung""#), "{line}");
    assert_eq!(serde_json::from_str::<TuneEvent>(&line).unwrap(), e);
    assert_eq!(thousands(1234567), "1,234,567");
}
