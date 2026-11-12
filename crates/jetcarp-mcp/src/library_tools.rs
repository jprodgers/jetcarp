//! The library tools: `library_query` (list or read patterns, ant runs and Quick Start entries,
//! or the library's counts) and `library_curate` (change, hide, remove, add to the Quick Start,
//! and the compounds pass).
//!
//! The library is `JETCARP_LIBRARY` (a folder or a `.sqlite` file), else
//! `<data>/library.sqlite`, shared with the app, scripts and `jetcarp library`; the server opens
//! it once.

use crate::schema::{self, named, object, words};
use crate::tools::{Call, Res, Tool, ToolOutput, ctx, err};
use jetcarp_api::op::{self, NoInput, Op, Quiet, Report, input_schema};
use jetcarp_api::ops::library::{
    GetIn, LibraryGet, LibraryInfoOp, LibraryQuery, LibraryRemove, LibraryUpdate, QueryIn,
    RemoveIn, UpdateIn,
};
use jetcarp_api::ops::quickstart::{
    AddIn, HideIn, IdIn, ListIn, QuickstartAdd, QuickstartGet, QuickstartList, QuickstartRemove,
    QuickstartUpdate,
};
use jetcarp_core::pattern::PatternRecord;
use serde_json::{Value, json};

/// The most records one listing returns.
const MAX_LIST: u64 = 100;

