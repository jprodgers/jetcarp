//! Edge model: each edge class stores one shape; partner edges derive theirs by the class's symmetry.
//! Ported from JETCARP's JavaScript prototype, the CART tiling editor (2026) (freshEdges, classOfEdge, nodesForEdge, flattenEdge, tileOutline,
//! selfIntersects, glueMaps).

use crate::geom::*;
use serde::{Deserialize, Serialize};

/// How partner edges in a class are related.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlueKind {
    /// Translate/rotate onto the partner edge.
    Direct,
    /// Glide reflection onto the partner edge.
    Flip,
    /// The edge maps to itself by a half turn about its midpoint.
    SelfHalfTurn,
    /// All edges share one half-turn-symmetric shape (Spectre).
    Shared,
    /// Isohedral types: the shape must be mirror symmetric about the edge's perpendicular
    /// bisector (tactile's U). Partners are placed by `EdgeClass::orient`.
    Mirror,
    /// Isohedral types: the edge must stay straight (tactile's I).
    Straight,
}

/// How an edge's shape is placed relative to its class's stored shape, as tactile's
/// `M_orients`: identity, half turn, flip end to end (t -> 1-t), flip side to side (d -> -d).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orient {
    Id,
    Rot,
    Flip,
    Rofl,
}

impl Orient {
    pub fn apply(self, q: Node) -> Node {
        match self {
            Orient::Id => q,
            Orient::Rot => Node {
                t: 1.0 - q.t,
                d: -q.d,
            },
            Orient::Flip => Node {
                t: 1.0 - q.t,
                d: q.d,
            },
            Orient::Rofl => Node { t: q.t, d: -q.d },
        }
    }
    /// Does the shape run backwards along the edge?
    pub fn reverses(self) -> bool {
        matches!(self, Orient::Rot | Orient::Flip)
    }
}

#[derive(Clone, Debug)]
pub struct EdgeClass {
    pub edges: &'static [usize],
    pub kind: GlueKind,
    /// Per edge, how the class shape is placed. Empty: the classic rule (first edge as stored,
    /// second edge turned for `Direct` and flipped for `Flip`).
    pub orient: &'static [Orient],
}

/// Edge shape modes from the prototype: J free, U mirror, S half-turn, I straight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeMode {
    J,
    U,
    S,
    I,
}

/// A node on an edge: `t` along the chord, `d` perpendicular, both in chord units.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub t: f64,
    pub d: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeShape {
    pub mode: EdgeMode,
    #[serde(default = "yes")]
    pub curved: bool,
    #[serde(default)]
    pub nodes: Vec<Node>,
}
fn yes() -> bool {
    true
}

impl EdgeShape {
    pub fn straight_for(kind: GlueKind) -> EdgeShape {
        let mode = match kind {
            GlueKind::SelfHalfTurn | GlueKind::Shared => EdgeMode::S,
            GlueKind::Mirror => EdgeMode::U,
            GlueKind::Straight => EdgeMode::I,
            _ => EdgeMode::J,
        };
        EdgeShape {
            mode,
            curved: true,
            nodes: vec![],
        }
    }
}

/// Which class an edge belongs to, and whether it is the derived ("slave") partner.
pub fn class_of_edge(classes: &[EdgeClass], e: usize) -> (usize, bool) {
    for (ci, cl) in classes.iter().enumerate() {
        if let Some(k) = cl.edges.iter().position(|&x| x == e) {
            return (ci, cl.kind != GlueKind::Shared && k == 1);
        }
    }
    panic!("edge {e} has no class");
}

/// How edge `e` places its class's stored shape.
pub fn edge_orient(classes: &[EdgeClass], e: usize) -> Orient {
    for cl in classes {
        if let Some(k) = cl.edges.iter().position(|&x| x == e) {
            if let Some(&o) = cl.orient.get(k) {
                return o;
            }
            return match (cl.kind, k == 1) {
                (GlueKind::Shared, _) | (_, false) => Orient::Id,
                (GlueKind::Flip, true) => Orient::Rofl,
                (_, true) => Orient::Rot,
            };
        }
    }
    panic!("edge {e} has no class");
}

pub fn nodes_for_edge(classes: &[EdgeClass], shapes: &[EdgeShape], e: usize) -> Vec<Node> {
    let (ci, slave) = class_of_edge(classes, e);
    let st = &shapes[ci];
    if st.mode == EdgeMode::I {
        return vec![];
    }
    if !classes[ci].orient.is_empty() {
        let o = edge_orient(classes, e);
        let mut out: Vec<Node> = st.nodes.iter().map(|&q| o.apply(q)).collect();
        if o.reverses() {
            out.reverse();
        }
        return out;
    }
    if !slave {
        return st.nodes.clone();
    }
    if classes[ci].kind == GlueKind::Flip {
        return st.nodes.iter().map(|q| Node { t: q.t, d: -q.d }).collect();
    }
    st.nodes
        .iter()
        .rev()
        .map(|q| Node {
            t: 1.0 - q.t,
            d: -q.d,
        })
        .collect()
}

