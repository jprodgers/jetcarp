//! The state hash, in two versions (#173).
//!
//! - **Version 2** (`state_hash_v2`, what [`World::hash_hex`] gives and records carry since
//!   0.4.3): SHA-256, 256 bits, over everything about a world that decides what it does next,
//!   and a digest of what the scene fixes (the tiling), so that two different worlds do not share
//!   a hash. Written as 64 hexadecimal digits. Each kind of state is destructured field by field
//!   here, so a field added to [`World`], [`CaInstance`], [`Field`] or [`ScriptState`] fails to
//!   compile until it is hashed, or marked as fixed by the scene, derived, display-only or not
//!   reproduced by a restore.
//! - **Version 1** (`state_hash_v1`, JETCARP 0.4.0 to 0.4.2): FNV-1a, 64 bits, over the fields and
//!   part of the CAs' bookkeeping, 16 hexadecimal digits. Kept unchanged, so that everything
//!   recorded with it (scene records, figures, the signed starter pack) still verifies: a record
//!   names the version it was made with, and no version means 1.
//!
//! What a world does next depends only on its present state, so its history (the command log)
//! is not hashed, and two command sequences reaching the same state hash alike. A piece that a
//! world reopened from its saved scene (a replay of its log) cannot reproduce exactly is left out
//! too, and marked so: the hash of a reopened world must equal the hash of the world that ran
//! straight through.

use super::{Agent, CaInstance, Field, PeriodWatch, Snapshot, World};
use crate::reseed::PolicyStatus;
use crate::rng::{Fnv, Rng};
use crate::script::ScriptState;
use crate::sha256::{Sha256, hex};

/// The state-hash version a world's records are written with.
pub const STATE_HASH_VERSION: u32 = 2;

/// A version of the state hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashVersion {
    /// FNV-1a, 16 hexadecimal digits (before 0.4.3).
    V1,
    /// SHA-256 over every piece of state, 64 hexadecimal digits.
    V2,
}

impl HashVersion {
    /// The version a record names (`0`, absent in older files, means 1).
    pub fn of_record(v: u32) -> Result<HashVersion, String> {
        match v {
            0 | 1 => Ok(HashVersion::V1),
            2 => Ok(HashVersion::V2),
            v => Err(format!(
                "state-hash version {v} is not one this JETCARP knows (1 or 2)"
            )),
        }
    }

    /// The version a hash is written in, by its length: 16 hexadecimal digits are version 1,
    /// 64 are version 2, and anything else is refused.
    pub fn of_hex(hash: &str) -> Result<HashVersion, String> {
        let h = hash.trim();
        if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("'{h}' is not a state hash: it is not hexadecimal"));
        }
        match h.len() {
            16 => Ok(HashVersion::V1),
            64 => Ok(HashVersion::V2),
            n => Err(format!(
                "'{h}' is not a state hash: a hash has 16 hexadecimal digits (version 1) or 64 \
                 (version 2), and this one has {n}"
            )),
        }
    }

    /// The number a record stores.
    pub fn number(self) -> u32 {
        match self {
            HashVersion::V1 => 1,
            HashVersion::V2 => 2,
        }
    }
}

impl super::Record {
    /// Whether `w` is at this record's generation with its hash, in the version the record
    /// names. A version this JETCARP does not know is an error.
    pub fn matches(&self, w: &World) -> Result<bool, String> {
        let v = HashVersion::of_record(self.hash_version)?;
        Ok(w.generation == self.generation
            && w.hash_hex_as(v) == self.hash.trim().to_ascii_lowercase())
    }
}

/// SHA-256 over typed, length-prefixed items.
struct Hasher {
    h: Sha256,
    buf: Vec<u8>,
}

