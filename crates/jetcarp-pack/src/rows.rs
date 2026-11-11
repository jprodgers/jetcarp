//! Library records as pack rows, and pack rows as library records.
//!
//! A pattern's identity text is assembled from its stored canonical fields and checked against
//! its id (`SHA-256(identity + "\n" + pin)`); a record whose stored fields do not give its id
//! has its identity computed from its cells instead ([`jetcarp_core::pattern::range::identity`]),
//! and one whose identity still does not give its id is not published.

use crate::tables::*;
use jetcarp_core::objects::LCell;
use jetcarp_core::pattern::movers::MoverRecord;
use jetcarp_core::pattern::range::{self, IDENTITY_VERSION, RuleRange, content_id};
use jetcarp_core::pattern::site::{GraphCell, Site, SiteGraph, SiteTile};
use jetcarp_core::pattern::verify::Verification;
use jetcarp_core::pattern::{
    self, CurationState, DiscoveredBy, GunInfo, PatternClass, PatternRecord, Provenance,
    decode_cells, encode_cells,
};
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::{Boundary, Nb, SizeSpec, Storage, TilingSpec, Topology};
use jetcarp_core::world::Scene;
use serde::Serialize;
use serde_json::Value;

/// JSON text of a value in RFC 8785 form (what a table cell holding JSON contains).
pub fn jcs<T: Serialize>(t: &T) -> String {
    jetcarp_core::jcs::to_string(&serde_json::to_value(t).unwrap_or(Value::Null))
}

fn nb_name(nb: Nb) -> &'static str {
    match nb {
        Nb::Touch => "touch",
        Nb::Edge => "edge",
    }
}

pub fn parse_nb(s: &str) -> Result<Nb, String> {
    match s {
        "touch" => Ok(Nb::Touch),
        "edge" => Ok(Nb::Edge),
        x => Err(format!("neighbourhood '{x}' (touch or edge)")),
    }
}

/// The tiling a record's pin is the pin of: its site's patch on aperiodic tilings.
pub fn pin_spec(rec: &PatternRecord) -> &TilingSpec {
    rec.site.as_ref().map(|s| &s.patch).unwrap_or(&rec.tiling)
}

/// The identity text a record's stored fields give (they are in canonical form when JETCARP
/// stored the record).
fn identity_stored(rec: &PatternRecord) -> Result<String, String> {
    let rule = Rule::parse(&rec.rule)?.0;
    let exact = match rec.class {
        PatternClass::Puffer | PatternClass::Mover | PatternClass::Gun | PatternClass::Other => {
            rule.notation()
        }
        _ => String::new(),
    };
    let d = rec.displacement.unwrap_or([0, 0]);
    Ok(format!(
        "{IDENTITY_VERSION}\nmodel={}\nnb={}\nsteps={}\ntiling={}\nclass={}\nperiod={}\nd={},{}\nrule={exact}\ncanonical={}\nphases={}",
        range::cell_model(&rule),
        nb_name(rec.neighborhood),
        rec.steps_per_gen,
        pattern::identity_key(&rec.tiling),
        rec.class.key(),
        rec.period,
        d[0],
        d[1],
        rec.canonical,
        rec.phases.join(";"),
    ))
}

/// A record's identity text, checked against its id and pin.
pub fn identity_text(rec: &PatternRecord) -> Result<String, String> {
    if let Ok(t) = identity_stored(rec)
        && content_id(&t, &rec.geometry_pin) == rec.id
    {
        return Ok(t);
    }
    let t = range::identity(rec)?;
    if content_id(&t, &rec.geometry_pin) == rec.id {
        return Ok(t);
    }
    Err(format!(
        "pattern {}: its identity does not hash to its id (reverify it)",
        rec.id
    ))
}

/// The identity text's `d=` line.
fn identity_d(identity: &str) -> [i32; 2] {
    identity
        .lines()
        .find_map(|l| l.strip_prefix("d="))
        .and_then(|d| d.split_once(','))
        .and_then(|(a, b)| Some([a.parse().ok()?, b.parse().ok()?]))
        .unwrap_or([0, 0])
}

