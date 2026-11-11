//! Phase 1 checks.

use jetcarp_core::rules::Rule;
use jetcarp_core::table::{RuleTable, Symmetry};
use jetcarp_core::world::*;
use jetcarp_core::*;

fn square(w: u32, h: u32, b: Boundary) -> TilingSpec {
    TilingSpec::rect("square", w, h, b)
}

fn run_rows(rule: &str, k: usize) -> Vec<Vec<u8>> {
    let mut w = World::new(TilingSpec::rect("hex", 81, 40, Boundary::Dead), 1).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new(rule),
    })
    .unwrap();
    w.apply(Command::Step { gens: k as u64 }).unwrap();
    let g = w.topo.hex_rows().unwrap().clone();
    (0..=k)
        .map(|j| {
            (0..g.w as i32)
                .map(|u| w.fields[0].state[g.index(u, j as i32, 0)])
                .collect()
        })
        .collect()
}

// Hex elementary rule "left XOR right" is Pascal's triangle mod 2: row k has 2^popcount(k) cells.
#[test]
fn hex_wolfram_binary_is_pascal_mod_2() {
    // index = 2*left + right: 01 -> 1, 10 -> 1, so the rule number is 0b0110 = 6.
    let rows = run_rows("HexW 6", 30);
    for (k, row) in rows.iter().enumerate() {
        let live = row.iter().filter(|&&v| v != 0).count();
        assert_eq!(live, 1 << (k as u32).count_ones(), "row {k}");
    }
}

// Ternary "left + right mod 3" is Pascal's triangle mod 3 (Lucas): nonzero count = prod(d_i + 1).
#[test]
fn hex_wolfram_ternary_is_pascal_mod_3() {
    let number: u32 = (0..9u32)
        .map(|idx| ((idx / 3 + idx % 3) % 3) * 3u32.pow(idx))
        .sum();
    let rows = run_rows(&format!("HexW3 {number}"), 30);
    for (k, row) in rows.iter().enumerate() {
        let mut want = 1;
        let mut x = k;
        while x > 0 {
            want *= x % 3 + 1;
            x /= 3;
        }
        assert_eq!(row.iter().filter(|&&v| v != 0).count(), want, "row {k}");
    }
}

// The table engine reproduces Life when Life is written as a permute-symmetric table.
#[test]
fn table_engine_matches_builtin_life() {
    let mut transitions = Vec::new();
    for c in 0..2u8 {
        for live in 0..=8usize {
            let mut n = vec!["1"; live];
            n.extend(vec!["0"; 8 - live]);
            let new = if live == 3 || (c == 1 && live == 2) {
                1
            } else {
                0
            };
            transitions.push(format!("{c},{}:{new}", n.join(",")));
        }
    }
    let table = RuleTable {
        name: "life-table".into(),
        description: String::new(),
        source: String::new(),
        states: 2,
        neighborhood: Nb::Touch,
        symmetry: Symmetry::Permute,
        arity: 8,
        transitions,
    };
    let mk = |rule: &str| {
        let mut w = World::new(square(40, 30, Boundary::Wrap), 5).unwrap();
        w.apply(Command::AddTable {
            table: table.clone(),
        })
        .unwrap();
        w.apply(Command::AddCa {
            ca: CaSpec::new(rule).seed(SeedSpec::Random { density: 0.35 }),
        })
        .unwrap();
        w.apply(Command::Step { gens: 40 }).unwrap();
        w.fields[0].state.clone()
    };
    assert_eq!(mk("Table life-table"), mk("B3/S23"));
}

// Positional table with rotation symmetry: parity (XOR of the four edge neighbours).
#[test]
fn table_rotation_symmetry_parity() {
    let mut w = World::new(square(32, 32, Boundary::Wrap), 9).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new("Table parity").seed(SeedSpec::Random { density: 0.4 }),
    })
    .unwrap();
    let g = w.topo.lattice().unwrap().clone();
    for _ in 0..10 {
        let before = w.fields[0].state.clone();
        w.apply(Command::Step { gens: 1 }).unwrap();
        for i in 0..w.topo.len() {
            let (u, j, _) = g.decompose(i);
            let at = |du: i32, dj: i32| {
                before[g
                    .resolve(u + g.row_start[j as usize] + du, j + dj, 0)
                    .unwrap()]
            };
            let x = at(0, -1) ^ at(1, 0) ^ at(0, 1) ^ at(-1, 0);
            assert_eq!(w.fields[0].state[i], x);
        }
    }
    // Saved scenes carry tables added by command.
    let s = w.to_scene();
    assert_eq!(World::from_scene(&s).unwrap().state_hash(), w.state_hash());
}

