//! Rotations: what a display shows over time. A rotation is a list of items, each a profile, scene,
//! Quick Start entry, or a whole rule series, each with the conditions that end it. The [`Player`]
//! decides what shows next from what it is told about the running world, so a screensaver, a
//! wallpaper, or a test can all drive one.
//!
//! An item ends when any of its conditions holds: minutes, generations, settled (or low activity),
//! a rows rule reaching the bottom, or a time of day. It can hold its last frame for a moment first.
//! An item with a [`Schedule`] runs only on those days and hours. A rotation may also have a Rhai
//! script, run about once a second, that answers continue, next, previous, reseed, or which item
//! to go to.
//!
//! Rotations are JSON files in `<config dir>/jetcarp/rotations/`, one each. Built-in ones ([`builtins`])
//! are in code; editing one saves a user copy under the same name.

use crate::profiles::{self, Profile, Show, slug};
use jetcarp_core::render::ColorBy;
use jetcarp_core::reseed::{AutoReseed, ReseedMode, ReseedWhen};
use jetcarp_core::{Boundary, Scene, TilingSpec};
use rhai::{AST, Dynamic, Engine, Scope};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

/// Most items one series may expand to.
pub const MAX_SERIES: usize = 100_000;
/// Seconds between runs of a rotation's script.
const SCRIPT_EVERY: f64 = 1.0;
/// Rhai operations one run of a rotation's script may use.
const SCRIPT_OPS: u64 = 200_000;

/// The local date and time, as rotations use it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    /// 0 is Monday.
    pub weekday: u8,
    /// Minutes since local midnight.
    pub minute: u32,
    /// Days since 1970-01-01, local.
    pub day: i64,
}

impl Clock {
    pub fn local() -> Clock {
        use chrono::{Datelike, Timelike};
        let t = chrono::Local::now();
        Clock {
            weekday: t.weekday().num_days_from_monday() as u8,
            minute: t.hour() * 60 + t.minute(),
            day: t.date_naive().num_days_from_ce() as i64 - 719_163,
        }
    }
}

/// `"HH:MM"` as minutes since midnight.
pub fn parse_time(s: &str) -> Result<u32, String> {
    let bad = || format!("{s:?} is not a time like 07:30");
    let (h, m) = s.trim().split_once(':').ok_or_else(bad)?;
    let h: u32 = h.trim().parse().map_err(|_| bad())?;
    let m: u32 = m.trim().parse().map_err(|_| bad())?;
    if h < 24 && m < 60 {
        Ok(h * 60 + m)
    } else {
        Err(bad())
    }
}

pub const DAY_NAMES: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

fn parse_day(s: &str) -> Result<u8, String> {
    let d = s.trim().to_ascii_lowercase();
    DAY_NAMES
        .iter()
        .position(|n| d.starts_with(n))
        .map(|k| k as u8)
        .ok_or(format!("{s:?} is not a day (mon, tue, ... sun)"))
}

/// When an item may run.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Schedule {
    /// Day names; empty means every day.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub days: Vec<String>,
    /// `"HH:MM"`; with `to`, a daily window, which may cross midnight.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

impl Schedule {
    pub fn validate(&self) -> Result<(), String> {
        for d in &self.days {
            parse_day(d)?;
        }
        match (&self.from, &self.to) {
            (None, None) => Ok(()),
            (Some(a), Some(b)) => parse_time(a).and(parse_time(b)).map(|_| ()),
            _ => Err("a schedule needs both `from` and `to`, or neither".into()),
        }
    }

    pub fn contains(&self, c: Clock) -> bool {
        let day = self.days.is_empty() || self.days.iter().any(|d| parse_day(d) == Ok(c.weekday));
        let hours = match (&self.from, &self.to) {
            (Some(a), Some(b)) => match (parse_time(a), parse_time(b)) {
                (Ok(a), Ok(b)) if a <= b => (a..b).contains(&c.minute),
                (Ok(a), Ok(b)) => c.minute >= a || c.minute < b,
                _ => true,
            },
            _ => true,
        };
        day && hours
    }
}

/// What ends an item.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum End {
    /// After this many minutes.
    Minutes { minutes: f64 },
    /// After this many generations.
    Generations { gens: u64 },
    /// When the world settles (the same test the reseed policies use); `latest` generations at
    /// most, for rules that never look settled.
    Settled {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        latest: Option<u64>,
    },
    /// When under the fraction `below` of cells changed for `for_gens` generations in a row.
    LowActivity { below: f64, for_gens: u64 },
    /// When no CA can change the world any more: a rows rule (the Wolfram series) reached the
    /// bottom, or everything died.
    Done,
    /// When the clock passes this `"HH:MM"`.
    Clock { at: String },
}