fn cells_row(p: &str) -> Result<Vec<Cell>, String> {
    Ok(decode_cells(p)?
        .into_iter()
        .map(|c| Cell {
            i: c.i,
            j: c.j,
            k: c.k as i32,
            s: c.s as i32,
        })
        .collect())
}

/// The canonical phase text of typed cells (sorted by row, column, tile type, state).
pub fn phase_text(cells: &[Cell]) -> Result<String, String> {
    let mut v: Vec<LCell> = cells
        .iter()
        .map(|c| {
            Ok(LCell {
                i: c.i,
                j: c.j,
                k: u32::try_from(c.k).map_err(|_| "a cell's tile type is negative")?,
                s: u8::try_from(c.s).map_err(|_| "a cell's state is not 0 to 255")?,
            })
        })
        .collect::<Result<_, String>>()?;
    v.sort();
    Ok(encode_cells(&v))
}

fn patch_level(t: &TilingSpec) -> i32 {
    if let Some(SizeSpec::Level(n)) = t.size {
        return n as i32;
    }
    t.params
        .as_ref()
        .and_then(|p| p.get("level"))
        .and_then(Value::as_u64)
        .map(|n| n as i32)
        .unwrap_or_else(|| jetcarp_core::pin::reference_level(&t.family) as i32)
}

pub fn site_row(s: &Site) -> SiteRow {
    SiteRow {
        patch: PatchRow {
            family: s.patch.family.clone(),
            params: s.patch.params.as_ref().map(jcs),
            level: patch_level(&s.patch),
        },
        anchor: s.anchor as i32,
        anchor_shape: s.anchor_shape as i32,
        anchor_point: s.anchor_point,
        radius: s.radius as i32,
        nb: s.nb.map(|n| nb_name(n).to_string()),
        tiles: s
            .tiles
            .iter()
            .map(|t| SiteTileRow {
                shape: t.shape as i32,
                dist: t.dist as i32,
                point: t.point,
                corners: t.corners.clone(),
            })
            .collect(),
        graph: s.graph.as_ref().map(|g| GraphRow {
            rule: g.rule.clone(),
            degrees: g.degrees,
            chirality: g.chirality,
            cells: g
                .cells
                .iter()
                .map(|c| GraphCellRow {
                    shape: c.shape as i32,
                    flip: c.flip,
                    degree: c.degree as i32,
                    states: c.states.iter().map(|&x| x as i32).collect(),
                    neighbours: c.nbrs.iter().map(|&x| x as i32).collect(),
                })
                .collect(),
            around: g
                .around
                .iter()
                .map(|(t, n)| AroundRow {
                    tiles: t.iter().map(|&x| x as i32).collect(),
                    count: *n as i32,
                })
                .collect(),
        }),
    }
}

fn u<T: TryFrom<i32>>(x: i32, what: &str) -> Result<T, String> {
    T::try_from(x).map_err(|_| format!("site: {what} {x} out of range"))
}