pub(crate) fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "library_query",
            title: "List or read library records",
            read_only: true,
            destructive: false,
            description: || {
                "Lists or reads the library: what searches found and what you and the user kept. `kind`: \"pattern\" (default: verified still lifes, oscillators, spaceships, guns, puffers and movers, each with the rules it was verified in and the range of rules it runs identically in), \"ant\" (ant and turmite runs, classed highway, periodic, symmetric, bounded or chaotic), \"quickstart\" (the app's Quick Start entries: built-in examples, packs, explore atlases, promotions) or \"summary\" (counts). Listings come best first (`limit`, default 20, at most 100; `offset` pages); each filter says which kind it applies to. With `id`, one record in full: a pattern's rules, sightings, the cells of its phases as text, and which way it moves per paste orientation; an ant run's outcome and proof; a Quick Start entry with its scene. Examples: {\"class\": \"spaceship\", \"tiling\": \"hex\"}, {\"works_in\": \"B36/S23\"}, {\"kind\": \"ant\", \"outcome\": \"highway\"}, {\"id\": \"glider\", \"phases\": 4}. Next: world_new {\"pattern\": id} or world_edit {\"paste\": {\"id\": id}} to run one; library_curate to curate; media_render for pictures.".into()
            },
            schema: || {
                let mut p = named(vec![(
                    "kind",
                    words(
                        &["pattern", "ant", "quickstart", "summary"],
                        "What to list or read (default pattern; with `id`, the id's form says).",
                    ),
                )]);
                let q = schema::props(&input_schema::<QueryIn>());
                for (k, v) in q {
                    if k != "kind" && k != "count_only" {
                        p.insert(k, v);
                    }
                }
                let mut limit = p["limit"].clone();
                limit["description"] =
                    json!("At most this many records (default 20, at most 100).");
                p.insert("limit".into(), limit);
                let mut text = p["text"].clone();
                text["description"] = json!(
                    "Patterns whose name or label contains this; Quick Start entries whose title, rule or tiling contains it."
                );
                p.insert("text".into(), text);
                let g = schema::props(&input_schema::<GetIn>());
                p.insert(
                    "id".into(),
                    json!({"type": "string", "description": "Read this record in full instead: a pattern's id, prefix or name (\"glider\"), an ant run (ant-...) or a Quick Start handle (find:..., life)."}),
                );
                p.insert("phases".into(), g["phases"].clone());
                let l = schema::props(&input_schema::<ListIn>());
                for k in ["source", "hidden"] {
                    let mut v = l[k].clone();
                    let d = v["description"].as_str().unwrap_or("").to_string();
                    v["description"] = json!(format!("Quick Start: {d}"));
                    p.insert(k.into(), v);
                }
                schema::retell(
                    &mut p,
                    "source",
                    "Quick Start: only entries from this source: builtin, pack, atlas, promotion, or all (default: all but builtin).",
                );
                object(p, &[])
            },
            run: library_query,
        },
        Tool {
            name: "library_curate",
            title: "Curate library records",
            read_only: false,
            destructive: true,
            description: || {
                "Changes the library. With `id` (a pattern's id, prefix or name; ant-...; or a Quick Start handle) give one of: `changes` (name, tags, notes, rating 1-5, state), e.g. {\"id\": \"glider\", \"changes\": {\"rating\": 4, \"append_notes\": \"LLM: a clean c/4 ship\"}} (mark text you wrote as yours); `hidden` (Quick Start entries: true leaves it out of the menu, packs and the website; false shows it again); or `remove`: true (deletes the record for good, a pattern with its rules, sightings and Quick Start entry; ask the user first). Without `id`: `quickstart` adds an entry to the app's Quick Start under \"My finds\", e.g. {\"quickstart\": {\"pattern\": \"<id>\", \"title\": \"Trihex glider\"}}; or `compounds` finds stored patterns that are really several independent objects (two gliders side by side) and lists their parts ({\"compounds\": {}}; with {\"apply\": true} it stores the parts instead).".into()
            },
            schema: || {
                let mut p = named(vec![
                    (
                        "id",
                        json!({"type": "string", "description": "The record: a pattern's id, unique prefix or name; an ant run's id (ant-...); or a Quick Start entry's handle (find:..., atlas:..., life)."}),
                    ),
                    (
                        "kind",
                        words(
                            &["pattern", "ant", "quickstart"],
                            "What `id` names (default: from its form).",
                        ),
                    ),
                    (
                        "changes",
                        schema::without(
                            schema::op(
                                input_schema::<UpdateIn>(),
                                Some(
                                    "Patterns and ant runs: the changes (what is not given stays). Patterns: name, tags or add_tags / remove_tags, notes or append_notes, rating 1-5 (0 clears), state new / reviewed / promoted / rejected, discovered_by; ant runs: name, notes, rating, state.",
                                ),
                            ),
                            &["id", "kind"],
                        ),
                    ),
                    (
                        "hidden",
                        json!({"type": "boolean", "description": "Quick Start entries: leave it out of the menu, packs and the website (true), or show it again (false)."}),
                    ),
                    (
                        "remove",
                        json!({"type": "boolean", "description": "Delete the record for good (true)."}),
                    ),
                    (
                        "quickstart",
                        schema::op(
                            input_schema::<AddIn>(),
                            Some(
                                "Add an entry to the Quick Start under \"My finds\". It takes one of `pattern`, `ant`, `folder` with `run`, `search_run` (with `job`), or `scene`, e.g. {\"pattern\": \"<id>\", \"title\": \"Trihex glider\"}, {\"ant\": \"ant-...\"}, {\"search_run\": <run id>, \"job\": <explore job>}, {\"folder\": \"<sweep folder>\", \"run\": \"#12\"} (a run of a run_script sweep), {\"scene\": \"<scene file>\"}.",
                            ),
                        ),
                    ),
                ]);
                if let Some(s) = crate::compounds::schema() {
                    p.insert("compounds".into(), s);
                }
                object(p, &[])
            },
            run: library_curate,
        },
    ]
}

/// A pattern as listings show it.
fn summary(r: &PatternRecord) -> Value {
    json!({
        "id": r.id, "label": r.label(), "name": r.name, "rule": r.rule, "rules": r.rules,
        "range": r.range.as_ref().map(|x| x.describe()), "tiling": r.tiling_name,
        "class": r.class, "period": r.period, "speed": r.speed, "displacement": r.displacement,
        "cells": r.cells, "seen": r.seen, "score": r.score, "rating": r.rating, "state": r.state,
        "tags": r.tags,
    })
}

/// An ant run as listings show it.
pub(crate) fn ant_json(r: &jetcarp_store::AntRow) -> Value {
    json!({
        "id": r.id, "rule": r.rule, "tiling": r.tiling_name, "start": r.start.label(),
        "outcome": r.outcome.name(), "period": r.period, "d": r.d, "speed": r.speed,
        "onset": r.onset, "steps": r.steps, "cells": r.cells, "score": r.score,
        "name": r.name, "rating": r.rating, "state": r.state, "line": r.line(),
    })
}

