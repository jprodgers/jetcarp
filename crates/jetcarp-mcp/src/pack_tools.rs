//! `pack`: packs and the website export: a version's summary (`pack.info`), checking
//! (`pack.check`), importing (`pack.import`), publishing (`pack.publish`), exporting
//! (`pack.export`), and the website (`site.export`), within the server's folder rules.
//!
//! Two things a person does are not here: making the publisher's key (`jetcarp pack key`; publishing
//! signs with the existing one and says how to make it when there is none), and recording a
//! rater's consent to publish their ratings (`jetcarp pack rater`).

use crate::schema::{self, object};
use crate::tools::{Call, Res, Tool, ToolOutput, ctx, err};
use jetcarp_api::op::{self, Op, Report, input_schema};
use jetcarp_api::ops::media::{SiteExport, SiteIn};
use jetcarp_api::ops::pack::{
    ExportIn, ImportIn, InfoIn, PackCheck, PackExport, PackImport, PackInfo, PackPublish,
    PublishIn, publisher_key,
};
use serde_json::{Map, Value, json};

pub(crate) const ACTIONS: &[&str] = &["info", "check", "import", "publish", "export", "site"];

pub(crate) fn tools() -> Vec<Tool> {
    vec![Tool {
        name: "pack",
        title: "Packs and the website export",
        read_only: false,
        destructive: false,
        description: || {
            "Packs: signed, content-addressed folders of library records (Parquet tables and JSON) that anyone can read, check and import. Give one action as a key holding its options: `info` (a version's summary), `check` (every hash, id and signature, importing nothing), `import` (into the library, records merged by id), `publish` (the library, or a selection, as a new signed version of a feed in a store), `export` (one version laid out with readable file names, for a single download) or `site` (the website export). Paths must be inside the output folder (relative paths start there), the library's folder, or the packs that came with JETCARP. Publishing signs with the publisher key in JETCARP's settings, which only the user makes (`jetcarp pack key`). Examples: {\"info\": {\"path\": \"packs/starter\"}}, {\"import\": {\"path\": \"packs/starter\"}}, {\"publish\": {\"feed\": \"my-finds\", \"to\": \"packs/my-finds\", \"query\": {\"class\": \"spaceship\"}}}, {\"export\": {\"from\": \"packs/my-finds\", \"to\": \"my-finds-1\"}}.".into()
        },
        schema: || {
            let publish = schema::without(input_schema::<PublishIn>(), &["key"]);
            let mut export = schema::op(
                input_schema::<ExportIn>(),
                Some(
                    "Lay one version out with readable file names. It comes from a store (`from`), or from publishing the library first (`publish`).",
                ),
            );
            export["properties"]["publish"] = schema::without(
                schema::op(
                    publish.clone(),
                    Some(
                        "Without `from`: what to publish first (the publish options; its `to` is ignored).",
                    ),
                ),
                &["to"],
            );
            let mut p = Map::new();
            p.insert(
                "info".into(),
                schema::op(input_schema::<InfoIn>(), Some("A version's summary. It gives the title, publisher and signature, tables and searches, without reading every file.")),
            );
            p.insert(
                "check".into(),
                schema::op(
                    input_schema::<ImportIn>(),
                    Some("Check a store or export without importing it. Every hash, id and signature is checked."),
                ),
            );
            p.insert(
                "import".into(),
                schema::op(
                    input_schema::<ImportIn>(),
                    Some("Import a store or export into the library. Records are merged by id, and a sample is verified again."),
                ),
            );
            p.insert(
                "publish".into(),
                schema::op(
                    publish,
                    Some("Publish the library (or a selection) as a new signed version of a feed. A selection is `ids` or `query`."),
                ),
            );
            p.insert("export".into(), export);
            p.insert(
                "site".into(),
                schema::op(
                    input_schema::<SiteIn>(),
                    Some("The website export into the folder `to`. It is the public part of the library as JSON and media, with manifest.json and a viewer page."),
                ),
            );
            object(p, &[])
        },
        run: pack,
    }]
}

/// Publishing signs with the user's own key, which only the user makes.
fn key_or_refuse() -> Result<(), String> {
    let (path, made) = publisher_key().map_err(err)?;
    if made {
        return Ok(());
    }
    Err(format!(
        "pack: there is no publisher key yet ({} is missing). Publishing signs the pack with the user's own key, which these tools never make: ask the user to run `jetcarp pack key` at a command line (with --name, --orcid or --github to be named in packs), then try again.",
        path.display()
    ))
}

fn object_of(a: &Value, key: &str) -> Result<Value, String> {
    match &a[key] {
        Value::Object(_) => Ok(a[key].clone()),
        Value::String(s) => {
            serde_json::from_str(s).map_err(|e| format!("pack {key}: not a JSON object ({e})"))
        }
        v => Err(format!("pack: `{key}` holds an object of options, not {v}")),
    }
}

fn no_key(v: &Value, what: &str) -> Result<(), String> {
    if v.get("key").is_some() {
        return Err(format!(
            "pack {what}: `key` is not taken here; publishing signs with the publisher key in JETCARP's settings"
        ));
    }
    Ok(())
}

fn pack(c: &mut Call) -> Res {
    let a = c.args;
    let which = schema::one_of("pack", a, ACTIONS)?.ok_or(
        "pack: give one of `info`, `check`, `import`, `publish`, `export`, or `site`, holding its options",
    )?;
    let v = object_of(a, which)?;
    let p = c.callbacks();
    let cx = ctx();
    let text = match which {
        "info" => {
            let i: InfoIn = schema::read(&input_schema::<InfoIn>(), "pack info", v)?;
            PackInfo::run(cx, i, &p).map_err(err)?.text()
        }
        "check" => {
            let i: ImportIn = schema::read(&input_schema::<ImportIn>(), "pack check", v)?;
            let out = PackCheck::run(cx, i, &p).map_err(err)?;
            format!("{}{}", out.text(), out.notes())
        }
        "import" => {
            let i: ImportIn = schema::read(&input_schema::<ImportIn>(), "pack import", v)?;
            let out = PackImport::run(cx, i, &p).map_err(err)?;
            let mut t = format!("{}{}", out.text(), out.notes());
            if let Err(e) = out.check() {
                t.push_str(&format!("Warning: {e}\n"));
            }
            t
        }
        "publish" => {
            no_key(&v, "publish")?;
            key_or_refuse()?;
            let i: PublishIn = schema::read(&input_schema::<PublishIn>(), "pack publish", v)?;
            let out = PackPublish::run(cx, i, &p).map_err(err)?;
            format!("{}{}", out.text(), out.notes())
        }
        "export" => {
            if let Some(pb) = v.get("publish") {
                no_key(pb, "export publish")?;
            }
            if v.get("from").is_none_or(Value::is_null) {
                key_or_refuse()?;
            }
            let mut v = v;
            if let Some(pb) = v.get_mut("publish").and_then(Value::as_object_mut) {
                pb.entry("to").or_insert(json!(""));
            }
            let i: ExportIn = schema::read(&input_schema::<ExportIn>(), "pack export", v)?;
            PackExport::run(cx, i, &p).map_err(err)?.text()
        }
        _ => {
            let i: SiteIn = schema::read(&input_schema::<SiteIn>(), "pack site", v)?;
            let out = SiteExport::run(cx, i, &p).map_err(err)?;
            return Ok(ToolOutput::json(&op::to_value(&out)));
        }
    };
    Ok(ToolOutput::text(text))
}