impl Hasher {
    fn new(tag: &str) -> Hasher {
        let mut h = Hasher {
            h: Sha256::new(),
            buf: Vec::with_capacity(1 << 16),
        };
        h.str(tag);
        h
    }
    fn flush(&mut self) {
        self.h.update(&self.buf);
        self.buf.clear();
    }
    fn raw(&mut self, b: &[u8]) {
        if self.buf.len() + b.len() > self.buf.capacity() {
            self.flush();
        }
        if b.len() >= self.buf.capacity() {
            self.h.update(b);
        } else {
            self.buf.extend_from_slice(b);
        }
    }
    fn u8(&mut self, x: u8) {
        self.raw(&[x]);
    }
    fn bool(&mut self, x: bool) {
        self.u8(x as u8);
    }
    fn u32(&mut self, x: u32) {
        self.raw(&x.to_le_bytes());
    }
    fn u64(&mut self, x: u64) {
        self.raw(&x.to_le_bytes());
    }
    fn len(&mut self, n: usize) {
        self.u64(n as u64);
    }
    fn bytes(&mut self, b: &[u8]) {
        self.len(b.len());
        self.raw(b);
    }
    fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
    fn u16s(&mut self, v: &[u16]) {
        self.len(v.len());
        for x in v {
            self.raw(&x.to_le_bytes());
        }
    }
    fn u32s(&mut self, v: &[u32]) {
        self.len(v.len());
        for x in v {
            self.raw(&x.to_le_bytes());
        }
    }
    /// By their exact bits.
    fn f32s(&mut self, v: &[f32]) {
        self.len(v.len());
        for x in v {
            self.raw(&x.to_bits().to_le_bytes());
        }
    }
    fn opt_u64(&mut self, x: Option<u64>) {
        self.bool(x.is_some());
        self.u64(x.unwrap_or(0));
    }
    /// A value of a serde type, as canonical JSON (RFC 8785).
    fn json<T: serde::Serialize>(&mut self, v: &T) {
        let v = serde_json::to_value(v).unwrap_or(serde_json::Value::Null);
        self.str(&crate::jcs::to_string_safe(&v));
    }
    fn finish(mut self) -> [u8; 32] {
        self.flush();
        self.h.finish()
    }
}

fn agents(h: &mut Hasher, v: &[Agent]) {
    h.len(v.len());
    for a in v {
        let Agent {
            cell,
            heading,
            state,
        } = a;
        h.u32(*cell);
        h.u32(*heading as u32);
        h.u8(*state);
    }
}

fn field(h: &mut Hasher, f: &Field) {
    let Field {
        kind,
        shared,
        state,
        age,
        height,
        value,
        lost,
        // Scratch buffers of the stepper.
        back: _,
        back_age: _,
        back_h: _,
        back_v: _,
    } = f;
    h.json(kind);
    h.bool(*shared);
    h.bytes(state);
    h.u16s(age);
    h.u32s(height);
    h.f32s(value);
    h.u64(*lost);
}

fn ca(h: &mut Hasher, c: &CaInstance) {
    let CaInstance {
        id,
        // Display only: what the CA is called and how it is drawn.
        name: _,
        rule,
        nb,
        enabled,
        affects_others,
        steps_per_gen,
        palette: _,
        seed,
        drop_per_gen,
        field,
        agents: ants,
        cursor,
        // Derived: compiled from the world's tables by the rule's name.
        table: _,
        // Derived: from the tiling, the rule and its neighbourhood.
        reach: _,
        kernel: _,
        layers,
        noise,
        ticks,
        color_by: _,
        agent_edge,
        palette_neg: _,
        glow: _,
        // Display and status only (JACA's annihilation glow and counts).
        heat: _,
        // Derived: the rule run from the empty world on this tiling, `ticks` steps.
        background: _,
    } = c;
    h.u32(*id);
    h.str(&crate::rules::notation_on(rule, *nb));
    // The meaning of the rule: a family's new version changes the hash of its worlds only.
    h.str(crate::semantics::family(rule));
    h.str(crate::semantics::rule_version(rule));
    h.json(nb);
    h.bool(*enabled);
    h.bool(*affects_others);
    h.u32(*steps_per_gen);
    h.json(seed);
    h.u32(*drop_per_gen);
    h.len(*field);
    agents(h, ants);
    h.u32(*cursor);
    h.bool(layers.is_some());
    h.u32s(layers.as_deref().map_or(&[][..], |l| &l[..]));
    h.u64(*noise);
    h.u64(*ticks);
    h.json(agent_edge);
}