#[test]
fn square_edge_slots_are_n_e_s_w() {
    let t = Topology::build(&square(10, 10, Boundary::Dead)).unwrap();
    let g = t.lattice().unwrap();
    let dirs: Vec<(i32, i32)> = g.offsets[Nb::Edge as usize][0]
        .iter()
        .map(|o| (o.di, o.dj))
        .collect();
    assert_eq!(dirs, vec![(0, -1), (1, 0), (0, 1), (-1, 0)]);
}

fn one_step(rule: &str, cells: &[(i32, i32, u32)]) -> (World, jetcarp_core::topology::LatticeGrid) {
    let mut w = World::new(square(9, 9, Boundary::Dead), 1).unwrap();
    let g = w.topo.lattice().unwrap().clone();
    let c: Vec<[u32; 2]> = cells
        .iter()
        .map(|&(x, y, v)| [g.index(x, y, 0) as u32, v])
        .collect();
    w.apply(Command::AddCa {
        ca: CaSpec::new(rule).seed(SeedSpec::Cells { cells: c }),
    })
    .unwrap();
    w.apply(Command::Step { gens: 1 }).unwrap();
    (w, g)
}

#[test]
fn cyclic_and_gh_single_steps() {
    // Cyclic 4/1 (edge neighbours): a 0 next to a 1 becomes 1; the 1 (no 2 nearby) stays.
    let (w, g) = one_step("Cyclic 4/1", &[(4, 4, 1)]);
    let s = |x, y| w.fields[0].state[g.index(x, y, 0)];
    assert_eq!((s(4, 4), s(4, 3), s(5, 4), s(3, 3)), (1, 1, 1, 0));
    // GH 4/1 (touching): excited -> refractory 2; resting neighbours become excited.
    let (w, g) = one_step("GH 4/1", &[(4, 4, 1)]);
    let s = |x, y| w.fields[0].state[g.index(x, y, 0)];
    assert_eq!((s(4, 4), s(3, 3), s(5, 5), s(6, 6)), (2, 1, 1, 0));
}

#[test]
fn hodgepodge_single_step() {
    // n = 10, k1 = 2, k2 = 3, g = 4 on touching neighbours.
    // Healthy cell with 4 infected and 3 ill neighbours: 4/2 + 3/3 = 3.
    let cells = [
        (3, 3, 5),
        (4, 3, 5),
        (5, 3, 5),
        (3, 4, 5),
        (5, 4, 10),
        (3, 5, 10),
        (4, 5, 10),
    ];
    let (w, g) = one_step("Hodgepodge 10/2/3/4", &cells);
    assert_eq!(w.fields[0].state[g.index(4, 4, 0)], 3);
    // Ill cells become healthy.
    assert_eq!(w.fields[0].state[g.index(5, 4, 0)], 0);
    // Infected (3,3) = 5: neighbours (4,3)=5, (3,4)=5, others 0 -> S = 15, a = 2, b = 0:
    // 15/3 + 4 = 9.
    assert_eq!(w.fields[0].state[g.index(3, 3, 0)], 9);
}

#[test]
fn forest_fire_is_thread_independent() {
    let mut s = Scene::new(square(80, 60, Boundary::Wrap));
    s.cas = vec![CaSpec::new("ForestFire 0.05/0.001")];
    let run = |t| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(t)
            .build()
            .unwrap()
            .install(|| {
                let mut w = World::from_scene(&s).unwrap();
                w.apply(Command::Step { gens: 100 }).unwrap();
                (w.state_hash(), w.fields[0].population())
            })
    };
    let (a, pop) = run(1);
    assert_eq!(a, run(8).0);
    assert!(pop > 100, "forest grew");
}

