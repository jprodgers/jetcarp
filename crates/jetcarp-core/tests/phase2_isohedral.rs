//! Phase 2: Kaplan's isohedral tiling types (tactile's 81 of IH01–IH93).

use jetcarp_core::edges::{self, EdgeMode, EdgeShape, GlueKind, Node};
use jetcarp_core::families;
use jetcarp_core::geom::{self, P};
use jetcarp_core::isohedral::{self, IH_FAMILIES, IH_TYPES};
use jetcarp_core::rng::Rng;
use jetcarp_core::world::*;
use jetcarp_core::*;
use serde_json::json;

fn spec(id: &str, params: Option<serde_json::Value>, shapes: Option<Vec<EdgeShape>>) -> TilingSpec {
    TilingSpec {
        params,
        edges: shapes,
        size: Some(SizeSpec::Cells(300)),
        ..TilingSpec::new(id)
    }
}

fn outline(t: &Topology, i: usize) -> Vec<P> {
    let m = t.transform(i);
    t.outlines[t.shape_of(i)]
        .iter()
        .map(|&p| m.apply(p))
        .collect()
}

/// No gaps or overlaps: the unit cell's area equals the tiles' area, every tile has the same
/// area, and every outline point of the centre tile lies on a touching neighbour's outline.
fn check_tiles(t: &Topology, what: &str) {
    let g = t.lattice().expect("isohedral tilings use lattice storage");
    let cell = geom::cross(g.a1, g.a2).abs();
    let tiles: f64 = (0..g.ntypes as usize)
        .map(|k| geom::area(&outline(t, k)))
        .sum();
    assert!(
        (cell - tiles).abs() < 1e-6 * cell,
        "{what}: unit cell area {cell} but tiles cover {tiles}"
    );
    let corner_area: f64 = (0..g.ntypes as usize)
        .map(|k| geom::area(&t.polygon(k)))
        .sum();
    assert!(
        (cell - corner_area).abs() < 1e-6 * cell,
        "{what}: straight tiles cover {corner_area} of {cell}"
    );
    let c = t.center_cell();
    for k in 0..g.ntypes {
        let (u, j, _) = g.decompose(c);
        let i = g.index(u, j, k);
        let mine = outline(t, i);
        let mut others = Vec::new();
        t.for_each_neighbor(i, Nb::Touch, |j| others.extend(outline(t, j)));
        let scale = (cell / g.ntypes as f64).sqrt();
        let miss = mine
            .iter()
            .filter(|p| !others.iter().any(|q| geom::dist(**p, *q) < 1e-6 * scale))
            .count();
        assert_eq!(
            miss,
            0,
            "{what}: tile type {k}: {miss} of {} outline points unmatched",
            mine.len()
        );
        // The centre tile's own neighbours do not overlap it: their centroids lie outside it.
        let cen = geom::centroid(&t.polygon(i));
        t.for_each_neighbor(i, Nb::Touch, |j| {
            assert!(
                geom::dist(geom::centroid(&t.polygon(j)), cen) > 1e-6 * scale,
                "{what}: two tiles share a centroid"
            );
        });
    }
}

/// Reshape every edge class within its constraint: two free nodes (J), a mirrored pair (U),
/// a half-turn pair (S), nothing for straight edges.
fn reshaped(fam: &families::Family, amp: f64) -> Vec<EdgeShape> {
    let mut shapes = families::fresh_edges(fam);
    for (ci, st) in shapes.iter_mut().enumerate() {
        let kind = fam.classes[ci].kind;
        st.nodes.clear();
        st.mode = edges::allowed_modes(kind)[0];
        edges::insert_node(st, kind, Node { t: 0.3, d: amp });
        if st.mode == EdgeMode::J {
            edges::insert_node(
                st,
                kind,
                Node {
                    t: 0.7,
                    d: -amp * 0.6,
                },
            );
        }
        match kind {
            GlueKind::Straight => assert!(st.mode == EdgeMode::I),
            _ => assert_eq!(st.nodes.len(), 2, "{}", fam.id),
        }
    }
    shapes
}

