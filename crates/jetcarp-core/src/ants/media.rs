//! Media for ant runs, chosen by what the run did (see [`crate::media`] for files and storage):
//!
//! - **Build-up** (mode `seed`, every outcome): the run from step 0 on a dead-edged grid that
//!   just holds every cell the ant reaches (a scene, so the app replays it too), about 150
//!   frames. A highway runs to its onset plus four periods, so the highway shows leaving the
//!   tangle; other runs show their whole length (at most [`BUILD_MAX_STEPS`]).
//! - **Highway loop** (mode `alone`, highways only, the automatic choice for them): one period of
//!   the steady highway in a window that moves with the ant at the highway's speed, a frame per
//!   step (or per a divisor of the period, at most 240 frames). The loop starts enough periods
//!   after the onset that the window sees only highway, so the frame after the last is the first
//!   pixel for pixel (checked; `seamless` says so).

use super::plane::{Ant, Compiled, Field, Lattice, step};
use super::run::{Outcome, Start};
use super::search::parse_rule;
use crate::library::AntRow;
use crate::media::{
    Animation, EMPTY_BOX, MediaMode, MediaOptions, apply_style, bbox_union, draw, fit_scale, grow,
    pixels,
};
use crate::topology::{Boundary, SizeSpec, Topology};
use crate::world::{Agent, CaSpec, Command, Scene, SeedSpec, World};

/// The longest build-up shown.
pub const BUILD_MAX_STEPS: u64 = 2_000_000;
/// Frames in a build-up.
pub const BUILD_FRAMES: u64 = 150;

/// The engine run to `steps`: its field, the ant, and the box of unit cells reached.
fn replay(row: &AntRow, steps: u64) -> Result<(Lattice, Field, Ant, [i32; 4]), String> {
    let lat = Lattice::new(&row.tiling)?;
    let m = parse_rule(&row.rule)?;
    let comp = Compiled::new(&m, &lat);
    let mut f = row.start.field(&lat, m.colors);
    let mut b = [0i32; 4];
    for (i, j, _, _) in f.nonzero() {
        b = [b[0].min(i), b[1].min(j), b[2].max(i), b[3].max(j)];
    }
    let mut a = Ant::default();
    for _ in 0..steps {
        step(&lat, &comp, &mut f, &mut a);
        b = [b[0].min(a.i), b[1].min(a.j), b[2].max(a.i), b[3].max(a.j)];
    }
    Ok((lat, f, a, b))
}

/// A dead-edged grid holding the unit cells `b` (relative to an origin) with `m` cells of
/// margin: the tiling spec and the unsheared lattice coordinates of the origin in it.
fn grid_for(
    row: &AntRow,
    b: [i32; 4],
    m: i32,
) -> Result<(crate::topology::TilingSpec, i32, i32), String> {
    let h = (b[3] - b[1] + 1 + 2 * m) as u32;
    let mut w = (b[2] - b[0] + 1 + 2 * m) as u32 + h;
    for _ in 0..4 {
        let mut spec = crate::pattern::tiling_identity(&row.tiling);
        spec.size = Some(SizeSpec::Rect { w, h });
        spec.boundary = Boundary::Dead;
        let topo = Topology::build(&spec)?;
        let g = topo.lattice().ok_or("not a lattice")?;
        let j0 = m - b[1];
        let rows = (b[1]..=b[3]).map(|j| g.row_start[(j + j0) as usize]);
        let (lo, hi) = rows.fold((i32::MAX, i32::MIN), |(lo, hi), r| (lo.min(r), hi.max(r)));
        let i0 = hi - b[0] + m;
        let need = (i0 + b[2] + m + 1 - lo) as u32;
        if need <= w {
            // Trim the width to what is needed (row starts do not depend on it).
            spec.size = Some(SizeSpec::Rect { w: need, h });
            return Ok((spec, i0, j0));
        }
        w = need;
    }
    Err("could not fit a grid around the run".into())
}

fn index_in(topo: &Topology, i: i32, j: i32, k: u8) -> Result<usize, String> {
    topo.lattice()
        .and_then(|g| g.resolve(i, j, k as u32))
        .ok_or_else(|| format!("cell ({i}, {j}, {k}) is outside the media grid (internal error)"))
}

/// Steps a build-up shows: the onset plus four periods for a highway (or orbit), else the run.
pub fn build_steps(row: &AntRow) -> u64 {
    match (row.outcome, row.onset, row.period) {
        (Outcome::Highway | Outcome::Periodic, Some(o), Some(p)) => o + 4 * p.max(1),
        _ => row.steps,
    }
    .clamp(1, BUILD_MAX_STEPS)
}

