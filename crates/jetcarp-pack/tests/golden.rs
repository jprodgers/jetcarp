//! Golden packs: example stores (and an export) kept in the repository, each with its expected
//! manifest hash (`tests/golden/hashes.txt`). Every JETCARP must read them all: every hash,
//! signature and id checked, every record imported, every sample verified. A new format
//! version adds golden packs; none is ever deleted or rewritten.
//!
//! `write_golden_packs` (ignored; run it by hand with `cargo test -p jetcarp-pack --release
//! --test golden -- --ignored write_golden_packs`) made them from small libraries with fixed
//! times, fixed keys (Ed25519 from fixed seeds) and invented raters. Never rerun it to "fix" a
//! failing read: a golden pack that no longer reads is a broken promise.

mod common;

use common::*;
use jetcarp_core::pattern::verify::{default_periods, verify};
use jetcarp_pack::import::{self, VerifyMode};
use jetcarp_pack::read::Source;
use jetcarp_pack::sign;
use jetcarp_store::Library;
use std::path::{Path, PathBuf};

const FIXED: &str = "2026-11-11T00:00:00Z";

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// The golden packs: (name, manifest hash).
fn listed() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(golden_dir().join("hashes.txt")).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (n, h) = l.split_once(' ').unwrap();
            (n.to_string(), h.trim().to_string())
        })
        .collect()
}

#[test]
fn every_golden_pack_reads_and_checks() {
    let packs = listed();
    for want in [
        "minimal",
        "retraction",
        "merge",
        "searches",
        "media",
        "aperiodic",
        "ants",
        "export",
        "numbers",
    ] {
        assert!(packs.iter().any(|p| p.0 == want), "no golden pack {want}");
    }
    let tmp = temp("golden-read");
    for (name, hash) in &packs {
        let dir = golden_dir().join(name);
        let src = Source::open(&dir).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(&src.default_manifest(None).unwrap(), hash, "{name}");
        let v = src.version(hash).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(v.signed.by_publisher, "{name}: {:?}", v.signed);
        assert!(v.signed.bad.is_empty(), "{name}: {:?}", v.signed);
        // A full import into an empty library: every object, id, pin and signature checked,
        // every new pattern verified again.
        let lib = Library::open(tmp.join(name));
        let o = import::Options {
            path: dir.clone(),
            verify: VerifyMode::Full,
            ..Default::default()
        };
        let r = import::import(&lib, &o, &|_| {}).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(r.patterns_added > 0 || name == "ants", "{name}: {r:?}");
        assert!(
            !r.notes.iter().any(|n| n.starts_with("left out")),
            "{name}: {:?}",
            r.notes
        );
        assert!(r.sample.failures.is_empty(), "{name}: {:?}", r.sample);
        assert_eq!(r.sample.checked, r.patterns_added - r.flagged, "{name}");
        assert_eq!(r.flagged, 0, "{name}");
        match name.as_str() {
            "retraction" => {
                assert_eq!(v.manifest.parents.len(), 1);
                assert_eq!(v.manifest.retracts.len(), 1);
                assert_eq!(lib.retractions().unwrap().len(), 1);
            }
            "merge" => assert_eq!(v.manifest.parents.len(), 2),
            "searches" => {
                assert_eq!(r.searches.len(), 1);
                // Its working database is rebuilt from its six runs.
                assert_eq!(r.rebuilt.len(), 1, "{r:?}");
                assert_eq!(r.rebuilt[0].runs, 6);
            }
            "media" => assert!(r.media > 0),
            "aperiodic" => {
                let t = &v.tables["tilings"];
                assert!(!t.parts.is_empty());
            }
            "ants" => {
                // Every ant imported, and run again here it gives the pack's run exactly.
                assert_eq!(r.ants, 2, "{r:?}");
                let ants = lib.ants(&Default::default()).unwrap();
                assert_eq!(ants.len(), 2);
                for a in &ants {
                    let m = jetcarp_core::ants::search::parse_rule(&a.rule).unwrap();
                    let again =
                        jetcarp_search::ants::run_one(&m, &a.tiling, &a.start, &a.limits).unwrap();
                    assert_eq!(again.run, a.run, "{}", a.id);
                    assert_eq!(again.id, a.id);
                }
                let langton = ants.iter().find(|a| a.rule.ends_with(" RL")).unwrap();
                assert_eq!(langton.name.as_deref(), Some("Langton's ant"));
                assert_eq!(
                    (langton.outcome.name(), langton.period, langton.onset),
                    ("highway", Some(104), Some(9977))
                );
                assert!(ants.iter().any(|a| a.run.symmetry.is_some()));
            }
            "numbers" => {
                // Random states beyond 2^53, written as decimal strings, come back exactly.
                let q = lib.quickstart(&[jetcarp_store::QsSource::Pack]).unwrap();
                assert_eq!(q.len(), 1);
                assert_eq!(q[0].scene.rng_state, BIG);
                let p = lib.list().unwrap();
                assert_eq!(p.len(), 1);
                assert_eq!(lib.get(&p[0].id).unwrap().provenance.rng_state, Some(BIG));
                let ants = lib.ants(&Default::default()).unwrap();
                assert_eq!(ants.len(), 1);
                let a = &ants[0];
                assert!(
                    matches!(
                        a.start,
                        jetcarp_core::ants::run::Start::Soup { state: BIG, .. }
                    ),
                    "{:?}",
                    a.start
                );
                let m = jetcarp_core::ants::search::parse_rule(&a.rule).unwrap();
                let again =
                    jetcarp_search::ants::run_one(&m, &a.tiling, &a.start, &a.limits).unwrap();
                assert_eq!((again.id.as_str(), &again.run), (a.id.as_str(), &a.run));
            }
            _ => {}
        }
        // OpenSSH agrees with the pure-Rust signatures, where it is installed.
        if name != "export" {
            ssh_keygen_verify(&dir, &v);
        }
    }
}

