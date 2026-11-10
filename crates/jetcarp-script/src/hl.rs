//! HashLife bindings: `w.jump(gens)` (in place, wrap-edge power-of-two tori) and
//! `hashlife(w, gens)` / `hashlife(w, gens, #{...})` (unbounded plane, exported to a new world).

use crate::Ctx;
use crate::sim::Shared;
use jetcarp_core::hashlife::{self, Options, Plane};
use jetcarp_core::*;
use rhai::{Dynamic, Engine, EvalAltResult, INT, Map};
use std::sync::Arc;

type R<T> = Result<T, Box<EvalAltResult>>;

fn gens_of(v: &Dynamic) -> R<u64> {
    if let Ok(n) = v.as_int() {
        if n < 0 {
            return Err("generations must be 0 or more".into());
        }
        return Ok(n as u64);
    }
    if let Ok(s) = v.clone().into_string() {
        return hashlife::parse_gens(&s).map_err(|e| e.into());
    }
    Err("generations: a number or text like \"2^40\"".into())
}

fn int(m: &Map, k: &str) -> R<Option<i64>> {
    match m.get(k) {
        None => Ok(None),
        Some(v) => v
            .as_int()
            .map(Some)
            .map_err(|_| format!("hashlife: '{k}' must be a number").into()),
    }
}

pub(crate) fn register(e: &mut Engine, ctx: &Arc<Ctx>) {
    e.register_fn("jump", |s: &mut Shared, gens: Dynamic| -> R<()> {
        let n = gens_of(&gens)?;
        s.lock()
            .world
            .apply(Command::Jump { gens: n })
            .map(|_| ())
            .map_err(|e| e.into())
    });
    e.register_fn("states_hash", |s: &mut Shared| {
        format!("{:016x}", s.lock().world.states_hash())
    });
    e.register_fn("can_jump", |s: &mut Shared| -> Dynamic {
        match hashlife::torus_problem(&s.lock().world) {
            None => Dynamic::TRUE,
            Some(why) => why.into(),
        }
    });
    let c = ctx.clone();
    e.register_fn("hashlife", move |s: Shared, gens: Dynamic| {
        plane(&c, &s, gens, &Map::new())
    });
    let c = ctx.clone();
    e.register_fn("hashlife", move |s: Shared, gens: Dynamic, m: Map| {
        plane(&c, &s, gens, &m)
    });
}

/// The world's pattern on the unbounded plane, `gens` later, as a new world. Options: `region:
/// [i0, j0, w, h]` (default: the source world's frame), `boundary` (default: the source's),
/// `node_cap`, `margin` with `region: "fit"` (bounding box plus margin).
fn plane(c: &Arc<Ctx>, s: &Shared, gens: Dynamic, m: &Map) -> R<Shared> {
    for k in m.keys() {
        if !["region", "boundary", "node_cap", "margin"].contains(&k.as_str()) {
            return Err(format!(
                "hashlife: unknown option '{k}' (region, boundary, node_cap, margin)"
            )
            .into());
        }
    }
    let n = gens_of(&gens)?;
    let opts = Options {
        node_cap: int(m, "node_cap")?
            .map(|v| v.max(4096) as usize)
            .unwrap_or(hashlife::DEFAULT_NODE_CAP),
        ..Options::default()
    };
    let g = s.lock();
    let mut p = Plane::from_world(&g.world, &opts)?;
    let screen = g.screen;
    let (frame, bnd) = match g.world.topo.spec.size {
        Some(SizeSpec::Rect { w, h }) => (
            Some((0i64, 0i64, w as u64, h as u64)),
            g.world.topo.spec.boundary,
        ),
        _ => (None, g.world.topo.spec.boundary),
    };
    drop(g);
    p.step(n)?;
    let region = match m.get("region") {
        None => frame,
        Some(v) if v.clone().into_string().is_ok_and(|t| t == "fit") => {
            p.default_region(int(m, "margin")?.unwrap_or(8).max(0) as u64)
        }
        Some(v) => {
            let a = v
                .clone()
                .into_array()
                .map_err(|_| "hashlife: region is [i0, j0, w, h] or \"fit\"")?;
            let x: Vec<INT> = a.iter().filter_map(|d| d.as_int().ok()).collect();
            if x.len() != 4 || x[2] <= 0 || x[3] <= 0 {
                return Err("hashlife: region is [i0, j0, w, h]".into());
            }
            Some((x[0], x[1], x[2] as u64, x[3] as u64))
        }
    }
    .unwrap_or((0, 0, 16, 16));
    let bnd = match m.get("boundary").map(|v| v.to_string()) {
        None => bnd,
        Some(b) => match b.as_str() {
            "dead" => Boundary::Dead,
            "wrap" => Boundary::Wrap,
            "live" => Boundary::Live,
            _ => return Err("hashlife: boundary is dead, wrap, or live".into()),
        },
    };
    let sc = p.to_scene(region, bnd)?;
    let w = World::from_scene(&sc)?;
    c.check_cells(w.topo.len())?;
    Ok(Shared::new(w, screen))
}