/// The palette ant scenes open in.
pub const SCENE_PALETTE: &str = "Prism";

/// The scene that replays a run from step 0: a grid holding every cell reached in `steps`
/// steps, the ant at the start facing slot 0, the soup (if any) as cells, and `steps_per_gen`
/// ant steps per generation, in [`SCENE_PALETTE`] ([`render::recolor_scene`] gives it another).
///
/// [`render::recolor_scene`]: crate::render::recolor_scene
pub fn scene(row: &AntRow, steps: u64, steps_per_gen: u32) -> Result<Scene, String> {
    let (_, _, _, b) = replay(row, steps)?;
    let (spec, i0, j0) = grid_for(row, b, 3)?;
    let topo = Topology::build(&spec)?;
    let start = index_in(&topo, i0, j0, 0)?;
    let mut sc = Scene::new(spec);
    sc.name = format!("{} on {}", row.rule, row.tiling_name);
    sc.description = format!(
        "{} (catalog {}), from step 0, {} ant steps per generation.",
        row.line(),
        row.id,
        steps_per_gen
    );
    let mut ca = CaSpec::new(&row.rule)
        .steps(steps_per_gen.max(1))
        .seed(SeedSpec::AntAt {
            cell: start as u32,
            heading: 0,
        });
    ca.id = Some(1);
    ca.palette = Some(SCENE_PALETTE.into());
    sc.cas.push(ca);
    if row.start != Start::Empty {
        let lat = Lattice::new(&row.tiling)?;
        let m = parse_rule(&row.rule)?;
        let cells: Vec<[u32; 2]> = row
            .start
            .field(&lat, m.colors)
            .nonzero()
            .into_iter()
            .map(|(i, j, k, c)| index_in(&topo, i0 + i, j0 + j, k).map(|x| [x as u32, c as u32]))
            .collect::<Result<_, _>>()?;
        sc.commands.push(Command::SetCells { id: 1, cells });
    }
    Ok(sc)
}

/// The build-up scene with the default frame spacing.
pub fn build_scene(row: &AntRow) -> Result<Scene, String> {
    let steps = build_steps(row);
    let per = steps.div_ceil(BUILD_FRAMES).max(1);
    scene(row, steps, per.min(u32::MAX as u64) as u32)
}

/// The build-up scene in a built-in palette (`None`: [`SCENE_PALETTE`]); an unknown name is an
/// error that lists the palettes.
pub fn build_scene_in(row: &AntRow, palette: Option<&str>) -> Result<Scene, String> {
    let mut sc = build_scene(row)?;
    if let Some(p) = palette {
        crate::render::builtin_palette(p)?;
        crate::render::recolor_scene(&mut sc, p);
    }
    Ok(sc)
}

/// Generations of the build-up scene (frames minus one).
pub fn build_gens(row: &AntRow) -> u64 {
    let steps = build_steps(row);
    let per = steps.div_ceil(BUILD_FRAMES).max(1);
    steps.div_ceil(per)
}

fn smallest_stride(p: u64, max_frames: u64) -> u64 {
    (1..=p)
        .find(|e| p.is_multiple_of(*e) && p / e <= max_frames)
        .unwrap_or(p)
}