pub fn edge_world(r: P, s: P, q: Node) -> P {
    let (dx, dy) = (s[0] - r[0], s[1] - r[1]);
    [r[0] + q.t * dx - q.d * dy, r[1] + q.t * dy + q.d * dx]
}

/// Catmull-Rom through `pts`, `per_span` samples per span, excluding the final point.
pub fn catmull(pts: &[P], per_span: usize) -> Vec<P> {
    let mut out = Vec::with_capacity((pts.len() - 1) * per_span);
    let n = pts.len();
    for i in 0..n - 1 {
        let p0 = pts[i.saturating_sub(1)];
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = pts[(i + 2).min(n - 1)];
        for k in 0..per_span {
            let u = k as f64 / per_span as f64;
            let (u2, u3) = (u * u, u * u * u);
            let f = |a: f64, b: f64, c: f64, d: f64| {
                0.5 * (2.0 * b
                    + (c - a) * u
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * u2
                    + (3.0 * b - a - 3.0 * c + d) * u3)
            };
            out.push([f(p0[0], p1[0], p2[0], p3[0]), f(p0[1], p1[1], p2[1], p3[1])]);
        }
    }
    out
}

/// Points along edge `e` from its start corner, excluding the end corner.
pub fn flatten_edge(
    classes: &[EdgeClass],
    shapes: &[EdgeShape],
    corners: &[P],
    e: usize,
    per_span: usize,
) -> Vec<P> {
    let r = corners[e];
    let s = corners[(e + 1) % corners.len()];
    let nodes = nodes_for_edge(classes, shapes, e);
    let (ci, _) = class_of_edge(classes, e);
    let mut pts = vec![r];
    pts.extend(nodes.iter().map(|&q| edge_world(r, s, q)));
    pts.push(s);
    if shapes[ci].curved && !nodes.is_empty() {
        catmull(&pts, per_span)
    } else {
        pts.pop();
        pts
    }
}

pub fn tile_outline(
    classes: &[EdgeClass],
    shapes: &[EdgeShape],
    corners: &[P],
    per_span: usize,
) -> Vec<P> {
    (0..corners.len())
        .flat_map(|e| flatten_edge(classes, shapes, corners, e, per_span))
        .collect()
}

pub fn self_intersects(poly: &[P]) -> bool {
    let m = poly.len();
    let cr = |a: P, b: P, c: P| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    for i in 0..m {
        let (a, b) = (poly[i], poly[(i + 1) % m]);
        for j in i + 2..m {
            if i == 0 && j == m - 1 {
                continue;
            }
            let (c, e) = (poly[j], poly[(j + 1) % m]);
            let (d1, d2, d3, d4) = (cr(a, b, c), cr(a, b, e), cr(c, e, a), cr(c, e, b));
            if d1 * d2 < -1e-12 && d3 * d4 < -1e-12 {
                return true;
            }
        }
    }
    false
}

/// For each edge, the map that carries the tile onto its neighbour across that edge.
pub fn glue_maps(classes: &[EdgeClass], corners: &[P]) -> Vec<Affine> {
    let m = corners.len();
    let mut t = vec![Affine::ID; m];
    let seg = |e: usize| (corners[e], corners[(e + 1) % m]);
    for cl in classes {
        if cl.kind == GlueKind::SelfHalfTurn {
            let (p, q) = seg(cl.edges[0]);
            t[cl.edges[0]] = direct_map(p, q, p, q);
            continue;
        }
        let (a, b) = (cl.edges[0], cl.edges[1]);
        let (r, s) = seg(a);
        let (p, q) = seg(b);
        let mm = if cl.kind == GlueKind::Flip {
            flip_map(p, q, r, s)
        } else {
            direct_map(p, q, r, s)
        };
        t[a] = mm;
        t[b] = mm.inv();
    }
    t
}

// ---------------------------------------------------------------------------------------------
// Editing (ported from the prototype's editor).

pub fn world_to_edge(r: P, s: P, p: P) -> Node {
    let (dx, dy) = (s[0] - r[0], s[1] - r[1]);
    let l = dx * dx + dy * dy;
    let (x, y) = (p[0] - r[0], p[1] - r[1]);
    Node {
        t: (x * dx + y * dy) / l,
        d: (-x * dy + y * dx) / l,
    }
}