#[test]
fn new_rules_parse_and_run_everywhere() {
    for rule in [
        "Cyclic 14/1",
        "GH 8/2",
        "RPS 3/3",
        "ForestFire 0.02/0.0001",
        "Hodgepodge 100/2/3/28",
    ] {
        for fam in ["square", "hex", "cairo", "trihex"] {
            let mut w =
                World::new(TilingSpec::new(fam).with_size(SizeSpec::Cells(400)), 3).unwrap();
            w.apply(Command::AddCa {
                ca: CaSpec::new(rule),
            })
            .unwrap();
            w.apply(Command::Step { gens: 20 }).unwrap();
            assert!(matches!(
                w.cas[0].rule,
                Rule::Cyclic { .. }
                    | Rule::GreenbergHastings { .. }
                    | Rule::Rps { .. }
                    | Rule::ForestFire { .. }
                    | Rule::Hodgepodge { .. }
            ));
        }
    }
    // Hex elementary rules refuse non-hex tilings with a clear message.
    let mut w = World::new(square(20, 20, Boundary::Dead), 1).unwrap();
    let e = w
        .apply(Command::AddCa {
            ca: CaSpec::new("HexW 6"),
        })
        .unwrap_err();
    assert!(e.contains("hexagon"), "{e}");
}

#[test]
fn script_steps_fire_in_order_and_replay() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/data/scenes/script-five.json"
    ))
    .unwrap();
    let scene = Scene::from_json(&text).unwrap();
    let mut w = World::from_scene(&scene).unwrap();
    let running = |w: &World| -> Vec<String> {
        w.cas
            .iter()
            .filter(|c| c.enabled)
            .map(|c| c.name.clone())
            .collect()
    };
    assert_eq!(running(&w), vec!["Rule 30"]);
    w.apply(Command::Step { gens: 149 }).unwrap();
    assert_eq!(
        w.script_state.step, 1,
        "Rule 30 step ends after 149 generations"
    );
    assert_eq!(running(&w), vec!["Day & Night"]);
    // Run until the ants step, recording when each step began.
    let mut starts = vec![(0usize, 0u64), (1, 149)];
    for _ in 0..4000 {
        let k = w.script_state.step;
        w.apply(Command::Step { gens: 1 }).unwrap();
        if w.script_state.step != k {
            starts.push((w.script_state.step, w.generation));
        }
        if w.script_state.step == 4 {
            break;
        }
    }
    assert_eq!(w.script_state.step, 4, "reached the ants: {starts:?}");
    assert_eq!(running(&w), vec!["Langton's ant"]);
    let seeds =
        starts.iter().find(|s| s.0 == 3).unwrap().1 - starts.iter().find(|s| s.0 == 2).unwrap().1;
    assert_eq!(seeds, 12, "Seeds runs exactly 12 generations");
    w.apply(Command::Step { gens: 5 }).unwrap();
    // Replays exactly from the saved scene.
    let again = World::from_scene(&w.to_scene()).unwrap();
    assert_eq!(again.state_hash(), w.state_hash());
    assert_eq!(again.script_state, w.script_state);
}

#[test]
fn script_population_and_goto() {
    let mut s = Scene::new(square(30, 30, Boundary::Dead));
    s.cas = vec![
        CaSpec::new("B2/S").seed(SeedSpec::Random { density: 0.03 }),
        CaSpec::new("B3/S23"),
    ];
    s.script = Some(jetcarp_core::script::Script {
        steps: vec![
            jetcarp_core::script::ScriptStep {
                name: "grow".into(),
                run: vec![jetcarp_core::script::CaRef::Id(1)],
                until: jetcarp_core::script::Trigger::PopulationAbove { n: 100 },
                max_gens: None,
            },
            jetcarp_core::script::ScriptStep {
                name: "life".into(),
                run: vec![jetcarp_core::script::CaRef::Id(2)],
                until: jetcarp_core::script::Trigger::Never,
                max_gens: None,
            },
        ],
        repeat: false,
    });
    let mut w = World::from_scene(&s).unwrap();
    while w.script_state.step == 0 && w.generation < 50 {
        w.apply(Command::Step { gens: 1 }).unwrap();
    }
    assert_eq!(w.script_state.step, 1);
    assert!(w.fields[0].population() >= 100);
    w.apply(Command::ScriptGoto { step: 0 }).unwrap();
    assert!(w.cas[0].enabled && !w.cas[1].enabled);
}

