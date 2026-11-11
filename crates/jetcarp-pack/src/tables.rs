//! The tables of pack format 1.0: each one's schema id, columns, sort order and partition
//! columns, and the row types publishing and importing use. The written spec
//! (`docs/src/reference/pack-format.md`) lists the same columns; a test checks the page against
//! these definitions.

use crate::columns::{Col, Ty, c, list, n};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// One table.
#[derive(Debug)]
pub struct TableDef {
    /// Its name in a manifest's `tables`.
    pub name: &'static str,
    /// `jetcarp-<table> <n>`.
    pub schema: &'static str,
    pub cols: Vec<Col>,
    /// Rows are sorted by these columns (then by the whole row, so the order is total).
    pub sort: &'static [&'static str],
    /// Partition columns (each part holds one value of each).
    pub partition: &'static [&'static str],
    /// Facts (one row per key, the same in every pack) or events (rows per publisher).
    pub facts: bool,
}

fn cell() -> Ty {
    Ty::Struct(vec![
        c("i", Ty::I32),
        c("j", Ty::I32),
        c("k", Ty::I32),
        c("s", Ty::I32),
    ])
}

fn offsets() -> Ty {
    list(list(Ty::Struct(vec![
        c("di", Ty::I32),
        c("dj", Ty::I32),
        c("k", Ty::I32),
    ])))
}

fn points() -> Ty {
    list(list(Ty::F64))
}

fn site() -> Ty {
    Ty::Struct(vec![
        c(
            "patch",
            Ty::Struct(vec![
                c("family", Ty::Str),
                n("params", Ty::Str),
                c("level", Ty::I32),
            ]),
        ),
        c("anchor", Ty::I32),
        c("anchor_shape", Ty::I32),
        c("anchor_point", list(Ty::F64)),
        c("radius", Ty::I32),
        n("nb", Ty::Str),
        c(
            "tiles",
            list(Ty::Struct(vec![
                c("shape", Ty::I32),
                c("dist", Ty::I32),
                c("point", list(Ty::F64)),
                c("corners", points()),
            ])),
        ),
        n(
            "graph",
            Ty::Struct(vec![
                c("rule", Ty::Str),
                c("degrees", Ty::Bool),
                c("chirality", Ty::Bool),
                c(
                    "cells",
                    list(Ty::Struct(vec![
                        c("shape", Ty::I32),
                        c("flip", Ty::Bool),
                        c("degree", Ty::I32),
                        c("states", list(Ty::I32)),
                        c("neighbours", list(Ty::I32)),
                    ])),
                ),
                c(
                    "around",
                    list(Ty::Struct(vec![
                        c("tiles", list(Ty::I32)),
                        c("count", Ty::I32),
                    ])),
                ),
            ]),
        ),
    ])
}