/// What an id names: `kind` when given, else from the id's form.
fn kind_of(kind: Option<&str>, id: &str) -> String {
    match kind {
        Some(k) => k.to_string(),
        None if id.starts_with("ant-") => "ant".into(),
        None if id.starts_with("find:")
            || id.starts_with("atlas:")
            || id.starts_with("qs-")
            || jetcarp_store::quickstart::unmade_builtin(&ctx().library(), id).is_some()
            || (ctx().library().get(id).is_err() && ctx().library().get_quickstart(id).is_ok()) =>
        {
            "quickstart".into()
        }
        None => "pattern".into(),
    }
}

fn given<'a>(a: &'a Value, keys: &[&'a str]) -> Vec<&'a str> {
    keys.iter()
        .copied()
        .filter(|k| a.get(*k).is_some_and(|v| !v.is_null()))
        .collect()
}

fn refuse(tool: &str, what: &str, keys: &[&str]) -> Res {
    Err(format!(
        "{tool}: `{}` do{} not apply to {what}",
        keys.join("`, `"),
        if keys.len() == 1 { "es" } else { "" }
    ))
}

const PATTERN_FILTERS: &[&str] = &[
    "rule",
    "works_in",
    "tiling",
    "class",
    "state",
    "min_period",
    "max_period",
    "min_score",
    "tag",
    "text",
    "discovered_by",
    "neighborhood",
    "order",
    "offset",
    "limit",
];
const ANT_FILTERS: &[&str] = &["outcome", "tiling", "rule", "job", "sort", "limit"];
const QS_FILTERS: &[&str] = &["source", "hidden", "text", "offset", "limit"];

fn library_query(c: &mut Call) -> Res {
    let a = c.args;
    let mut all: Vec<&str> = vec!["id", "phases"];
    for k in PATTERN_FILTERS.iter().chain(ANT_FILTERS).chain(QS_FILTERS) {
        if !all.contains(k) {
            all.push(k);
        }
    }
    let kind = a.get("kind").and_then(Value::as_str);
    let limit = |d: u64| -> Result<usize, String> {
        match a.get("limit") {
            None | Some(Value::Null) => Ok(d as usize),
            Some(v) => v
                .as_u64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
                .map(|n| n.clamp(1, MAX_LIST) as usize)
                .ok_or_else(|| format!("library_query: `limit` is a whole number, not {v}")),
        }
    };
    if kind == Some("summary") {
        let extra = given(a, &all);
        if !extra.is_empty() {
            return refuse("library_query", "kind summary", &extra);
        }
        let i = LibraryInfoOp::run(ctx(), NoInput {}, &Quiet).map_err(err)?;
        return Ok(ToolOutput::text(i.text()));
    }
    if let Some(id) = a.get("id").filter(|v| !v.is_null()) {
        let id = match id {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            v => return Err(format!("library_query: `id` is text, not {v}")),
        };
        let extra: Vec<&str> = given(a, &all)
            .into_iter()
            .filter(|k| !["id", "phases"].contains(k))
            .collect();
        if !extra.is_empty() {
            return refuse("library_query", "one record (`id`)", &extra);
        }
        return get(&id, kind, a);
    }
    match kind.unwrap_or("pattern") {
        "pattern" => {
            let extra: Vec<&str> = given(a, &all)
                .into_iter()
                .filter(|k| !PATTERN_FILTERS.contains(k))
                .collect();
            if !extra.is_empty() {
                return refuse("library_query", "patterns", &extra);
            }
            let mut q: QueryIn = schema::read(
                &input_schema::<QueryIn>(),
                "library_query",
                schema::pick(a, PATTERN_FILTERS),
            )?;
            q.kind = Some("pattern".into());
            q.limit = Some(limit(20)?);
            let out = LibraryQuery::run(ctx(), q, &Quiet).map_err(err)?;
            let shown: Vec<Value> = out.patterns.iter().map(summary).collect();
            Ok(ToolOutput::json(&json!({
                "catalog": out.catalog,
                "matching": out.matching,
                "shown": shown.len(),
                "patterns": shown,
            })))
        }
        "ant" => {
            let extra: Vec<&str> = given(a, &all)
                .into_iter()
                .filter(|k| !ANT_FILTERS.contains(k))
                .collect();
            if !extra.is_empty() {
                return refuse("library_query", "ant runs", &extra);
            }
            let mut q: QueryIn = schema::read(
                &input_schema::<QueryIn>(),
                "library_query",
                schema::pick(a, ANT_FILTERS),
            )?;
            q.kind = Some("ant".into());
            q.limit = Some(limit(20)?);
            let out = LibraryQuery::run(ctx(), q, &Quiet).map_err(err)?;
            Ok(ToolOutput::json(&json!({
                "catalog": out.catalog,
                "census": out.census,
                "shown": out.ants.len(),
                "ants": out.ants.iter().map(ant_json).collect::<Vec<_>>(),
            })))
        }
        "quickstart" => {
            let extra: Vec<&str> = given(a, &all)
                .into_iter()
                .filter(|k| !QS_FILTERS.contains(k))
                .collect();
            if !extra.is_empty() {
                return refuse("library_query", "Quick Start entries", &extra);
            }
            let l: ListIn = schema::read(
                &input_schema::<ListIn>(),
                "library_query",
                schema::pick(a, &["source", "hidden"]),
            )?;
            let text = a
                .get("text")
                .and_then(Value::as_str)
                .map(|t| t.to_lowercase());
            let offset = a.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
            let n = limit(20)?;
            let out = QuickstartList::run(ctx(), l, &Quiet).map_err(err)?;
            let matching: Vec<&jetcarp_api::ops::quickstart::Entry> = out
                .entries
                .iter()
                .rev()
                .filter(|e| {
                    text.as_ref().is_none_or(|t| {
                        [&e.title, &e.rule, &e.grid, &e.id, &e.behaviour]
                            .iter()
                            .any(|s| s.to_lowercase().contains(t))
                    })
                })
                .collect();
            let shown: Vec<Value> = matching
                .iter()
                .skip(offset)
                .take(n)
                .map(|e| {
                    json!({
                        "id": e.id, "title": e.title, "rule": e.rule, "grid": e.grid,
                        "behaviour": e.behaviour, "source": e.source, "made_from": e.target_kind,
                        "hidden": e.hidden, "score": e.score,
                    })
                })
                .collect();
            Ok(ToolOutput::json(&json!({
                "catalog": out.catalog,
                "matching": matching.len(),
                "shown": shown.len(),
                "order": "newest first",
                "entries": shown,
                "next": "world_new {\"quickstart\": id} opens one; library_query {\"id\": id} shows its scene.",
            })))
        }
        k => Err(format!(
            "library_query: `kind` is pattern, ant, quickstart, or summary, not {k}"
        )),
    }
}

/// One record in full.
fn get(id: &str, kind: Option<&str>, a: &Value) -> Res {
    let kind = kind_of(kind, id);
    if kind == "quickstart" {
        if a.get("phases").is_some_and(|v| !v.is_null()) {
            return refuse("library_query", "Quick Start entries", &["phases"]);
        }
        let q = QuickstartGet::run(ctx(), IdIn { id: id.into() }, &Quiet).map_err(err)?;
        return Ok(ToolOutput::json(&op::to_value(&q)));
    }
    let mut g: GetIn = schema::read(
        &input_schema::<GetIn>(),
        "library_query",
        json!({"id": id, "kind": kind, "phases": a.get("phases").cloned().unwrap_or(Value::Null)}),
    )?;
    let n = g.phases.unwrap_or(1).clamp(1, 16);
    g.phases = Some(n);
    let v = LibraryGet::run(ctx(), g, &Quiet).map_err(err)?;
    if let Some(r) = &v.ant {
        let mut out = ToolOutput::text(r.line());
        out.content.push(
            json!({"type": "text", "text": serde_json::to_string_pretty(r).unwrap_or_default()}),
        );
        return Ok(out);
    }
    let Some(r) = &v.pattern else {
        return Err(format!("no record '{id}'"));
    };
    let mut text = format!("{} ({})\n", r.label(), r.id);
    text.push_str(&format!(
        "verified in: {}\n",
        if r.rules.is_empty() {
            r.rule.clone()
        } else {
            r.rules.join(", ")
        }
    ));
    if let Some(x) = &r.range {
        text.push_str(&format!("works in: {}\n", x.describe()));
    }
    if !v.sightings.is_empty() {
        text.push_str(&format!(
            "sightings: {}\n",
            v.sightings
                .iter()
                .map(|s| format!("{} x{} (job {})", s.rule, s.count, s.job))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !v.orientations.is_empty() {
        text.push_str(&format!(
            "moves per period by paste orientation: {}\n",
            v.orientations
                .iter()
                .enumerate()
                .map(|(k, v)| format!("{k}: ({}, {})", v[0], v[1]))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for p in v.phases.iter().take(n.min(r.period.max(1)) as usize) {
        text.push_str(&p.text());
    }
    let mut out = ToolOutput::text(text);
    out.content
        .push(json!({"type": "text", "text": r.to_json()}));
    Ok(out)
}

fn library_curate(c: &mut Call) -> Res {
    let a = c.args;
    if let Some(which) = schema::one_of("library_curate", a, &["quickstart", "compounds"])? {
        let extra = given(a, &["id", "kind", "changes", "hidden", "remove"]);
        if !extra.is_empty() {
            return refuse("library_curate", &format!("`{which}`"), &extra);
        }
        if which == "compounds" {
            return crate::compounds::run(c, &a["compounds"]);
        }
        let input: AddIn = schema::read(
            &input_schema::<AddIn>(),
            "library_curate quickstart",
            a["quickstart"].clone(),
        )?;
        let out = QuickstartAdd::run(ctx(), input, &Quiet).map_err(err)?;
        return Ok(ToolOutput::text(format!(
            "{}Open it with world_new {{\"quickstart\": \"{}\"}}.",
            out.text(),
            out.entry
        )));
    }
    let id = match a.get("id") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => {
            return Err(
                "library_curate: give `id` with changes, hidden or remove; or `quickstart` to add an entry; or `compounds`"
                    .into(),
            );
        }
    };
    let kind = kind_of(a.get("kind").and_then(Value::as_str), &id);
    let remove = match a.get("remove") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Bool(true)) => true,
        Some(Value::String(s)) if s == "true" || s == "false" => s == "true",
        Some(v) => {
            return Err(format!(
                "library_curate: `remove` is true or false, not {v}"
            ));
        }
    };
    let action = schema::one_of("library_curate", a, &["changes", "hidden"])?;
    if remove {
        if let Some(k) = action {
            return Err(format!("library_curate: give `{k}` or `remove`, not both"));
        }
        let text = if kind == "quickstart" {
            QuickstartRemove::run(ctx(), IdIn { id }, &Quiet)
                .map_err(err)?
                .text()
        } else {
            LibraryRemove::run(
                ctx(),
                RemoveIn {
                    id,
                    kind: Some(kind),
                },
                &Quiet,
            )
            .map_err(err)?
            .text()
        };
        return Ok(ToolOutput::text(text));
    }
    match action {
        Some("hidden") => {
            if kind != "quickstart" {
                return Err(format!(
                    "library_curate: `hidden` is for Quick Start entries; '{id}' names a {kind} (patterns are hidden from the Quick Start through their entry, find:<id>; or set their state to rejected)"
                ));
            }
            let hidden = match &a["hidden"] {
                Value::Bool(b) => *b,
                Value::String(s) if s == "true" || s == "false" => s == "true",
                v => {
                    return Err(format!(
                        "library_curate: `hidden` is true or false, not {v}"
                    ));
                }
            };
            let r = QuickstartUpdate::run(ctx(), HideIn { id, hidden }, &Quiet).map_err(err)?;
            Ok(ToolOutput::text(r.text()))
        }
        Some(_) => {
            if kind == "quickstart" {
                return Err(
                    "library_curate: a Quick Start entry takes `hidden` or `remove`; curate the pattern or ant run it was made from with `changes`"
                        .into(),
                );
            }
            let mut ch = a["changes"].clone();
            if let Value::String(s) = &ch
                && let Ok(v) = serde_json::from_str::<Value>(s)
            {
                ch = v;
            }
            let Value::Object(mut o) = ch else {
                return Err("library_curate: `changes` is an object".into());
            };
            o.insert("id".into(), json!(id));
            o.insert("kind".into(), json!(kind));
            let input: UpdateIn = schema::read(
                &input_schema::<UpdateIn>(),
                "library_curate changes",
                Value::Object(o),
            )?;
            let r = LibraryUpdate::run(ctx(), input, &Quiet).map_err(err)?;
            Ok(ToolOutput::json(&match (&r.pattern, &r.ant) {
                (Some(p), _) => summary(p),
                (_, Some(x)) => ant_json(x),
                _ => Value::Null,
            }))
        }
        None => Err(
            "library_curate: with `id`, give `changes`, `hidden` (Quick Start entries) or `remove`"
                .into(),
        ),
    }
}