// Tile editing: every reshapeable family still tiles after reshaping every edge class.
// Port of the prototype's curve check: each outline point of the centre tile must lie on a
// neighbour's outline.
#[test]
fn edited_tiles_still_interlock() {
    use jetcarp_core::edges::{self, EdgeMode, Node};
    use jetcarp_core::families::{self, FAMILIES, FamilyKind};
    for fam in FAMILIES.iter().filter(|f| f.kind != FamilyKind::Static) {
        let mut shapes = families::fresh_edges(fam);
        for (ci, st) in shapes.iter_mut().enumerate() {
            let kind = fam.classes[ci].kind;
            st.nodes.clear();
            st.mode = if edges::allowed_modes(kind).contains(&EdgeMode::J) {
                EdgeMode::J
            } else {
                EdgeMode::S
            };
            edges::insert_node(st, kind, Node { t: 0.3, d: 0.12 });
            if st.mode == EdgeMode::J {
                edges::insert_node(st, kind, Node { t: 0.7, d: -0.08 });
            }
        }
        let spec = TilingSpec {
            edges: Some(shapes),
            size: Some(if fam.id == "spectre" {
                SizeSpec::Level(2)
            } else {
                SizeSpec::Cells(200)
            }),
            ..TilingSpec::new(fam.id)
        };
        let t = Topology::build(&spec).unwrap();
        assert!(t.warnings.is_empty(), "{}: {:?}", fam.id, t.warnings);
        let c = t.center_cell();
        let outline = |i: usize| -> Vec<geom::P> {
            let m = t.transform(i);
            t.outlines[t.shape_of(i)]
                .iter()
                .map(|&p| m.apply(p))
                .collect()
        };
        let mine = outline(c);
        let mut others = Vec::new();
        t.for_each_neighbor(c, Nb::Touch, |j| others.extend(outline(j)));
        let miss = mine
            .iter()
            .filter(|p| !others.iter().any(|q| geom::dist(**p, *q) < 1e-6))
            .count();
        assert_eq!(
            miss,
            0,
            "{}: {miss} of {} outline points unmatched",
            fam.id,
            mine.len()
        );
    }
}

#[test]
fn editor_node_symmetry_and_handles() {
    use jetcarp_core::edges::{self, EdgeMode, EdgeShape, GlueKind, Node};
    let mut st = EdgeShape {
        mode: EdgeMode::S,
        curved: true,
        nodes: vec![],
    };
    edges::insert_node(&mut st, GlueKind::SelfHalfTurn, Node { t: 0.2, d: 0.1 });
    assert_eq!(st.nodes.len(), 2);
    edges::set_node(&mut st, 0, Node { t: 0.3, d: 0.2 });
    assert!((st.nodes[1].t - 0.7).abs() < 1e-12 && (st.nodes[1].d + 0.2).abs() < 1e-12);
    st.mode = EdgeMode::U;
    edges::resymmetrize(&mut st);
    assert!(
        (st.nodes[1].d - 0.2).abs() < 1e-12,
        "mirror twin keeps the offset"
    );
    edges::delete_node(&mut st, 1);
    assert!(st.nodes.is_empty());
    // Dragging a hexagon handle changes the tiling but keeps six neighbours.
    let mut p = jetcarp_core::families::init_params("hex");
    jetcarp_core::families::set_handle("hex", &mut p, 2, [1.7, 0.95]);
    let spec = TilingSpec {
        params: Some(p.clone()),
        ..TilingSpec::new("hex")
    };
    let t = Topology::build(&spec).unwrap();
    assert_eq!(t.degree_sets(), (vec![6], vec![6]));
    // Tile JSON round-trips through the prototype format.
    let fam = jetcarp_core::families::family("hex").unwrap();
    let json =
        jetcarp_core::families::tile_json(fam, &p, Some(&jetcarp_core::families::fresh_edges(fam)));
    let back: TilingSpec = serde_json::from_value(json).unwrap();
    assert_eq!(back.params, Some(p));
    assert!(jetcarp_core::topology::curve_check(&back).unwrap().1 > 0);
}

#[test]
fn every_catalog_entry_runs_and_names_itself() {
    use jetcarp_core::rules::CATALOG;
    for e in CATALOG {
        let fam = if e.notation.starts_with("HexW") {
            "hex"
        } else if e.notation.starts_with("HexT") {
            "hex_flat"
        } else if e.notation == "SpectreGlider" {
            "spectre"
        } else {
            "square"
        };
        let size = if fam == "spectre" {
            SizeSpec::Level(3)
        } else {
            SizeSpec::Rect { w: 30, h: 30 }
        };
        let mut w = World::new(TilingSpec::new(fam).with_size(size), 2).unwrap();
        w.apply(Command::AddCa {
            ca: CaSpec::new(e.notation),
        })
        .unwrap_or_else(|err| panic!("{}: {err}", e.name));
        w.apply(Command::Step { gens: 5 }).unwrap();
        assert_eq!(w.cas[0].name, e.name, "{}", e.notation);
        assert_eq!(w.cas[0].notation(), e.notation, "{} round-trips", e.name);
    }
    assert!(CATALOG.len() >= 60);
}

