//! MCP resources: what a client can read without calling a tool.
//!
//! | URI | What |
//! | --- | --- |
//! | `jetcarp://docs/{page}` | a page of the JETCARP book (Markdown), e.g. `jetcarp://docs/guides/searching` |
//! | `jetcarp://reference/rules`, `tilings`, `palettes`, `script-api` | the rule names and notation, tiling ids, palettes, and the Rhai function reference |
//! | `jetcarp://schema/scene.json`, `pack.json` | JSON Schemas of the scene file and the pack manifest |
//! | `jetcarp://job/{id}` | one library job's status (JSON) |
//! | `jetcarp://pattern/{id}` | one library pattern's record (JSON) |
//! | `jetcarp://quickstart/{handle}` | one Quick Start entry with its scene (JSON) |

use crate::tools::{ctx, err};
use jetcarp_api::op::{self, NoInput, Op, Quiet, Report};
use jetcarp_api::ops::jobs::{JobsStatus, StatusIn, StatusOut};
use jetcarp_api::ops::library::{GetIn, LibraryGet};
use jetcarp_api::ops::quickstart::{IdIn, QuickstartGet};
use jetcarp_api::ops::reference::ReferencePalettes;
use serde_json::{Value, json};

pub const DOCS: &str = "jetcarp://docs/";
const REFERENCE: &[(&str, &str, &str, &str)] = &[
    (
        "rules",
        "Rules and rule notation",
        "The named rules (name, notation, family, one line each) and how rule notation works.",
        "text/plain",
    ),
    (
        "tilings",
        "Tilings",
        "The tiling families with their neighbour counts, and the catalogue's tilings in summary.",
        "text/plain",
    ),
    (
        "palettes",
        "Palettes",
        "The built-in palettes with their background, stage and ramp colours.",
        "text/plain",
    ),
    (
        "script-api",
        "Scripting (Rhai) API",
        "Every function a JETCARP script (run_script) can call.",
        "text/markdown",
    ),
];
const SCHEMAS: &[(&str, &str, &str)] = &[
    (
        "scene.json",
        "Scene file JSON Schema",
        "The JSON Schema of a JETCARP scene file (world_status {\"scene\": true} returns one).",
    ),
    (
        "pack.json",
        "Pack manifest JSON Schema",
        "The JSON Schema of a pack version's manifest (pack.json in an export).",
    ),
];

/// `resources/list`: the book's pages, the references and the schemas.
pub fn list() -> Value {
    let mut v: Vec<Value> = REFERENCE
        .iter()
        .map(|(n, title, d, mime)| {
            json!({"uri": format!("jetcarp://reference/{n}"), "name": format!("reference/{n}"),
                   "title": title, "description": d, "mimeType": mime})
        })
        .collect();
    for (n, title, d) in SCHEMAS {
        if crate::docs::schema(n).is_some() {
            v.push(
                json!({"uri": format!("jetcarp://schema/{n}"), "name": format!("schema/{n}"),
                          "title": title, "description": d, "mimeType": "application/schema+json"}),
            );
        }
    }
    for p in crate::docs::pages() {
        v.push(json!({
            "uri": format!("{DOCS}{}", p.path),
            "name": format!("docs/{}", p.path),
            "title": p.title,
            "description": format!("The JETCARP book: {}", p.title),
            "mimeType": "text/markdown",
        }));
    }
    json!({"resources": v})
}

/// `resources/templates/list`.
pub fn templates() -> Value {
    json!({"resourceTemplates": [
        {"uriTemplate": "jetcarp://docs/{page}", "name": "docs", "title": "A page of the JETCARP book",
         "description": "A page by its path without .md, e.g. jetcarp://docs/guides/searching (resources/list lists every page).",
         "mimeType": "text/markdown"},
        {"uriTemplate": "jetcarp://job/{id}", "name": "job", "title": "A library job",
         "description": "One search job's status and progress (as job_status reports it), by its id.",
         "mimeType": "application/json"},
        {"uriTemplate": "jetcarp://pattern/{id}", "name": "pattern", "title": "A library pattern",
         "description": "One pattern's record (rules, range, sightings, the cells of its phases), by id, unique prefix, or name.",
         "mimeType": "application/json"},
        {"uriTemplate": "jetcarp://quickstart/{handle}", "name": "quickstart", "title": "A Quick Start entry",
         "description": "One Quick Start entry with its scene, by handle (life, find:<id>, atlas:<...>).",
         "mimeType": "application/json"},
    ]})
}