pub fn site_of(s: &SiteRow) -> Result<Site, String> {
    let params: Option<Value> = s
        .patch
        .params
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| format!("site patch params: {e}"))?;
    let has_level = params.as_ref().is_some_and(|p| p.get("level").is_some());
    let patch = TilingSpec {
        family: s.patch.family.clone(),
        params,
        edges: None,
        size: (!has_level).then_some(SizeSpec::Level(u(s.patch.level, "level")?)),
        boundary: Boundary::Dead,
    };
    Ok(Site {
        patch,
        anchor: u(s.anchor, "anchor")?,
        anchor_shape: u(s.anchor_shape, "anchor shape")?,
        anchor_point: s.anchor_point,
        radius: u(s.radius, "radius")?,
        tiles: s
            .tiles
            .iter()
            .map(|t| {
                Ok(SiteTile {
                    shape: u(t.shape, "shape")?,
                    dist: u(t.dist, "dist")?,
                    point: t.point,
                    corners: t.corners.clone(),
                })
            })
            .collect::<Result<_, String>>()?,
        graph: s
            .graph
            .as_ref()
            .map(|g| -> Result<Box<SiteGraph>, String> {
                Ok(Box::new(SiteGraph {
                    rule: g.rule.clone(),
                    degrees: g.degrees,
                    chirality: g.chirality,
                    cells: g
                        .cells
                        .iter()
                        .map(|c| {
                            Ok(GraphCell {
                                shape: u(c.shape, "shape")?,
                                flip: c.flip,
                                degree: u(c.degree, "degree")?,
                                states: c
                                    .states
                                    .iter()
                                    .map(|&x| u(x, "state"))
                                    .collect::<Result<_, String>>()?,
                                nbrs: c
                                    .neighbours
                                    .iter()
                                    .map(|&x| u(x, "neighbour"))
                                    .collect::<Result<_, String>>()?,
                            })
                        })
                        .collect::<Result<_, String>>()?,
                    around: g
                        .around
                        .iter()
                        .map(|a| {
                            Ok((
                                a.tiles
                                    .iter()
                                    .map(|&x| u(x, "tile"))
                                    .collect::<Result<_, String>>()?,
                                u(a.count, "count")?,
                            ))
                        })
                        .collect::<Result<_, String>>()?,
                }))
            })
            .transpose()?,
        nb: s.nb.as_deref().map(parse_nb).transpose()?,
    })
}

/// A record's `patterns` row (its identity checked against its id).
pub fn pattern_row(rec: &PatternRecord) -> Result<PatternRow, String> {
    let identity = identity_text(rec)?;
    let d = identity_d(&identity);
    let exact = matches!(
        rec.class,
        PatternClass::Puffer | PatternClass::Mover | PatternClass::Gun | PatternClass::Other
    );
    let i32of = |x: usize, what: &str| {
        i32::try_from(x).map_err(|_| format!("pattern {}: {what} {x} too large", rec.id))
    };
    Ok(PatternRow {
        id: rec.id.clone(),
        pin_key: jetcarp_core::pin::pin_key(pin_spec(rec)),
        pin: rec.geometry_pin.clone(),
        model: range::cell_model(&Rule::parse(&rec.rule)?.0).to_string(),
        neighbourhood: nb_name(rec.neighborhood).into(),
        steps_per_gen: rec.steps_per_gen as i32,
        class: rec.class.key().into(),
        period: i64::try_from(rec.period).map_err(|_| "period too large")?,
        dx: d[0],
        dy: d[1],
        speed: rec.speed.clone(),
        cells: i32of(rec.cells, "cells")?,
        min_cells: i32of(rec.min_cells, "cells")?,
        max_cells: i32of(rec.max_cells, "cells")?,
        phases: rec
            .phases
            .iter()
            .map(|p| cells_row(p))
            .collect::<Result<_, _>>()?,
        canonical: rec.canonical.clone(),
        rule: exact.then(|| {
            Rule::parse(&rec.rule)
                .map(|r| r.0.notation())
                .unwrap_or_default()
        }),
        site: rec.site.as_ref().map(site_row),
        gun: rec.gun.as_ref().map(jcs),
        family: jetcarp_core::semantics::family(&Rule::parse(&rec.rule)?.0).into(),
        semantics: if rec.semantics.is_empty() {
            jetcarp_core::semantics::FIRST.into()
        } else {
            rec.semantics.clone()
        },
        class_group: rec.class.key().into(),
        tiling_group: tiling_group(&rec.tiling),
        identity,
    })
}

/// The tiling an identity's `tiling=` line names.
pub fn tiling_of_key(key: &str) -> Result<TilingSpec, String> {
    let mut t: TilingSpec =
        serde_json::from_str(key).map_err(|e| format!("tiling key '{key}': {e}"))?;
    t.size = None;
    t.boundary = Boundary::Dead;
    Ok(t)
}