// Plan test for the palette editor: one palette applied to Life, Brian's Brain, a sandpile, and
// rule 90, including a row gradient for rule 90.
#[test]
fn one_palette_on_four_cas() {
    use jetcarp_core::render::{ColorBy, Palette, cell_colors};
    let mut w = World::new(square(60, 40, Boundary::Dead), 4).unwrap();
    let pal = Palette::new("Sunset", vec![[255, 0, 0], [0, 0, 255]]);
    w.apply(Command::SetPalette {
        palette: pal.clone(),
    })
    .unwrap();
    let ids: Vec<u32> = ["B3/S23", "B2/S/C3", "Sandpile", "W90"]
        .iter()
        .map(|r| {
            w.apply(Command::AddCa {
                ca: CaSpec::new(r).private(),
            })
            .unwrap()
            .new_id
            .unwrap()
        })
        .collect();
    for &id in &ids {
        w.apply(Command::UpdateCa {
            id,
            change: CaChange {
                palette: Some("Sunset".into()),
                ..Default::default()
            },
        })
        .unwrap();
    }
    w.apply(Command::UpdateCa {
        id: ids[3],
        change: CaChange {
            color_by: Some(ColorBy::Row),
            ..Default::default()
        },
    })
    .unwrap();
    w.apply(Command::Step { gens: 30 }).unwrap();
    // Only rule 90 is drawn where it has cells and nothing above it; check its row gradient.
    let colors = cell_colors(&w);
    let g = w.topo.lattice().unwrap().clone();
    let f90 = &w.fields[w.ca(ids[3]).unwrap().field];
    let top = (0..g.w as i32)
        .map(|u| g.index(u, 0, 0))
        .find(|&i| f90.state[i] != 0)
        .unwrap();
    let low = (0..g.w as i32)
        .map(|u| g.index(u, 25, 0))
        .find(|&i| f90.state[i] != 0)
        .unwrap();
    assert!(
        colors[top][0] > colors[low][0],
        "red at the top, bluer lower down"
    );
    assert!(colors[low][2] > colors[top][2]);
    // Palettes travel with the scene.
    let s = w.to_scene();
    assert!(
        s.commands
            .iter()
            .any(|c| matches!(c, Command::SetPalette { .. }))
    );
    let again = World::from_scene(&s).unwrap();
    assert_eq!(again.palettes, vec![pal]);
    assert_eq!(cell_colors(&again), colors);
    assert!(w.palette_names().contains(&"Sunset".to_string()));
}

// A palette name the world does not know (a typo, or another case) is refused with the names,
// rather than quietly drawn in the default palette, when a command gives it: a new CA or a change.
#[test]
fn an_unknown_palette_is_refused() {
    use jetcarp_core::render::Palette;
    let mut w = World::new(square(20, 20, Boundary::Wrap), 1).unwrap();
    let mut ca = CaSpec::new("B3/S23");
    ca.palette = Some("ember".into());
    let e = w.apply(Command::AddCa { ca: ca.clone() }).unwrap_err();
    assert!(
        e.contains("no palette named 'ember'") && e.contains("Palettes: Ember, Tide"),
        "{e}"
    );
    assert!(w.cas.is_empty() && w.log().is_empty());
    assert!(w.warnings().is_empty());
    // Built-ins, hex colours, and the world's own palettes are all fine.
    let id = w
        .apply(Command::AddCa {
            ca: CaSpec::new("B3/S23"),
        })
        .unwrap()
        .new_id
        .unwrap();
    w.apply(Command::SetPalette {
        palette: Palette::new("Mine", vec![[1, 2, 3]]),
    })
    .unwrap();
    for p in ["Tide", "#4fc3ff", "#103050-#4fc3ff", "Mine"] {
        let change = CaChange {
            palette: Some(p.into()),
            ..Default::default()
        };
        w.apply(Command::UpdateCa { id, change }).unwrap();
    }
    // A refused change leaves the CA as it was, its rule too.
    let change = CaChange {
        rule: Some("B36/S23".into()),
        palette_neg: Some("Nonesuch".into()),
        ..Default::default()
    };
    let e = w.apply(Command::UpdateCa { id, change }).unwrap_err();
    assert!(e.contains("'Nonesuch'") && e.contains("Mine"), "{e}");
    assert_eq!(
        (
            w.ca(id).unwrap().rule.notation(),
            w.ca(id).unwrap().palette.as_str()
        ),
        ("B3/S23".to_string(), "Mine")
    );
}

