//! Content ids: one pattern has one id however its record is stored.
//!
//! A record's id is SHA-256 of its canonical identity text and its tiling's geometry pin
//! (`pattern::range`). These tests take real records (Conway's glider on the square grid, Life
//! ships from the k-uniform census scenes in `docs/scenes/life-census/`, the hat glider, and an
//! oscillator on a Spectre patch), write them again with their phases rotated to start at every
//! generation of the period, in every symmetry image of the tiling, moved elsewhere, and with the
//! stored canonical text spoiled, and check that every version gets the record's id and that
//! `canonicalize` gives back the record itself.

use jetcarp_core::objects::{LCell, ObjClass, ObjectOptions, Phase, find_objects, place_rows};
use jetcarp_core::pattern::canon::tiling_symmetries;
use jetcarp_core::pattern::movers::{TrackSpec, mover_record, track};
use jetcarp_core::pattern::site::patch_topology;
use jetcarp_core::pattern::site_graph::{build_mover_site, build_site_for, other_sites};
use jetcarp_core::pattern::{
    self, DiscoveredBy, PatternClass, PatternRecord, Provenance, decode_cells, encode_cells,
    record_from_object,
};
use jetcarp_core::rules::Rule;
use jetcarp_core::*;

fn prov() -> Provenance {
    Provenance {
        source: "test".into(),
        jetcarp_version: VERSION.into(),
        date: pattern::now_utc(),
        ..Default::default()
    }
}

/// The periodic objects of a world, as records.
fn records_of(w: &World) -> Vec<PatternRecord> {
    let r = find_objects(w, &ObjectOptions::default()).unwrap();
    r.objects
        .iter()
        .filter(|o| !matches!(o.class, ObjClass::Other { .. }))
        .filter_map(|o| record_from_object(w, w.cas[0].id, o, prov(), DiscoveredBy::User).ok())
        .collect()
}

fn glider() -> PatternRecord {
    let mut w = World::new(TilingSpec::rect("square", 48, 48, Boundary::Wrap), 1).unwrap();
    let g = w.topo.lattice().unwrap().clone();
    let cells = place_rows(&g, ".o./..o/ooo", 20, 20);
    w.apply(Command::AddCa {
        ca: CaSpec::new("B3/S23").seed(SeedSpec::Cells { cells }),
    })
    .unwrap();
    let v = records_of(&w);
    assert_eq!(v.len(), 1);
    v.into_iter().next().unwrap()
}