fn script_state(h: &mut Hasher, s: &ScriptState) {
    let ScriptState {
        step,
        started,
        streak,
        finished,
        last_activity,
    } = s;
    h.len(*step);
    h.u64(*started);
    h.u64(*streak);
    h.bool(*finished);
    h.opt_u64(last_activity.map(f64::to_bits));
}

fn policy(h: &mut Hasher, p: &PolicyStatus) {
    let PolicyStatus {
        since,
        streak,
        fired,
        last_fired,
        error,
    } = p;
    h.u64(*since);
    h.u64(*streak);
    h.u64(*fired);
    h.opt_u64(*last_fired);
    h.bool(error.is_some());
    h.str(error.as_deref().unwrap_or(""));
}

fn watch(h: &mut Hasher, w: &PeriodWatch) {
    let PeriodWatch {
        power,
        lam,
        snap,
        found,
    } = w;
    h.u64(*power);
    h.u64(*lam);
    h.bool(found.is_some());
    let (p, at) = found.unwrap_or((0, 0));
    h.u64(p);
    h.u64(at);
    // Once a repeat is found the watch stops, and its last snapshot no longer decides anything
    // (a GPU run does not keep it then).
    let snap = snap.as_ref().filter(|_| found.is_none());
    h.bool(snap.is_some());
    if let Some(s) = snap {
        let Snapshot {
            states,
            heights,
            agents: ants,
            cursors,
            phases,
            values,
        } = s;
        h.len(states.len());
        for s in states {
            h.bytes(s);
        }
        h.len(heights.len());
        for x in heights {
            h.u32s(x);
        }
        h.len(ants.len());
        for a in ants {
            agents(h, a);
        }
        h.u32s(cursors);
        h.len(phases.len());
        for p in phases {
            h.u64(*p);
        }
        h.len(values.len());
        for v in values {
            h.f32s(v);
        }
    }
}

impl World {
    /// Version 2 of the state hash (see the module notes): SHA-256 over every piece of state
    /// that decides what the world does next, after a digest of what the scene fixes.
    pub fn state_hash_v2(&self) -> [u8; 32] {
        let World {
            // Fixed by the scene: built from the tiling, which the definition digest holds.
            topo: _,
            fields,
            cas,
            generation,
            rng,
            next_id,
            // Fixed by the scene: the tiling goes into the definition digest. The rest are
            // starting values (RNG state, tables, CA specs, script, policies), superseded by the
            // present ones hashed below.
            base,
            // History, not state.
            log: _,
            watch: period,
            tables,
            script,
            script_state: ss,
            // Display only.
            palettes: _,
            // This process's revision counter.
            rev: _,
            // A front end's option (whether every generation's changes are counted).
            track_changes: _,
            // Not reproduced by a restore: counted only while changes are tracked, a trigger
            // or policy measures them, or the GPU counts them. The one decision that reads them
            // (a script waiting for `done`) makes the world count every generation's changes
            // itself, on the CPU and the GPU alike, so it reads the same however the world is
            // stepped.
            last_changed: _,
            last_moved: _,
            // A cache of a topology built ahead.
            prebuilt: _,
            reseed,
            reseed_status,
            // Not reproduced by a restore: edits a GPU run has planned but not yet applied,
            // which the stepper applies before the world is used again (`external`).
            pending_reseed: _,
            manual_reseeds,
            // Kept for every generation whose changes were counted, which depends on whether a
            // front end tracked them or the GPU counted them, so not reproduced by a restore as a
            // whole. What decides anything is hashed below: the generations each settled policy
            // measured itself, which its steadiness check reads (`measured`). A script's
            // activity trigger measures each generation itself (`ScriptState` is hashed).
            activity: _,
            // A cache.
            tile_size: _,
            // Messages for a front end.
            warnings: _,
        } = self;
        let mut d = Hasher::new("JETCARP scene definition v2");
        d.json(&base.tiling);
        let definition = d.finish();

        let mut h = Hasher::new("JETCARP state v2");
        h.raw(&definition);
        h.u64(*generation);
        let Rng { state } = rng;
        h.u64(*state);
        h.u32(*next_id);
        h.len(fields.len());
        for f in fields {
            field(&mut h, f);
        }
        h.len(cas.len());
        for c in cas {
            ca(&mut h, c);
        }
        let mut names: Vec<&String> = tables.tables.keys().collect();
        names.sort();
        h.len(names.len());
        for n in names {
            h.str(n);
            h.json(&tables.tables[n].table);
        }
        h.json(script);
        script_state(&mut h, ss);
        h.json(reseed);
        h.len(reseed_status.len());
        for p in reseed_status {
            policy(&mut h, p);
        }
        h.u64(*manual_reseeds);
        // What each settled policy's steadiness check will read.
        for (p, st) in reseed.iter().zip(reseed_status) {
            if let crate::reseed::ReseedWhen::Settled {
                for_gens,
                cycle: Some(c),
                ..
            } = p.when
            {
                let need = (for_gens.max(1) + c) as usize;
                let window = self.measured(st.since, st.since.saturating_add(p.cooldown), need);
                h.len(window.len());
                for a in window {
                    h.u64(a.generation);
                    h.u64(a.changed);
                }
            }
        }
        watch(&mut h, period);
        h.finish()
    }