/// Fields of a verification row as a library verification.
pub fn verification_of(v: &VerificationRow) -> Verification {
    Verification {
        periods: v.periods.max(0) as u64,
        generations: v.generations.max(0) as u64,
        field: v.field.clone(),
        sites: v.sites.unwrap_or(0).max(0) as u32,
        jetcarp_version: v.engine_version.clone(),
        date: v.date.clone(),
    }
}

/// A rule as JETCARP's library writes it for a neighbourhood: the notation, with `V` on B/S and
/// Generations rules (and JACA's edge form) for edge neighbours, as a CA reports its rule. The
/// pack's `rules` table writes the neighbourhood apart.
pub fn library_spelling(notation: &str, nb: Nb) -> Result<String, String> {
    let rule = Rule::parse(notation)?.0;
    let n = rule.notation();
    Ok(match &rule {
        Rule::LifeLike { .. } | Rule::Generations { .. } if nb == Nb::Edge => format!("{n}V"),
        Rule::Jaca(j) if nb == Nb::Edge => j.notation_with(true),
        _ => n,
    })
}

/// A pack's pattern as a library record (curation and counts are filled in by the importer).
pub fn record_of(
    p: &PatternRow,
    main: Option<&VerificationRow>,
    provenance: Option<Provenance>,
) -> Result<PatternRecord, String> {
    let tiling = match &p.site {
        Some(_) => tiling_of_key(&p.pin_key)?,
        None => tiling_of_key(
            p.identity
                .lines()
                .find_map(|l| l.strip_prefix("tiling="))
                .ok_or("identity without a tiling= line")?,
        )?,
    };
    let class = PatternClass::parse(&p.class)?;
    let neighborhood = parse_nb(&p.neighbourhood)?;
    let site = p.site.as_ref().map(site_of).transpose()?;
    let rule = match (&p.rule, main) {
        (Some(r), _) if !r.is_empty() => library_spelling(r, neighborhood)?,
        (_, Some(v)) => library_spelling(&v.rule, neighborhood)?,
        _ => return Err(format!("pattern {}: no rule to run it in", p.id)),
    };
    let phases: Vec<String> = p
        .phases
        .iter()
        .map(|c| phase_text(c))
        .collect::<Result<_, _>>()?;
    let moving = matches!(class, PatternClass::Spaceship | PatternClass::Puffer);
    let displacement = (moving || (p.dx, p.dy) != (0, 0)).then_some([p.dx, p.dy]);
    let p0 = decode_cells(phases.first().ok_or("a pattern with no phases")?)?;
    let span = |f: fn(&LCell) -> i32| {
        p0.iter().map(f).max().unwrap_or(0) - p0.iter().map(f).min().unwrap_or(0)
    };
    let bbox = [0, 0, span(|c| c.i), span(|c| c.j)];
    let verified = main.map(verification_of).unwrap_or_default();
    let mover: Option<MoverRecord> = main
        .and_then(|v| v.mover.as_deref())
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| format!("pattern {}: mover: {e}", p.id))?;
    let range: Option<RuleRange> = main
        .and_then(|v| v.range.as_deref())
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| format!("pattern {}: range: {e}", p.id))?;
    let gun: Option<GunInfo> = p
        .gun
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| format!("pattern {}: gun: {e}", p.id))?;
    let mut rec = PatternRecord {
        format: pattern::RECORD_FORMAT,
        id: p.id.clone(),
        name: None,
        rule,
        rules: vec![],
        range,
        neighborhood,
        steps_per_gen: u32::try_from(p.steps_per_gen).map_err(|_| "steps per generation")?,
        tiling_name: pattern::tiling_display_name(&tiling),
        tile_types: 0,
        tiling,
        class,
        period: u64::try_from(p.period).map_err(|_| "a negative period")?,
        displacement,
        displacement_xy: None,
        speed: p.speed.clone(),
        cells: p.cells.max(0) as usize,
        min_cells: p.min_cells.max(0) as usize,
        max_cells: p.max_cells.max(0) as usize,
        bbox,
        canonical: p.canonical.clone(),
        phases,
        site,
        verified,
        mover,
        gun,
        seen: 0,
        provenance: provenance.unwrap_or_default(),
        discovered_by: DiscoveredBy::Search,
        tags: vec![],
        notes: String::new(),
        score: 0.0,
        rating: None,
        state: CurationState::New,
        created: String::new(),
        updated: String::new(),
        geometry_pin: p.pin.clone(),
        engine_version: main.map(|v| v.engine_version.clone()).unwrap_or_default(),
        semantics: main.map(|v| v.semantics.clone()).unwrap_or_default(),
        needs_reverify: false,
    };
    if rec.site.is_none()
        && let Ok(info) = jetcarp_core::pattern::verify::lattice_info(&rec.tiling, rec.neighborhood)
    {
        rec.tile_types = info.ntypes;
        if moving {
            let d = [p.dx as f64, p.dy as f64];
            let xy = [
                d[0] * info.a1[0] + d[1] * info.a2[0],
                d[0] * info.a1[1] + d[1] * info.a2[1],
            ];
            rec.displacement_xy = Some([(xy[0] * 1e6).round() / 1e6, (xy[1] * 1e6).round() / 1e6]);
        }
    }
    Ok(rec)
}