fn contents(uri: &str, mime: &str, text: String) -> Value {
    json!({"contents": [{"uri": uri, "mimeType": mime, "text": text}]})
}

/// `resources/read`: the resource's text, or why there is none.
pub fn read(uri: &str) -> Result<Value, String> {
    let Some(rest) = uri.strip_prefix("jetcarp://") else {
        return Err(format!("not a JETCARP resource: {uri}"));
    };
    let (kind, key) = rest.split_once('/').unwrap_or((rest, ""));
    let key = percent_decode(key);
    match kind {
        "docs" => {
            let page = key.trim_end_matches(".md").trim_matches('/');
            crate::docs::page(page)
                .map(|t| contents(uri, "text/markdown", t.to_string()))
                .ok_or_else(|| format!("no book page '{page}' (resources/list lists them)"))
        }
        "reference" => {
            let text = match key.as_str() {
                "rules" => crate::help_tools::rules_text(None)?,
                "tilings" => crate::help_tools::tilings_text(None, None)?,
                "palettes" => ReferencePalettes::run(ctx(), NoInput {}, &Quiet)
                    .map_err(err)?
                    .text(),
                "script-api" => jetcarp_script::help_text().to_string(),
                k => {
                    return Err(format!(
                        "no reference '{k}': rules, tilings, palettes, or script-api"
                    ));
                }
            };
            let mime = REFERENCE
                .iter()
                .find(|r| r.0 == key)
                .map(|r| r.3)
                .unwrap_or("text/plain");
            Ok(contents(uri, mime, text))
        }
        "schema" => crate::docs::schema(&key)
            .map(|t| contents(uri, "application/schema+json", t.to_string()))
            .ok_or_else(|| format!("no schema '{key}': scene.json or pack.json")),
        "job" => {
            let job: i64 = key
                .parse()
                .map_err(|_| format!("a job id is a number, not '{key}'"))?;
            let a = StatusIn {
                job: Some(job),
                finds: Some(10),
                ..Default::default()
            };
            let StatusOut::One(v) = JobsStatus::run(ctx(), a, &Quiet).map_err(err)? else {
                return Err(format!("no job {job}"));
            };
            let mut j = op::to_value(&*v);
            if let Some(o) = j.as_object_mut() {
                o.remove("spec");
            }
            Ok(contents(
                uri,
                "application/json",
                serde_json::to_string_pretty(&j).unwrap_or_default(),
            ))
        }
        "pattern" => {
            let g = GetIn {
                id: key.clone(),
                kind: Some("pattern".into()),
                phases: Some(1),
                bare: false,
            };
            let v = LibraryGet::run(ctx(), g, &Quiet).map_err(err)?;
            Ok(contents(uri, "application/json", op::to_pretty(&v)))
        }
        "quickstart" => {
            let q = QuickstartGet::run(ctx(), IdIn { id: key }, &Quiet).map_err(err)?;
            Ok(contents(uri, "application/json", op::to_pretty(&q)))
        }
        k => Err(format!(
            "no resource kind '{k}': docs, reference, schema, job, pattern, quickstart"
        )),
    }
}

/// `%3A` and the like in a URI's path, as a client may send a handle (`find%3A...`).
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn percent_decoding() {
        assert_eq!(super::percent_decode("find%3Aab12"), "find:ab12");
        assert_eq!(super::percent_decode("plain"), "plain");
        assert_eq!(super::percent_decode("a%"), "a%");
    }
}