impl End {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            End::Minutes { minutes } if *minutes <= 0.0 => Err("minutes must be above zero".into()),
            End::Generations { gens: 0 } => Err("generations must be above zero".into()),
            End::LowActivity { below, for_gens }
                if !(0.0..=1.0).contains(below) || *for_gens == 0 =>
            {
                Err("low activity needs a fraction from 0 to 1 and at least one generation".into())
            }
            End::Clock { at } => parse_time(at).map(|_| ()),
            _ => Ok(()),
        }
    }

    /// The world-side watch this condition needs: a reseed policy that changes nothing, so the
    /// engine's settle detection (which knows about GPU batches and Life's ash) reports it.
    pub fn watch(&self) -> Option<AutoReseed> {
        let when = match self {
            End::Settled { latest } => {
                let mut w = ReseedWhen::settled();
                if let ReseedWhen::Settled { latest: l, .. } = &mut w {
                    *l = *latest;
                }
                w
            }
            End::LowActivity { below, for_gens } => ReseedWhen::Settled {
                activity: *below,
                for_gens: *for_gens,
                cycle: None,
                period: None,
                latest: None,
            },
            _ => return None,
        };
        let nothing = ReseedMode::Random {
            flip: 0.0,
            sign: Default::default(),
        };
        Some(AutoReseed::new(nothing, when))
    }

    /// In words, for the editor.
    pub fn describe(&self) -> String {
        match self {
            End::Minutes { minutes } => format!("after {minutes} min"),
            End::Generations { gens } => format!("after {gens} generations"),
            End::Settled { latest: None } => "when settled".into(),
            End::Settled { latest: Some(g) } => format!("when settled (at most {g} generations)"),
            End::LowActivity { below, for_gens } => {
                format!(
                    "below {}% changing for {for_gens} generations",
                    below * 100.0
                )
            }
            End::Done => "when finished".into(),
            End::Clock { at } => format!("at {at}"),
        }
    }
}

/// What an item shows.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// A screensaver profile by name.
    Profile { name: String },
    /// A scene file.
    Scene { path: String },
    /// A Quick Start id (`rule:B3/S23`, `langtons-ant`, ...).
    QuickStart { id: String },
    /// One item per rule of a series, as a sweep names them: `rules` is a series name (`"W"`,
    /// `"HexW"`, ...), a template with `{n}`, or a comma-separated list.
    Series {
        rules: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<u64>,
        /// A tiling family.
        #[serde(default = "square")]
        family: String,
        #[serde(default = "dead")]
        boundary: Boundary,
        /// A seed as a sweep writes it (`"top_center"`, `"random:0.3"`); empty is the rule's own.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        seed: String,
    },
}

fn square() -> String {
    "square".into()
}
fn dead() -> Boundary {
    Boundary::Dead
}

impl Source {
    pub fn describe(&self) -> String {
        match self {
            Source::Profile { name } => name.clone(),
            Source::Scene { path } => format!("scene {path}"),
            Source::QuickStart { id } => format!("Quick Start {id}"),
            Source::Series {
                rules, from, to, ..
            } => match (from, to) {
                (Some(a), Some(b)) => format!("series {rules} {a} to {b}"),
                _ => format!("series {rules}"),
            },
        }
    }
}

/// Settings that replace what an item's source would use. Unset ones leave it alone.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Look {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_px: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refill: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_by: Option<ColorBy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reseed: Option<Vec<AutoReseed>>,
}

impl Look {
    fn apply(&self, p: &mut Profile) {
        if let Some(x) = self.cell_px {
            p.cell_px = x;
        }
        if let Some(x) = self.speed {
            p.speed = x;
        }
        if let Some(x) = self.refill {
            p.refill = x;
        }
        if let Some(x) = self.gpu {
            p.gpu = x;
        }
        if let Some(x) = &self.palette {
            p.palette = Some(x.clone());
        }
        if let Some(x) = self.color_by {
            p.color_by = Some(x);
        }
        if let Some(x) = &self.reseed {
            p.reseed = Some(x.clone());
            p.reseed_cells = None;
        }
    }
}

/// One entry of a rotation.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Item {
    pub source: Source,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Any of these ends the item. Empty: the profile's minutes, else the rotation's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub end: Vec<End>,
    /// Seconds to keep showing the finished world before moving on.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub hold_s: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub look: Look,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<Schedule>,
}

fn is_zero(x: &f32) -> bool {
    *x == 0.0
}
fn is_default(x: &Look) -> bool {
    *x == Look::default()
}

impl Item {
    pub fn new(source: Source) -> Item {
        Item {
            source,
            label: None,
            end: vec![],
            hold_s: 0.0,
            look: Look::default(),
            schedule: None,
        }
    }
    pub fn end(mut self, e: End) -> Item {
        self.end.push(e);
        self
    }
    pub fn hold(mut self, s: f32) -> Item {
        self.hold_s = s;
        self
    }
    pub fn look(mut self, l: Look) -> Item {
        self.look = l;
        self
    }
}

/// The order items play in.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    #[default]
    InOrder,
    /// A new random order each time through.
    Shuffle,
    /// One item per calendar day, the next one tomorrow; it restarts if it ends during the day.
    Daily,
}

impl Order {
    pub const ALL: [Order; 3] = [Order::InOrder, Order::Shuffle, Order::Daily];
    pub fn label(self) -> &'static str {
        match self {
            Order::InOrder => "In order",
            Order::Shuffle => "Shuffled",
            Order::Daily => "One a day",
        }
    }
}

