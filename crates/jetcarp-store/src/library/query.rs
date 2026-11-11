//! Changes to a pattern ([`Update`]) and filters for listing patterns ([`Query`]), for the
//! catalog's [`Library::update`](super::Library::update) and
//! [`Library::query`](super::Library::query).

use jetcarp_core::pattern::{CurationState, DiscoveredBy, PatternClass, PatternRecord, range};
use jetcarp_core::topology::Nb;
use serde::{Deserialize, Serialize};

/// Changes for [`Library::update`]. `None` leaves a field alone.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Update {
    /// New name (empty string clears it).
    #[serde(default)]
    pub name: Option<String>,
    /// Replace all tags.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub add_tags: Vec<String>,
    #[serde(default)]
    pub remove_tags: Vec<String>,
    /// Replace the notes.
    #[serde(default)]
    pub notes: Option<String>,
    /// Append a paragraph to the notes.
    #[serde(default)]
    pub append_notes: Option<String>,
    /// 1 to 5, or 0 to clear.
    #[serde(default)]
    pub rating: Option<u8>,
    #[serde(default)]
    pub state: Option<CurationState>,
    #[serde(default)]
    pub discovered_by: Option<DiscoveredBy>,
}

/// Filters for [`Library::query`]. Every set field must match.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Query {
    /// Patterns verified in a rule whose notation is this or contains it (case-insensitive;
    /// `B3/S23` matches `b3/s23`).
    #[serde(default)]
    pub rule: Option<String>,
    /// Patterns that work in this rule: its notation is inside the pattern's rule range (B/S and
    /// Generations), or the pattern was verified in it. A `V` suffix (or `neighborhood`) picks
    /// edge neighbours; B/S rules default to touching neighbours.
    #[serde(default)]
    pub works_in: Option<String>,
    /// Only this neighbourhood (`touch`, `edge`).
    #[serde(default)]
    pub neighborhood: Option<Nb>,
    /// Tiling family id (`hex`), catalog id (`k2-001`), or a substring of the tiling name.
    #[serde(default)]
    pub tiling: Option<String>,
    #[serde(default)]
    pub class: Option<PatternClass>,
    #[serde(default)]
    pub min_period: Option<u64>,
    #[serde(default)]
    pub max_period: Option<u64>,
    #[serde(default)]
    pub min_score: Option<f64>,
    #[serde(default)]
    pub state: Option<CurationState>,
    #[serde(default)]
    pub tag: Option<String>,
    /// Substring of the name or label.
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub discovered_by: Option<DiscoveredBy>,
    /// Sort order: `score` (default, highest first), `seen`, `rare` (fewest sightings first),
    /// `newest`, `updated`, `cells`, `period`, `rating`, `id`.
    #[serde(default)]
    pub order: Option<String>,
    /// At most this many (after sorting).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Skip this many first (paging).
    #[serde(default)]
    pub offset: Option<usize>,
}

impl Query {
    /// The same test as the catalog's SQL, on a record in memory (ignores order, limit, offset).
    pub fn matches(&self, r: &PatternRecord) -> bool {
        let norm = |s: &str| s.to_ascii_lowercase().replace(' ', "");
        let rules = || std::iter::once(&r.rule).chain(r.rules.iter());
        if let Some(q) = &self.rule {
            let a = norm(q);
            if !rules().any(|b| norm(b).contains(&a)) {
                return false;
            }
        }
        if self.neighborhood.is_some_and(|n| n != r.neighborhood) {
            return false;
        }
        if let Some(q) = &self.works_in {
            let Ok((rule, nb)) = jetcarp_core::rules::Rule::parse(q) else {
                return false;
            };
            let n = rule.notation();
            let exact = rules().any(|b| *b == n);
            let ok = match range::bs_parts(&rule) {
                Some(_) => {
                    self.neighborhood.or(nb).unwrap_or(Nb::Touch) == r.neighborhood
                        && (exact || r.range.as_ref().is_some_and(|x| x.contains(&rule)))
                }
                None => exact,
            };
            if !ok {
                return false;
            }
        }
        if let Some(q) = &self.tiling {
            let q = q.to_ascii_lowercase();
            let cat = r
                .tiling
                .params
                .as_ref()
                .and_then(|p| p.get("catalog"))
                .and_then(|v| v.as_str());
            let qc = q.trim_start_matches("catalog:");
            // A family or catalog id matches exactly ("hex" is not "trihex"); other text matches
            // part of the tiling name.
            let ok = if jetcarp_core::families::family(&q).is_some()
                || jetcarp_core::catalog::get(qc).is_some()
            {
                (r.tiling.family.eq_ignore_ascii_case(&q) && cat.is_none())
                    || cat.is_some_and(|c| c.eq_ignore_ascii_case(qc))
            } else {
                r.tiling_name.to_ascii_lowercase().contains(&q)
            };
            if !ok {
                return false;
            }
        }
        if self.class.is_some_and(|c| c != r.class)
            || self.min_period.is_some_and(|p| r.period < p)
            || self.max_period.is_some_and(|p| r.period > p)
            || self.min_score.is_some_and(|s| r.score < s)
            || self.state.is_some_and(|s| s != r.state)
            || self.discovered_by.is_some_and(|d| d != r.discovered_by)
        {
            return false;
        }
        if let Some(t) = &self.tag
            && !r.tags.iter().any(|x| x.eq_ignore_ascii_case(t))
        {
            return false;
        }
        if let Some(t) = &self.text
            && !r
                .label()
                .to_ascii_lowercase()
                .contains(&t.to_ascii_lowercase())
        {
            return false;
        }
        true
    }
}

/// Highest score first, then rating, then id.
pub fn sort_records(v: &mut [PatternRecord]) {
    v.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.rating.cmp(&a.rating))
            .then(a.id.cmp(&b.id))
    });
}
