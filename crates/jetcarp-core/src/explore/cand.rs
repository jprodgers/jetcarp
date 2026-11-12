//! A candidate: one configuration to try.

use super::space::Fam;
use crate::rules::Rule;
use crate::topology::{Nb, TilingSpec};
use crate::world::{CaSpec, Scene, SeedSpec};
use serde::{Deserialize, Serialize};

/// One thing to try.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cand {
    pub tiling: String,
    pub nb: Nb,
    pub fam: Fam,
    /// Rule notation.
    pub rule: String,
    pub seed: SeedSpec,
    pub cells: u32,
    pub gens: u32,
    pub steps: u32,
    #[serde(with = "crate::json_int::uint")]
    pub rng: u64,
    /// How this candidate was made (`random`, `mutate`, `seed`, `transplant`, `cover`, `named`).
    pub origin: String,
}

impl Cand {
    /// A scene that reproduces this run (on the grid `spec`).
    pub fn scene(&self, spec: &TilingSpec) -> Scene {
        let mut s = Scene::new(spec.clone());
        s.rng_state = self.rng;
        s.cas = vec![self.ca()];
        s
    }

    pub fn ca(&self) -> CaSpec {
        let mut ca = CaSpec::new(&self.rule)
            .seed(self.seed.clone())
            .steps(self.steps.max(1));
        if self.fam.nb_matters() {
            ca = ca.nb(self.nb);
        }
        ca
    }

    pub fn rule(&self) -> Option<Rule> {
        Rule::parse(&self.rule).ok().map(|(r, _)| r)
    }

    /// A 64-bit identity for deduplication: FNV-1a over the tiling, neighbourhood, rule, seed,
    /// size, and random state, stable across platforms and Rust versions (the search keeps the
    /// identities it has tried in its working database).
    pub fn id_hash(&self) -> u64 {
        let mut h = crate::rng::Fnv::default();
        let text = |h: &mut crate::rng::Fnv, s: &str| {
            h.bytes(s.as_bytes());
            h.bytes(&[0xff]);
        };
        text(&mut h, &self.tiling);
        h.bytes(&[self.nb as u8]);
        text(&mut h, &self.rule);
        text(
            &mut h,
            &serde_json::to_string(&self.seed).unwrap_or_default(),
        );
        h.bytes(&self.cells.to_le_bytes());
        h.u64(self.rng);
        h.0
    }
}
