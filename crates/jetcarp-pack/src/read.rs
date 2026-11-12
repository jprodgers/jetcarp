//! Reading a pack: a store or an export, one version of it, checked.
//!
//! [`Source::open`] tells a store (`jetcarp-store.json`) from an export (`pack.json`).
//! [`Source::version`] reads a version: the manifest's format, version and `requires`; its hash;
//! its signatures and the publisher's key statement ([`Signed`]); every index (to 8 levels of
//! children, 10,000 parts each), with every hash checked. Parts are read on demand
//! ([`Source::part_rows`]), each checked against its hash before it is decoded.
//!
//! In an export, files have readable names (see [`export_path`]); the indexes still name each
//! one's hash, so an export is checked exactly like a store.

use crate::docs::{self, Index, KeyStatement, Manifest, Part, Ref, SearchIndex};
use crate::hash;
use crate::sign;
use crate::store::{self, MAX_OBJECT, Store};
use crate::tables::{self, TableDef};
use bytes::Bytes;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Most parts one index may list.
pub const MAX_PARTS: usize = 10_000;
/// Deepest nesting of child indexes.
pub const MAX_INDEX_DEPTH: usize = 8;
/// Features this reader understands in `requires` (none in 1.0).
pub const KNOWN_FEATURES: [&str; 0] = [];

/// Where a pack is read from.
#[derive(Clone, Debug)]
pub enum Source {
    Store(Store),
    /// An export's folder.
    Export(PathBuf),
}

/// A table of a version: its schema and its parts, in index order (children flattened), each
/// with where its file is.
#[derive(Clone, Debug, Default)]
pub struct TableParts {
    pub schema: String,
    pub index: String,
    pub parts: Vec<(Part, PathBuf)>,
}

/// A search of a version, its children's parts and scores merged in.
#[derive(Clone, Debug)]
pub struct SearchParts {
    pub index_hash: String,
    pub index: SearchIndex,
    /// Each run segment's file.
    pub run_files: Vec<PathBuf>,
    /// Each (score version, part) file.
    pub score_files: Vec<Vec<PathBuf>>,
}

/// What the signatures on a version say.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Signed {
    /// A signature by the publisher's current key (or a machine key its statement authorises)
    /// verifies, and that key is not revoked.
    pub by_publisher: bool,
    /// The publisher's key statement verifies.
    pub key_statement: bool,
    /// The publisher's id: the first key of its chain of statements.
    pub publisher_id: String,
    /// Who the statement names, if anyone.
    pub name: String,
    pub orcid: String,
    /// The keys whose signatures made `by_publisher`.
    pub publisher_signers: Vec<String>,
    /// Fingerprints of every key whose signature verifies (endorsements included).
    pub signers: Vec<String>,
    /// Keys found revoked while reading (a reader keeps them revoked for good).
    pub revoked: Vec<String>,
    /// Signatures that did not verify, and why.
    pub bad: Vec<String>,
}

/// One version, checked.
#[derive(Clone, Debug)]
pub struct Version {
    pub hash: String,
    pub bytes: Vec<u8>,
    pub manifest: Manifest,
    pub signed: Signed,
    pub tables: BTreeMap<String, TableParts>,
    pub searches: BTreeMap<String, SearchParts>,
    /// Every index object read (hash, bytes), for copying a version.
    pub indexes: Vec<(String, Vec<u8>)>,
}

/// The path of a file in an export (relative to its root), from its role. Every part comes
/// from validated names: table names from the format, partition values checked, numbers.
pub fn export_path(table: &str, partition: &BTreeMap<String, String>, n: usize) -> String {
    let mut s = format!("tables/{table}");
    for (k, v) in partition {
        s.push_str(&format!("/{k}={v}"));
    }
    s.push_str(&format!("/part-{n:06}.parquet"));
    s
}

/// An export's search segment: `searches/<uuid>/runs/<segment>.parquet`.
pub fn export_run_path(uuid: &str, segment: &str) -> String {
    format!("searches/{uuid}/runs/{segment}.parquet")
}