/// A named rotation.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Rotation {
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub order: Order,
    pub items: Vec<Item>,
    /// Minutes for items with no conditions of their own and no minutes in their profile. None:
    /// they run until something else moves on.
    pub default_minutes: Option<f64>,
    /// Settings for every item that is not a stored profile (an item's own look comes after).
    #[serde(skip_serializing_if = "is_default")]
    pub look: Look,
    /// A Rhai script run about once a second. Empty: none.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub script: String,
}

impl Default for Rotation {
    fn default() -> Self {
        Rotation {
            name: String::new(),
            description: String::new(),
            order: Order::InOrder,
            items: vec![],
            default_minutes: Some(5.0),
            look: Look::default(),
            script: String::new(),
        }
    }
}

/// One thing a [`Player`] can show.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Slot {
    pub label: String,
    pub profile: Profile,
    /// A stored profile (by its name), not one made for this rotation.
    pub stored: bool,
    pub end: Vec<End>,
    pub hold_s: f32,
    pub schedule: Option<Schedule>,
    /// The series number, for series items.
    pub n: Option<u64>,
}

impl Slot {
    /// The world-side watches this slot needs, in order.
    pub fn watches(&self) -> Vec<AutoReseed> {
        self.end.iter().filter_map(End::watch).collect()
    }
}

impl Rotation {
    pub fn from_json(text: &str) -> Result<Rotation, String> {
        let r: Rotation = serde_json::from_str(text).map_err(|e| e.to_string())?;
        r.validate()?;
        Ok(r)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a rotation serializes")
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("a rotation needs a name".into());
        }
        if self.items.is_empty() {
            return Err("a rotation needs at least one item".into());
        }
        if self.default_minutes.is_some_and(|m| m <= 0.0) {
            return Err("default minutes must be above zero".into());
        }
        for (k, it) in self.items.iter().enumerate() {
            let at = |e: String| format!("item {}: {e}", k + 1);
            it.end.iter().try_for_each(End::validate).map_err(at)?;
            if let Some(s) = &it.schedule {
                s.validate().map_err(at)?;
            }
            if !(0.0..=3600.0).contains(&it.hold_s) {
                return Err(at("hold must be 0 to 3600 seconds".into()));
            }
        }
        if !self.script.trim().is_empty() {
            Hook::new(&self.script)?;
        }
        Ok(())
    }

    /// What each item becomes, in order. `base` gives the settings for anything that is not a
    /// stored profile and `lookup` finds stored profiles. Items that cannot be made are left out
    /// and described in the warnings.
    pub fn expand(
        &self,
        base: &Profile,
        lookup: &dyn Fn(&str) -> Option<Profile>,
    ) -> (Vec<Slot>, Vec<String>) {
        let mut slots = vec![];
        let mut warn = vec![];
        for it in &self.items {
            let mut made: Vec<(String, Profile, bool, Option<u64>)> = vec![];
            let adhoc = |show: Show, label: String| {
                let mut p = base.clone();
                p.name = label;
                p.show = show;
                self.look.apply(&mut p);
                p
            };
            match &it.source {
                Source::Profile { name } => match lookup(name) {
                    Some(p) => made.push((format!("profile {}", p.name), p, true, None)),
                    None => {
                        warn.push(format!("no profile named {name:?}"));
                        continue;
                    }
                },
                Source::Scene { path } => {
                    let label = format!("scene {path}");
                    let p = adhoc(Show::File { path: path.clone() }, label.clone());
                    made.push((label, p, false, None));
                }
                Source::QuickStart { id } => {
                    let label = format!("Quick Start {id}");
                    let p = adhoc(Show::QuickStart { id: id.clone() }, label.clone());
                    made.push((label, p, false, None));
                }
                Source::Series { .. } => match series_scenes(&it.source) {
                    Ok(list) => {
                        for (rule, n, scene) in list {
                            let p = adhoc(
                                Show::Scene {
                                    scene: Box::new(scene),
                                },
                                rule.clone(),
                            );
                            made.push((rule, p, false, n));
                        }
                    }
                    Err(e) => {
                        warn.push(format!("{}: {e}", it.source.describe()));
                        continue;
                    }
                },
            }
            for (label, mut p, stored, n) in made {
                let mut end = it.end.clone();
                if end.is_empty() {
                    let minutes = p.minutes.map(f64::from).or(self.default_minutes);
                    if let Some(minutes) = minutes {
                        end.push(End::Minutes { minutes });
                    }
                }
                it.look.apply(&mut p);
                let label = match (&it.label, n) {
                    (Some(l), None) => l.clone(),
                    _ => label,
                };
                slots.push(Slot {
                    label,
                    profile: p,
                    stored,
                    end,
                    hold_s: it.hold_s,
                    schedule: it.schedule.clone(),
                    n,
                });
            }
        }
        (slots, warn)
    }
}