/// Every table, in the order the spec lists them.
pub fn all() -> &'static [TableDef] {
    static T: OnceLock<Vec<TableDef>> = OnceLock::new();
    T.get_or_init(|| {
        vec![
            TableDef {
                name: "tilings",
                schema: "jetcarp-tilings 1",
                cols: vec![
                    c("pin_key", Ty::Str),
                    c("pin", Ty::Str),
                    c("polygon_hash", Ty::Str),
                    c("neighbour_hash", Ty::Str),
                    c("name", Ty::Str),
                    c("kind", Ty::Str),
                    c("family", Ty::Str),
                    n("params", Ty::Str),
                    n("edges", Ty::Str),
                    c("tiling_group", Ty::Str),
                    n("a1", list(Ty::F64)),
                    n("a2", list(Ty::F64)),
                    n(
                        "tile_types",
                        list(Ty::Struct(vec![
                            c("shape", Ty::I32),
                            c("corners", points()),
                        ])),
                    ),
                    n("edge", offsets()),
                    n("touch", offsets()),
                    n("reference_level", Ty::I32),
                    n("reference", Ty::Str),
                    n("reference_tiles", Ty::I32),
                ],
                sort: &["pin_key"],
                partition: &[],
                facts: true,
            },
            TableDef {
                name: "rules",
                schema: "jetcarp-rules 1",
                cols: vec![
                    c("notation", Ty::Str),
                    c("neighbourhood", Ty::Str),
                    c("steps_per_gen", Ty::I32),
                    c("family", Ty::Str),
                    c("semantics", Ty::Str),
                    c("states", Ty::I32),
                    n("birth", Ty::I64),
                    n("survive", Ty::I64),
                ],
                sort: &["notation", "neighbourhood", "steps_per_gen"],
                partition: &[],
                facts: true,
            },
            TableDef {
                name: "patterns",
                schema: "jetcarp-patterns 1",
                cols: vec![
                    c("id", Ty::Str),
                    c("identity", Ty::Str),
                    c("pin_key", Ty::Str),
                    c("pin", Ty::Str),
                    c("model", Ty::Str),
                    c("neighbourhood", Ty::Str),
                    c("steps_per_gen", Ty::I32),
                    c("class", Ty::Str),
                    c("period", Ty::I64),
                    c("dx", Ty::I32),
                    c("dy", Ty::I32),
                    n("speed", Ty::Str),
                    c("cells", Ty::I32),
                    c("min_cells", Ty::I32),
                    c("max_cells", Ty::I32),
                    c("phases", list(list(cell()))),
                    c("canonical", Ty::Str),
                    n("rule", Ty::Str),
                    n("site", site()),
                    n("gun", Ty::Str),
                    c("class_group", Ty::Str),
                    c("tiling_group", Ty::Str),
                ],
                sort: &["id"],
                partition: &["class_group", "tiling_group"],
                facts: true,
            },
            TableDef {
                name: "verifications",
                schema: "jetcarp-verifications 1",
                cols: vec![
                    c("pattern_id", Ty::Str),
                    c("rule", Ty::Str),
                    c("publisher", Ty::Str),
                    c("main", Ty::Bool),
                    c("periods", Ty::I64),
                    c("generations", Ty::I64),
                    c("field", Ty::Str),
                    n("sites", Ty::I32),
                    c("engine_version", Ty::Str),
                    c("pin", Ty::Str),
                    c("semantics", Ty::Str),
                    c("date", Ty::Str),
                    n("range", Ty::Str),
                    n("mover", Ty::Str),
                ],
                sort: &["pattern_id", "rule", "publisher", "date"],
                partition: &[],
                facts: false,
            },
            TableDef {
                name: "sightings",
                schema: "jetcarp-sightings 1",
                cols: vec![
                    c("pattern_id", Ty::Str),
                    c("rule", Ty::Str),
                    c("publisher", Ty::Str),
                    c("source", Ty::Str),
                    n("source_kind", Ty::Str),
                    c("count", Ty::I64),
                    c("soups", Ty::I64),
                    c("first_seen", Ty::Str),
                    c("last_seen", Ty::Str),
                ],
                sort: &["pattern_id", "rule", "publisher", "source"],
                partition: &[],
                facts: false,
            },
            TableDef {
                name: "provenance",
                schema: "jetcarp-provenance 1",
                cols: vec![
                    c("target", Ty::Str),
                    c("publisher", Ty::Str),
                    c("seq", Ty::I32),
                    c("kind", Ty::Str),
                    n("finder", Ty::Str),
                    n("orcid", Ty::Str),
                    n("search_kind", Ty::Str),
                    n("search", Ty::Str),
                    n("machine", Ty::Str),
                    c("engine_version", Ty::Str),
                    c("pin", Ty::Str),
                    c("date", Ty::Str),
                    n("source", Ty::Str),
                    n("pack", Ty::Str),
                    n("run", Ty::Str),
                ],
                sort: &["target", "publisher", "seq"],
                partition: &[],
                facts: false,
            },
            TableDef {
                name: "annotations",
                schema: "jetcarp-annotations 1",
                cols: vec![
                    c("target", Ty::Str),
                    c("publisher", Ty::Str),
                    c("field", Ty::Str),
                    c("value", Ty::Str),
                    n("score_model", Ty::Str),
                    c("date", Ty::Str),
                ],
                sort: &["target", "publisher", "field", "date"],
                partition: &[],
                facts: false,
            },
            TableDef {
                name: "ants",
                schema: "jetcarp-ants 1",
                cols: vec![
                    c("id", Ty::Str),
                    c("identity", Ty::Str),
                    c("pin_key", Ty::Str),
                    c("pin", Ty::Str),
                    c("rule", Ty::Str),
                    c("kind", Ty::Str),
                    c("family", Ty::Str),
                    c("semantics", Ty::Str),
                    c("states", Ty::I32),
                    c("colors", Ty::I32),
                    c("start", Ty::Str),
                    c("outcome", Ty::Str),
                    n("period", Ty::I64),
                    n("dx", Ty::I32),
                    n("dy", Ty::I32),
                    n("onset", Ty::I64),
                    n("speed", Ty::F64),
                    c("steps", Ty::I64),
                    c("cells", Ty::I64),
                    c("run", Ty::Str),
                    c("tiling_group", Ty::Str),
                ],
                sort: &["id"],
                partition: &["tiling_group"],
                facts: true,
            },
            TableDef {
                name: "quickstart",
                schema: "jetcarp-quickstart 1",
                cols: vec![
                    c("id", Ty::Str),
                    c("title", Ty::Str),
                    c("description", Ty::Str),
                    n("source", Ty::Str),
                    c("scene", Ty::Str),
                    c("scene_format", Ty::Str),
                    c("rule", Ty::Str),
                    c("rule_family", Ty::Str),
                    c("semantics", Ty::Str),
                    n("behaviour", Ty::Str),
                    n("pin_key", Ty::Str),
                    n("pin", Ty::Str),
                    c("tiling_group", Ty::Str),
                ],
                sort: &["id"],
                partition: &["tiling_group"],
                facts: true,
            },
            TableDef {
                name: "media",
                schema: "jetcarp-media 1",
                cols: vec![
                    c("target", Ty::Str),
                    c("kind", Ty::Str),
                    c("from_seed", Ty::Bool),
                    c("object", Ty::Str),
                    c("media_type", Ty::Str),
                    c("width", Ty::I32),
                    c("height", Ty::I32),
                    c("frames", Ty::I32),
                    c("bytes", Ty::I64),
                    c("render_key", Ty::Str),
                    c("render", Ty::Str),
                ],
                sort: &["target", "kind", "from_seed"],
                partition: &[],
                facts: false,
            },
            TableDef {
                name: "aliases",
                schema: "jetcarp-aliases 1",
                cols: vec![
                    c("old_id", Ty::Str),
                    c("target", Ty::Str),
                    c("reason", Ty::Str),
                    c("date", Ty::Str),
                ],
                sort: &["old_id"],
                partition: &[],
                facts: true,
            },
            TableDef {
                name: "ratings",
                schema: "jetcarp-ratings 1",
                cols: vec![
                    c("rater", Ty::Str),
                    c("target", Ty::Str),
                    c("scale", Ty::Str),
                    c("verdict", Ty::Str),
                    n("note", Ty::Str),
                    n("round", Ty::Str),
                    c("date", Ty::Str),
                ],
                sort: &["rater", "target", "date"],
                partition: &[],
                facts: false,
            },
            TableDef {
                name: "raters",
                schema: "jetcarp-raters 1",
                cols: vec![
                    c("rater", Ty::Str),
                    c("consent_date", Ty::Str),
                    c("consent_scope", Ty::Str),
                    n("display_name", Ty::Str),
                    n("orcid", Ty::Str),
                    n("members", list(Ty::Str)),
                    c("notes", Ty::Bool),
                ],
                sort: &["rater"],
                partition: &[],
                facts: false,
            },
            TableDef {
                name: "score_models",
                schema: "jetcarp-score-models 1",
                cols: vec![
                    c("model", Ty::Str),
                    c("version", Ty::Str),
                    c("kind", Ty::Str),
                    c("features", list(Ty::Str)),
                    c("weights", list(Ty::F64)),
                    c("intercept", Ty::F64),
                    c("fitted_on", Ty::Str),
                    n("rater", Ty::Str),
                    c("metrics", Ty::Str),
                ],
                sort: &["model", "version"],
                partition: &[],
                facts: false,
            },
        ]
    })
}