#[test]
fn all_81_types_listed() {
    assert_eq!(IH_TYPES.len(), 81);
    assert_eq!(IH_FAMILIES.len(), 81);
    let missing = [19, 35, 48, 60, 63, 65, 70, 75, 80, 87, 89, 92];
    for n in 1..=93u32 {
        assert_eq!(
            isohedral::id_for(n).is_some(),
            !missing.contains(&n),
            "IH{n:02}"
        );
    }
    for f in &IH_FAMILIES {
        let found = families::family(f.id).unwrap();
        assert_eq!(found.id, f.id);
        // Classic families are untouched and still listed first.
        assert!(families::FAMILIES.iter().all(|g| g.id != f.id));
    }
    // A few wallpaper groups computed by the generator, checked against Grünbaum & Shephard.
    for (id, sym) in [
        ("ih01", "p1"),
        ("ih41", "p1"),
        ("ih04", "p2"),
        ("ih84", "p2"),
        ("ih28", "p4"),
        ("ih93", "p6m"),
    ] {
        assert_eq!(families::family(id).unwrap().sym, sym, "{id}");
    }
}

#[test]
fn every_type_builds_and_tiles_at_defaults() {
    for f in &IH_FAMILIES {
        let t =
            Topology::build(&spec(f.id, None, None)).unwrap_or_else(|e| panic!("{}: {e}", f.id));
        assert!(t.warnings.is_empty(), "{}: {:?}", f.id, t.warnings);
        assert!(t.len() >= 64, "{}: {} cells", f.id, t.len());
        let (e, touch) = t.degree_sets();
        assert!(e.iter().all(|&d| d >= 3), "{}: edge degrees {e:?}", f.id);
        assert!(
            touch.iter().all(|&d| d >= 3),
            "{}: touch degrees {touch:?}",
            f.id
        );
        check_tiles(&t, f.id);
    }
}

#[test]
fn reshaped_edges_still_interlock() {
    for f in &IH_FAMILIES {
        for amp in [0.12, -0.09] {
            let s = spec(f.id, None, Some(reshaped(f, amp)));
            let t = Topology::build(&s).unwrap_or_else(|e| panic!("{}: {e}", f.id));
            check_tiles(&t, &format!("{} reshaped {amp}", f.id));
            // Curved outlines leave the straight-edge neighbour graph alone at small amplitude.
            assert!(
                jetcarp_core::topology::curve_check(&s).unwrap().1 > 0,
                "{}",
                f.id
            );
        }
    }
}

#[test]
fn each_class_reshaped_alone_still_interlocks() {
    for f in &IH_FAMILIES {
        let all = reshaped(f, 0.1);
        for ci in 0..f.classes.len() {
            let mut shapes = families::fresh_edges(f);
            shapes[ci] = all[ci].clone();
            let t = Topology::build(&spec(f.id, None, Some(shapes))).unwrap();
            check_tiles(&t, &format!("{} class {ci}", f.id));
        }
    }
}

#[test]
fn random_parameters_still_tile() {
    let mut rng = Rng::new(0x1504_EDA1);
    let mut built = 0;
    for (f, t) in IH_FAMILIES.iter().zip(IH_TYPES.iter()) {
        let mut ok = 0;
        for trial in 0..6 {
            let p: Vec<f64> = t
                .defaults
                .iter()
                .map(|&x| x + (rng.next_f64() - 0.5) * 0.16)
                .collect();
            let corners = t.vertices(&p);
            if edges::self_intersects(&corners) {
                continue;
            }
            let params = Some(json!({ "p": p }));
            let shapes = (trial % 2 == 1).then(|| reshaped(f, 0.08));
            let topo = Topology::build(&spec(f.id, params, shapes))
                .unwrap_or_else(|e| panic!("{} trial {trial}: {e}", f.id));
            check_tiles(&topo, &format!("{} trial {trial}", f.id));
            ok += 1;
        }
        assert!(
            t.np == 0 || ok >= 3,
            "{}: only {ok} of 6 perturbations tiled",
            f.id
        );
        built += ok;
    }
    assert!(built > 300);
}