/// The golden aperiodic library, made and published twice with the wall clock in another
/// second each time, gives byte-identical stores: a search's finds keep no time of their own.
#[test]
fn an_aperiodic_library_publishes_the_same_bytes_twice() {
    let tmp = temp("golden-twice");
    let mut stores = vec![];
    for n in 0..2 {
        let lib = aperiodic_library(&tmp.join(format!("lib{n}")));
        let to = tmp.join(format!("store{n}"));
        publish_into(
            &lib,
            PUBLISHER_A,
            "Golden Publisher A",
            &opts("golden-ap", &to, "aperiodic"),
        );
        stores.push(files(&to));
        // The next library is made in a later second, so a time that leaks into a pack shows.
        let end = jetcarp_core::pattern::now_utc();
        while jetcarp_core::pattern::now_utc() == end {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    assert!(stores[0].iter().any(|f| f.0.ends_with(".parquet")));
    let names = |s: &[(String, Vec<u8>)]| s.iter().map(|f| f.0.clone()).collect::<Vec<_>>();
    assert_eq!(names(&stores[0]), names(&stores[1]));
    for (a, b) in stores[0].iter().zip(&stores[1]) {
        assert!(a.1 == b.1, "{} differs between the two", a.0);
    }
}

/// `ssh-keygen -Y verify` on a version's manifest signature, if `ssh-keygen` is installed.
fn ssh_keygen_verify(store: &Path, v: &jetcarp_pack::read::Version) {
    if std::process::Command::new("ssh-keygen")
        .arg("-?")
        .output()
        .is_err()
    {
        return;
    }
    let hex = jetcarp_pack::hash::hex_of(&v.hash).unwrap();
    let key: jetcarp_pack::docs::KeyStatement = jetcarp_pack::json::parse_as(
        &std::fs::read(
            store
                .join("keys")
                .join(format!("{}.json", sign::signer_name(&v.manifest.publisher))),
        )
        .unwrap(),
        "key",
        true,
    )
    .unwrap();
    let sig = store.join(format!(
        "sigs/sha256/{}/{hex}/{}.sig",
        &hex[..2],
        sign::signer_name(&v.manifest.publisher)
    ));
    let tmp = temp(&format!("ssh-{}", &hex[..8]));
    let allowed = tmp.join("allowed");
    let k: Vec<&str> = key.key.split_whitespace().collect();
    std::fs::write(
        &allowed,
        format!(
            "publisher namespaces=\"jetcarp-manifest\" {} {}\n",
            k[0], k[1]
        ),
    )
    .unwrap();
    let manifest = store.join(jetcarp_pack::store::Store::object_rel(hex, "json"));
    let out = std::process::Command::new("ssh-keygen")
        .args(["-Y", "verify", "-f"])
        .arg(&allowed)
        .args(["-I", "publisher", "-n", "jetcarp-manifest", "-s"])
        .arg(&sig)
        .stdin(std::fs::File::open(&manifest).unwrap())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "ssh-keygen -Y verify: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

// ---------------------------------------------------------------------------------------------
// Making them

/// A record verified now, with its verification dated [`FIXED`].
fn fixed(mut r: jetcarp_core::pattern::PatternRecord) -> jetcarp_core::pattern::PatternRecord {
    let mut v = verify(&r, default_periods(r.period)).unwrap();
    v.date = FIXED.into();
    r.verified = v;
    r.created = FIXED.into();
    r.updated = FIXED.into();
    r
}

/// Fix every time and random uuid a library holds, so the same steps make the same pack.
fn fix_library(lib: &Library) {
    let c = rusqlite::Connection::open(lib.path()).unwrap();
    // Row 0 of a history is fixed by a trigger; these test libraries' rows are rewritten once,
    // with the trigger put back after.
    let trigger: String = c
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name = 'provenance_row0_is_fixed'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    c.execute_batch("DROP TRIGGER provenance_row0_is_fixed")
        .unwrap();
    let jobs: Vec<(i64, String)> = c
        .prepare("SELECT id, uuid FROM jobs ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for (id, old) in jobs {
        let new = format!("00000000-0000-4000-8000-{id:012}");
        c.execute(
            "UPDATE provenance SET job_uuid = ?2 WHERE job_uuid = ?1",
            [&old, &new],
        )
        .unwrap();
        c.execute("UPDATE jobs SET uuid = ?2 WHERE uuid = ?1", [&old, &new])
            .unwrap();
    }
    c.execute_batch(&format!(
        "UPDATE patterns SET created = '{FIXED}', updated = '{FIXED}';
         UPDATE pattern_rules SET first_seen = '{FIXED}';
         UPDATE sightings SET first_seen = '{FIXED}', last_seen = '{FIXED}';
         UPDATE ratings SET created = '{FIXED}';
         UPDATE provenance SET machine = 'linux-x86_64' WHERE machine != '';
         UPDATE provenance SET date = '{FIXED}' WHERE kind != 'found' OR target_kind = 'ant';
         UPDATE ants SET created = '{FIXED}', updated = '{FIXED}';
         UPDATE jobs SET created = '{FIXED}', updated = '{FIXED}';
         UPDATE media SET created = '{FIXED}', updated = '{FIXED}';"
    ))
    .unwrap();
    c.execute_batch(&trigger).unwrap();
}

const PUBLISHER_A: u8 = 11;
/// A random state beyond 2^53 (a soup's, as searches make them).
const BIG: u64 = 17705299566207515059;
const PUBLISHER_B: u8 = 12;

fn golden_book() -> publish::RaterBook {
    let mut b = book(&["Golden Publisher A", "Test Rater B"]);
    b.raters[1].show_name = false;
    b.raters.push(publish::RaterEntry {
        name: "Golden Publisher A & Test Rater B".into(),
        id: "rater-0000000000002000".into(),
        consent_date: "2026-11-01T00:00:00Z".into(),
        show_name: false,
        orcid: String::new(),
        notes: true,
        members: vec!["Golden Publisher A".into(), "Test Rater B".into()],
    });
    b
}

use jetcarp_pack::publish;

fn rate(lib: &Library, kind: &str, id: &str, rater: &str, rating: f64, verdict: Option<&str>) {
    lib.import_ratings(&[jetcarp_store::RatingRow {
        id: 0,
        target_kind: kind.into(),
        target_id: id.into(),
        rater: rater.into(),
        rating,
        verdict: verdict.map(String::from),
        note: format!("{rater} on {id}"),
        source: "review".into(),
        created: FIXED.into(),
    }])
    .unwrap();
}

/// Publisher A's library: a glider, a blinker and a block in Life with a sighting, a Quick
/// Start entry, and ratings by the publisher, a consented rater, a group of both and a rater
/// who did not consent (left out).
fn library_a(dir: &Path) -> Library {
    let lib = Library::open(dir.join("lib-a"));
    let mut g = fixed(one("B3/S23", GLIDER));
    g.name = Some("glider".into());
    g.tags = vec!["classic".into()];
    let b = fixed(one("B3/S23", BLINKER));
    let k = fixed(one("B3/S23", BLOCK));
    let job = lib
        .create_job("hunt", &serde_json::json!({"label": "golden hunt"}))
        .unwrap();
    lib.add_many(vec![g.clone(), b.clone(), k], Some(job))
        .unwrap();
    let ids: Vec<String> = lib.list().unwrap().into_iter().map(|r| r.id).collect();
    let rid = lib
        .rule_id("B3/S23", jetcarp_core::topology::Nb::Touch, 1)
        .unwrap();
    lib.add_sightings(
        job,
        &[jetcarp_store::Sighting {
            pattern_id: ids[0].clone(),
            rule_id: rid,
            count: 7,
            soups: 3,
        }],
    )
    .unwrap();
    rate(&lib, "pattern", &ids[0], "user", 4.0, None);
    rate(&lib, "pattern", &ids[1], "Test Rater B", 2.0, None);
    rate(
        &lib,
        "pattern",
        &ids[1],
        "Golden Publisher A & Test Rater B",
        5.0,
        None,
    );
    rate(&lib, "pattern", &ids[2], "Test Rater C", 1.0, None);
    // A Quick Start entry: the glider on a small square grid.
    let mut scene = jetcarp_core::Scene::new(jetcarp_core::TilingSpec::rect(
        "square",
        32,
        32,
        jetcarp_core::Boundary::Wrap,
    ));
    scene.name = "Glider".into();
    scene.jetcarp_version = jetcarp_core::VERSION.into();
    scene.cas.push(jetcarp_core::CaSpec::new("B3/S23"));
    let mut q = jetcarp_store::QuickStartRow::for_scene("golden-glider", "scene", scene);
    q.created = FIXED.into();
    q.updated = FIXED.into();
    lib.put_quickstart(&q).unwrap();
    fix_library(&lib);
    lib
}

/// Publisher A's ant runs on the square grid: Langton's ant (a highway, named) and LLRR
/// (symmetric growth).
fn library_ants(dir: &Path) -> Library {
    let lib = Library::open(dir.join("lib-ants"));
    lib.ants_flush(0, &[ant("RL", 20_000), ant("LLRR", 20_000)], None, None)
        .unwrap();
    let langton = lib
        .ants(&Default::default())
        .unwrap()
        .into_iter()
        .find(|a| a.rule.ends_with(" RL"))
        .unwrap();
    lib.update_ant(
        &langton.id,
        &jetcarp_store::AntUpdate {
            name: Some("Langton's ant".into()),
            ..Default::default()
        },
    )
    .unwrap();
    fix_library(&lib);
    lib
}

/// Random states beyond 2^53: a block whose find's soup had one, a Quick Start scene starting
/// from one, and Langton's ant from a soup made from one.
fn library_numbers(dir: &Path) -> Library {
    let lib = Library::open(dir.join("lib-numbers"));
    let mut k = fixed(one("B3/S23", BLOCK));
    k.provenance.source = "search".into();
    k.provenance.rng_state = Some(BIG);
    k.provenance.soup = Some(7);
    lib.add(k).unwrap();
    let mut scene = jetcarp_core::Scene::new(jetcarp_core::TilingSpec::rect(
        "square",
        32,
        32,
        jetcarp_core::Boundary::Wrap,
    ));
    scene.name = "A Life soup".into();
    scene.jetcarp_version = jetcarp_core::VERSION.into();
    scene.rng_state = BIG;
    scene.cas.push(jetcarp_core::CaSpec::new("B3/S23"));
    let mut q = jetcarp_store::QuickStartRow::for_scene("golden-soup", "scene", scene);
    q.created = FIXED.into();
    q.updated = FIXED.into();
    lib.put_quickstart(&q).unwrap();
    let m = jetcarp_core::ants::search::parse_rule("RL").unwrap();
    let t = jetcarp_core::pattern::tiling_list("square").unwrap();
    let start = jetcarp_core::ants::run::Start::Soup {
        w: 4,
        h: 4,
        density: 0.5,
        state: BIG,
    };
    let lim = jetcarp_core::ants::run::Limits {
        steps: 2_000,
        max_period: 2000,
        symmetry: true,
    };
    let a = jetcarp_search::ants::run_one(&m, &t[0], &start, &lim).unwrap();
    lib.ants_flush(0, &[a], None, None).unwrap();
    fix_library(&lib);
    lib
}

fn opts(feed: &str, to: &Path, title: &str) -> publish::Options {
    publish::Options {
        feed: feed.into(),
        to: to.to_path_buf(),
        title: title.into(),
        description: format!("Golden pack: {title}. Made by JETCARP's tests; never rewritten."),
        version_label: "golden-1".into(),
        created: Some(FIXED.into()),
        ..Default::default()
    }
}

fn publish_into(lib: &Library, seed: u8, name: &str, o: &publish::Options) -> String {
    let mut b = golden_book();
    let p = publisher(seed, name);
    publish::publish(lib, &p, &mut b, o, &|_| {})
        .unwrap()
        .manifest
}

#[test]
#[ignore]
fn write_golden_packs() {
    let out = golden_dir();
    let work = temp("golden-write");
    let mut lines = vec![
        "# The golden packs and their manifest hashes (tests/golden.rs). Never rewritten."
            .to_string(),
    ];
    let mut put = |name: &str, h: String| lines.push(format!("{name} {h}"));
    for n in [
        "minimal",
        "retraction",
        "merge",
        "searches",
        "media",
        "aperiodic",
        "ants",
        "export",
        "numbers",
    ] {
        let _ = std::fs::remove_dir_all(out.join(n));
    }

    // minimal: one version of publisher A's library.
    let a = library_a(&work);
    let h = publish_into(
        &a,
        PUBLISHER_A,
        "Golden Publisher A",
        &opts("golden-a", &out.join("minimal"), "minimal"),
    );
    put("minimal", h);

    // retraction: a second version that retracts the blinker, adds a boat and renames the
    // glider (a later annotation, rev 2).
    let ra = library_a(&work.join("r"));
    let o = opts("golden-a", &out.join("retraction"), "retraction");
    publish_into(&ra, PUBLISHER_A, "Golden Publisher A", &o);
    let blinker = ra
        .list()
        .unwrap()
        .into_iter()
        .find(|r| r.period == 2)
        .unwrap()
        .id;
    ra.add(fixed(one("B3/S23", "oo/o.o/.o"))).unwrap();
    let glider = ra
        .list()
        .unwrap()
        .into_iter()
        .find(|r| r.name.is_some())
        .unwrap()
        .id;
    ra.update(
        &glider,
        &jetcarp_store::Update {
            name: Some("the glider".into()),
            ..Default::default()
        },
    )
    .unwrap();
    fix_library(&ra);
    let mut o2 = o.clone();
    o2.version_label = "golden-2".into();
    o2.retract = vec![(format!("pattern:{blinker}"), "a test retraction".into())];
    put(
        "retraction",
        publish_into(&ra, PUBLISHER_A, "Golden Publisher A", &o2),
    );

    // merge: publisher B's own version (a boat and the glider, which A has too), then a
    // version merging A's minimal pack: two parents.
    let b = Library::open(work.join("lib-b"));
    b.add_many(
        vec![
            fixed(one("B3/S23", "oo/o.o/.o")),
            fixed(one("B3/S23", GLIDER)),
        ],
        None,
    )
    .unwrap();
    fix_library(&b);
    let ob = opts("golden-b", &out.join("merge"), "merge");
    publish_into(&b, PUBLISHER_B, "Golden Publisher B", &ob);
    let mut ob2 = ob.clone();
    ob2.version_label = "golden-2".into();
    ob2.merge = vec![(out.join("minimal"), None)];
    put(
        "merge",
        publish_into(&b, PUBLISHER_B, "Golden Publisher B", &ob2),
    );

    // searches: an explore search's folder with two sealed segments and a score sidecar, and
    // verdicts on its runs.
    let s = Library::open(work.join("lib-s"));
    s.add(fixed(one("B3/S23", BLOCK))).unwrap();
    let uuid = "00000000-0000-4000-8000-00000000abcd";
    s.create_job(
        "explore",
        &serde_json::json!({"uuid": uuid, "label": "golden explore"}),
    )
    .unwrap();
    fake_search(&s.folder().join("searches").join(uuid), uuid);
    for (run, v) in [(1, "keep"), (4, "drop")] {
        rate(
            &s,
            "search_run",
            &format!("{uuid}:{run}"),
            "user",
            if v == "keep" { 5.0 } else { 1.0 },
            Some(v),
        );
    }
    fix_library(&s);
    put(
        "searches",
        publish_into(
            &s,
            PUBLISHER_A,
            "Golden Publisher A",
            &opts("golden-s", &out.join("searches"), "searches"),
        ),
    );

    // media: a poster and a thumbnail of the block.
    let m = Library::open(work.join("lib-m"));
    m.add(fixed(one("B3/S23", BLOCK))).unwrap();
    let id = m.list().unwrap()[0].id.clone();
    let mut rows = vec![];
    for (kind, w) in [("poster", 4u32), ("thumb", 2)] {
        let png = tiny_png(w);
        let hash = jetcarp_store::media::put_file(&m.folder(), &png, "png").unwrap();
        rows.push(jetcarp_store::library::pack::PackMedia {
            target_kind: "pattern".into(),
            target_id: id.clone(),
            kind: kind.into(),
            from_seed: false,
            hash,
            ext: "png".into(),
            render: r#"{"scale":4}"#.into(),
            width: w as i64,
            height: w as i64,
            frames: 1,
            bytes: png.len() as i64,
            key: "golden".into(),
            created: FIXED.into(),
        });
    }
    m.import_media(&rows).unwrap();
    fix_library(&m);
    put(
        "media",
        publish_into(
            &m,
            PUBLISHER_A,
            "Golden Publisher A",
            &opts("golden-m", &out.join("media"), "media"),
        ),
    );

    // aperiodic: patterns found on a Penrose kite-and-dart patch, with the family's reference
    // patch.
    let ap = aperiodic_library(&work.join("ap"));
    put(
        "aperiodic",
        publish_into(
            &ap,
            PUBLISHER_A,
            "Golden Publisher A",
            &opts("golden-ap", &out.join("aperiodic"), "aperiodic"),
        ),
    );

    // ants: two ant runs, one named.
    let an = library_ants(&work);
    put(
        "ants",
        publish_into(
            &an,
            PUBLISHER_A,
            "Golden Publisher A",
            &opts("golden-ants", &out.join("ants"), "ants"),
        ),
    );

    // export: the minimal pack laid out for a single download.
    let src = Source::open(&out.join("minimal")).unwrap();
    let v = src.version(&src.default_manifest(None).unwrap()).unwrap();
    jetcarp_pack::export::export(&src, &v, &out.join("export")).unwrap();
    put("export", v.hash.clone());

    // numbers: random states beyond 2^53, in a scene, a find's history and an ant's start.
    let nu = library_numbers(&work);
    put(
        "numbers",
        publish_into(
            &nu,
            PUBLISHER_A,
            "Golden Publisher A",
            &opts("golden-numbers", &out.join("numbers"), "numbers"),
        ),
    );

    std::fs::write(out.join("hashes.txt"), lines.join("\n") + "\n").unwrap();
    // Each golden pack stays small.
    for (name, _) in listed() {
        let bytes: usize = files(&out.join(&name)).iter().map(|f| f.1.len()).sum();
        assert!(bytes < 1 << 20, "{name}: {bytes} bytes");
    }
}

/// A 1-bit PNG of `w` x `w` pixels.
fn tiny_png(w: u32) -> Vec<u8> {
    let mut out = vec![];
    {
        let mut e = png::Encoder::new(&mut out, w, w);
        e.set_color(png::ColorType::Grayscale);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().unwrap();
        wr.write_image_data(&vec![128u8; (w * w) as usize]).unwrap();
    }
    out
}

/// A search folder as an explore search seals it: two segments of three runs, one rescore.
fn fake_search(dir: &Path, uuid: &str) {
    use jetcarp_search::explore::runs::{Manifest, ScoreFile, ScoreSet, SegmentEntry};
    use jetcarp_search::explore::segments::{RunRec, SegmentWriter};
    std::fs::create_dir_all(dir.join("runs")).unwrap();
    let mut segments = vec![];
    for seg in 1..=2i64 {
        let file = format!("runs/part-{seg:06}.parquet");
        let kv = vec![
            ("jetcarp.search".to_string(), uuid.to_string()),
            ("jetcarp.segment".to_string(), format!("part-{seg:06}")),
            ("jetcarp.engine_version".to_string(), "golden".to_string()),
        ];
        let mut w = SegmentWriter::create(&dir.join(&file), &kv).unwrap();
        for k in 0..3 {
            let id = (seg - 1) * 3 + k + 1;
            w.push(RunRec {
                id,
                stage: 1,
                tiling: "square".into(),
                fam: "life_like".into(),
                rule: "B3/S23".into(),
                seed: "{}".into(),
                seed_kind: "soup".into(),
                cells: 4096,
                gens: 1024,
                steps: 1,
                rng: id * 7919,
                origin: "random".into(),
                fate: "cycle".into(),
                settle: 100 + id,
                period: 2,
                score: 1.5,
                bin: 3,
                ts: 1_794_355_200,
                ..Default::default()
            })
            .unwrap();
        }
        let wr = w.finish().unwrap();
        let (hex, bytes) = jetcarp_pack::hash::file_hex(&dir.join(&file), 1 << 30).unwrap();
        segments.push(SegmentEntry {
            file,
            sha256: hex,
            bytes,
            rows: wr.rows,
            first_id: wr.first_id,
            last_id: wr.last_id,
            sealed: FIXED.into(),
            engine_version: "golden".into(),
        });
    }
    // One rescore of segment 1.
    std::fs::create_dir_all(dir.join("scores-1")).unwrap();
    let sf = "scores-1/part-000001.parquet".to_string();
    let mut w = SegmentWriter::create_scores(&dir.join(&sf), &[], vec![3]).unwrap();
    for id in 1..=3 {
        w.push(RunRec {
            id,
            score: 2.0,
            ..Default::default()
        })
        .unwrap();
    }
    w.finish().unwrap();
    let (hex, bytes) = jetcarp_pack::hash::file_hex(&dir.join(&sf), 1 << 30).unwrap();
    let m = Manifest {
        format: "jetcarp-search-manifest 1".into(),
        search: uuid.into(),
        kind: "explore".into(),
        engine_version: "golden".into(),
        run_schema: "jetcarp-explore-runs 1".into(),
        score_schema: "jetcarp-explore-scores 1".into(),
        created: FIXED.into(),
        updated: FIXED.into(),
        spec: serde_json::json!({"uuid": uuid, "screen_gens": 1024, "density": 0.35}),
        segments,
        scores: vec![ScoreSet {
            version: 1,
            created: FIXED.into(),
            engine_version: "golden".into(),
            files: vec![ScoreFile {
                segment: "runs/part-000001.parquet".into(),
                file: sf,
                sha256: hex,
                bytes,
                rows: 3,
            }],
        }],
        remeasure: None,
    };
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&m).unwrap(),
    )
    .unwrap();
}

/// A library of patterns found by a bounded search on a Penrose kite-and-dart patch, with
/// every time fixed.
///
/// A search stamps what it finds with the wall clock: each record's find (`provenance.date`,
/// which becomes its provenance row 0 and the `run` JSON) and its verification. So the search
/// runs in a scratch library, and its finds are copied into the golden one with those times
/// set to [`FIXED`], under a job of the search's kind.
fn aperiodic_library(dir: &Path) -> Library {
    let found = Library::open(dir.join("search"));
    aperiodic_search(&found);
    let lib = Library::open(dir.join("lib"));
    let job = lib
        .create_job("search", &serde_json::json!({"label": "golden search"}))
        .unwrap();
    let recs: Vec<_> = found
        .list()
        .unwrap()
        .into_iter()
        .map(|mut r| {
            r.provenance.date = FIXED.into();
            r.verified.date = FIXED.into();
            r.created = FIXED.into();
            r.updated = FIXED.into();
            r
        })
        .collect();
    for r in lib.add_many(recs, Some(job)).unwrap() {
        r.unwrap();
    }
    fix_library(&lib);
    lib
}

/// Patterns from a bounded search on a Penrose kite-and-dart patch.
fn aperiodic_search(lib: &Library) {
    use jetcarp_core::topology::{Boundary, SizeSpec, TilingSpec};
    use jetcarp_search::jobs::{self, JobSpec, Runner};
    for rule in ["B2/S23", "B3/S23", "B35/S234"] {
        let spec = jetcarp_search::bounded::SearchSpec {
            rules: vec![rule.into()],
            tilings: vec![TilingSpec::new("penrose-kd")],
            size: SizeSpec::Level(3),
            boundary: Boundary::Dead,
            soups: 6,
            gens: 400,
            seed: 3,
            ..Default::default()
        };
        let js = JobSpec {
            strategy: "search".into(),
            spec: serde_json::to_value(jetcarp_search::jobs::search::SearchJobSpec {
                search: spec,
                store: true,
            })
            .unwrap(),
            ..Default::default()
        };
        let job = jobs::create(lib, &js).unwrap();
        Runner::new(lib.clone())
            .run(job, &Default::default())
            .unwrap();
        if !lib.list().unwrap().is_empty() {
            return;
        }
    }
    panic!("no pattern found on the Penrose patch");
}