/// The Life ship of a census scene, found again on a larger wrapped grid of its tiling.
fn census_ship(name: &str) -> PatternRecord {
    let path = format!(
        "{}/../../docs/scenes/life-census/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut tiling: TilingSpec = serde_json::from_value(v["tiling"].clone()).unwrap();
    tiling.size = Some(SizeSpec::Rect { w: 64, h: 64 });
    tiling.boundary = Boundary::Wrap;
    let mut w = World::new(tiling, 1).unwrap();
    let g = w.topo.lattice().unwrap().clone();
    let cells: Vec<[u32; 2]> = decode_cells(v["commands"][0]["pattern"]["cells"].as_str().unwrap())
        .unwrap()
        .iter()
        .map(|c| {
            [
                g.resolve(c.i + 30, c.j + 30, c.k).unwrap() as u32,
                c.s as u32,
            ]
        })
        .collect();
    w.apply(Command::AddCa {
        ca: CaSpec::new(v["cas"][0]["rule"].as_str().unwrap()).seed(SeedSpec::Cells { cells }),
    })
    .unwrap();
    let ships: Vec<PatternRecord> = records_of(&w)
        .into_iter()
        .filter(|r| r.class == PatternClass::Spaceship)
        .collect();
    assert_eq!(ships.len(), 1, "{name}");
    ships.into_iter().next().unwrap()
}

/// The record written again from generation `k` of its period, in symmetry image `m`, moved by
/// `shift`, its canonical text spoiled.
fn image(rec: &PatternRecord, k: u64, m: &pattern::CellMap, shift: [i32; 2]) -> PatternRecord {
    let p = rec.phases.len() as u64;
    let phases: Vec<Phase> = (0..p)
        .map(|t| {
            let mut v: Phase = rec
                .phase(t + k)
                .unwrap()
                .iter()
                .map(|c| {
                    let mut c = m.apply(c);
                    c.i += shift[0];
                    c.j += shift[1];
                    c
                })
                .collect();
            v.sort_unstable();
            v
        })
        .collect();
    let mut x = rec.clone();
    x.phases = phases.iter().map(|v| encode_cells(v)).collect();
    x.displacement = rec.displacement.map(|d| m.vector(d));
    x.canonical = "not canonical".into();
    x.id = "0123456789abcdef".into();
    x
}

/// Every rotation of the period and every symmetry image of a lattice record gets its id, and
/// canonicalises back to it.
fn check_lattice(rec: &PatternRecord) -> usize {
    assert_eq!(pattern::pattern_id(rec).unwrap(), rec.id, "{}", rec.label());
    let syms = tiling_symmetries(&rec.tiling, rec.neighborhood, true).unwrap();
    let mut n = 0;
    for (mi, m) in syms.iter().enumerate() {
        for k in 0..rec.phases.len() as u64 {
            let x = image(rec, k, m, [7, -3]);
            assert_eq!(
                pattern::pattern_id(&x).unwrap(),
                rec.id,
                "{} rotated {k}, symmetry {mi}",
                rec.label()
            );
            let mut c = x.clone();
            c.canonicalize().unwrap();
            assert_eq!(
                c.phases,
                rec.phases,
                "{} rotated {k}, symmetry {mi}",
                rec.label()
            );
            assert_eq!(c.canonical, rec.canonical);
            assert_eq!(c.displacement, rec.displacement);
            assert_eq!(c.speed, rec.speed);
            assert_eq!(
                (c.cells, c.min_cells, c.max_cells),
                (rec.cells, rec.min_cells, rec.max_cells)
            );
            assert_eq!(c.bbox, rec.bbox);
            assert_eq!(c.id, rec.id);
            assert_eq!(c.geometry_pin, rec.geometry_pin);
            n += 1;
        }
    }
    n
}

#[test]
fn the_glider_has_one_id_in_every_phase_and_orientation() {
    let g = glider();
    assert_eq!((g.period, g.cells), (4, 5));
    // 4 phases x 8 symmetries of the square grid.
    assert_eq!(check_lattice(&g), 32);
}

#[test]
fn k_uniform_census_ships_have_one_id_in_every_phase_and_orientation() {
    // k2-003 (p16 5c/16), k3-011 (p4 c/4), and the snub square tiling, whose symmetries include
    // glide reflections (each a tile-type permutation with a shift).
    for (name, period) in [("f89d8f11", 16), ("8488e7e9", 4), ("a5e9ba80", 0)] {
        let s = census_ship(name);
        if period > 0 {
            assert_eq!(s.period, period, "{name}");
        }
        let syms = tiling_symmetries(&s.tiling, s.neighborhood, true).unwrap();
        assert!(syms.len() >= 2, "{name}: {} symmetries", syms.len());
        assert_eq!(check_lattice(&s), syms.len() * s.period as usize, "{name}");
    }
}

// An oscillator on an aperiodic patch (a pattern graph): every rotation of its period, and the
// same oscillator at the other sites of the patch where it fits (rotated and reflected copies
// among them), get one id.
#[test]
fn an_aperiodic_oscillator_has_one_id_at_every_start_and_site() {
    let patch = TilingSpec::new("spectre").with_size(SizeSpec::Level(4));
    let mut w = World::new(patch.clone(), 1).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new("B35/S234").seed(SeedSpec::Random { density: 0.4 }),
    })
    .unwrap();
    w.apply(Command::Step { gens: 600 }).unwrap();
    let rec = records_of(&w)
        .into_iter()
        .filter(|r| {
            r.class == PatternClass::Oscillator
                && r.site.as_ref().is_some_and(|s| s.graph.is_some())
        })
        .max_by_key(|r| (r.period, r.cells))
        .expect("an oscillator on the Spectre patch");
    assert!(rec.period >= 2, "{}", rec.label());
    assert_eq!(pattern::pattern_id(&rec).unwrap(), rec.id);
    let p = rec.phases.len();
    for k in 0..p {
        // The phases and the graph's states rotated by k generations.
        let mut x = rec.clone();
        x.phases = (0..p).map(|t| rec.phases[(t + k) % p].clone()).collect();
        let g = x.site.as_mut().unwrap().graph.as_mut().unwrap();
        for c in &mut g.cells {
            c.states = (0..p).map(|t| c.states[(t + k) % p]).collect();
        }
        x.canonical = "not canonical".into();
        assert_eq!(pattern::pattern_id(&x).unwrap(), rec.id, "rotated {k}");
        let mut c = x.clone();
        c.canonicalize().unwrap();
        assert_eq!(
            (&c.phases, &c.canonical, &c.id),
            (&rec.phases, &rec.canonical, &rec.id)
        );
    }
    // The graph relabelled (cells in reverse order): the same id.
    let mut x = rec.clone();
    let site = x.site.as_mut().unwrap();
    let g = site.graph.as_mut().unwrap();
    let n = g.cells.len() as u32;
    let flip = |v: u32| n - 1 - v;
    g.cells.reverse();
    for c in &mut g.cells {
        c.nbrs = c.nbrs.iter().map(|&v| flip(v)).collect();
        c.nbrs.sort_unstable();
    }
    x.phases = rec
        .all_phases()
        .unwrap()
        .iter()
        .map(|ph| {
            let mut v: Phase = ph
                .iter()
                .map(|c| LCell {
                    i: flip(c.i as u32) as i32,
                    ..*c
                })
                .collect();
            v.sort_unstable();
            encode_cells(&v)
        })
        .collect();
    assert_eq!(pattern::pattern_id(&x).unwrap(), rec.id, "relabelled");
    // The same oscillator at other sites of the patch.
    let topo = patch_topology(&rec.site.as_ref().unwrap().patch).unwrap();
    let rule = Rule::parse(&rec.rule).unwrap().0;
    let others = other_sites(&topo, rec.site.as_ref().unwrap(), 6);
    assert!(!others.is_empty(), "{}: no other site", rec.label());
    for (x0, m) in &others {
        let phases: Vec<Phase> = rec
            .all_phases()
            .unwrap()
            .iter()
            .map(|ph| {
                ph.iter()
                    .map(|c| LCell {
                        i: m[c.i as usize] as i32,
                        ..*c
                    })
                    .collect()
            })
            .collect();
        let (site, ph, canonical) =
            build_site_for(&topo, rec.neighborhood, &rule, &phases).unwrap();
        let mut y = rec.clone();
        y.site = Some(site);
        y.phases = ph.iter().map(|v| encode_cells(v)).collect();
        y.canonical = canonical;
        assert_eq!(pattern::pattern_id(&y).unwrap(), rec.id, "at tile {x0}");
    }
}

