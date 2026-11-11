//! Hostile packs: oversized and deeply nested documents, decompression and decoding bombs,
//! names that would leave the store, altered objects, unknown versions and features, bad ids
//! and signatures. Each must be refused with a message, never a crash or a hang.

mod common;

use common::*;
use jetcarp_pack::docs::{Index, Manifest};
use jetcarp_pack::import::{self, VerifyMode};
use jetcarp_pack::read::Source;
use jetcarp_pack::store::Store;
use jetcarp_pack::{json, parquet_io, tables};
use jetcarp_store::Library;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// A published store of the small library, and its manifest hash.
fn store(name: &str) -> (PathBuf, PathBuf, String) {
    let dir = temp(name);
    let lib = small_library(&dir);
    let p = publisher(9, "Test Publisher");
    let mut b = book(&[]);
    let out = run(&lib, &p, &mut b, &options("hostile", &dir.join("store")));
    (dir.clone(), dir.join("store"), out.manifest)
}

fn manifest(s: &Path, h: &str) -> Manifest {
    let st = Store::open(s).unwrap();
    json::parse_as(&st.get(h, "json", 1 << 24).unwrap(), "m", true).unwrap()
}

/// Store a changed manifest; its hash.
fn put_manifest(s: &Path, m: &Manifest) -> String {
    Store::open(s)
        .unwrap()
        .put(&json::canonical_of(m).unwrap(), "json")
        .unwrap()
}

fn refused(r: Result<impl std::fmt::Debug, String>, want: &str) {
    match r {
        Ok(v) => panic!("accepted, wanted a refusal ({want}): {v:?}"),
        Err(e) => assert!(
            e.contains(want),
            "refused for another reason: {e} (wanted {want})"
        ),
    }
}

fn import_from(s: &Path, h: &str, lib_dir: &Path) -> Result<import::Imported, String> {
    let lib = Library::open(lib_dir);
    let o = import::Options {
        path: s.to_path_buf(),
        manifest: Some(h.to_string()),
        allow_unsigned: true,
        verify: VerifyMode::None,
        ..Default::default()
    };
    import::import(&lib, &o, &|_| {})
}

