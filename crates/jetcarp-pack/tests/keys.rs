//! Key statements: a first statement signed by its own key, a successor signed by its
//! predecessor, a revocation by the key or a successor, machine keys a statement authorises,
//! and a revoked key that signs nothing.

mod common;

use common::*;
use jetcarp_pack::docs::{AuthorisedKey, KeyStatement};
use jetcarp_pack::read::Source;
use jetcarp_pack::sign::{self, Signer};
use jetcarp_pack::store::Store;
use jetcarp_pack::{json, publish};

/// Write `k` into the store's `keys/`, signed by `by`.
fn put(st: &Store, mut k: KeyStatement, by: &Signer) {
    k.signature.clear();
    let mut v = serde_json::to_value(&k).unwrap();
    v.as_object_mut().unwrap().remove("signature");
    k.signature = by
        .sign(sign::NS_KEY, &json::canonical(&v).unwrap())
        .unwrap();
    let p = st.key_path(&k.fingerprint).unwrap();
    jetcarp_pack::store::write_atomic(&p, &json::canonical_of(&k).unwrap()).unwrap();
}

fn statement(s: &Signer) -> KeyStatement {
    KeyStatement {
        format: "jetcarp-key".into(),
        version: "1.0".into(),
        key: s.public_openssh(),
        fingerprint: s.fingerprint().into(),
        created: "2026-11-11T00:00:00Z".into(),
        ..Default::default()
    }
}

#[test]
fn successions_revocations_and_machine_keys() {
    let dir = temp("keys");
    let st = Store::create(&dir.join("s")).unwrap();
    let (k1, k2, k3, m) = (
        Signer::from_seed(&[21; 32], "k1"),
        Signer::from_seed(&[22; 32], "k2"),
        Signer::from_seed(&[23; 32], "k3"),
        Signer::from_seed(&[24; 32], "laptop"),
    );
    let src = || Source::open(&dir.join("s")).unwrap();
    // A first statement signed by its own key stands; one signed by another key does not.
    put(&st, statement(&k1), &k1);
    let i = src().key_info(k1.fingerprint()).unwrap();
    assert_eq!((i.root.as_str(), i.revoked), (k1.fingerprint(), false));
    put(&st, statement(&k3), &k1);
    assert!(src().key_info(k3.fingerprint()).is_err());
    // A successor signed by its predecessor: the publisher's id stays the first key.
    let mut s2 = statement(&k2);
    s2.supersedes = k1.fingerprint().into();
    s2.authorised = vec![AuthorisedKey {
        key: m.public_openssh(),
        fingerprint: m.fingerprint().into(),
        label: "laptop".into(),
        revoked: false,
    }];
    put(&st, s2.clone(), &k1);
    let i2 = src().key_info(k2.fingerprint()).unwrap();
    assert_eq!(i2.root, k1.fingerprint());
    assert!(jetcarp_pack::read::authorised(
        &i2.statement,
        m.fingerprint()
    ));
    // A successor signed by itself, not its predecessor, does not stand.
    let mut bad = statement(&k3);
    bad.supersedes = k1.fingerprint().into();
    put(&st, bad, &k3);
    assert!(src().key_info(k3.fingerprint()).is_err());
    // k1 revoked by its successor k2: k1 signs nothing, k2 still stands.
    let mut r1 = statement(&k1);
    r1.revoked = true;
    put(&st, r1.clone(), &k2);
    assert!(src().key_info(k1.fingerprint()).unwrap().revoked);
    assert!(src().key_statement(k1.fingerprint()).is_err());
    assert!(!src().key_info(k2.fingerprint()).unwrap().revoked);
    // k1 revoked by itself: its successor k2 falls with it.
    put(&st, r1.clone(), &k1);
    assert!(src().key_info(k2.fingerprint()).is_err());
    // A revocation signed by a stranger is refused (the statement is not k1's to revoke).
    put(&st, r1, &k3);
    assert!(src().key_info(k1.fingerprint()).is_err());
}