// The hat glider (a mover: its starting configuration and the tiles around it): the same id at
// every other site of the patch where it fits, and after its record is canonicalised.
#[test]
fn the_hat_glider_has_one_id_at_every_site() {
    let mut spec = TrackSpec::new(
        "Priority 4: 3|n2>=1->2; 0|n3>=2->0; 0|n2>=1->3; 0|n1>=1->2",
        Nb::Touch,
        TilingSpec::new("hat").with_size(SizeSpec::Level(4)),
        vec![(0, 1), (3, 2)],
    );
    spec.max_level = Some(5);
    let rep = track(&spec).unwrap();
    let rec = mover_record(&spec, &rep, prov(), DiscoveredBy::Search).unwrap();
    assert_eq!(rec.class, PatternClass::Mover);
    assert_eq!(pattern::pattern_id(&rec).unwrap(), rec.id);
    let mut c = rec.clone();
    c.canonical = "not canonical".into();
    c.canonicalize().unwrap();
    assert_eq!(
        (&c.phases, &c.canonical, &c.id),
        (&rec.phases, &rec.canonical, &rec.id)
    );
    let site = rec.site.as_ref().unwrap();
    let topo = patch_topology(&site.patch).unwrap();
    let rule = Rule::parse(&rec.rule).unwrap().0;
    let others = other_sites(&topo, site, 6);
    assert!(!others.is_empty(), "no other site for the hat glider");
    let start = rec.phase(0).unwrap();
    for (x0, m) in &others {
        let at: Phase = start
            .iter()
            .map(|c| LCell {
                i: m[c.i as usize] as i32,
                ..*c
            })
            .collect();
        let (s2, ph, canonical) = build_mover_site(&topo, rec.neighborhood, &rule, &at).unwrap();
        let mut y = rec.clone();
        y.site = Some(s2);
        y.phases = ph.iter().map(|v| encode_cells(v)).collect();
        y.canonical = canonical;
        assert_eq!(
            pattern::pattern_id(&y).unwrap(),
            rec.id,
            "hat glider at tile {x0}"
        );
    }
}

