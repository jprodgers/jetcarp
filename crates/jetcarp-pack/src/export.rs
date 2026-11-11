//! An export: one version laid out with readable names, for a single download (a folder, or
//! a tar of it).
//!
//! ```text
//! pack.json                                   the manifest's exact bytes
//! index/<hex>.json                            every index object of the version
//! history/<hex>.json                          the earlier manifests the store holds (parents...)
//! tables/<table>/<key>=<value>/.../part-NNNNNN.parquet
//! tilings/reference/<family>.parquet          aperiodic reference patches
//! searches/<uuid>/runs/<segment>.parquet      search segments
//! searches/<uuid>/scores-<n>/<segment>.parquet
//! media/<target kind>/<target id>/<kind>[-seed].<ext>
//! sigs/<signer>.sig                           the manifest's signatures
//! keys/<signer>.json                          the signers' key statements
//! views.sql                                   DuckDB views over the files
//! LICENSE, README.md, CITATION.cff            made from the manifest
//! ```
//!
//! Parts are numbered from 1 within each partition folder, in the order the table's index
//! lists them (children in order). Every file is checked against the hash its index or table
//! row names, exactly as in a store.

use crate::docs::Manifest;
use crate::publish::media_ext;
use crate::read::{self, Source, Version};
use crate::store::{read_limited, write_atomic};
use crate::tables::{MediaPackRow, TilingRow};
use std::path::Path;

/// What an export wrote.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Exported {
    pub to: String,
    pub manifest: String,
    pub files: u64,
    pub bytes: u64,
    /// Files hard-linked to the store's objects, and files copied.
    pub linked: u64,
    pub copied: u64,
}

/// How an export places a store's file: `std::fs::hard_link` by default.
pub type LinkFn = fn(&Path, &Path) -> std::io::Result<()>;

/// Place a store's file in an export: a hard link (no copy), or a copy when linking fails
/// (another file system, `EXDEV`; a file system without hard links). The placed file must have
/// the source's size. Returns its size and whether it was linked.
pub fn link_or_copy_with(from: &Path, to: &Path, link: LinkFn) -> Result<(u64, bool), String> {
    if let Some(d) = to.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let _ = std::fs::remove_file(to);
    let linked = match link(from, to) {
        Ok(()) => true,
        Err(_) => {
            let _ = std::fs::remove_file(to);
            std::fs::copy(from, to).map_err(|e| format!("{}: {e}", to.display()))?;
            false
        }
    };
    let want = std::fs::metadata(from)
        .map_err(|e| format!("{}: {e}", from.display()))?
        .len();
    let got = std::fs::metadata(to)
        .map_err(|e| format!("{}: {e}", to.display()))?
        .len();
    if got != want {
        return Err(format!("{}: {got} bytes placed of {want}", to.display()));
    }
    Ok((got, linked))
}

/// Export a version of a store into the folder `to` (made; files in it are replaced), its
/// tables, segments, media and reference patches hard-linked to the store's objects where the
/// file system allows, else copied.
pub fn export(src: &Source, v: &Version, to: &Path) -> Result<Exported, String> {
    export_with(src, v, to, |a, b| std::fs::hard_link(a, b))
}