// ---------------------------------------------------------------------------------------------
// Tilings

/// A tiling's `tilings` row, and its reference patch's rows for an aperiodic family.
pub fn tiling_row(pin_key: &str) -> Result<(TilingRow, Option<Vec<RefTileRow>>), String> {
    let spec = tiling_of_key(pin_key)?;
    let pin = jetcarp_core::pin::geometry_pin(&spec)?;
    let mut row = TilingRow {
        pin_key: pin_key.to_string(),
        pin: pin.pin,
        polygon_hash: pin.polygon_hash,
        neighbour_hash: pin.neighbour_hash,
        name: pattern::tiling_display_name(&spec),
        kind: "lattice".into(),
        family: spec.family.clone(),
        params: spec.params.as_ref().map(jcs),
        edges: spec.edges.as_ref().map(jcs),
        tiling_group: tiling_group(&spec),
        ..Default::default()
    };
    let mut build = jetcarp_core::pin::pin_spec(&spec);
    if jetcarp_core::pin::is_aperiodic(&spec) {
        let topo = Topology::build(&build)?;
        row.kind = "aperiodic".into();
        let level = jetcarp_core::pin::reference_level(&spec.family);
        row.reference_level = Some(level as i32);
        row.reference_tiles = Some(topo.len() as i32);
        let mut tiles = Vec::with_capacity(topo.len());
        for x in 0..topo.len() {
            let mut edge = vec![];
            topo.for_each_neighbor(x, Nb::Edge, |y| edge.push(y as i32));
            let mut touch = vec![];
            topo.for_each_neighbor(x, Nb::Touch, |y| touch.push(y as i32));
            tiles.push(RefTileRow {
                tile: x as i32,
                shape: topo.shape_of(x) as i32,
                corners: topo.polygon(x),
                edge,
                touch,
                open: topo.is_open(x),
            });
        }
        return Ok((row, Some(tiles)));
    }
    build.size = Some(SizeSpec::Rect { w: 8, h: 8 });
    build.boundary = Boundary::Dead;
    let topo = Topology::build(&build)?;
    let Storage::Lattice(g) = &topo.storage else {
        return Err(format!("{pin_key}: not a lattice"));
    };
    row.a1 = Some(g.a1);
    row.a2 = Some(g.a2);
    row.tile_types = Some(
        g.types
            .iter()
            .map(|(m, s)| TileType {
                shape: *s as i32,
                corners: topo.shapes[*s].iter().map(|&p| m.apply(p)).collect(),
            })
            .collect(),
    );
    let offs = |nb: Nb| {
        g.offsets[nb as usize]
            .iter()
            .map(|slots| {
                slots
                    .iter()
                    .map(|o| OffsetRow {
                        di: o.di,
                        dj: o.dj,
                        k: o.k as i32,
                    })
                    .collect()
            })
            .collect()
    };
    row.edge = Some(offs(Nb::Edge));
    row.touch = Some(offs(Nb::Touch));
    Ok((row, None))
}