// The identity text has its documented form, and ids are fixed: these pins change only when the
// identity text deliberately changes meaning (a new version line), since packs carry ids.
#[test]
fn identity_text_and_ids_are_pinned() {
    let g = glider();
    let text = pattern::identity(&g).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines,
        vec![
            "jetcarp-pattern-v3",
            "model=lifelike",
            "nb=touch",
            "steps=1",
            "tiling={\"family\":\"square\",\"boundary\":\"dead\"}",
            "class=spaceship",
            "period=4",
            &format!(
                "d={},{}",
                g.displacement.unwrap()[0],
                g.displacement.unwrap()[1]
            ),
            "rule=",
            &format!("canonical={}", g.canonical),
            &format!("phases={}", g.phases.join(";")),
        ]
    );
    assert!(!text.ends_with('\n'));
    let pin = Topology::geometry_pin(&TilingSpec::new("square")).unwrap();
    assert_eq!(g.geometry_pin, pin.pin);
    assert_eq!(g.id, pattern::content_id(&text, &pin.pin));
    assert!(pattern::range::is_content_id(&g.id));
    println!("glider {} pin {}\n{text}", g.id, pin.pin);
    assert_eq!(pin.pin, SQUARE_PIN, "the square grid's geometry pin");
    assert_eq!(g.id, GLIDER_ID, "the Life glider's id");
}

/// The tilings at their default shapes: every family, every Archimedean and k-uniform catalog
/// tiling, every isohedral type (the list `geometry_pins` pins).
fn default_tilings() -> Vec<TilingSpec> {
    let mut out = vec![];
    for f in jetcarp_core::families::FAMILIES {
        if f.id != "periodic" {
            out.push(TilingSpec::new(f.id));
        }
    }
    for t in jetcarp_core::catalog::all() {
        let mut s = TilingSpec::new("periodic");
        s.params = Some(serde_json::json!({ "catalog": t.id }));
        out.push(s);
    }
    for n in 1..=93 {
        if let Some(id) = jetcarp_core::isohedral::id_for(n) {
            out.push(TilingSpec::new(id));
        }
    }
    out
}

// The `tiling=` line is RFC 8785 canonical JSON in the members' fixed order (`tests/jcs_vectors.rs`
// checks it against an independent implementation). On every tiling at its default shape it is
// the text serde_json wrote before RFC 8785 was adopted, so no id of such a tiling changed;
// numbers in a reshaped tiling's parameters get their one RFC 8785 spelling.
#[test]
fn the_tiling_line_is_canonical_and_unchanged_on_default_shapes() {
    let all = default_tilings();
    assert!(all.len() > 1300, "{} tilings", all.len());
    for t in &all {
        let before = serde_json::to_string(&pattern::identity_tiling(t)).unwrap();
        assert_eq!(pattern::identity_key(t), before, "{}", t.family);
        // Sized, wrapped and on a patch level: the same line.
        let sized = t
            .clone()
            .with_size(SizeSpec::Cells(4096))
            .with_boundary(Boundary::Wrap);
        assert_eq!(pattern::identity_key(&sized), before, "{}", t.family);
    }
    assert_eq!(
        pattern::identity_key(&patch_at("spectre", 3, true)),
        r#"{"family":"spectre","boundary":"dead"}"#
    );
    let mut t = TilingSpec::new("cairo");
    t.params = Some(serde_json::json!({ "b": 1.0, "a": [1e-7, 0.000001, -0.0, 2.5e21] }));
    assert_eq!(
        pattern::identity_key(&t),
        r#"{"family":"cairo","params":{"a":[1e-7,0.000001,0,2.5e+21],"b":1},"boundary":"dead"}"#
    );
    t.edges = Some(vec![jetcarp_core::edges::EdgeShape {
        mode: jetcarp_core::edges::EdgeMode::J,
        curved: true,
        nodes: vec![jetcarp_core::edges::Node { t: 0.5, d: -1.0 }],
    }]);
    assert!(
        pattern::identity_key(&t).ends_with(
            r#""edges":[{"curved":true,"mode":"J","nodes":[{"d":-1,"t":0.5}]}],"boundary":"dead"}"#
        ),
        "{}",
        pattern::identity_key(&t)
    );
}