/// [`export`] placing files with `link` (a test forces the copy).
pub fn export_with(
    src: &Source,
    v: &Version,
    to: &Path,
    link_fn: LinkFn,
) -> Result<Exported, String> {
    let Source::Store(store) = src else {
        return Err("export from a store".into());
    };
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let mut out = Exported {
        to: to.display().to_string(),
        manifest: v.hash.clone(),
        ..Default::default()
    };
    let mut put = |rel: &str, bytes: &[u8]| -> Result<(), String> {
        write_atomic(&to.join(rel), bytes)?;
        out.files += 1;
        out.bytes += bytes.len() as u64;
        Ok(())
    };
    put("pack.json", &v.bytes)?;
    for (h, b) in &v.indexes {
        put(&format!("index/{}.json", crate::hash::hex_of(h)?), b)?;
    }
    // Earlier manifests the store holds, so a reader of the export can order annotations.
    let mut queue = v.manifest.parents.clone();
    let mut seen = std::collections::HashSet::new();
    while let Some(h) = queue.pop() {
        if !seen.insert(h.clone()) || seen.len() > 10_000 {
            continue;
        }
        if let Ok(b) = src.doc(&h) {
            let m: Manifest = crate::json::parse_as(&b, "manifest", true)?;
            put(&format!("history/{}.json", crate::hash::hex_of(&h)?), &b)?;
            queue.extend(m.parents);
        }
    }
    let mut files = 0u64;
    let mut bytes = 0u64;
    let (mut linked, mut copied) = (0u64, 0u64);
    let mut link = |from: &Path, rel: &str| -> Result<(), String> {
        let (n, l) = link_or_copy_with(from, &to.join(rel), link_fn)?;
        bytes += n;
        files += 1;
        if l {
            linked += 1;
        } else {
            copied += 1;
        }
        Ok(())
    };
    for (name, tp) in &v.tables {
        let mut n: std::collections::HashMap<_, usize> = Default::default();
        for (p, path) in &tp.parts {
            let k = n.entry(p.partition.clone()).or_default();
            *k += 1;
            link(path, &read::export_path(name, &p.partition, *k))?;
        }
    }
    for (uuid, sp) in &v.searches {
        for (p, f) in sp.index.parts.iter().zip(&sp.run_files) {
            link(f, &read::export_run_path(uuid, &p.segment))?;
        }
        for (s, fs) in sp.index.scores.iter().zip(&sp.score_files) {
            for (p, f) in s.parts.iter().zip(fs) {
                link(f, &read::export_score_path(uuid, s.version, &p.segment))?;
            }
        }
    }
    let mut media = vec![];
    src.table_rows(v, "media", |_, rows| {
        for r in rows {
            media.push(serde_json::from_value::<MediaPackRow>(r).map_err(|e| e.to_string())?);
        }
        Ok(())
    })?;
    for m in &media {
        let ext = media_ext(&m.media_type)?;
        let rel = read::export_media_path(&m.target, &m.kind, m.from_seed, ext)
            .ok_or("a media row with an unsafe target")?;
        link(&store.object_path(&m.object, ext)?, &rel)?;
    }
    let mut refs = vec![];
    src.table_rows(v, "tilings", |_, rows| {
        for r in rows {
            let t: TilingRow = serde_json::from_value(r).map_err(|e| e.to_string())?;
            if let Some(h) = t.reference {
                refs.push((t.family, h));
            }
        }
        Ok(())
    })?;
    for (family, h) in &refs {
        let rel = read::export_reference_path(family).ok_or("unsafe family name")?;
        link(&store.object_path(h, "parquet")?, &rel)?;
    }
    for (stem, text) in store.signatures(&v.hash)? {
        put(&format!("sigs/{stem}.sig"), text.as_bytes())?;
    }
    for fp in v.signed.signers.iter().chain([&v.manifest.publisher]) {
        if let Ok(p) = store.key_path(fp)
            && p.is_file()
        {
            let b = read_limited(&p, crate::json::MAX_DOC)?;
            put(&format!("keys/{}.json", crate::sign::signer_name(fp)), &b)?;
        }
    }
    put("views.sql", views_sql(v, Some("export")).as_bytes())?;
    put("README.md", readme(v).as_bytes())?;
    put("CITATION.cff", citation(&v.manifest, &v.hash).as_bytes())?;
    put("LICENSE", licence(&v.manifest).as_bytes())?;
    out.files += files;
    out.bytes += bytes;
    out.linked = linked;
    out.copied = copied;
    Ok(out)
}

fn sql_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// DuckDB views over a version's tables: for a store (`layout` `None`), the objects by their
/// paths relative to the store's folder; for an export (`Some("export")`), its readable files.
/// Run DuckDB from that folder, or replace the paths' start with the store's URL.
pub fn views_sql(v: &Version, layout: Option<&str>) -> String {
    let mut s = format!(
        "-- DuckDB views over JETCARP pack version {} (feed {}).\n\
         -- Not part of the format: run `duckdb -init views.sql` in this folder, or replace\n\
         -- the paths with this folder's URL to query a pack in place over HTTPS.\n",
        v.hash, v.manifest.feed
    );
    for (name, tp) in &v.tables {
        if tp.parts.is_empty() {
            continue;
        }
        let mut n: std::collections::HashMap<_, usize> = Default::default();
        let files: Vec<String> = tp
            .parts
            .iter()
            .map(|(p, _)| {
                let k = n.entry(p.partition.clone()).or_default();
                *k += 1;
                match layout {
                    Some(_) => read::export_path(name, &p.partition, *k),
                    None => crate::store::Store::object_rel(
                        crate::hash::hex_of(&p.object).unwrap_or_default(),
                        "parquet",
                    ),
                }
            })
            .map(|f| sql_str(&f))
            .collect();
        s.push_str(&format!(
            "CREATE OR REPLACE VIEW {name} AS SELECT * FROM read_parquet([{}], union_by_name = true);\n",
            files.join(", ")
        ));
    }
    for (uuid, sp) in &v.searches {
        let files: Vec<String> = sp
            .index
            .parts
            .iter()
            .map(|p| match layout {
                Some(_) => read::export_run_path(uuid, &p.segment),
                None => crate::store::Store::object_rel(
                    crate::hash::hex_of(&p.object).unwrap_or_default(),
                    "parquet",
                ),
            })
            .map(|f| sql_str(&f))
            .collect();
        if files.is_empty() {
            continue;
        }
        let view = format!("runs_{}", uuid.replace('-', "_"));
        s.push_str(&format!(
            "CREATE OR REPLACE VIEW {view} AS SELECT * FROM read_parquet([{}]);\n",
            files.join(", ")
        ));
    }
    s
}