/// Modes an edge class may use. Half-turn and shared classes must stay half-turn symmetric.
pub fn allowed_modes(kind: GlueKind) -> &'static [EdgeMode] {
    match kind {
        GlueKind::SelfHalfTurn | GlueKind::Shared => &[EdgeMode::S, EdgeMode::I],
        GlueKind::Mirror => &[EdgeMode::U, EdgeMode::I],
        GlueKind::Straight => &[EdgeMode::I],
        _ => &[EdgeMode::J, EdgeMode::U, EdgeMode::S, EdgeMode::I],
    }
}

/// A node on edge `e` expressed in its class's stored frame (works for every family).
pub fn to_class_frame(classes: &[EdgeClass], e: usize, q: Node) -> Node {
    // Every orientation is its own inverse.
    edge_orient(classes, e).apply(q)
}

/// Index in the class's node list of node `i` (of `count`) as drawn on edge `e`.
pub fn class_node_index(classes: &[EdgeClass], e: usize, i: usize, count: usize) -> usize {
    if edge_orient(classes, e).reverses() {
        count - 1 - i
    } else {
        i
    }
}

/// A node on a derived (slave) edge expressed on the class's master edge.
pub fn to_master(kind: GlueKind, slave: bool, q: Node) -> Node {
    if !slave {
        return q;
    }
    if kind == GlueKind::Flip {
        Node { t: q.t, d: -q.d }
    } else {
        Node {
            t: 1.0 - q.t,
            d: -q.d,
        }
    }
}

/// Index of a slave edge's node `i` in the master list.
pub fn master_index(kind: GlueKind, slave: bool, i: usize, count: usize) -> usize {
    if slave && kind != GlueKind::Flip {
        count - 1 - i
    } else {
        i
    }
}

fn mate(mode: EdgeMode, q: Node) -> Node {
    if mode == EdgeMode::U {
        Node {
            t: 1.0 - q.t,
            d: q.d,
        }
    } else {
        Node {
            t: 1.0 - q.t,
            d: -q.d,
        }
    }
}

/// Move node `i`, keeping the mode's symmetry (its twin moves too).
pub fn set_node(st: &mut EdgeShape, i: usize, q: Node) {
    let m = st.nodes.len();
    if i >= m {
        return;
    }
    let j = m - 1 - i;
    let mut q = Node {
        t: q.t.clamp(-0.4, 1.4),
        d: q.d.clamp(-1.2, 1.2),
    };
    match st.mode {
        EdgeMode::U => {
            if i == j {
                q.t = 0.5;
            }
            st.nodes[i] = q;
            if i != j {
                st.nodes[j] = mate(EdgeMode::U, q);
            }
        }
        EdgeMode::S => {
            if i == j {
                q = Node { t: 0.5, d: 0.0 };
            }
            st.nodes[i] = q;
            if i != j {
                st.nodes[j] = mate(EdgeMode::S, q);
            }
        }
        _ => st.nodes[i] = q,
    }
}

/// Insert a node (and its twin in symmetric modes), keeping nodes ordered along the edge.
pub fn insert_node(st: &mut EdgeShape, kind: GlueKind, q: Node) {
    if st.mode == EdgeMode::I {
        st.mode = match kind {
            GlueKind::Direct | GlueKind::Flip => EdgeMode::J,
            GlueKind::Mirror => EdgeMode::U,
            GlueKind::Straight => return,
            _ => EdgeMode::S,
        };
    }
    let mut q = q;
    let pos = |nodes: &[Node], t: f64| nodes.iter().position(|x| x.t > t).unwrap_or(nodes.len());
    if st.mode == EdgeMode::J {
        let at = pos(&st.nodes, q.t);
        st.nodes.insert(at, q);
        return;
    }
    if q.t > 0.5 {
        q = mate(st.mode, q);
    }
    if (q.t - 0.5).abs() < 0.02 {
        q.t = 0.42;
    }
    let at = pos(&st.nodes, q.t);
    st.nodes.insert(at, q);
    let m = mate(st.mode, q);
    let at2 = st.nodes.len() - at;
    st.nodes.insert(at2, m);
}

/// Delete node `i` (and its twin in symmetric modes).
pub fn delete_node(st: &mut EdgeShape, i: usize) {
    let m = st.nodes.len();
    if i >= m {
        return;
    }
    if st.mode == EdgeMode::J {
        st.nodes.remove(i);
    } else {
        let j = m - 1 - i;
        st.nodes.remove(i.max(j));
        if i != j {
            st.nodes.remove(i.min(j));
        }
    }
}

/// After a mode change, rebuild the second half of the nodes from the first.
pub fn resymmetrize(st: &mut EdgeShape) {
    if matches!(st.mode, EdgeMode::J | EdgeMode::I) {
        return;
    }
    let mut half: Vec<Node> = st.nodes.iter().copied().filter(|q| q.t < 0.5).collect();
    half.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
    let mut out = half.clone();
    out.extend(half.iter().rev().map(|&q| mate(st.mode, q)));
    st.nodes = out;
}
