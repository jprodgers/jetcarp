//! The JSON documents: the store marker, manifests, indexes (table and search), refs and key
//! statements. Reading ignores fields it does not know (a higher minor version may add them);
//! writing goes through [`crate::json::canonical_of`].

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// The format's major and minor version, as documents write it.
pub const VERSION: &str = "1.0";
/// The major version this reader understands.
pub const MAJOR: u32 = 1;

/// `jetcarp-store.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoreMarker {
    pub format: String,
    pub version: String,
}

/// A creator, DataCite-shaped.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Creator {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub orcid: String,
    #[serde(default)]
    pub role: String,
}

/// A related identifier (`IsSupplementTo`, `doi:...`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Related {
    pub relation: String,
    pub id: String,
}

/// What made the data.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Engine {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub schemes: BTreeMap<String, String>,
    #[serde(default)]
    pub rule_semantics: BTreeMap<String, String>,
}

/// A record withdrawn by the publisher.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Retract {
    pub target: String,
    /// Whose assertions it withdraws: the publisher id of the version that first listed it.
    #[serde(default)]
    pub publisher: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub date: String,
}

/// One version of a pack.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub version: String,
    #[serde(default)]
    pub requires: Vec<String>,
    pub feed: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version_label: String,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub licence: String,
    #[serde(default)]
    pub creators: Vec<Creator>,
    #[serde(default)]
    pub doi: String,
    #[serde(default)]
    pub related: Vec<Related>,
    #[serde(default)]
    pub parents: Vec<String>,
    pub publisher: String,
    #[serde(default)]
    pub engine: Engine,
    #[serde(default)]
    pub tables: BTreeMap<String, String>,
    #[serde(default)]
    pub searches: BTreeMap<String, String>,
    #[serde(default)]
    pub retracts: Vec<Retract>,
    #[serde(default)]
    pub retain: Vec<String>,
    #[serde(default)]
    pub stats: BTreeMap<String, u64>,
    /// A slice of another manifest (section "Slices"): informational.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slice_of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}

/// One Parquet part of a table.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Part {
    pub object: String,
    pub rows: u64,
    pub bytes: u64,
    #[serde(default)]
    pub partition: BTreeMap<String, String>,
    #[serde(default)]
    pub first: String,
    #[serde(default)]
    pub last: String,
}

/// A table's index.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub format: String,
    pub version: String,
    pub table: String,
    pub schema: String,
    #[serde(default)]
    pub parts: Vec<Part>,
    #[serde(default)]
    pub children: Vec<String>,
    /// Reserved (compaction): parts whose rows this index's parts hold again. Never written by
    /// this JETCARP; read and ignored.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compacts: Vec<String>,
}

/// One run segment of a search.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchPart {
    pub object: String,
    pub rows: u64,
    pub bytes: u64,
    /// The segment's name in a search folder: `part-000001`.
    pub segment: String,
    /// Its first and last run ids, as decimal strings.
    pub first: String,
    pub last: String,
    #[serde(default)]
    pub sealed: String,
    #[serde(default)]
    pub engine_version: String,
}

/// One segment's scores from a rescore.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScorePart {
    pub segment: String,
    pub object: String,
    pub rows: u64,
    pub bytes: u64,
}

/// One rescore.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScoreVersion {
    pub version: u64,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub engine_version: String,
    #[serde(default)]
    pub parts: Vec<ScorePart>,
}

/// A search's index (`table: "search"`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchIndex {
    pub format: String,
    pub version: String,
    pub table: String,
    pub schema: String,
    pub search: String,
    pub kind: String,
    pub run_schema: String,
    pub score_schema: String,
    #[serde(default)]
    pub engine_version: String,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub updated: String,
    #[serde(default)]
    pub spec: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remeasure: Option<Value>,
    #[serde(default)]
    pub parts: Vec<SearchPart>,
    #[serde(default)]
    pub scores: Vec<ScoreVersion>,
    #[serde(default)]
    pub children: Vec<String>,
}

/// A feed's signed pointer to its newest manifest.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ref {
    pub format: String,
    pub version: String,
    pub feed: String,
    pub seq: u64,
    pub manifest: String,
    #[serde(default)]
    pub updated: String,
    pub publisher: String,
    #[serde(default)]
    pub signature: String,
}

/// A publisher's key statement.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct KeyStatement {
    pub format: String,
    pub version: String,
    /// The OpenSSH public key (`ssh-ed25519 AAAA...`).
    pub key: String,
    pub fingerprint: String,
    /// Who the publisher is, only if they chose to be named.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub orcid: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub github: String,
    #[serde(default)]
    pub created: String,
    /// The fingerprint of the key this one succeeds (then the statement is signed by that key).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub supersedes: String,
    #[serde(default)]
    pub revoked: bool,
    /// Further keys authorised to sign manifests and refs for this publisher (one per machine).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authorised: Vec<AuthorisedKey>,
    #[serde(default)]
    pub signature: String,
}

/// A machine key a publisher's statement authorises.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AuthorisedKey {
    /// The OpenSSH public key.
    pub key: String,
    pub fingerprint: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// Retired: it signs nothing (its entry stays, so the list only grows).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub revoked: bool,
}

/// `major.minor` of a version string.
pub fn parse_version(v: &str) -> Result<(u32, u32), String> {
    let (a, b) = v
        .split_once('.')
        .ok_or_else(|| format!("version '{v}' is not major.minor"))?;
    let num = |s: &str| {
        (!s.is_empty() && s.len() <= 9 && s.bytes().all(|c| c.is_ascii_digit()))
            .then(|| s.parse::<u32>().ok())
            .flatten()
            .ok_or_else(|| format!("version '{v}' is not major.minor"))
    };
    Ok((num(a)?, num(b)?))
}

/// Refuse a document of another format, or of a major version this reader does not know.
pub fn check_format(format: &str, version: &str, want: &str) -> Result<(), String> {
    if format != want {
        return Err(format!("format is '{format}', expected '{want}'"));
    }
    let (major, _) = parse_version(version)?;
    if major != MAJOR {
        return Err(format!(
            "{want} version {version}: this JETCARP reads major version {MAJOR} only"
        ));
    }
    Ok(())
}

/// Is a feed name letters, digits and `-` (1 to 64 of them)?
pub fn valid_feed(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