/// `(rule, series number, scene)` for each rule of a series item.
fn series_scenes(source: &Source) -> Result<Vec<(String, Option<u64>, Scene)>, String> {
    let Source::Series {
        rules,
        from,
        to,
        family,
        boundary,
        seed,
    } = source
    else {
        return Ok(vec![]);
    };
    if rules.trim().is_empty() {
        return Err("no rules given".into());
    }
    let rules_json = if rules.contains(',') {
        let list: Vec<&str> = rules.split(',').map(str::trim).collect();
        json!(list)
    } else {
        json!(rules)
    };
    let mut spec = json!({ "rules": rules_json });
    if let Some(x) = from {
        spec["from"] = json!(x);
    }
    if let Some(x) = to {
        spec["to"] = json!(x);
    }
    let items = jetcarp_script::series::expand(&spec)?;
    if items.len() > MAX_SERIES {
        return Err(format!(
            "{} rules is too many for one rotation (at most {MAX_SERIES}); give `from` and `to`",
            items.len()
        ));
    }
    let options = if seed.trim().is_empty() {
        json!({})
    } else {
        json!({ "seed": seed })
    };
    items
        .into_iter()
        .map(|i| {
            let mut scene = Scene::new(TilingSpec::rect(family, 200, 140, *boundary));
            scene
                .cas
                .push(jetcarp_script::spec::ca_spec(&i.rule, &options)?);
            Ok((i.rule, i.n, scene))
        })
        .collect()
}

/// A rotation's script.
struct Hook {
    engine: Engine,
    ast: AST,
    /// `s`, and `state`, which the script may keep things in between runs.
    scope: Scope<'static>,
}

impl Hook {
    fn new(source: &str) -> Result<Hook, String> {
        let mut engine = Engine::new();
        engine.set_max_operations(SCRIPT_OPS);
        engine.set_max_call_levels(32);
        engine.set_max_expr_depths(64, 32);
        engine.set_max_string_size(1 << 16);
        engine.set_max_array_size(1 << 14);
        engine.set_max_map_size(1 << 14);
        engine.on_print(|_| {});
        engine.on_debug(|_, _, _| {});
        let ast = engine.compile(source).map_err(|e| format!("script: {e}"))?;
        let mut scope = Scope::new();
        scope.push("state", rhai::Map::new());
        Ok(Hook { engine, ast, scope })
    }

    fn run(&mut self, s: rhai::Map) -> Result<Dynamic, String> {
        let keep = self.scope.len();
        self.scope.push("s", s);
        let r = self
            .engine
            .eval_ast_with_scope::<Dynamic>(&mut self.scope, &self.ast)
            .map_err(|e| format!("script: {e}"));
        self.scope.rewind(keep);
        r
    }
}

/// What a script's answer asks for.
#[derive(Debug, PartialEq)]
enum Reply {
    Continue,
    Next,
    Previous,
    Reseed,
    Goto(usize),
}

/// What a [`Player`] is told about the running world, each frame.
#[derive(Clone, Copy, Debug)]
pub struct Observation {
    /// Seconds, from any fixed start.
    pub now: f64,
    pub clock: Clock,
    pub generation: u64,
    pub population: u64,
    pub cells: usize,
    /// Fraction of cells that changed last generation, when it was measured.
    pub activity: Option<f64>,
    /// `World::done`.
    pub done: bool,
    /// A watch from [`Slot::watches`] has fired.
    pub settled: bool,
}