/// An export's score sidecar: `searches/<uuid>/scores-<n>/<segment>.parquet`.
pub fn export_score_path(uuid: &str, version: u64, segment: &str) -> String {
    format!("searches/{uuid}/scores-{version}/{segment}.parquet")
}

/// An export's media file: `media/<kind>/<id>/<media kind>[-seed].<ext>`.
pub fn export_media_path(target: &str, kind: &str, from_seed: bool, ext: &str) -> Option<String> {
    let (tk, id) = target.split_once(':')?;
    if !matches!(tk, "pattern" | "ant" | "quickstart") || !safe_id(id) {
        return None;
    }
    if !matches!(kind, "gif" | "webp" | "poster" | "thumb") {
        return None;
    }
    store::check_ext(ext).ok()?;
    Some(format!(
        "media/{tk}/{id}/{kind}{}.{ext}",
        if from_seed { "-seed" } else { "" }
    ))
}

/// An export's reference patch: `tilings/reference/<family>.parquet`.
pub fn export_reference_path(family: &str) -> Option<String> {
    tables::valid_partition_value(family).then(|| format!("tilings/reference/{family}.parquet"))
}

/// An id that is safe as a folder name: letters, digits, `-` and `_`, 1 to 80.
pub fn safe_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Is this a search uuid (hex digits and `-`, 8 to 64)?
pub fn safe_uuid(s: &str) -> bool {
    (8..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

/// Is this a segment name (`part-` and digits)?
pub fn safe_segment(s: &str) -> bool {
    s.strip_prefix("part-")
        .is_some_and(|d| !d.is_empty() && d.len() <= 12 && d.bytes().all(|b| b.is_ascii_digit()))
}

impl Source {
    /// A store or an export at `path`.
    pub fn open(path: &Path) -> Result<Source, String> {
        if path.join(store::MARKER).is_file() {
            return Ok(Source::Store(Store::open(path)?));
        }
        if path.join("pack.json").is_file() {
            return Ok(Source::Export(path.to_path_buf()));
        }
        Err(format!(
            "{} is neither a pack store (jetcarp-store.json) nor an export (pack.json)",
            path.display()
        ))
    }

    pub fn root(&self) -> &Path {
        match self {
            Source::Store(s) => s.root(),
            Source::Export(p) => p,
        }
    }

    /// The feeds a store has refs for (an export has none).
    pub fn feeds(&self) -> Vec<String> {
        let Source::Store(s) = self else {
            return vec![];
        };
        let Ok(rd) = std::fs::read_dir(s.root().join("refs")) else {
            return vec![];
        };
        let mut v: Vec<String> = rd
            .flatten()
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().to_string();
                let f = n.strip_suffix(".json")?.to_string();
                docs::valid_feed(&f).then_some(f)
            })
            .collect();
        v.sort();
        v
    }

    /// A feed's ref, its signature checked against the publisher's key statement in the store.
    pub fn read_ref(&self, feed: &str) -> Result<Ref, String> {
        let Source::Store(s) = self else {
            return Err("an export has no refs".into());
        };
        let p = s.ref_path(feed)?;
        let b = store::read_limited(&p, crate::json::MAX_DOC)?;
        let r: Ref = crate::json::parse_as(&b, &format!("refs/{feed}.json"), true)?;
        docs::check_format(&r.format, &r.version, "jetcarp-ref")?;
        if r.feed != feed {
            return Err(format!("refs/{feed}.json names feed '{}'", r.feed));
        }
        hash::hex_of(&r.manifest)?;
        let key = self.key_info(&r.publisher)?;
        let msg = unsigned_bytes(&b)?;
        let fp = sign::verify(&r.signature, sign::NS_REF, &msg)
            .map_err(|e| format!("refs/{feed}.json: {e}"))?;
        if key.revoked {
            return Err(format!(
                "refs/{feed}.json: its publisher's key {} is revoked",
                r.publisher
            ));
        }
        if !authorised(&key.statement, &fp) {
            return Err(format!(
                "refs/{feed}.json is signed by {fp}, not by its publisher {}",
                r.publisher
            ));
        }
        Ok(r)
    }

    /// The manifest a store or export holds by default: an export's own, or the newest of a
    /// store's feed (the only feed when `feed` is `None`).
    pub fn default_manifest(&self, feed: Option<&str>) -> Result<String, String> {
        match self {
            Source::Export(p) => {
                let b = store::read_limited(&p.join("pack.json"), crate::json::MAX_DOC)?;
                Ok(hash::of(&b))
            }
            Source::Store(_) => {
                let feeds = self.feeds();
                let f = match feed {
                    Some(f) => f.to_string(),
                    None if feeds.len() == 1 => feeds[0].clone(),
                    None if feeds.is_empty() => return Err("the store has no refs".into()),
                    None => {
                        return Err(format!(
                            "the store has several feeds ({}); choose one",
                            feeds.join(", ")
                        ));
                    }
                };
                Ok(self.read_ref(&f)?.manifest)
            }
        }
    }

    /// The bytes of a JSON object (a manifest or index) by hash, checked.
    pub fn doc(&self, h: &str) -> Result<Vec<u8>, String> {
        let hex = hash::hex_of(h)?.to_string();
        let b = match self {
            Source::Store(s) => s.get(h, "json", crate::json::MAX_DOC)?,
            Source::Export(p) => {
                let pack = p.join("pack.json");
                let cand = if hash::of(&store::read_limited(&pack, crate::json::MAX_DOC)?) == h {
                    pack
                } else if p.join("index").join(format!("{hex}.json")).is_file() {
                    p.join("index").join(format!("{hex}.json"))
                } else {
                    p.join("history").join(format!("{hex}.json"))
                };
                store::read_limited(&cand, crate::json::MAX_DOC)?
            }
        };
        if hash::of(&b) != h {
            return Err(format!("object {h} is altered"));
        }
        Ok(b)
    }

    /// Does the source hold this JSON object?
    pub fn has_doc(&self, h: &str) -> bool {
        self.doc(h).is_ok()
    }

    /// The statement in `keys/` for a key, read and its own signature checked: it is the key's
    /// own statement (self-signed, or signed by the key it `supersedes`), or a revocation
    /// (signed by the key itself or by a successor of it). Who signed it is returned.
    fn read_statement(&self, fingerprint: &str) -> Result<(KeyStatement, String), String> {
        let p = match self {
            Source::Store(s) => s.key_path(fingerprint)?,
            Source::Export(r) => {
                if !sign::valid_fingerprint(fingerprint) {
                    return Err(format!("'{fingerprint}' is not a key fingerprint"));
                }
                r.join("keys")
                    .join(format!("{}.json", sign::signer_name(fingerprint)))
            }
        };
        let b = store::read_limited(&p, crate::json::MAX_DOC)
            .map_err(|_| format!("no key statement for {fingerprint}"))?;
        let k: KeyStatement = crate::json::parse_as(&b, "key statement", true)?;
        docs::check_format(&k.format, &k.version, "jetcarp-key")?;
        let fp = sign::fingerprint_of(&k.key)?;
        if fp != k.fingerprint || fp != fingerprint {
            return Err(format!(
                "key statement for {fingerprint} holds the key {fp}"
            ));
        }
        for a in &k.authorised {
            if sign::fingerprint_of(&a.key)? != a.fingerprint {
                return Err(format!(
                    "key statement for {fingerprint}: an authorised key's fingerprint is wrong"
                ));
            }
        }
        let msg = unsigned_bytes(&b)?;
        let by = sign::verify(&k.signature, sign::NS_KEY, &msg)
            .map_err(|e| format!("key statement for {fingerprint}: {e}"))?;
        Ok((k, by))
    }

    /// A key's standing: its statement, the first key of its chain (the publisher's id), and
    /// whether it is revoked.
    ///
    /// - A statement without `supersedes` must be signed by its own key; one with it, by the
    ///   key it supersedes, whose own standing must hold (a revoked predecessor's successors
    ///   stand only if this successor signed the revocation).
    /// - A revocation (`revoked: true`) must be signed by the key itself or by a successor (a
    ///   key whose statement supersedes it and is signed by it). A revoked key signs nothing:
    ///   no manifest, ref or successor counts by it, whatever the dates say.
    pub fn key_info(&self, fingerprint: &str) -> Result<KeyInfo, String> {
        self.key_info_depth(fingerprint, 0)
    }

    fn key_info_depth(&self, fingerprint: &str, depth: usize) -> Result<KeyInfo, String> {
        if depth > 16 {
            return Err("key statements succeed each other too deeply".into());
        }
        let (k, by) = self.read_statement(fingerprint)?;
        let mut info = KeyInfo {
            root: fingerprint.to_string(),
            revoked: false,
            revoked_by: String::new(),
            statement: k.clone(),
        };
        if k.revoked {
            let ok = by == fingerprint
                || self.read_statement(&by).is_ok_and(|(s, sby)| {
                    s.supersedes == fingerprint && sby == fingerprint && !s.revoked
                });
            if !ok {
                return Err(format!(
                    "the revocation of {fingerprint} is signed by {by}, neither the key nor a successor"
                ));
            }
            info.revoked = true;
            info.revoked_by = by.clone();
        }
        if !k.supersedes.is_empty() {
            if !k.revoked && by != k.supersedes {
                return Err(format!(
                    "the statement of {fingerprint} succeeds {} but is signed by {by}",
                    k.supersedes
                ));
            }
            let prev = self.key_info_depth(&k.supersedes, depth + 1)?;
            if prev.revoked && prev.revoked_by != fingerprint {
                return Err(format!(
                    "{fingerprint} succeeds {}, which is revoked",
                    k.supersedes
                ));
            }
            info.root = prev.root;
        } else if !k.revoked && by != fingerprint {
            return Err(format!(
                "the statement of {fingerprint} is signed by {by}, not by itself"
            ));
        }
        Ok(info)
    }

    /// A key's statement, if its standing holds and it is not revoked.
    pub fn key_statement(&self, fingerprint: &str) -> Result<KeyStatement, String> {
        let k = self.key_info(fingerprint)?;
        if k.revoked {
            return Err(format!("the key {fingerprint} is revoked"));
        }
        Ok(k.statement)
    }

    /// Read and check a version.
    pub fn version(&self, manifest_hash: &str) -> Result<Version, String> {
        let bytes = self.doc(manifest_hash)?;
        let m: Manifest = crate::json::parse_as(&bytes, "manifest", true)?;
        check_manifest(&m)?;
        let signed = self.check_signatures(manifest_hash, &bytes, &m)?;
        let mut tables = BTreeMap::new();
        let mut indexes = vec![];
        for (name, h) in &m.tables {
            let Some(def) = tables::table(name) else {
                // A table this reader does not know: ignored (a later minor version's).
                continue;
            };
            let mut tp = TableParts {
                index: h.clone(),
                ..Default::default()
            };
            let mut seen = HashSet::new();
            self.flatten(def, h, 0, &mut tp, &mut indexes, &mut seen)?;
            // A part listed twice (by two child indexes) counts once.
            let mut objs = HashSet::new();
            tp.parts.retain(|(p, _)| objs.insert(p.object.clone()));
            // Number each partition's parts for an export's file names.
            let mut count: HashMap<BTreeMap<String, String>, usize> = HashMap::new();
            for (part, path) in tp.parts.iter_mut() {
                let n = count.entry(part.partition.clone()).or_default();
                *n += 1;
                *path = match self {
                    Source::Store(s) => s.object_path(&part.object, "parquet")?,
                    Source::Export(r) => r.join(export_path(name, &part.partition, *n)),
                };
            }
            tables.insert(name.clone(), tp);
        }
        let mut searches = BTreeMap::new();
        for (uuid, h) in &m.searches {
            if !safe_uuid(uuid) {
                return Err(format!("search uuid '{uuid}' is not a uuid"));
            }
            let sp = self.search(uuid, h, &mut indexes)?;
            searches.insert(uuid.clone(), sp);
        }
        Ok(Version {
            hash: manifest_hash.to_string(),
            bytes,
            manifest: m,
            signed,
            tables,
            searches,
            indexes,
        })
    }

    fn flatten(
        &self,
        def: &TableDef,
        h: &str,
        depth: usize,
        out: &mut TableParts,
        indexes: &mut Vec<(String, Vec<u8>)>,
        seen: &mut HashSet<String>,
    ) -> Result<(), String> {
        if depth >= MAX_INDEX_DEPTH {
            return Err(format!(
                "{}: indexes nested more than {MAX_INDEX_DEPTH} deep",
                def.name
            ));
        }
        if !seen.insert(h.to_string()) {
            // An index reached twice (a child listed by two indexes) is read once.
            return Ok(());
        }
        let b = self.doc(h)?;
        let ix: Index = crate::json::parse_as(&b, &format!("{} index", def.name), true)?;
        docs::check_format(&ix.format, &ix.version, "jetcarp-index")?;
        if ix.table != def.name {
            return Err(format!(
                "index of table '{}' where {} was expected",
                ix.table, def.name
            ));
        }
        if ix.schema != def.schema {
            return Err(format!(
                "{}: schema '{}' (this JETCARP reads '{}')",
                def.name, ix.schema, def.schema
            ));
        }
        if ix.parts.len() > MAX_PARTS || ix.children.len() > MAX_PARTS {
            return Err(format!(
                "{}: an index with more than {MAX_PARTS} parts",
                def.name
            ));
        }
        out.schema = ix.schema.clone();
        indexes.push((h.to_string(), b));
        for p in &ix.parts {
            hash::hex_of(&p.object)?;
            if p.bytes > MAX_OBJECT {
                return Err(format!("{}: a part larger than 4 GiB", def.name));
            }
            if p.partition
                .keys()
                .map(String::as_str)
                .ne(def.partition.iter().copied())
            {
                return Err(format!(
                    "{}: a part's partition names other columns",
                    def.name
                ));
            }
            if p.partition
                .values()
                .any(|v| !tables::valid_partition_value(v))
            {
                return Err(format!(
                    "{}: a partition value that is not a safe name",
                    def.name
                ));
            }
            out.parts.push((p.clone(), PathBuf::new()));
        }
        for c in &ix.children {
            self.flatten(def, c, depth + 1, out, indexes, seen)?;
        }
        Ok(())
    }

    fn search(
        &self,
        uuid: &str,
        h: &str,
        indexes: &mut Vec<(String, Vec<u8>)>,
    ) -> Result<SearchParts, String> {
        let mut top: Option<SearchIndex> = None;
        let mut queue = vec![(h.to_string(), 0usize)];
        let mut seen = HashSet::new();
        while let Some((h, depth)) = queue.pop() {
            if depth >= MAX_INDEX_DEPTH || !seen.insert(h.clone()) {
                return Err(format!(
                    "search {uuid}: indexes nested too deeply or in a loop"
                ));
            }
            let b = self.doc(&h)?;
            let ix: SearchIndex = crate::json::parse_as(&b, "search index", true)?;
            docs::check_format(&ix.format, &ix.version, "jetcarp-index")?;
            if ix.table != "search" || ix.schema != "jetcarp-search 1" || ix.search != uuid {
                return Err(format!("search {uuid}: its index is not this search's"));
            }
            if ix.parts.len() > MAX_PARTS || ix.children.len() > MAX_PARTS {
                return Err(format!(
                    "search {uuid}: an index with more than {MAX_PARTS} parts"
                ));
            }
            for p in &ix.parts {
                hash::hex_of(&p.object)?;
                if !safe_segment(&p.segment) {
                    return Err(format!("search {uuid}: segment name '{}'", p.segment));
                }
            }
            for s in &ix.scores {
                for p in &s.parts {
                    hash::hex_of(&p.object)?;
                    if !safe_segment(&p.segment) {
                        return Err(format!("search {uuid}: segment name '{}'", p.segment));
                    }
                }
            }
            for c in ix.children.iter().rev() {
                queue.push((c.clone(), depth + 1));
            }
            indexes.push((h.clone(), b));
            match &mut top {
                None => top = Some(ix),
                Some(t) => {
                    t.parts.extend(ix.parts);
                    t.scores.extend(ix.scores);
                }
            }
        }
        let index = top.ok_or("empty search index")?;
        let run_files = index
            .parts
            .iter()
            .map(|p| match self {
                Source::Store(s) => s.object_path(&p.object, "parquet"),
                Source::Export(r) => Ok(r.join(export_run_path(uuid, &p.segment))),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let score_files = index
            .scores
            .iter()
            .map(|v| {
                v.parts
                    .iter()
                    .map(|p| match self {
                        Source::Store(s) => s.object_path(&p.object, "parquet"),
                        Source::Export(r) => {
                            Ok(r.join(export_score_path(uuid, v.version, &p.segment)))
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(SearchParts {
            index_hash: h.to_string(),
            index,
            run_files,
            score_files,
        })
    }

    fn check_signatures(&self, h: &str, bytes: &[u8], m: &Manifest) -> Result<Signed, String> {
        let mut out = Signed::default();
        let sigs: Vec<(String, String)> = match self {
            Source::Store(s) => s.signatures(h)?,
            Source::Export(r) => {
                let mut v = vec![];
                if let Ok(rd) = std::fs::read_dir(r.join("sigs")) {
                    for e in rd.flatten().take(1000) {
                        let n = e.file_name().to_string_lossy().to_string();
                        if let Some(stem) = n.strip_suffix(".sig") {
                            let b = store::read_limited(&e.path(), sign::MAX_SIG)?;
                            v.push((stem.to_string(), String::from_utf8_lossy(&b).to_string()));
                        }
                    }
                }
                v.sort();
                v
            }
        };
        let key = self.key_info(&m.publisher);
        match &key {
            Ok(k) => {
                out.key_statement = true;
                out.publisher_id = k.root.clone();
                out.name = k.statement.name.clone();
                out.orcid = k.statement.orcid.clone();
                if k.revoked {
                    out.revoked.push(m.publisher.clone());
                }
            }
            Err(e) => out.bad.push(format!("key statement: {e}")),
        }
        for (stem, pem) in sigs {
            match sign::verify(&pem, sign::NS_MANIFEST, bytes) {
                Ok(fp) => {
                    if sign::signer_name(&fp) != stem {
                        out.bad.push(format!("{stem}.sig is signed by {fp}"));
                        continue;
                    }
                    if let Ok(k) = &key
                        && !k.revoked
                        && authorised(&k.statement, &fp)
                    {
                        out.by_publisher = true;
                        out.publisher_signers.push(fp.clone());
                    }
                    out.signers.push(fp);
                }
                Err(e) => out.bad.push(format!("{stem}.sig: {e}")),
            }
        }
        if key.is_err() && out.signers.is_empty() {
            // An unsigned version without key statements: nothing is wrong, nothing is known.
            out.bad.clear();
        }
        Ok(out)
    }

    /// A file of the store or export as a path within it (`tables/rules/part-000001.parquet`),
    /// for messages.
    pub fn within(&self, path: &Path) -> String {
        path.strip_prefix(self.root())
            .unwrap_or(path)
            .display()
            .to_string()
            .replace('\\', "/")
    }

    /// A part's bytes, checked against its hash and size. An error names the part's hash and its
    /// file within the store or export (`tables/rules/part-000001.parquet`).
    pub fn part_bytes(&self, part: &Part, path: &Path) -> Result<Bytes, String> {
        let file = self.within(path);
        let b = store::read_limited(path, MAX_OBJECT.min(part.bytes.max(1)))?;
        if b.len() as u64 != part.bytes {
            return Err(format!(
                "part {} ({file}) is {} bytes, its index says {}",
                part.object,
                b.len(),
                part.bytes
            ));
        }
        if hash::of(&b) != part.object {
            return Err(format!("part {} ({file}) is altered", part.object));
        }
        Ok(Bytes::from(b))
    }

    /// Every row of a table's parts, a batch at a time (`f` gets the part's index too).
    pub fn table_rows(
        &self,
        v: &Version,
        table: &str,
        mut f: impl FnMut(usize, Vec<Value>) -> Result<(), String>,
    ) -> Result<u64, String> {
        let Some(tp) = v.tables.get(table) else {
            return Ok(0);
        };
        let def = tables::table(table).ok_or("unknown table")?;
        let mut n = 0;
        for (i, (part, path)) in tp.parts.iter().enumerate() {
            let b = self.part_bytes(part, path)?;
            let k = crate::parquet_io::read_rows(b, &def.cols, |rows| f(i, rows))
                .map_err(|e| format!("{table} part {}: {e}", part.object))?;
            if k != part.rows {
                return Err(format!(
                    "{table} part {} has {k} rows, its index says {}",
                    part.object, part.rows
                ));
            }
            n += k;
        }
        Ok(n)
    }

    /// A media or reference-patch object's bytes, checked (`rel` is its export path).
    pub fn object(&self, h: &str, ext: &str, rel: &str, max: u64) -> Result<Vec<u8>, String> {
        match self {
            Source::Store(s) => s.get(h, ext, max),
            Source::Export(r) => {
                let b = store::read_limited(&r.join(rel), max)?;
                if hash::of(&b) != h {
                    return Err(format!("{rel} is altered"));
                }
                Ok(b)
            }
        }
    }

    /// The path of an object (store) or file (export), unchecked.
    pub fn object_file(&self, h: &str, ext: &str, rel: &str) -> Result<PathBuf, String> {
        match self {
            Source::Store(s) => s.object_path(h, ext),
            Source::Export(r) => Ok(r.join(rel)),
        }
    }
}

/// The ref's or key statement's bytes as signed: its canonical JSON without `signature`.
pub fn unsigned_bytes(b: &[u8]) -> Result<Vec<u8>, String> {
    let mut v = crate::json::parse(b, "signed document", false)?;
    if let Value::Object(m) = &mut v {
        m.remove("signature");
    }
    crate::json::canonical(&v)
}

/// May this key sign for the publisher whose statement this is: the key itself, or a machine
/// key its statement authorises (and has not retired)?
pub fn authorised(k: &KeyStatement, fp: &str) -> bool {
    fp == k.fingerprint
        || k.authorised
            .iter()
            .any(|a| !a.revoked && a.fingerprint == fp)
}

/// A key's standing ([`Source::key_info`]).
#[derive(Clone, Debug)]
pub struct KeyInfo {
    pub statement: KeyStatement,
    /// The publisher's id: the first key of the chain.
    pub root: String,
    pub revoked: bool,
    pub revoked_by: String,
}

/// A manifest's format, version and `requires`, and its names.
pub fn check_manifest(m: &Manifest) -> Result<(), String> {
    docs::check_format(&m.format, &m.version, crate::FORMAT)?;
    for r in &m.requires {
        if !KNOWN_FEATURES.contains(&r.as_str()) {
            return Err(format!(
                "this pack requires '{r}', which this JETCARP does not understand"
            ));
        }
    }
    if !docs::valid_feed(&m.feed) {
        return Err(format!(
            "feed name '{}' is not letters, digits and -",
            m.feed
        ));
    }
    if !sign::valid_fingerprint(&m.publisher) {
        return Err(format!(
            "publisher '{}' is not a key fingerprint",
            m.publisher
        ));
    }
    for p in m
        .parents
        .iter()
        .chain(&m.retain)
        .chain(m.tables.values())
        .chain(m.searches.values())
    {
        hash::hex_of(p)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Latest wins

/// Of two annotation rows for one target, publisher and field, is `b` the one a reader shows
/// rather than `a`? The higher `rev` (the publisher's ref `seq` of the version that added the
/// row); on a tie (versions on parallel branches), the later `date`, then the greater `value`
/// by its UTF-8 bytes.
pub fn later_annotation(a: &Value, b: &Value) -> bool {
    let ra = a["rev"].as_i64().unwrap_or(0);
    let rb = b["rev"].as_i64().unwrap_or(0);
    if ra != rb {
        return rb > ra;
    }
    let da = a["date"].as_str().unwrap_or("");
    let db = b["date"].as_str().unwrap_or("");
    if da != db {
        return db > da;
    }
    b["value"].as_str().unwrap_or("").as_bytes() > a["value"].as_str().unwrap_or("").as_bytes()
}
