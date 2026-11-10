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

    /// A 64-bit identity for deduplication.
    pub fn id_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.tiling.hash(&mut h);
        (self.nb as u8).hash(&mut h);
        self.rule.hash(&mut h);
        serde_json::to_string(&self.seed)
            .unwrap_or_default()
            .hash(&mut h);
        self.cells.hash(&mut h);
        self.rng.hash(&mut h);
        h.finish()
    }
}