#[test]
fn oversized_and_deep_documents_are_refused() {
    let (dir, s, _) = store("big-deep");
    let st = Store::open(&s).unwrap();
    // A 17 MiB "manifest".
    let big = format!("{{\"x\":\"{}\"}}", "a".repeat(17 << 20));
    let h = st.put(big.as_bytes(), "json").unwrap();
    refused(
        Source::open(&s).unwrap().version(&h).map(|_| ()),
        "larger than the limit",
    );
    // Nested 10,000 deep: refused before parsing (no stack overflow).
    let deep = "[".repeat(10_000) + &"]".repeat(10_000);
    let h = st.put(deep.as_bytes(), "json").unwrap();
    refused(Source::open(&s).unwrap().version(&h).map(|_| ()), "nested");
    // A decimal number in a document.
    let h = st
        .put(br#"{"format":"jetcarp-pack","n":0.5}"#, "json")
        .unwrap();
    refused(
        Source::open(&s).unwrap().version(&h).map(|_| ()),
        "integers",
    );
    let _ = dir;
}

#[test]
fn unknown_versions_features_and_altered_objects_are_refused() {
    let (dir, s, h) = store("versions");
    let src = || Source::open(&s).unwrap();
    let mut m = manifest(&s, &h);
    m.requires = vec!["zebra-stripes".into()];
    refused(
        src().version(&put_manifest(&s, &m)).map(|_| ()),
        "requires 'zebra-stripes'",
    );
    let mut m = manifest(&s, &h);
    m.version = "2.0".into();
    refused(
        src().version(&put_manifest(&s, &m)).map(|_| ()),
        "major version",
    );
    // A higher minor is read.
    let mut m = manifest(&s, &h);
    m.version = "1.7".into();
    assert!(src().version(&put_manifest(&s, &m)).is_ok());
    // A manifest that is not canonical is refused.
    let st = Store::open(&s).unwrap();
    let pretty = serde_json::to_vec_pretty(&manifest(&s, &h)).unwrap();
    refused(
        src().version(&st.put(&pretty, "json").unwrap()).map(|_| ()),
        "canonical",
    );
    // An altered part: refused on import, before anything is stored.
    let v = src().version(&h).unwrap();
    let (part, path) = &v.tables["patterns"].parts[0];
    let mut b = std::fs::read(path).unwrap();
    let n = b.len();
    b[n / 2] ^= 0xff;
    std::fs::write(path, &b).unwrap();
    refused(import_from(&s, &h, &dir.join("lib-a")), "altered");
    assert!(
        !Library::open(dir.join("lib-a"))
            .contains(&part.rows.to_string())
            .unwrap()
    );
    assert_eq!(Library::open(dir.join("lib-a")).list().unwrap().len(), 0);
}

#[test]
fn names_that_would_leave_the_store_are_refused() {
    let (_dir, s, h) = store("names");
    let st = Store::open(&s).unwrap();
    let src = Source::open(&s).unwrap();
    // A partition value that is a path.
    let m = manifest(&s, &h);
    let ih = &m.tables["patterns"];
    let mut ix: Index = json::parse_as(&st.get(ih, "json", 1 << 24).unwrap(), "i", true).unwrap();
    ix.parts[0]
        .partition
        .insert("tiling_group".into(), "../../../etc".into());
    let mut m2 = m.clone();
    m2.tables.insert(
        "patterns".into(),
        st.put(&json::canonical_of(&ix).unwrap(), "json").unwrap(),
    );
    refused(src.version(&put_manifest(&s, &m2)).map(|_| ()), "safe name");
    // A search uuid that is a path.
    let mut m3 = m.clone();
    m3.searches
        .insert("../../x".into(), m.tables["patterns"].clone());
    refused(
        src.version(&put_manifest(&s, &m3)).map(|_| ()),
        "not a uuid",
    );
    // A hash that is not one.
    let mut m4 = m.clone();
    m4.tables
        .insert("rules".into(), "sha256:../../../etc/passwd".into());
    refused(
        src.version(&put_manifest(&s, &m4)).map(|_| ()),
        "malformed hash",
    );
    let mut m5 = m.clone();
    m5.tables
        .insert("rules".into(), format!("md5:{}", "0".repeat(32)));
    refused(
        src.version(&put_manifest(&s, &m5)).map(|_| ()),
        "unknown hash algorithm",
    );
    // A feed name that is a path.
    assert!(st.ref_path("../x").is_err());
    assert!(src.read_ref("a/b").is_err());
    // Media and reference paths come only from checked names.
    assert!(
        jetcarp_pack::read::export_media_path("pattern:../../x", "gif", false, "gif").is_none()
    );
    assert!(jetcarp_pack::read::export_media_path("pattern:abc", "gif", false, "exe").is_none());
    assert!(jetcarp_pack::read::export_reference_path("../hat").is_none());
}

#[test]
fn too_many_parts_and_deep_indexes_are_refused() {
    let (_dir, s, h) = store("indexes");
    let st = Store::open(&s).unwrap();
    let src = Source::open(&s).unwrap();
    let m = manifest(&s, &h);
    let ih = &m.tables["rules"];
    let ix: Index = json::parse_as(&st.get(ih, "json", 1 << 24).unwrap(), "i", true).unwrap();
    let mut many = ix.clone();
    many.parts = vec![ix.parts[0].clone(); 10_001];
    let mut m2 = m.clone();
    m2.tables.insert(
        "rules".into(),
        st.put(&json::canonical_of(&many).unwrap(), "json").unwrap(),
    );
    refused(
        src.version(&put_manifest(&s, &m2)).map(|_| ()),
        "more than 10000 parts",
    );
    // A chain of child indexes nine deep.
    let mut child = ih.clone();
    for _ in 0..9 {
        let mut up = ix.clone();
        up.parts = vec![];
        up.children = vec![child];
        child = st.put(&json::canonical_of(&up).unwrap(), "json").unwrap();
    }
    let mut m3 = m.clone();
    m3.tables.insert("rules".into(), child);
    refused(src.version(&put_manifest(&s, &m3)).map(|_| ()), "deep");
    // A child listed twice is read once, its parts counted once.
    let mut twice = ix.clone();
    twice.parts = vec![];
    twice.children = vec![ih.clone(), ih.clone()];
    let mut m4 = m.clone();
    m4.tables.insert(
        "rules".into(),
        st.put(&json::canonical_of(&twice).unwrap(), "json")
            .unwrap(),
    );
    let v = src.version(&put_manifest(&s, &m4)).unwrap();
    assert_eq!(v.tables["rules"].parts.len(), ix.parts.len());
}

/// Rows of the annotations table whose values add up to `n` MiB.
fn heavy_rows(n: usize, distinct: bool) -> Vec<Value> {
    (0..n)
        .map(|i| {
            let fill = if distinct {
                // Distinct but compressible: a counter and a long run.
                format!("{i:08}{}", "a".repeat((1 << 20) - 16))
            } else {
                "b".repeat((1 << 20) - 8)
            };
            json!({"target": format!("pattern:{i:032x}"), "publisher": "SHA256:x", "field": "notes",
                   "value": serde_json::to_string(&fill).unwrap(), "score_model": null,
                   "date": "2026-11-11T00:00:00Z", "rev": 1})
        })
        .collect()
}

#[test]
fn decompression_and_decoding_bombs_are_refused() {
    let dir = temp("bombs");
    let st = Store::create(&dir.join("s")).unwrap();
    let def = tables::table("annotations").unwrap();
    // 200 distinct 1 MiB strings of one letter: 200 MiB of pages, a few hundred KB stored.
    let parts = parquet_io::write_table(&st, def, heavy_rows(200, true), u64::MAX).unwrap();
    let b = st.get(&parts[0].object, "parquet", 1 << 32).unwrap();
    assert!(b.len() < 4 << 20, "{}", b.len());
    refused(
        parquet_io::read_all(b.into(), &def.cols).map(|_| ()),
        "decompression bomb",
    );
    // 200 copies of one 1 MiB string: dictionary encoded, so the pages are small, but it
    // decodes to 200 MiB.
    let parts = parquet_io::write_table(&st, def, heavy_rows(200, false), u64::MAX).unwrap();
    let b = st.get(&parts[0].object, "parquet", 1 << 32).unwrap();
    refused(
        parquet_io::read_all(b.into(), &def.cols).map(|_| ()),
        "bomb",
    );
    // Garbage and truncated files are refusals, not crashes.
    let small: Vec<Value> = heavy_rows(2, true)
        .into_iter()
        .map(|mut r| {
            r["value"] = json!("\"short\"");
            r
        })
        .collect();
    let parts = parquet_io::write_table(&st, def, small, u64::MAX).unwrap();
    let good = st.get(&parts[0].object, "parquet", 1 << 32).unwrap();
    for bad in [
        good[..good.len() / 2].to_vec(),
        b"PAR1 not really PAR1".to_vec(),
        {
            let mut x = good.clone();
            let n = x.len();
            for i in (n / 3..n / 2).step_by(7) {
                x[i] ^= 0x5a;
            }
            x
        },
        {
            // A footer claiming to be larger than the file.
            let mut x = good.clone();
            let n = x.len();
            x[n - 8..n - 4].copy_from_slice(&u32::MAX.to_le_bytes());
            x
        },
    ] {
        assert!(parquet_io::read_all(bad.into(), &def.cols).is_err());
    }
    // The good file reads.
    assert_eq!(
        parquet_io::read_all(good.into(), &def.cols).unwrap().len(),
        2
    );
}

#[test]
fn records_that_do_not_hash_to_their_ids_are_refused() {
    let (dir, s, h) = store("ids");
    let st = Store::open(&s).unwrap();
    let src = Source::open(&s).unwrap();
    let v = src.version(&h).unwrap();
    // Rewrite the patterns table with one id changed.
    let mut rows = vec![];
    src.table_rows(&v, "patterns", |_, r| {
        rows.extend(r);
        Ok(())
    })
    .unwrap();
    rows[0]["id"] = json!("0".repeat(32));
    let def = tables::table("patterns").unwrap();
    let parts = parquet_io::write_table(&st, def, rows.clone(), u64::MAX).unwrap();
    let ih = jetcarp_pack::publish::write_index(&st, def, parts).unwrap();
    let mut m = manifest(&s, &h);
    m.tables.insert("patterns".into(), ih);
    refused(
        import_from(&s, &put_manifest(&s, &m), &dir.join("lib-a")),
        "not the hash",
    );
    // A period beyond the limit.
    let mut rows2 = rows.clone();
    rows2[0] = rows[1].clone();
    rows2.truncate(1);
    rows2[0]["period"] = json!(1u64 << 40);
    let parts = parquet_io::write_table(&st, def, rows2, u64::MAX).unwrap();
    let mut m = manifest(&s, &h);
    m.tables.insert(
        "patterns".into(),
        jetcarp_pack::publish::write_index(&st, def, parts).unwrap(),
    );
    refused(
        import_from(&s, &put_manifest(&s, &m), &dir.join("lib-b")),
        "",
    );
}

#[test]
fn unsigned_and_badly_signed_versions_are_refused() {
    let (dir, s, h) = store("signatures");
    let st = Store::open(&s).unwrap();
    // A copy of the manifest with a changed title has no signature.
    let mut m = manifest(&s, &h);
    m.title = "changed".into();
    let h2 = put_manifest(&s, &m);
    let lib = Library::open(dir.join("lib"));
    let o = import::Options {
        path: s.clone(),
        manifest: Some(h2.clone()),
        verify: VerifyMode::None,
        ..Default::default()
    };
    refused(
        import::import(&lib, &o, &|_| {}),
        "not signed by its publisher",
    );
    // The original's signature copied onto it does not verify: always refused.
    let sig = std::fs::read_dir(st.sig_dir(&h).unwrap())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let to = st.sig_dir(&h2).unwrap();
    std::fs::create_dir_all(&to).unwrap();
    std::fs::copy(&sig, to.join(sig.file_name().unwrap())).unwrap();
    let o2 = import::Options {
        allow_unsigned: true,
        ..o.clone()
    };
    refused(import::import(&lib, &o2, &|_| {}), "does not verify");
    // The real one imports.
    let o3 = import::Options {
        manifest: Some(h),
        ..o
    };
    assert!(import::import(&lib, &o3, &|_| {}).is_ok());
}