/// The reference patch of an aperiodic family (an object a `tilings` row names).
pub fn reference_patch() -> &'static TableDef {
    static T: OnceLock<TableDef> = OnceLock::new();
    T.get_or_init(|| TableDef {
        name: "reference",
        schema: "jetcarp-reference-patch 1",
        cols: vec![
            c("tile", Ty::I32),
            c("shape", Ty::I32),
            c("corners", points()),
            c("edge", list(Ty::I32)),
            c("touch", list(Ty::I32)),
            c("open", Ty::Bool),
        ],
        sort: &["tile"],
        partition: &[],
        facts: true,
    })
}

/// A table by name.
pub fn table(name: &str) -> Option<&'static TableDef> {
    all().iter().find(|t| t.name == name)
}

/// Is a partition value safe as a folder name: lower-case letters, digits, `-` and `_`, 1 to
/// 64 of them, starting with a letter or digit?
pub fn valid_partition_value(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

// ---------------------------------------------------------------------------------------------
// Row types (the columns above, as serde structs)

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Cell {
    pub i: i32,
    pub j: i32,
    pub k: i32,
    pub s: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OffsetRow {
    pub di: i32,
    pub dj: i32,
    pub k: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TileType {
    pub shape: i32,
    pub corners: Vec<[f64; 2]>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TilingRow {
    pub pin_key: String,
    pub pin: String,
    pub polygon_hash: String,
    pub neighbour_hash: String,
    pub name: String,
    pub kind: String,
    pub family: String,
    pub params: Option<String>,
    pub edges: Option<String>,
    pub tiling_group: String,
    pub a1: Option<[f64; 2]>,
    pub a2: Option<[f64; 2]>,
    pub tile_types: Option<Vec<TileType>>,
    pub edge: Option<Vec<Vec<OffsetRow>>>,
    pub touch: Option<Vec<Vec<OffsetRow>>>,
    pub reference_level: Option<i32>,
    pub reference: Option<String>,
    pub reference_tiles: Option<i32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RefTileRow {
    pub tile: i32,
    pub shape: i32,
    pub corners: Vec<[f64; 2]>,
    pub edge: Vec<i32>,
    pub touch: Vec<i32>,
    pub open: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RuleRow {
    pub notation: String,
    pub neighbourhood: String,
    pub steps_per_gen: i32,
    pub family: String,
    pub semantics: String,
    pub states: i32,
    pub birth: Option<i64>,
    pub survive: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PatchRow {
    pub family: String,
    pub params: Option<String>,
    pub level: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SiteTileRow {
    pub shape: i32,
    pub dist: i32,
    pub point: [f64; 2],
    pub corners: Vec<[f64; 2]>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GraphCellRow {
    pub shape: i32,
    pub flip: bool,
    pub degree: i32,
    pub states: Vec<i32>,
    pub neighbours: Vec<i32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AroundRow {
    pub tiles: Vec<i32>,
    pub count: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GraphRow {
    pub rule: String,
    pub degrees: bool,
    pub chirality: bool,
    pub cells: Vec<GraphCellRow>,
    pub around: Vec<AroundRow>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SiteRow {
    pub patch: PatchRow,
    pub anchor: i32,
    pub anchor_shape: i32,
    pub anchor_point: [f64; 2],
    pub radius: i32,
    pub nb: Option<String>,
    pub tiles: Vec<SiteTileRow>,
    pub graph: Option<GraphRow>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PatternRow {
    pub id: String,
    pub identity: String,
    pub pin_key: String,
    pub pin: String,
    pub model: String,
    pub neighbourhood: String,
    pub steps_per_gen: i32,
    pub class: String,
    pub period: i64,
    pub dx: i32,
    pub dy: i32,
    pub speed: Option<String>,
    pub cells: i32,
    pub min_cells: i32,
    pub max_cells: i32,
    pub phases: Vec<Vec<Cell>>,
    pub canonical: String,
    pub rule: Option<String>,
    pub site: Option<SiteRow>,
    pub gun: Option<String>,
    pub class_group: String,
    pub tiling_group: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VerificationRow {
    pub pattern_id: String,
    pub rule: String,
    pub publisher: String,
    pub main: bool,
    pub periods: i64,
    pub generations: i64,
    pub field: String,
    pub sites: Option<i32>,
    pub engine_version: String,
    pub pin: String,
    pub semantics: String,
    pub date: String,
    pub range: Option<String>,
    pub mover: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SightingRow {
    pub pattern_id: String,
    pub rule: String,
    pub publisher: String,
    pub source: String,
    pub source_kind: Option<String>,
    pub count: i64,
    pub soups: i64,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvenanceRow {
    pub target: String,
    pub publisher: String,
    pub seq: i32,
    pub kind: String,
    pub finder: Option<String>,
    pub orcid: Option<String>,
    pub search_kind: Option<String>,
    pub search: Option<String>,
    pub machine: Option<String>,
    pub engine_version: String,
    pub pin: String,
    pub date: String,
    pub source: Option<String>,
    pub pack: Option<String>,
    pub run: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnotationRow {
    pub target: String,
    pub publisher: String,
    pub field: String,
    pub value: String,
    pub score_model: Option<String>,
    pub date: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AntPackRow {
    pub id: String,
    pub identity: String,
    pub pin_key: String,
    pub pin: String,
    pub rule: String,
    pub kind: String,
    pub family: String,
    pub semantics: String,
    pub states: i32,
    pub colors: i32,
    pub start: String,
    pub outcome: String,
    pub period: Option<i64>,
    pub dx: Option<i32>,
    pub dy: Option<i32>,
    pub onset: Option<i64>,
    pub speed: Option<f64>,
    pub steps: i64,
    pub cells: i64,
    pub run: String,
    pub tiling_group: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuickstartPackRow {
    pub id: String,
    pub title: String,
    pub description: String,
    pub source: Option<String>,
    pub scene: String,
    pub scene_format: String,
    pub rule: String,
    pub rule_family: String,
    pub semantics: String,
    pub behaviour: Option<String>,
    pub pin_key: Option<String>,
    pub pin: Option<String>,
    pub tiling_group: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaPackRow {
    pub target: String,
    pub kind: String,
    pub from_seed: bool,
    pub object: String,
    pub media_type: String,
    pub width: i32,
    pub height: i32,
    pub frames: i32,
    pub bytes: i64,
    pub render_key: String,
    pub render: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AliasPackRow {
    pub old_id: String,
    pub target: String,
    pub reason: String,
    pub date: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RatingPackRow {
    pub rater: String,
    pub target: String,
    pub scale: String,
    pub verdict: String,
    pub note: Option<String>,
    pub round: Option<String>,
    pub date: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaterRow {
    pub rater: String,
    pub consent_date: String,
    pub consent_scope: String,
    pub display_name: Option<String>,
    pub orcid: Option<String>,
    pub members: Option<Vec<String>>,
    pub notes: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScoreModelRow {
    pub model: String,
    pub version: String,
    pub kind: String,
    pub features: Vec<String>,
    pub weights: Vec<f64>,
    pub intercept: f64,
    pub fitted_on: String,
    pub rater: Option<String>,
    pub metrics: String,
}

/// The `tiling_group` of a tiling (fixed for good): `k1` to `k6` for catalogue tilings (family
/// `periodic` with a catalogue id, from the catalogue's `k`), `isohedral` for the isohedral
/// types, else the family id.
pub fn tiling_group(t: &jetcarp_core::TilingSpec) -> String {
    if t.family == "periodic" {
        let k = t
            .params
            .as_ref()
            .and_then(|p| p.get("catalog"))
            .and_then(|v| v.as_str())
            .and_then(jetcarp_core::catalog::get)
            .map(|c| c.k)
            .unwrap_or(0);
        return if (1..=6).contains(&k) {
            format!("k{k}")
        } else {
            "periodic".into()
        };
    }
    if jetcarp_core::isohedral::family(&t.family).is_some() {
        return "isohedral".into();
    }
    t.family.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_is_well_formed() {
        let mut names = std::collections::HashSet::new();
        for t in all().iter().chain([reference_patch()]) {
            assert!(names.insert(t.name), "{}", t.name);
            assert!(t.schema.ends_with(" 1"), "{}", t.schema);
            for k in t.sort.iter().chain(t.partition) {
                let col = t.cols.iter().find(|c| c.name == *k);
                assert!(col.is_some_and(|c| !c.nullable), "{}.{k}", t.name);
            }
            let s = crate::columns::schema(&t.cols);
            assert_eq!(s.fields().len(), t.cols.len());
        }
        assert!(valid_partition_value("k3"));
        assert!(valid_partition_value("penrose-kd"));
        assert!(valid_partition_value("still_life"));
        for bad in ["", "..", "a/b", "K3", "-x", &"x".repeat(65)] {
            assert!(!valid_partition_value(bad), "{bad}");
        }
        let mut k = jetcarp_core::TilingSpec::new("periodic");
        k.params = Some(serde_json::json!({"catalog": "k3-019"}));
        assert_eq!(tiling_group(&k), "k3");
        assert_eq!(
            tiling_group(&jetcarp_core::TilingSpec::new("square")),
            "square"
        );
        assert_eq!(
            tiling_group(&jetcarp_core::TilingSpec::new("ih79")),
            "isohedral"
        );
    }
}