/// What to do after a [`Player::tick`].
impl Observation {
    /// What a world shows now. `watch_from` is where the item's settle watches start in its reseed
    /// policies; the slower measures are taken only when wanted.
    pub fn of(
        w: &jetcarp_core::World,
        watch_from: usize,
        now: f64,
        clock: Clock,
        want_population: bool,
        want_done: bool,
    ) -> Observation {
        let population = if want_population {
            let mut fields: Vec<usize> = w
                .cas
                .iter()
                .filter(|c| c.enabled)
                .map(|c| c.field)
                .collect();
            fields.sort_unstable();
            fields.dedup();
            fields.iter().map(|&f| w.fields[f].population()).sum()
        } else {
            0
        };
        Observation {
            now,
            clock,
            generation: w.generation,
            population,
            cells: w.topo.len(),
            activity: w.last_activity(),
            done: want_done && w.done(),
            settled: w
                .reseed_status()
                .get(watch_from..)
                .is_some_and(|st| st.iter().any(|x| x.fired > 0)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Stay,
    /// Show this slot.
    Load(usize),
    /// Reseed the running world.
    Reseed,
}

/// Plays a rotation.
pub struct Player {
    slots: Vec<Slot>,
    order: Vec<usize>,
    mode: Order,
    pos: usize,
    seed: u64,
    hook: Option<Hook>,
    loaded: bool,
    item_started: f64,
    gens_base: u64,
    hold_until: Option<f64>,
    last_script: f64,
    last_minute: u32,
    day: i64,
    warning: Option<String>,
}

/// A small shuffle (SplitMix64): the order only has to differ between runs.
fn shuffle<T>(v: &mut [T], mut seed: u64) -> u64 {
    for i in (1..v.len()).rev() {
        seed = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        v.swap(i, (z % (i as u64 + 1)) as usize);
    }
    seed
}

impl Player {
    /// A player for `slots` (from [`Rotation::expand`]). `seed` varies the shuffled order.
    pub fn new(rotation: &Rotation, slots: Vec<Slot>, seed: u64) -> Result<Player, String> {
        let script = rotation.script.trim();
        let hook = (!script.is_empty())
            .then(|| Hook::new(script))
            .transpose()?;
        let mut order: Vec<usize> = (0..slots.len()).collect();
        let mut seed = seed;
        if rotation.order == Order::Shuffle {
            seed = shuffle(&mut order, seed);
        }
        Ok(Player {
            slots,
            order,
            mode: rotation.order,
            pos: 0,
            seed,
            hook,
            loaded: false,
            item_started: 0.0,
            gens_base: 0,
            hold_until: None,
            last_script: f64::NEG_INFINITY,
            last_minute: 0,
            day: 0,
            warning: None,
        })
    }

    /// The slot to show first, if there is anything to show.
    pub fn start(&mut self, clock: Clock) -> Option<usize> {
        if self.slots.is_empty() {
            return None;
        }
        self.day = clock.day;
        self.pos = match self.mode {
            Order::Daily => clock.day.rem_euclid(self.order.len() as i64) as usize,
            _ => (0..self.order.len())
                .find(|&p| self.eligible(p, clock))
                .unwrap_or(0),
        };
        self.loaded = false;
        Some(self.order[self.pos])
    }

    pub fn slots(&self) -> &[Slot] {
        &self.slots
    }

    /// The slot showing now.
    pub fn current(&self) -> Option<&Slot> {
        self.order.get(self.pos).map(|&k| &self.slots[k])
    }

    pub fn current_index(&self) -> usize {
        self.order.get(self.pos).copied().unwrap_or(0)
    }

    /// The slot's world is built and running.
    pub fn on_loaded(&mut self, now: f64, generation: u64, clock: Clock) {
        self.loaded = true;
        self.item_started = now;
        self.gens_base = generation;
        self.hold_until = None;
        self.last_minute = clock.minute;
    }

    /// Whether the script runs at `now`, so the world's slower measures are needed.
    pub fn script_due(&self, now: f64) -> bool {
        self.hook.is_some() && now - self.last_script >= SCRIPT_EVERY
    }

    /// Whether the current item can end on [`Observation::done`].
    pub fn watches_done(&self) -> bool {
        self.current()
            .is_some_and(|s| s.end.iter().any(|e| matches!(e, End::Done)))
    }

    /// A script's or a run's problem, once.
    pub fn take_warning(&mut self) -> Option<String> {
        self.warning.take()
    }

    /// Seconds until the current item's time limit, if it has one and is running.
    pub fn seconds_left(&self, now: f64) -> Option<f64> {
        if !self.loaded {
            return None;
        }
        self.current()?
            .end
            .iter()
            .filter_map(|e| match e {
                End::Minutes { minutes } => {
                    Some((minutes * 60.0 - (now - self.item_started)).max(0.0))
                }
                _ => None,
            })
            .reduce(f64::min)
    }

    fn eligible(&self, pos: usize, clock: Clock) -> bool {
        self.slots[self.order[pos]]
            .schedule
            .as_ref()
            .is_none_or(|s| s.contains(clock))
    }

    /// Moves by `dir` to the nearest item that may run now. Stays if there is none.
    pub fn advance(&mut self, dir: i64, clock: Clock) -> Action {
        let n = self.order.len();
        if n == 0 {
            return Action::Stay;
        }
        for step in 1..=n {
            let p = (self.pos as i64 + dir * step as i64).rem_euclid(n as i64) as usize;
            if !self.eligible(p, clock) {
                continue;
            }
            let mut p = p;
            if self.mode == Order::Shuffle && dir > 0 && p <= self.pos && n > 1 {
                // Through once: a new order, not starting with what just showed.
                let last = self.order[self.pos];
                self.seed = shuffle(&mut self.order, self.seed);
                if self.order[0] == last {
                    self.order.swap(0, n - 1);
                }
                p = (0..n).find(|&q| self.eligible(q, clock)).unwrap_or(0);
            }
            return self.go(p);
        }
        Action::Stay
    }

    fn go(&mut self, pos: usize) -> Action {
        self.pos = pos;
        self.loaded = false;
        self.hold_until = None;
        Action::Load(self.order[pos])
    }

    fn script_input(&self, o: &Observation) -> rhai::Map {
        let slot = &self.slots[self.order[self.pos]];
        let mut m = rhai::Map::new();
        let mut put = |k: &str, v: Dynamic| {
            m.insert(k.into(), v);
        };
        put("index", (self.order[self.pos] as i64).into());
        put("count", (self.slots.len() as i64).into());
        put("label", slot.label.clone().into());
        put("n", slot.n.map_or(Dynamic::UNIT, |n| (n as i64).into()));
        put(
            "generation",
            (o.generation.saturating_sub(self.gens_base) as i64).into(),
        );
        put("seconds", (o.now - self.item_started).into());
        put("population", (o.population as i64).into());
        put("cells", (o.cells as i64).into());
        put("activity", o.activity.map_or(Dynamic::UNIT, Dynamic::from));
        put("done", o.done.into());
        put("settled", o.settled.into());
        put("hour", ((o.clock.minute / 60) as i64).into());
        put("minute", ((o.clock.minute % 60) as i64).into());
        put("weekday", (o.clock.weekday as i64).into());
        m
    }

    /// Reads a script's answer: nothing or "continue", "next", "previous", "reseed", a slot
    /// number, or `#{item: number or label}`.
    fn reply(&self, v: Dynamic) -> Result<Reply, String> {
        let named = |label: &str| -> Result<Reply, String> {
            self.slots
                .iter()
                .position(|s| s.label.eq_ignore_ascii_case(label))
                .map(Reply::Goto)
                .ok_or(format!("script: no item labelled {label:?}"))
        };
        let number = |n: i64| -> Result<Reply, String> {
            if (0..self.slots.len() as i64).contains(&n) {
                Ok(Reply::Goto(n as usize))
            } else {
                Err(format!(
                    "script: item {n} is out of range (there are {})",
                    self.slots.len()
                ))
            }
        };
        if v.is_unit() {
            return Ok(Reply::Continue);
        }
        if let Some(n) = v.clone().try_cast::<i64>() {
            return number(n);
        }
        if let Some(m) = v.clone().try_cast::<rhai::Map>() {
            let target = m
                .get("item")
                .ok_or("script: a map answer needs `item`".to_string())?;
            if let Some(n) = target.clone().try_cast::<i64>() {
                return number(n);
            }
            let label = target
                .clone()
                .into_string()
                .map_err(|_| "script: `item` takes a number or a label".to_string())?;
            return named(&label);
        }
        let text = v
            .into_string()
            .map_err(|t| format!("script: cannot use an answer of type {t}"))?;
        match text.trim().to_ascii_lowercase().as_str() {
            "" | "continue" | "stay" => Ok(Reply::Continue),
            "next" => Ok(Reply::Next),
            "previous" | "prev" => Ok(Reply::Previous),
            "reseed" => Ok(Reply::Reseed),
            other => Err(format!(
                "script: {other:?} is not an answer (continue, next, previous, reseed, or #{{item: ...}})"
            )),
        }
    }

    fn end_fired(&self, e: &End, o: &Observation) -> bool {
        match e {
            End::Minutes { minutes } => o.now - self.item_started >= minutes * 60.0,
            End::Generations { gens } => o.generation.saturating_sub(self.gens_base) >= *gens,
            End::Settled { .. } | End::LowActivity { .. } => o.settled,
            End::Done => o.done,
            End::Clock { at } => parse_time(at).is_ok_and(|at| {
                let (a, b) = (self.last_minute, o.clock.minute);
                if a <= b {
                    a < at && at <= b
                } else {
                    at > a || at <= b
                }
            }),
        }
    }

    /// What to do this frame.
    pub fn tick(&mut self, o: &Observation) -> Action {
        if !self.loaded || self.slots.is_empty() {
            return Action::Stay;
        }
        if self.mode == Order::Daily && o.clock.day != self.day {
            self.day = o.clock.day;
            return self.go(o.clock.day.rem_euclid(self.order.len() as i64) as usize);
        }
        if !self.eligible(self.pos, o.clock) {
            let a = self.advance(1, o.clock);
            if a != Action::Stay {
                return a;
            }
        }
        if let Some(t) = self.hold_until {
            return if o.now >= t {
                self.finish(o.clock)
            } else {
                Action::Stay
            };
        }
        if self.hook.is_some() && o.now - self.last_script >= SCRIPT_EVERY {
            self.last_script = o.now;
            let input = self.script_input(o);
            let result = self.hook.as_mut().expect("a script").run(input);
            match result.and_then(|v| self.reply(v)) {
                Ok(Reply::Continue) => {}
                Ok(Reply::Next) => return self.advance(1, o.clock),
                Ok(Reply::Previous) => return self.advance(-1, o.clock),
                Ok(Reply::Reseed) => return Action::Reseed,
                Ok(Reply::Goto(k)) => {
                    if let Some(p) = self.order.iter().position(|&x| x == k) {
                        return self.go(p);
                    }
                }
                Err(e) => self.warning = Some(e),
            }
        }
        let slot = &self.slots[self.order[self.pos]];
        let fired = slot.end.iter().any(|e| self.end_fired(e, o));
        let hold = slot.hold_s as f64;
        self.last_minute = o.clock.minute;
        if !fired {
            return Action::Stay;
        }
        if hold > 0.0 {
            self.hold_until = Some(o.now + hold);
            return Action::Stay;
        }
        self.finish(o.clock)
    }

    /// The current item is over.
    fn finish(&mut self, clock: Clock) -> Action {
        if self.mode == Order::Daily {
            return self.go(self.pos);
        }
        self.advance(1, clock)
    }
}

/// The built-in rotations.
pub fn builtins() -> Vec<Rotation> {
    let profile = |n: &str| Source::Profile {
        name: n.to_string(),
    };
    let quick = |id: &str| Source::QuickStart { id: id.to_string() };
    let classics = || {
        vec![
            Item::new(profile("Life")),
            Item::new(quick("langtons-ant")).look(Look {
                cell_px: Some(4.0),
                speed: Some(240.0),
                ..Default::default()
            }),
            Item::new(quick("rule:ForestFire 0.01/0.0001")).look(Look {
                cell_px: Some(2.0),
                speed: Some(30.0),
                ..Default::default()
            }),
            Item::new(profile("Cyclic spirals")),
            Item::new(quick("turmite-llrr")).look(Look {
                cell_px: Some(4.0),
                speed: Some(240.0),
                ..Default::default()
            }),
            Item::new(profile("Penrose waves")),
            Item::new(profile("Hodgepodge")),
        ]
    };
    vec![
        Rotation {
            name: "Wolfram series".into(),
            description: "All 256 elementary rules, each drawn line by line to the bottom of the screen, then the next.".into(),
            default_minutes: None,
            look: Look {
                cell_px: Some(4.0),
                speed: Some(60.0),
                refill: Some(true),
                palette: Some("Ink".into()),
                ..Default::default()
            },
            items: vec![
                Item::new(Source::Series {
                    rules: "W".into(),
                    from: None,
                    to: None,
                    family: square(),
                    boundary: dead(),
                    seed: String::new(),
                })
                .end(End::Done)
                .hold(3.0),
            ],
            ..Default::default()
        },
        Rotation {
            name: "A classic a day".into(),
            description: "A different classic each day, restarting if it ends.".into(),
            order: Order::Daily,
            default_minutes: None,
            items: classics(),
            look: Look {
                refill: Some(true),
                ..Default::default()
            },
            ..Default::default()
        },
        Rotation {
            name: "Classics, hourly".into(),
            description: "The classics in a new random order, an hour each.".into(),
            order: Order::Shuffle,
            default_minutes: Some(60.0),
            items: classics()
                .into_iter()
                .map(|i| i.end(End::Minutes { minutes: 60.0 }))
                .collect(),
            look: Look {
                refill: Some(true),
                ..Default::default()
            },
            ..Default::default()
        },
        Rotation {
            name: "Until it settles".into(),
            description: "Life-like rules with no reseeding, each until it settles, then the next.".into(),
            default_minutes: None,
            look: Look {
                cell_px: Some(3.0),
                speed: Some(60.0),
                refill: Some(true),
                reseed: Some(vec![]),
                ..Default::default()
            },
            items: ["rule:B3/S23", "rule:B36/S23", "rule:B368/S245"]
                .iter()
                .map(|id| {
                    Item::new(quick(id))
                        .end(End::Settled {
                            latest: Some(30_000),
                        })
                        .hold(4.0)
                })
                .collect(),
            ..Default::default()
        },
    ]
}

/// A rotation as listed.
#[derive(Clone, Debug)]
pub struct Listed {
    pub rotation: Rotation,
    pub builtin: bool,
    pub file: Option<PathBuf>,
}

/// User rotations on disk and the built-ins.
#[derive(Default)]
pub struct RotationStore {
    dir: Option<PathBuf>,
    loaded: bool,
    user: Vec<(PathBuf, Rotation)>,
    /// Files that could not be read.
    pub errors: Vec<String>,
    builtins: Vec<Rotation>,
}

impl RotationStore {
    /// A store over `dir` (None: only the built-ins).
    pub fn at(dir: Option<PathBuf>) -> RotationStore {
        let mut s = RotationStore {
            dir,
            ..Default::default()
        };
        s.reload();
        s
    }

    /// The rotations folder beside a settings file.
    pub fn dir_for_settings(settings_file: Option<&Path>) -> Option<PathBuf> {
        Some(settings_file?.parent()?.join("rotations"))
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn reload(&mut self) {
        if self.builtins.is_empty() {
            self.builtins = builtins();
        }
        self.loaded = true;
        self.user.clear();
        self.errors.clear();
        let Some(dir) = &self.dir else { return };
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut files: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        for f in files {
            match std::fs::read_to_string(&f)
                .map_err(|e| e.to_string())
                .and_then(|t| Rotation::from_json(&t))
            {
                Ok(r) if self.user.iter().any(|(_, q)| q.name == r.name) => self.errors.push(
                    format!("{}: a second rotation named {:?}", f.display(), r.name),
                ),
                Ok(r) => self.user.push((f, r)),
                Err(e) => self.errors.push(format!("{}: {e}", f.display())),
            }
        }
    }

    /// The built-ins in order (a user copy in place of its built-in), then the user's by name.
    pub fn list(&self) -> Vec<Listed> {
        let mut v: Vec<Listed> = self
            .builtins
            .iter()
            .map(|b| match self.user.iter().find(|(_, r)| r.name == b.name) {
                Some((f, r)) => Listed {
                    rotation: r.clone(),
                    builtin: true,
                    file: Some(f.clone()),
                },
                None => Listed {
                    rotation: b.clone(),
                    builtin: true,
                    file: None,
                },
            })
            .collect();
        let mut own: Vec<Listed> = self
            .user
            .iter()
            .filter(|(_, r)| !self.builtins.iter().any(|b| b.name == r.name))
            .map(|(f, r)| Listed {
                rotation: r.clone(),
                builtin: false,
                file: Some(f.clone()),
            })
            .collect();
        own.sort_by_key(|l| l.rotation.name.to_lowercase());
        v.extend(own);
        v
    }

    pub fn names(&self) -> Vec<String> {
        self.list().into_iter().map(|l| l.rotation.name).collect()
    }

    pub fn get(&self, name: &str) -> Option<Rotation> {
        let list = self.list();
        list.iter()
            .find(|l| l.rotation.name == name)
            .or_else(|| {
                list.iter()
                    .find(|l| l.rotation.name.eq_ignore_ascii_case(name.trim()))
            })
            .map(|l| l.rotation.clone())
    }

    pub fn listed(&self, name: &str) -> Option<Listed> {
        self.list().into_iter().find(|l| l.rotation.name == name)
    }

    /// Save a rotation to its file (a new file for a new name). Returns the path.
    pub fn save(&mut self, r: &Rotation) -> Result<PathBuf, String> {
        r.validate()?;
        let dir = self.dir.clone().ok_or(
            "no settings folder for this run (set JETCARP_CONFIG_DIR), so rotations cannot be saved",
        )?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = match self.user.iter().find(|(_, q)| q.name == r.name) {
            Some((f, _)) => f.clone(),
            None => {
                let base = slug(&r.name);
                let mut k = 1;
                loop {
                    let f = if k == 1 {
                        dir.join(format!("{base}.json"))
                    } else {
                        dir.join(format!("{base}-{k}.json"))
                    };
                    if !f.exists() {
                        break f;
                    }
                    k += 1;
                }
            }
        };
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, r.to_json())
            .and_then(|_| std::fs::rename(&tmp, &path))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        match self.user.iter_mut().find(|(_, q)| q.name == r.name) {
            Some(slot) => slot.1 = r.clone(),
            None => self.user.push((path.clone(), r.clone())),
        }
        Ok(path)
    }

    /// Delete a user rotation's file (for a built-in's user copy: back to the built-in).
    pub fn delete(&mut self, name: &str) -> Result<(), String> {
        let k = self
            .user
            .iter()
            .position(|(_, r)| r.name == name)
            .ok_or_else(|| {
                if self.builtins.iter().any(|b| b.name == name) {
                    format!("{name:?} is built in and cannot be deleted")
                } else {
                    format!("no rotation named {name:?}")
                }
            })?;
        std::fs::remove_file(&self.user[k].0)
            .map_err(|e| format!("{}: {e}", self.user[k].0.display()))?;
        self.user.remove(k);
        Ok(())
    }

    /// Point items at a profile's new name, in the user's rotations.
    pub fn rename_profile(&mut self, old: &str, new: &str) -> Result<(), String> {
        let mut changed = vec![];
        for (_, r) in &mut self.user {
            let mut any = false;
            for it in &mut r.items {
                if let Source::Profile { name } = &mut it.source
                    && name == old
                {
                    *name = new.to_string();
                    any = true;
                }
            }
            if any {
                changed.push(r.clone());
            }
        }
        changed.iter().try_for_each(|r| self.save(r).map(|_| ()))
    }
}

/// The profile for items that are not stored profiles, with sensible screensaver settings.
pub fn default_base() -> Profile {
    let mut p = Profile {
        name: "rotation item".into(),
        ..Default::default()
    };
    p.cell_px = 4.0;
    p
}

/// The profile and rotation stores beside JETCARP's settings file.
fn user_stores() -> (profiles::ProfileStore, RotationStore) {
    let file = crate::host_settings::app_settings_file();
    (
        profiles::ProfileStore::at(profiles::ProfileStore::dir_for_settings(file.as_deref())),
        RotationStore::at(RotationStore::dir_for_settings(file.as_deref())),
    )
}

/// The stored or built-in rotation called `name`, expanded and ready to run, with any warnings
/// from expanding it. An unknown name is an error, not a silent default.
pub fn load_stored(name: &str) -> Result<(Rotation, Vec<Slot>, Vec<String>), String> {
    let (profiles, rotations) = user_stores();
    let r = rotations
        .get(name)
        .ok_or_else(|| format!("no rotation named {name:?}"))?;
    let (slots, warnings) = r.expand(&default_base(), &|n| profiles.get(n));
    Ok((r, slots, warnings))
}

/// Every rotation that can be chosen.
pub fn stored_names() -> Vec<String> {
    user_stores().1.names()
}

/// Used by the editor: the built-in profile names, for a new rotation.
pub fn builtin_profile_names() -> Vec<String> {
    profiles::builtins().into_iter().map(|p| p.name).collect()
}