fn sha_hex(b: &[u8]) -> String {
    crate::hash::hex(b)
}

/// The pin a `tilings` row's columns give (lattice), or that a reference patch's rows give:
/// (pin, polygon hash, neighbour hash), as `jetcarp-geometry-v1` defines them.
pub fn pin_of_rows(
    row: &TilingRow,
    patch: Option<&[RefTileRow]>,
) -> Result<(String, String, String), String> {
    let mut ph: Vec<u8> = b"jetcarp-geometry-v1 polygons\n".to_vec();
    let mut nh: Vec<u8> = b"jetcarp-geometry-v1 neighbours\n".to_vec();
    let f = |v: &mut Vec<u8>, x: f64| v.extend_from_slice(&x.to_bits().to_le_bytes());
    let u = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
    match patch {
        None => {
            let (a1, a2) = (row.a1.ok_or("no a1")?, row.a2.ok_or("no a2")?);
            let types = row.tile_types.as_ref().ok_or("no tile_types")?;
            ph.extend_from_slice(b"lattice");
            for x in [a1[0], a1[1], a2[0], a2[1]] {
                f(&mut ph, x);
            }
            u(&mut ph, types.len() as u32);
            for t in types {
                u(&mut ph, t.shape as u32);
                u(&mut ph, t.corners.len() as u32);
                for c in &t.corners {
                    f(&mut ph, c[0]);
                    f(&mut ph, c[1]);
                }
            }
            for offs in [row.edge.as_ref(), row.touch.as_ref()] {
                for slots in offs.ok_or("no neighbour offsets")? {
                    u(&mut nh, slots.len() as u32);
                    for o in slots {
                        nh.extend_from_slice(&o.di.to_le_bytes());
                        nh.extend_from_slice(&o.dj.to_le_bytes());
                        u(&mut nh, o.k as u32);
                    }
                }
            }
        }
        Some(tiles) => {
            ph.extend_from_slice(b"patch");
            u(&mut ph, tiles.len() as u32);
            for t in tiles {
                u(&mut ph, t.shape as u32);
                u(&mut ph, t.corners.len() as u32);
                for c in &t.corners {
                    f(&mut ph, c[0]);
                    f(&mut ph, c[1]);
                }
            }
            for edge in [true, false] {
                for t in tiles {
                    let l = if edge { &t.edge } else { &t.touch };
                    u(&mut nh, l.len() as u32);
                    for &y in l {
                        u(&mut nh, y as u32);
                    }
                }
            }
            for t in tiles {
                nh.push(t.open as u8);
            }
        }
    }
    let (p, n) = (sha_hex(&ph), sha_hex(&nh));
    let pin = sha_hex(format!("jetcarp-geometry-v1\n{p}\n{n}").as_bytes())[..32].to_string();
    Ok((pin, p, n))
}

// ---------------------------------------------------------------------------------------------
// Rules, ants, Quick Start

/// A rule's row: family and semantics version from the engine's table, B/S masks.
pub fn rule_row(notation: &str, nb: &str, steps: u32) -> Result<RuleRow, String> {
    let rule = Rule::parse(notation)?.0;
    let family = jetcarp_core::semantics::family(&rule);
    let states = match rule.field_kind() {
        jetcarp_core::rules::FieldKind::Binary => 2,
        jetcarp_core::rules::FieldKind::Multi(n) => n as i32,
        _ => 0,
    };
    let bs = range::bs_parts(&rule);
    Ok(RuleRow {
        notation: rule.notation(),
        neighbourhood: nb.to_string(),
        steps_per_gen: steps as i32,
        family: family.into(),
        semantics: jetcarp_core::semantics::rule_version(&rule).into(),
        states,
        birth: bs.map(|b| b.0 as i64),
        survive: bs.map(|b| b.1 as i64),
    })
}