/// One period of the steady highway in a window moving with the ant (module docs).
pub fn highway_animation(row: &AntRow, o: &MediaOptions) -> Result<Animation, String> {
    let (Outcome::Highway, Some(p), Some(d), Some(onset)) =
        (row.outcome, row.period, row.d, row.onset)
    else {
        return Err(format!(
            "{} is {}, not a highway: only highways have a moving loop (use mode seed)",
            row.id,
            row.outcome.name()
        ));
    };
    let st = &o.style;
    let stride = smallest_stride(p, 240);
    // Periods behind the ant shown in the window, and how far past the onset the loop starts.
    let behind = 3u64;
    let mut lead = behind + 4;
    for _ in 0..6 {
        let t0 = onset + lead * p;
        // The trail: cells written in the last `behind` periods before t0 and the next one,
        // relative to the ant at t0.
        let (lat, mut f, mut a, _) = replay(row, t0 - behind * p)?;
        let comp = Compiled::new(&parse_rule(&row.rule)?, &lat);
        let mut abs: Vec<(i32, i32, u8)> = Vec::new();
        for _ in 0..behind * p {
            abs.push((a.i, a.j, a.k));
            step(&lat, &comp, &mut f, &mut a);
        }
        let a0 = a;
        {
            let (mut f2, mut a2) = (f.clone(), a);
            for _ in 0..p {
                abs.push((a2.i, a2.j, a2.k));
                step(&lat, &comp, &mut f2, &mut a2);
            }
        }
        let mut rel: Vec<(i32, i32, u8)> = abs
            .into_iter()
            .map(|(i, j, k)| (i - a0.i, j - a0.j, k))
            .collect();
        rel.sort();
        rel.dedup();
        let (mut bi, mut bj) = ((i32::MAX, i32::MIN), (i32::MAX, i32::MIN));
        for &(i, j, _) in &rel {
            bi = (bi.0.min(i), bi.1.max(i));
            bj = (bj.0.min(j), bj.1.max(j));
        }
        let bx = [
            bi.0.min(bi.0 + d[0]),
            bj.0.min(bj.0 + d[1]),
            bi.1.max(bi.1 + d[0]),
            bj.1.max(bj.1 + d[1]),
        ];
        let (spec, ci, cj) = grid_for(row, bx, 2)?;
        let mut canvas = World::new(spec, 1)?;
        canvas.apply(Command::AddCa {
            ca: CaSpec::new(&row.rule).seed(SeedSpec::Empty),
        })?;
        apply_style(&mut canvas, st);
        let topo = canvas.topo.clone();
        let cid = canvas.cas[0].id;
        // Window: the trail's tiles at t0, with the style's margin.
        let mut win = EMPTY_BOX;
        for &(i, j, k) in &rel {
            let x = index_in(&topo, ci + i, cj + j, k)?;
            bbox_union(&mut win, &topo.polygon(x), [0.0, 0.0]);
        }
        let cs = crate::world::cell_size(&topo);
        let win = grow(win, st.margin_cells as f64 * cs);
        let (s, px) = fit_scale(
            win[2] - win[0],
            win[3] - win[1],
            st.alone_side,
            cs,
            st.min_cell_px,
            st.max_cell_px,
        );
        let (pw, ph) = pixels(win, s);
        let g = topo.lattice().unwrap().clone();
        let (i_lo, j_lo) = (-ci, -cj);
        let (i_hi, j_hi) = (i_lo + g.w as i32 + g.h as i32, j_lo + g.h as i32);
        // Frame t shows the cells moved back by the whole lattice steps of t·d/p, and the
        // window moved by the fractional rest, so frame p is frame 0 bit for bit.
        let mut draw_now = |f: &Field, a: &Ant, t: u64| -> Result<crate::render::Image, String> {
            let num = [d[0] as i64 * t as i64, d[1] as i64 * t as i64];
            let l = [num[0].div_euclid(p as i64), num[1].div_euclid(p as i64)];
            let r = [
                (num[0] - l[0] * p as i64) as f64 / p as f64,
                (num[1] - l[1] * p as i64) as f64 / p as f64,
            ];
            let (li, lj) = (l[0] as i32, l[1] as i32);
            let mut cells: Vec<[u32; 2]> = Vec::new();
            for j in j_lo..j_hi {
                for i in i_lo..i_hi {
                    for k in 0..lat.ntypes as u8 {
                        let c = f.get(a0.i + i + li, a0.j + j + lj, k);
                        if c != 0
                            && let Some(x) = g.resolve(ci + i, cj + j, k as u32)
                        {
                            cells.push([x as u32, c as u32]);
                        }
                    }
                }
            }
            canvas.apply(Command::Clear { id: cid })?;
            canvas.apply(Command::SetCells { id: cid, cells })?;
            canvas.cas[0].agents = g
                .resolve(ci + a.i - a0.i - li, cj + a.j - a0.j - lj, a.k as u32)
                .map(|x| {
                    vec![Agent {
                        cell: x as u32,
                        heading: a.h as u16,
                        state: a.q,
                    }]
                })
                .unwrap_or_default();
            let off = lat.world_f(r);
            Ok(draw(
                &canvas,
                st,
                [win[0] + off[0], win[1] + off[1]],
                s,
                pw,
                ph,
            ))
        };
        let mut frames = Vec::new();
        let mut gens = Vec::new();
        for t in 0..p {
            if t % stride == 0 {
                frames.push(draw_now(&f, &a, t)?);
                gens.push(t0 + t);
            }
            step(&lat, &comp, &mut f, &mut a);
        }
        // The frame after the last (the field moved on by d, the window by d) is the first.
        let after = draw_now(&f, &a, p)?;
        let seamless = after.rgba == frames[0].rgba;
        if seamless || lead >= 64 * (behind + 4) {
            return Ok(Animation {
                frames,
                generations: gens,
                delay_ms: st.seed_frame_ms,
                mode: MediaMode::Alone,
                poster: 0,
                scale: s,
                cell_px: px,
                window: win,
                seamless,
                window_kind: "moving",
            });
        }
        lead *= 2;
    }
    Err("no seamless highway loop found".into())
}