// A scene file that names a palette this JETCARP does not know (one of a later release's
// built-ins, or a hand-edited name) still loads: the name is kept, so saving keeps it, the CA is
// drawn in Ember, and the load says so. Commands in its log are taken the same way.
#[test]
fn a_scene_with_an_unknown_palette_loads_and_says_so() {
    use jetcarp_core::render::Palette;
    let mut sc = Scene::new(square(20, 20, Boundary::Wrap));
    let mut ca = CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.3 });
    ca.palette = Some("Dusk".into());
    sc.cas = vec![ca];
    sc.commands = vec![
        Command::UpdateCa {
            id: 1,
            change: CaChange {
                palette_neg: Some("Twilight".into()),
                ..Default::default()
            },
        },
        Command::Step { gens: 3 },
    ];
    let w = World::from_scene(&sc).unwrap();
    assert_eq!(
        w.warnings(),
        [
            "palette 'Dusk' is unknown here; drawn in Ember",
            "palette 'Twilight' is unknown here; drawn in Ember"
        ]
    );
    assert_eq!(w.cas[0].palette, "Dusk");
    assert_eq!(w.palette("Dusk"), Palette::builtin("Ember").unwrap());
    assert_eq!(w.generation, 3);
    // Saved and loaded again, the names are still there, and so is the warning.
    let again = World::from_scene(&Scene::from_json(&w.to_scene().to_json()).unwrap()).unwrap();
    assert_eq!(again.cas[0].palette, "Dusk");
    assert_eq!(again.cas[0].palette_neg, "Twilight");
    assert_eq!(again.warnings(), w.warnings());
    assert_eq!(again.hash_hex(), w.hash_hex());
    // A reset or a new tiling keeps them too; a command that names one now is refused.
    let mut w = again;
    w.apply(Command::Reset).unwrap();
    w.apply(Command::SetTiling {
        tiling: square(24, 24, Boundary::Wrap),
    })
    .unwrap();
    assert_eq!(w.cas[0].palette, "Dusk");
    assert_eq!(w.warnings().len(), 2);
    let change = CaChange {
        palette: Some("Dawn".into()),
        ..Default::default()
    };
    let e = w.apply(Command::UpdateCa { id: 1, change }).unwrap_err();
    assert!(e.contains("no palette named 'Dawn'"), "{e}");
}

// The identity I of the sandpile group satisfies stab(I + I) = I; check on several grids.
#[test]
fn sandpile_identity_is_idempotent() {
    use jetcarp_core::world::SandpileOp;
    for (spec, nb) in [
        (square(24, 24, Boundary::Dead), Nb::Edge),
        (square(16, 16, Boundary::Dead), Nb::Touch),
        (TilingSpec::rect("hex", 20, 20, Boundary::Dead), Nb::Edge),
        (
            TilingSpec::new("cairo").with_size(SizeSpec::Cells(400)),
            Nb::Edge,
        ),
        (
            TilingSpec::new("spectre").with_size(SizeSpec::Level(3)),
            Nb::Edge,
        ),
    ] {
        let mut w = World::new(spec.clone(), 1).unwrap();
        let id = w
            .apply(Command::AddCa {
                ca: CaSpec::new("Sandpile").nb(nb).seed(SeedSpec::Empty),
            })
            .unwrap()
            .new_id
            .unwrap();
        w.apply(Command::Sandpile {
            id,
            action: SandpileOp::Identity,
        })
        .unwrap();
        let ident = w.fields[0].height.clone();
        assert!(
            ident.iter().any(|&h| h > 0),
            "{}: identity is not empty",
            spec.family
        );
        // I + I stabilises back to I.
        let cells: Vec<[u32; 2]> = ident
            .iter()
            .enumerate()
            .map(|(i, &h)| [i as u32, h])
            .collect();
        w.apply(Command::SetCells { id, cells }).unwrap();
        w.apply(Command::Sandpile {
            id,
            action: SandpileOp::Stabilize,
        })
        .unwrap();
        assert_eq!(w.fields[0].height, ident, "{} {nb:?}", spec.family);
    }
    // The square-grid identity is symmetric under the grid's half turn.
    let mut w = World::new(square(24, 24, Boundary::Dead), 1).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new("Sandpile").seed(SeedSpec::Empty),
    })
    .unwrap();
    w.apply(Command::Sandpile {
        id: 1,
        action: SandpileOp::Identity,
    })
    .unwrap();
    let h = &w.fields[0].height;
    let n = h.len();
    assert!((0..n).all(|i| h[i] == h[n - 1 - i]));
    // Wrapped grids have no identity.
    let mut w = World::new(square(20, 20, Boundary::Wrap), 1).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new("Sandpile").seed(SeedSpec::Empty),
    })
    .unwrap();
    assert!(
        w.apply(Command::Sandpile {
            id: 1,
            action: SandpileOp::Identity
        })
        .is_err()
    );
}