/// An ant run's row (its id checked against its identity).
pub fn ant_row(a: &jetcarp_store::AntRow, pin_key: &str) -> Result<AntPackRow, String> {
    let identity = jetcarp_store::library::ant_identity(&a.rule, &a.tiling, &a.start);
    if format!("ant-{}", content_id(&identity, &a.geometry_pin)) != a.id {
        return Err(format!(
            "ant {}: its identity does not hash to its id",
            a.id
        ));
    }
    let (family, semantics) = jetcarp_core::semantics::of_notation(&a.rule).unwrap_or(("ant", "1"));
    Ok(AntPackRow {
        id: a.id.clone(),
        identity,
        pin_key: pin_key.to_string(),
        pin: a.geometry_pin.clone(),
        rule: a.rule.clone(),
        kind: a.kind.clone(),
        family: family.into(),
        semantics: semantics.into(),
        states: a.states as i32,
        colors: a.colors as i32,
        start: serde_json::to_string(&a.start).unwrap_or_default(),
        outcome: a.outcome.name().into(),
        period: a.period.map(|x| x as i64),
        dx: a.d.map(|d| d[0]),
        dy: a.d.map(|d| d[1]),
        onset: a.onset.map(|x| x as i64),
        speed: a.speed,
        steps: a.steps as i64,
        cells: a.cells as i64,
        run: jcs(&serde_json::json!({"run": a.run, "limits": a.limits})),
        tiling_group: tiling_group(&a.tiling),
    })
}

/// A pack's ant run as a library row. An error names the ant and the column or `run` member
/// that is wrong (`ant ant-...: run.limits: missing field max_period`).
pub fn ant_of(r: &AntPackRow) -> Result<jetcarp_store::AntRow, String> {
    let bad = |what: &str, e: &dyn std::fmt::Display| format!("ant {}: {what}: {e}", r.id);
    let tiling = tiling_of_key(
        r.identity
            .lines()
            .find_map(|l| l.strip_prefix("tiling="))
            .ok_or_else(|| bad("identity", &"no tiling= line"))?,
    )
    .map_err(|e| bad("identity's tiling", &e))?;
    let start = serde_json::from_str(&r.start).map_err(|e| bad("start", &e))?;
    let body: Value = serde_json::from_str(&r.run).map_err(|e| bad("run", &e))?;
    let member = |k: &str| match body.get(k) {
        Some(v) => Ok(v.clone()),
        None => Err(bad("run", &format!("no member {k}"))),
    };
    let run = serde_json::from_value(member("run")?).map_err(|e| bad("run.run", &e))?;
    let limits = serde_json::from_value(member("limits")?).map_err(|e| bad("run.limits", &e))?;
    let outcome =
        serde_json::from_value(Value::String(r.outcome.clone())).map_err(|e| bad("outcome", &e))?;
    Ok(jetcarp_store::AntRow {
        id: r.id.clone(),
        rule: r.rule.clone(),
        kind: r.kind.clone(),
        states: u8::try_from(r.states).map_err(|e| bad("states", &e))?,
        colors: u8::try_from(r.colors).map_err(|e| bad("colors", &e))?,
        tiling_name: pattern::tiling_display_name(&tiling),
        tiling,
        start,
        outcome,
        period: r.period.map(|x| x.max(0) as u64),
        d: r.dx.zip(r.dy).map(|(a, b)| [a, b]),
        speed: r.speed,
        onset: r.onset.map(|x| x.max(0) as u64),
        steps: r.steps.max(0) as u64,
        cells: r.cells.max(0) as u64,
        run,
        limits,
        score: 0.0,
        rating: None,
        state: "new".into(),
        name: None,
        notes: String::new(),
        job: None,
        created: String::new(),
        updated: String::new(),
        geometry_pin: r.pin.clone(),
        engine_version: String::new(),
        needs_reverify: false,
    })
}