    /// Version 1 of the state hash (JETCARP 0.4.0 to 0.4.2): FNV-1a over generation, every
    /// field, and part of every CA's dynamic state. Unchanged, for records that name it.
    pub fn state_hash_v1(&self) -> u64 {
        let mut h = Fnv::default();
        h.u64(self.generation);
        for f in &self.fields {
            h.bytes(format!("{:?}{}", f.kind, f.shared).as_bytes());
            h.bytes(&f.state);
            h.u16s(&f.age);
            h.u32s(&f.height);
            // Continuous values by their exact bits (empty for other fields).
            h.f32s(&f.value);
            h.u64(f.lost);
        }
        if self.script.is_some() {
            h.u64(self.script_state.step as u64);
            h.u64(self.script_state.started);
            h.u64(self.script_state.finished as u64);
        }
        for st in &self.reseed_status {
            h.u64(st.since);
            h.u64(st.streak);
            h.u64(st.fired);
        }
        for c in &self.cas {
            h.u64(c.id as u64);
            h.u64(c.cursor as u64);
            h.u64(c.ticks);
            h.u64(c.enabled as u64);
            for a in &c.agents {
                h.u64(a.cell as u64);
                h.u64(a.heading as u64);
            }
        }
        h.0
    }

    /// The state hash as records store it: version 2, 64 hexadecimal digits.
    pub fn hash_hex(&self) -> String {
        hex(&self.state_hash_v2())
    }

    /// Version 1 of the state hash, 16 hexadecimal digits.
    pub fn hash_hex_v1(&self) -> String {
        format!("{:016x}", self.state_hash_v1())
    }

    /// The state hash in a given version.
    pub fn hash_hex_as(&self, v: HashVersion) -> String {
        match v {
            HashVersion::V1 => self.hash_hex_v1(),
            HashVersion::V2 => self.hash_hex(),
        }
    }