// Live edges: cells past the edge count as live. On an empty grid with B3/S23 and Moore
// neighbours, edge cells (3 missing neighbours) are born; corners (5 missing) are not.
#[test]
fn live_edges_count_as_live() {
    let mut w = World::new(square(10, 10, Boundary::Live), 1).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new("B3/S23").seed(SeedSpec::Empty),
    })
    .unwrap();
    w.apply(Command::Step { gens: 1 }).unwrap();
    let g = w.topo.lattice().unwrap().clone();
    let s = |x, y| w.fields[0].state[g.index(x, y, 0)];
    assert_eq!((s(0, 0), s(5, 0), s(0, 5), s(5, 5)), (0, 1, 1, 0));
    assert!(w.topo.describe().contains("edges live"));
    // Live edges also work on periodic multi-tile tilings.
    let mut w = World::new(
        TilingSpec::new("cairo")
            .with_size(SizeSpec::Cells(400))
            .with_boundary(Boundary::Live),
        1,
    )
    .unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new("B3/S23").seed(SeedSpec::Empty),
    })
    .unwrap();
    w.apply(Command::Step { gens: 1 }).unwrap();
    assert!(
        w.fields[0].population() > 0,
        "edge cells of the Cairo grid are born"
    );
    // Dead edges: nothing happens on an empty grid.
    let mut w = World::new(square(10, 10, Boundary::Dead), 1).unwrap();
    w.apply(Command::AddCa {
        ca: CaSpec::new("B3/S23").seed(SeedSpec::Empty),
    })
    .unwrap();
    w.apply(Command::Step { gens: 1 }).unwrap();
    assert_eq!(w.fields[0].population(), 0);
}

// Ants at an edge: bounce keeps them on the grid; fall off removes them.
#[test]
fn ants_bounce_or_fall_off() {
    use jetcarp_core::world::AgentEdge;
    for (edge, expect) in [(AgentEdge::Bounce, 1), (AgentEdge::FallOff, 0)] {
        let mut ca = CaSpec::new("Ant RL").seed(SeedSpec::Ants { count: 1 });
        ca.agent_edge = edge;
        let mut w = World::new(square(9, 9, Boundary::Dead), 1).unwrap();
        w.apply(Command::AddCa { ca }).unwrap();
        w.apply(Command::Step { gens: 3000 }).unwrap();
        assert_eq!(w.cas[0].agents.len(), expect, "{edge:?}");
    }
    // Wrap edges never lose ants.
    let mut ca = CaSpec::new("Ant RL").seed(SeedSpec::Ants { count: 1 });
    ca.agent_edge = AgentEdge::FallOff;
    let mut w = World::new(square(9, 9, Boundary::Wrap), 1).unwrap();
    w.apply(Command::AddCa { ca }).unwrap();
    w.apply(Command::Step { gens: 3000 }).unwrap();
    assert_eq!(w.cas[0].agents.len(), 1);
    // On the Spectre, falling off means reaching the open boundary.
    let mut ca = CaSpec::new("Ant RL").seed(SeedSpec::Ants { count: 1 });
    ca.agent_edge = AgentEdge::FallOff;
    let mut w = World::new(TilingSpec::new("spectre").with_size(SizeSpec::Level(2)), 1).unwrap();
    w.apply(Command::AddCa { ca }).unwrap();
    w.apply(Command::Step { gens: 20000 }).unwrap();
    assert_eq!(w.cas[0].agents.len(), 0);
}