/// A patch of `family` at `level`, the level given as the size or (`param`) as a parameter, as
/// the Spectre's default parameters and the app's sweeps give it.
fn patch_at(family: &str, level: u32, param: bool) -> TilingSpec {
    if param {
        let mut t = TilingSpec::new(family);
        t.params = Some(serde_json::json!({ "level": level }));
        t
    } else {
        TilingSpec::new(family).with_size(SizeSpec::Level(level))
    }
}

/// The record found again on another patch: its pattern graph matched there, and its site built
/// at the match (as a search on that patch would build it).
fn on_patch(rec: &PatternRecord, spec: &TilingSpec) -> Option<PatternRecord> {
    let site = rec.site.as_ref().unwrap();
    let topo = patch_topology(spec).unwrap();
    let rule = Rule::parse(&rec.rule).unwrap().0;
    let (_, m) = other_sites(&topo, site, 1).into_iter().next()?;
    let phases: Vec<Phase> = rec
        .all_phases()
        .unwrap()
        .iter()
        .map(|ph| {
            ph.iter()
                .map(|c| LCell {
                    i: m[c.i as usize] as i32,
                    ..*c
                })
                .collect()
        })
        .collect();
    let built = if rec.class == PatternClass::Mover {
        build_mover_site(&topo, rec.neighborhood, &rule, &phases[0])
    } else {
        build_site_for(&topo, rec.neighborhood, &rule, &phases)
    };
    let (s2, ph, canonical) = built.unwrap();
    let mut y = rec.clone();
    y.site = Some(s2);
    y.phases = ph.iter().map(|v| encode_cells(v)).collect();
    y.canonical = canonical;
    y.tiling = pattern::tiling_identity(spec);
    Some(y)
}

// The level of an aperiodic patch is its size: an oscillator on the Spectre and the hat glider,
// found again on levels 3, 4 and 5 (the level given as the size and as a parameter), have one id
// and one pin, the reference patch's.
#[test]
fn aperiodic_records_have_one_id_on_every_level() {
    let mut w = World::new(patch_at("spectre", 4, false), 1).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new("B35/S234").seed(SeedSpec::Random { density: 0.4 }),
    })
    .unwrap();
    w.apply(Command::Step { gens: 600 }).unwrap();
    let osc = records_of(&w)
        .into_iter()
        .filter(|r| {
            r.class == PatternClass::Oscillator
                && r.period >= 2
                && r.site.as_ref().is_some_and(|s| s.graph.is_some())
        })
        .min_by_key(|r| (r.cells, r.id.clone()))
        .expect("an oscillator on the Spectre patch");
    let mut spec = TrackSpec::new(
        "Priority 4: 3|n2>=1->2; 0|n3>=2->0; 0|n2>=1->3; 0|n1>=1->2",
        Nb::Touch,
        patch_at("hat", 4, false),
        vec![(0, 1), (3, 2)],
    );
    spec.max_level = Some(5);
    let glider = mover_record(&spec, &track(&spec).unwrap(), prov(), DiscoveredBy::Search).unwrap();
    for (family, rec) in [("spectre", &osc), ("hat", &glider)] {
        let reference = Topology::geometry_pin(&patch_at(family, 4, false))
            .unwrap()
            .pin;
        assert_eq!(rec.geometry_pin, reference);
        for level in [3, 4, 5] {
            for param in [false, true] {
                let y = on_patch(rec, &patch_at(family, level, param))
                    .unwrap_or_else(|| panic!("{family}: {} not on level {level}", rec.label()));
                assert_eq!(
                    pattern::pattern_id(&y).unwrap(),
                    rec.id,
                    "{family} {} on level {level} (as a parameter: {param})",
                    rec.label()
                );
                assert_eq!(pattern::record_pin(&y).unwrap().pin, reference);
            }
        }
    }
}

/// The square grid's geometry pin and the Life glider's id (B3/S23, touching neighbours).
const SQUARE_PIN: &str = "0145310ebd49abfe89eb01c42ecaf459";
const GLIDER_ID: &str = "5a080db389c0cab3caf884d2596a1e70";