    /// The first 16 digits of the version 2 hash, for status lines that a person only reads.
    pub fn hash_short(&self) -> String {
        let mut h = self.hash_hex();
        h.truncate(16);
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reseed::{AutoReseed, ReseedMode, ReseedWhen};
    use crate::script::{CaRef, Script, ScriptStep, Trigger};
    use crate::topology::{Boundary, TilingSpec};
    use crate::world::{CaSpec, Command};

    /// A world with a field of every kind, an ant, layers, a forest fire's noise, a script, a
    /// reseed policy and a table, a few generations in.
    fn world() -> World {
        let mut w = World::new(TilingSpec::rect("square", 16, 16, Boundary::Wrap), 5).unwrap();
        for rule in [
            "B3/S23",
            "Turmite LLRR",
            "Sandpile",
            "Lenia R=3,T=10,b=1,m=0.15,s=0.015",
            "Layer 2/01",
            "ForestFire 0.01/0.0001",
            "Table parity",
        ] {
            w.apply(Command::AddCa {
                ca: CaSpec::new(rule).private(),
            })
            .unwrap();
        }
        w.apply(Command::SetScript {
            script: Some(Script {
                steps: vec![ScriptStep {
                    name: "all".into(),
                    run: (1..=7).map(CaRef::Id).collect(),
                    until: Trigger::Generations { n: 1000 },
                    max_gens: None,
                }],
                repeat: false,
            }),
        })
        .unwrap();
        w.apply(Command::SetReseed {
            policies: vec![
                AutoReseed::new(ReseedMode::rain(), ReseedWhen::Every { gens: 3 }),
                AutoReseed::new(
                    ReseedMode::rain(),
                    ReseedWhen::Settled {
                        activity: 0.0,
                        for_gens: 2,
                        cycle: Some(2),
                        period: None,
                        latest: None,
                    },
                ),
            ],
        })
        .unwrap();
        w.apply(Command::Step { gens: 5 }).unwrap();
        w
    }

    // Every piece of state the hash covers changes it on its own; display-only pieces do not.
    #[test]
    fn every_piece_of_state_changes_the_hash() {
        let base = world();
        let h0 = base.state_hash_v2();
        let lenia = base
            .cas
            .iter()
            .position(|c| c.rule.field_kind() == crate::rules::FieldKind::Continuous)
            .unwrap();
        let ant = base.cas.iter().position(|c| !c.agents.is_empty()).unwrap();
        let layered = base.cas.iter().position(|c| c.layers.is_some()).unwrap();
        let pile = base
            .cas
            .iter()
            .position(|c| c.rule.field_kind() == crate::rules::FieldKind::Height)
            .unwrap();
        let (lf, pf) = (base.cas[lenia].field, base.cas[pile].field);
        type Change = Box<dyn Fn(&mut World)>;
        let changes: Vec<(&str, Change)> = vec![
            ("generation", Box::new(|w| w.generation += 1)),
            ("rng", Box::new(|w| w.rng.state ^= 1)),
            ("next_id", Box::new(|w| w.next_id += 1)),
            ("a cell's state", Box::new(|w| w.fields[0].state[3] ^= 1)),
            ("a cell's age", Box::new(|w| w.fields[0].age[3] += 1)),
            (
                "a grain count",
                Box::new(move |w| w.fields[pf].height[3] += 1),
            ),
            ("a value", Box::new(move |w| w.fields[lf].value[3] += 0.5)),
            ("lost grains", Box::new(move |w| w.fields[pf].lost += 1)),
            (
                "a field's sharing",
                Box::new(|w| w.fields[0].shared ^= true),
            ),
            ("a CA's id", Box::new(|w| w.cas[0].id += 100)),
            (
                "a rule",
                Box::new(|w| w.cas[0].rule = crate::rules::Rule::parse("B36/S23").unwrap().0),
            ),
            (
                "a neighbourhood",
                Box::new(|w| w.cas[0].nb = crate::topology::Nb::Edge),
            ),
            ("enabled", Box::new(|w| w.cas[0].enabled ^= true)),
            (
                "affects_others",
                Box::new(|w| w.cas[0].affects_others ^= true),
            ),
            (
                "steps per generation",
                Box::new(|w| w.cas[0].steps_per_gen += 1),
            ),
            (
                "a seed",
                Box::new(|w| w.cas[0].seed = crate::world::SeedSpec::Center),
            ),
            (
                "drop per generation",
                Box::new(move |w| w.cas[pile].drop_per_gen += 1),
            ),
            ("a CA's field", Box::new(|w| w.cas[0].field += 1)),
            (
                "an ant's cell",
                Box::new(move |w| w.cas[ant].agents[0].cell += 1),
            ),
            (
                "an ant's heading",
                Box::new(move |w| w.cas[ant].agents[0].heading += 1),
            ),
            (
                "an ant's state",
                Box::new(move |w| w.cas[ant].agents[0].state += 1),
            ),
            ("a cursor", Box::new(|w| w.cas[0].cursor += 1)),
            (
                "layers",
                Box::new(move |w| {
                    let mut l = (**w.cas[layered].layers.as_ref().unwrap()).clone();
                    l[0] += 1;
                    w.cas[layered].layers = Some(std::sync::Arc::new(l));
                }),
            ),
            ("no layers", Box::new(move |w| w.cas[layered].layers = None)),
            ("a noise seed", Box::new(|w| w.cas[5].noise ^= 1)),
            ("ticks", Box::new(|w| w.cas[0].ticks += 1)),
            (
                "agent edge",
                Box::new(move |w| w.cas[ant].agent_edge = crate::world::AgentEdge::FallOff),
            ),
            (
                "tables",
                Box::new(|w| {
                    let mut t = crate::table::parity_vn();
                    t.name = "other".into();
                    w.tables.insert(t).unwrap();
                }),
            ),
            ("the script", Box::new(|w| w.script = None)),
            ("the script's step", Box::new(|w| w.script_state.step += 1)),
            (
                "the script's start",
                Box::new(|w| w.script_state.started += 1),
            ),
            (
                "the script's streak",
                Box::new(|w| w.script_state.streak += 1),
            ),
            (
                "the script's end",
                Box::new(|w| w.script_state.finished ^= true),
            ),
            (
                "the script's activity",
                Box::new(|w| w.script_state.last_activity = Some(0.5)),
            ),
            ("the policies", Box::new(|w| w.reseed[0].cooldown += 1)),
            (
                "a policy's since",
                Box::new(|w| w.reseed_status[0].since += 1),
            ),
            (
                "a policy's streak",
                Box::new(|w| w.reseed_status[0].streak += 1),
            ),
            (
                "a policy's count",
                Box::new(|w| w.reseed_status[0].fired += 1),
            ),
            (
                "a policy's last",
                Box::new(|w| w.reseed_status[0].last_fired = Some(99)),
            ),
            (
                "a policy's error",
                Box::new(|w| w.reseed_status[0].error = Some("x".into())),
            ),
            ("manual reseeds", Box::new(|w| w.manual_reseeds += 1)),
            (
                "a change count a settled policy measured",
                Box::new(|w| w.activity.back_mut().unwrap().changed += 1),
            ),
            ("the watch's power", Box::new(|w| w.watch.power *= 2)),
            ("the watch's lam", Box::new(|w| w.watch.lam += 1)),
            (
                "the watch's find",
                Box::new(|w| w.watch.found = Some((2, 9))),
            ),
            ("the watch's snapshot", Box::new(|w| w.watch.snap = None)),
            (
                "the tiling",
                Box::new(|w| w.base.tiling.boundary = Boundary::Dead),
            ),
        ];
        let mut same = vec![];
        for (what, change) in &changes {
            let mut w = base.clone();
            change(&mut w);
            if w.state_hash_v2() == h0 {
                same.push(*what);
            }
        }
        assert!(
            same.is_empty(),
            "these changes leave the hash alone: {same:?}"
        );
        // What only shows, or is derived, or a restore does not reproduce, does not count.
        let mut w = base.clone();
        w.cas[0].name = "renamed".into();
        w.cas[0].palette = "Tide".into();
        w.track_changes = true;
        w.last_changed = Some(3);
        w.log.clear();
        w.cas[0].background = None;
        assert_eq!(w.state_hash_v2(), h0);
        // Version 1 is unchanged: FNV-1a over what it always covered.
        assert_eq!(base.hash_hex_v1().len(), 16);
        assert_eq!(base.hash_hex().len(), 64);
    }

    #[test]
    fn versions_by_length_and_by_record() {
        assert_eq!(HashVersion::of_hex("0123456789abcdef"), Ok(HashVersion::V1));
        assert_eq!(HashVersion::of_hex(&"ab".repeat(32)), Ok(HashVersion::V2));
        let e = HashVersion::of_hex("0123456789").unwrap_err();
        assert!(
            e.contains("16") && e.contains("64") && e.contains("10"),
            "{e}"
        );
        assert!(HashVersion::of_hex("xyz").is_err());
        assert_eq!(HashVersion::of_record(0), Ok(HashVersion::V1));
        assert_eq!(HashVersion::of_record(1), Ok(HashVersion::V1));
        assert_eq!(HashVersion::of_record(2), Ok(HashVersion::V2));
        assert!(HashVersion::of_record(3).is_err());
    }
}