#[test]
fn handles_move_corners_and_json_round_trips() {
    for f in &IH_FAMILIES {
        let mut p = families::init_params(f.id);
        let t = IH_TYPES
            .iter()
            .find(|t| format!("ih{:02}", t.num) == f.id)
            .unwrap();
        for &h in families::handles(f.id) {
            // Drag along a direction the corner can move in, plus a sideways part it may not.
            let d = *t.jacobian(h).iter().find(|d| geom::len(**d) > 0.0).unwrap();
            let d = geom::scale(d, 0.03 / geom::len(d));
            let start = families::corners(f.id, &p).unwrap()[h];
            let target = geom::add(start, geom::add(d, [-d[1] * 0.5, d[0] * 0.5]));
            families::set_handle(f.id, &mut p, h, target);
            let after = families::corners(f.id, &p).unwrap()[h];
            assert!(
                geom::dist(after, target) < geom::dist(start, target) - 0.01,
                "{} corner {h}: dragging did not move it closer",
                f.id
            );
        }
        assert_eq!(families::handles(f.id).is_empty(), t.np == 0, "{}", f.id);
        // Tile JSON round-trips, including reshaped edges.
        let shapes = reshaped(f, 0.1);
        let json = families::tile_json(f, &p, Some(&shapes));
        let text = serde_json::to_string(&json).unwrap();
        let back: TilingSpec = serde_json::from_str(&text).unwrap();
        assert_eq!(back.family, f.id);
        let near = |a: &serde_json::Value, b: &serde_json::Value| {
            let (a, b) = (a["p"].as_array().unwrap(), b["p"].as_array().unwrap());
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(x, y)| (x.as_f64().unwrap() - y.as_f64().unwrap()).abs() < 1e-12)
        };
        assert!(near(back.params.as_ref().unwrap(), &p), "{}", f.id);
        assert_eq!(back.edges.as_deref(), Some(&shapes[..]));
        let a = Topology::build(&TilingSpec {
            size: Some(SizeSpec::Cells(200)),
            ..back
        })
        .unwrap();
        assert!(a.warnings.is_empty(), "{}: {:?}", f.id, a.warnings);
    }
    // A full-rank corner lands exactly where it is dragged.
    let mut p = families::init_params("ih41");
    let h = families::handles("ih41")[0];
    let target = geom::add(families::corners("ih41", &p).unwrap()[h], [0.05, 0.04]);
    families::set_handle("ih41", &mut p, h, target);
    assert!(geom::dist(families::corners("ih41", &p).unwrap()[h], target) < 1e-9);
    // Wrong parameter counts are reported, not ignored.
    assert!(Topology::build(&spec("ih41", Some(json!({"p": [0.1]})), None)).is_err());
    assert!(Topology::build(&spec("ih41", None, Some(vec![]))).is_err());
}

#[test]
fn cas_run_on_isohedral_tilings_with_every_edge_mode() {
    for (id, b) in [
        ("ih01", Boundary::Wrap),
        ("ih28", Boundary::Dead),
        ("ih56", Boundary::Live),
        ("ih93", Boundary::Wrap),
    ] {
        let mut s = Scene::new(spec(id, None, None).with_boundary(b));
        s.cas = vec![
            CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.3 }),
            CaSpec::new("Ant RL"),
        ];
        let mut w = World::from_scene(&s).unwrap();
        w.apply(Command::Step { gens: 20 }).unwrap();
        let again = World::from_scene(&w.to_scene()).unwrap();
        assert_eq!(again.state_hash(), w.state_hash(), "{id}");
        assert_eq!(w.topo.wraps(), b == Boundary::Wrap);
    }
}

#[test]
fn ghosts_surround_the_prototile() {
    for f in &IH_FAMILIES {
        let s = TilingSpec::new(f.id);
        let g = jetcarp_core::topology::ghost_transforms(&s, 3).unwrap();
        let corners = families::corners(f.id, &families::init_params(f.id)).unwrap();
        // Every corner of the prototile is shared by at least two ghost tiles.
        for c in &corners {
            let n = g
                .iter()
                .filter(|(m, _)| corners.iter().any(|q| geom::dist(m.apply(*q), *c) < 1e-6))
                .count();
            assert!(n >= 2, "{}: corner shared by {n} ghosts", f.id);
        }
    }
}

// The editor maps a node dragged on any edge back to its class's stored node. For the classic
// families the new per-edge mapping must equal the old master/slave rule.
#[test]
fn editor_node_mapping_round_trips() {
    let all = IH_FAMILIES
        .iter()
        .chain(families::FAMILIES.iter().filter(|f| !f.classes.is_empty()));
    for f in all {
        let shapes = reshaped(f, 0.1);
        for ci in 0..f.classes.len() {
            for &e in f.classes[ci].edges {
                let drawn = edges::nodes_for_edge(f.classes, &shapes, e);
                for (i, q) in drawn.iter().enumerate() {
                    let back = edges::to_class_frame(f.classes, e, *q);
                    let k = edges::class_node_index(f.classes, e, i, drawn.len());
                    let want = shapes[ci].nodes[k];
                    assert!(
                        (back.t - want.t).abs() < 1e-12 && (back.d - want.d).abs() < 1e-12,
                        "{} edge {e} node {i}",
                        f.id
                    );
                    if f.classes[ci].orient.is_empty() {
                        let (_, slave) = edges::class_of_edge(f.classes, e);
                        let kind = f.classes[ci].kind;
                        assert_eq!(edges::to_master(kind, slave, *q), back, "{}", f.id);
                        assert_eq!(edges::master_index(kind, slave, i, drawn.len()), k);
                    }
                }
            }
        }
    }
}