#[test]
fn a_revoked_publisher_key_signs_nothing_and_stays_revoked() {
    let dir = temp("revoked");
    let lib = small_library(&dir);
    let p = publisher(31, "Test Publisher");
    let mut b = book(&[]);
    let store = dir.join("store");
    let out = publish::publish(&lib, &p, &mut b, &options("rev", &store), &|_| {}).unwrap();
    let st = Store::open(&store).unwrap();
    let lib2 = jetcarp_store::Library::open(dir.join("lib2"));
    let o = jetcarp_pack::import::Options {
        path: store.clone(),
        manifest: Some(out.manifest.clone()),
        verify: jetcarp_pack::import::VerifyMode::None,
        dry_run: false,
        ..Default::default()
    };
    // Revoke the key by itself: the version is no longer signed by its publisher.
    let original = std::fs::read(st.key_path(&out.publisher_key(&p)).unwrap()).unwrap();
    let mut k = statement(&p.signer);
    k.revoked = true;
    put(&st, k, &p.signer);
    let v = Source::open(&store)
        .unwrap()
        .version(&out.manifest)
        .unwrap();
    assert!(!v.signed.by_publisher, "{:?}", v.signed);
    let e = jetcarp_pack::import::import(&lib2, &o, &|_| {}).unwrap_err();
    assert!(e.contains("not signed by its publisher"), "{e}");
    // Its ref names a revoked key: refused.
    let e = Source::open(&store).unwrap().read_ref("rev").unwrap_err();
    assert!(e.contains("revoked"), "{e}");
    // The library keeps the key revoked, even when the store serves the old statement again.
    let mut allow = o.clone();
    allow.allow_unsigned = true;
    let _ = jetcarp_pack::import::import(&lib2, &allow, &|_| {});
    assert!(
        lib2.revoked_keys()
            .unwrap()
            .contains(p.signer.fingerprint())
    );
    std::fs::write(st.key_path(p.signer.fingerprint()).unwrap(), original).unwrap();
    let r = jetcarp_pack::import::import(&lib2, &allow, &|_| {}).unwrap();
    assert_eq!(r.signed, "unsigned");
    // Unsigned: no word on the key, and these imports make it no better known.
    assert_eq!(r.key, None);
    assert_eq!(
        jetcarp_pack::import::key_seen_with(&lib2, p.signer.fingerprint(), None).unwrap(),
        jetcarp_pack::import::KeySeen::New
    );
}

trait Key {
    fn publisher_key(&self, p: &publish::Publisher) -> String;
}
impl Key for publish::Published {
    fn publisher_key(&self, p: &publish::Publisher) -> String {
        p.signer.fingerprint().to_string()
    }
}

/// An import says who signed it and what the library knew of the key: new the first time
/// (a check, which records nothing, too), known once a signed version was imported, this
/// computer's own key as such; and an import of an unsigned version, allowed by hand, makes no
/// key known.
#[test]
fn an_import_names_its_signer_and_whether_the_key_is_new() {
    use jetcarp_pack::import::{self, KeySeen};
    let dir = temp("seen");
    let lib = small_library(&dir);
    let p = publisher(41, "Ada Test");
    let store = dir.join("store");
    let out =
        publish::publish(&lib, &p, &mut book(&[]), &options("seen", &store), &|_| {}).unwrap();
    let fp = p.signer.fingerprint().to_string();
    let lib2 = jetcarp_store::Library::open(dir.join("lib2"));
    let mut o = import::Options {
        path: store.clone(),
        manifest: Some(out.manifest.clone()),
        verify: import::VerifyMode::None,
        dry_run: true,
        ..Default::default()
    };
    let checked = import::import(&lib2, &o, &|_| {}).unwrap();
    assert_eq!(checked.key, Some(KeySeen::New));
    o.dry_run = false;
    let mut logged = vec![];
    let first = {
        let log = std::sync::Mutex::new(&mut logged);
        import::import(&lib2, &o, &|l| log.lock().unwrap().push(l.to_string())).unwrap()
    };
    assert_eq!(
        (first.signed.as_str(), first.key),
        ("publisher", Some(KeySeen::New))
    );
    assert!(
        logged
            .iter()
            .any(|l| l.contains("new key: never seen in this library")),
        "{logged:?}"
    );
    let words = import::signed_words(
        &first.publisher,
        &first.publisher_name,
        true,
        first.key,
        false,
    );
    assert_eq!(
        words,
        format!("signed by Ada Test, key {fp}; new key: never seen in this library")
    );
    // The library's record of the import keeps what it knew then.
    let rec = lib2.pack_imports().unwrap();
    assert_eq!(rec.last().unwrap().report["key"], "new");
    // Importing again: the key is known now.
    let again = import::import(&lib2, &o, &|_| {}).unwrap();
    assert_eq!(again.key, Some(KeySeen::Known));
    assert_eq!(
        import::key_seen_with(&lib2, &fp, None).unwrap(),
        KeySeen::Known
    );
    // This computer's own key comes first; a fresh library knows nothing.
    assert_eq!(
        import::key_seen_with(&lib2, &fp, Some(&fp)).unwrap(),
        KeySeen::Own
    );
    let lib3 = jetcarp_store::Library::open(dir.join("lib3"));
    assert_eq!(
        import::key_seen_with(&lib3, &fp, None).unwrap(),
        KeySeen::New
    );
    // In words: unsigned, no name, cut short.
    assert_eq!(
        import::signed_words(&fp, "", false, None, false),
        format!("not signed by its publisher (key {fp})")
    );
    assert!(
        import::signed_words(&fp, "", true, Some(KeySeen::Known), false)
            .starts_with("signed by its publisher, key SHA256:")
    );
    let short = import::short_key(&fp);
    assert!(
        short.ends_with('…') && fp.starts_with(short.trim_end_matches('…')),
        "{short}"
    );
    assert_eq!(short.chars().count(), "SHA256:".len() + 13);
}