fn readme(v: &Version) -> String {
    let m = &v.manifest;
    let creators: Vec<String> = m
        .creators
        .iter()
        .map(|c| {
            if c.orcid.is_empty() {
                c.name.clone()
            } else {
                format!("{} (ORCID {})", c.name, c.orcid)
            }
        })
        .collect();
    let tables: Vec<String> = v
        .tables
        .iter()
        .map(|(n, tp)| {
            format!(
                "- `{n}`: {} rows in {} parts",
                tp.parts.iter().map(|p| p.0.rows).sum::<u64>(),
                tp.parts.len()
            )
        })
        .collect();
    format!(
        "# {title}\n\n{desc}\n\n\
         - Feed: `{feed}`, version {label}, made {created}\n\
         - Version id (manifest hash): `{hash}`\n\
         - Publisher key: `{publisher}`\n\
         - Licence: {licence}\n\
         - Creators: {creators}\n\n\
         This is an export of a JETCARP pack (format {format} {version}). Every table is a Parquet\n\
         file under `tables/`, and `pack.json` is the manifest: its SHA-256 is the version id\n\
         above, and the indexes in `index/` give the SHA-256 of every file. The format is\n\
         described in the JETCARP book's *Pack format* reference.\n\n\
         ## Tables\n\n{tables}\n",
        title = if m.title.is_empty() {
            &m.feed
        } else {
            &m.title
        },
        desc = m.description,
        feed = m.feed,
        label = if m.version_label.is_empty() {
            "-"
        } else {
            &m.version_label
        },
        created = m.created,
        hash = v.hash,
        publisher = m.publisher,
        licence = m.licence,
        creators = if creators.is_empty() {
            "-".into()
        } else {
            creators.join(", ")
        },
        format = m.format,
        version = m.version,
        tables = tables.join("\n"),
    )
}

fn yaml_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn citation(m: &Manifest, hash: &str) -> String {
    let mut s = String::from(
        "cff-version: 1.2.0\nmessage: \"If you use this data, please cite it as below.\"\n",
    );
    s.push_str(&format!(
        "title: {}\n",
        yaml_str(if m.title.is_empty() {
            &m.feed
        } else {
            &m.title
        })
    ));
    s.push_str("type: dataset\n");
    if !m.version_label.is_empty() {
        s.push_str(&format!("version: {}\n", yaml_str(&m.version_label)));
    }
    if let Some(d) = m.created.get(..10) {
        s.push_str(&format!("date-released: {}\n", yaml_str(d)));
    }
    if !m.licence.is_empty() {
        s.push_str(&format!("license: {}\n", yaml_str(&m.licence)));
    }
    s.push_str("authors:\n");
    if m.creators.is_empty() {
        s.push_str(&format!("  - name: {}\n", yaml_str(&m.publisher)));
    }
    for c in &m.creators {
        match c.name.rsplit_once(' ') {
            Some((given, family)) => s.push_str(&format!(
                "  - given-names: {}\n    family-names: {}\n",
                yaml_str(given),
                yaml_str(family)
            )),
            None => s.push_str(&format!("  - name: {}\n", yaml_str(&c.name))),
        }
        if !c.orcid.is_empty() {
            s.push_str(&format!(
                "    orcid: {}\n",
                yaml_str(&format!("https://orcid.org/{}", c.orcid))
            ));
        }
    }
    s.push_str("identifiers:\n");
    s.push_str(&format!(
        "  - type: other\n    value: {}\n    description: \"The pack version's id (manifest hash)\"\n",
        yaml_str(hash)
    ));
    if let Some(doi) = m
        .doi
        .strip_prefix("doi:")
        .or(Some(&m.doi))
        .filter(|d| !d.is_empty())
    {
        s.push_str(&format!("  - type: doi\n    value: {}\n", yaml_str(doi)));
    }
    s
}

fn licence(m: &Manifest) -> String {
    match m.licence.as_str() {
        "CC0-1.0" => format!(
            "The data in this pack (feed {}) is dedicated to the public domain under the\n\
             Creative Commons CC0 1.0 Universal dedication (SPDX: CC0-1.0):\n\
             https://creativecommons.org/publicdomain/zero/1.0/legalcode\n",
            m.feed
        ),
        "CC-BY-4.0" => format!(
            "The data in this pack (feed {}) is licensed under the Creative Commons\n\
             Attribution 4.0 International licence (SPDX: CC-BY-4.0):\n\
             https://creativecommons.org/licenses/by/4.0/legalcode\n",
            m.feed
        ),
        l => format!(
            "The data in this pack (feed {}) is licensed under {l} (an SPDX identifier):\n\
             https://spdx.org/licenses/{l}.html\n",
            m.feed
        ),
    }
}