/// Does a scene name only palettes a reader can resolve without the publisher's settings (a
/// built-in name, colours written out, or a palette the scene itself holds)?
pub fn self_contained(scene: &Scene) -> Result<(), String> {
    use jetcarp_core::render::Palette;
    for ca in &scene.cas {
        for p in [&ca.palette, &ca.palette_neg].into_iter().flatten() {
            let ok = Palette::builtin(p).is_some()
                || Palette::from_hex(p).is_some()
                || scene.palettes.iter().any(|x| &x.name == p);
            if !ok {
                return Err(format!(
                    "the scene names palette '{p}', which is neither built in nor in the scene"
                ));
            }
        }
    }
    Ok(())
}

/// A Quick Start entry's row.
pub fn quickstart_row(
    q: &jetcarp_store::library::pack::PackQuickstart,
) -> Result<QuickstartPackRow, String> {
    self_contained(&q.row.scene).map_err(|e| format!("Quick Start {}: {e}", q.row.id))?;
    let (family, semantics) =
        jetcarp_core::semantics::of_notation(&q.row.rule).unwrap_or(("", "1"));
    let (id, scene_text) = quickstart_pack_id(q)?;
    Ok(QuickstartPackRow {
        id,
        title: q.row.title.clone(),
        description: q.row.description.clone(),
        source: match (q.row.source_kind.as_str(), &q.row.source_id) {
            ("pattern", Some(id)) => Some(format!("pattern:{id}")),
            ("ant", Some(id)) => Some(format!("ant:{id}")),
            _ => None,
        },
        scene: scene_text,
        scene_format: q.row.scene.format.to_string(),
        rule: q.row.rule.clone(),
        rule_family: family.into(),
        semantics: semantics.into(),
        behaviour: (!q.row.behaviour.is_empty()).then(|| q.row.behaviour.clone()),
        pin_key: q.pin_key.clone(),
        pin: q.pin.clone(),
        tiling_group: tiling_group(&q.row.scene.tiling),
    })
}

/// A Quick Start entry's id in a pack: `qs-` and the first 32 hex digits of SHA-256 over its
/// scene's text (the `scene` column), a newline and its pin, as a pattern's id is made.
pub fn quickstart_id(scene: &str, pin: &str) -> String {
    format!("qs-{}", content_id(scene, pin))
}

/// A library Quick Start entry's pack id, and its `scene` column's text. The library keys its
/// entries by name (a pattern's id, a scene's file stem, a sweep's id); a pack only by this id,
/// so its provenance, media and ratings name `quickstart:<this id>`.
pub fn quickstart_pack_id(
    q: &jetcarp_store::library::pack::PackQuickstart,
) -> Result<(String, String), String> {
    if q.pin.is_empty() {
        return Err(format!("Quick Start {}: no geometry pin", q.row.id));
    }
    let scene = serde_json::to_value(&q.row.scene).map_err(|e| e.to_string())?;
    let scene_text = jetcarp_core::jcs::to_string(&crate::paths::strip_json(scene));
    Ok((quickstart_id(&scene_text, &q.pin), scene_text))
}

/// A pack's Quick Start entry as a library row (approved: a pack is chosen by its reader).
pub fn quickstart_of(r: &QuickstartPackRow) -> Result<jetcarp_store::QuickStartRow, String> {
    let scene = Scene::from_json(&r.scene).map_err(|e| format!("Quick Start {}: {e}", r.id))?;
    let mut q = jetcarp_store::QuickStartRow::for_scene(&r.id, "scene", scene);
    if let Some(src) = &r.source
        && let Some((k, id)) = src.split_once(':')
    {
        q.source_kind = if k == "pattern" {
            "pattern".into()
        } else {
            "scene".into()
        };
        q.source_id = Some(id.to_string());
    }
    q.title = r.title.clone();
    q.description = r.description.clone();
    q.rule = r.rule.clone();
    q.behaviour = r.behaviour.clone().unwrap_or_default();
    Ok(q)
}
