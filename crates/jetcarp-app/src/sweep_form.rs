//! The click-to-build sweep form. It fills the same JSON options as `sweep(#{...})` in a script
//! (`jetcarp_script::sweep_to_script` turns it into one), so everything here can also be written
//! by hand. The model and its JSON are plain Rust (tested below); `ui` draws it.

use jetcarp_core::families::{FAMILIES, FamilyKind};
use jetcarp_core::rules::Rule;
use jetcarp_core::*;
use eframe::egui::{self, RichText};
use serde_json::{Map, Value, json};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Series {
    W,
    HexW,
    HexW3,
    Bs,
    Template,
    List,
}

impl Series {
    pub const ALL: [(Series, &'static str); 6] = [
        (Series::W, "Wolfram W 0–255 (square rows)"),
        (Series::HexW, "HexW 0–15 (binary hexagon rows)"),
        (Series::HexW3, "HexW3 0–19682 (ternary hexagon rows)"),
        (Series::Bs, "B/S rules (Life-like, Generations)"),
        (Series::Template, "Numbered rule template ({n})"),
        (Series::List, "A list of rules"),
    ];
    pub fn label(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|x| x.0 == self)
            .map(|x| x.1)
            .unwrap_or("")
    }
    /// Last number of a named series.
    pub fn last(self) -> Option<u64> {
        match self {
            Series::W => Some(255),
            Series::HexW => Some(15),
            Series::HexW3 => Some(19682),
            _ => None,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Series::W => "W",
            Series::HexW => "HexW",
            Series::HexW3 => "HexW3",
            _ => "",
        }
    }
    /// Row rules: they run from the top row to the bottom.
    pub fn rows(self) -> bool {
        matches!(self, Series::W | Series::HexW | Series::HexW3)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum TilingChoice {
    /// A family or catalog id (`hex`, `k3-012`, `ih41`).
    Id(String),
    /// An exact tiling (the current world's shape and parameters).
    Exact {
        spec: Box<TilingSpec>,
        label: String,
    },
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SizeChoice {
    /// Fill the screen (the app's view) at its tile size.
    Screen,
    Rect(u32, u32),
    Cells(u32),
    Level(u32),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Until {
    Done,
    Generations,
    Repeats,
    Frozen,
    Empty,
    PopBelow,
    PopAbove,
    ActBelow,
    ActAbove,
}

impl Until {
    pub const ALL: [(Until, &'static str); 9] = [
        (
            Until::Done,
            "done (nothing can change: bottom row written, died out, frozen)",
        ),
        (Until::Generations, "a number of generations"),
        (Until::Repeats, "it repeats (exact period check)"),
        (Until::Frozen, "it stops changing"),
        (Until::Empty, "it dies out"),
        (Until::PopBelow, "population drops below"),
        (Until::PopAbove, "population rises above"),
        (Until::ActBelow, "activity drops below"),
        (Until::ActAbove, "activity rises above"),
    ];
    pub fn label(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|x| x.0 == self)
            .map(|x| x.1)
            .unwrap_or("")
    }
}

/// Start seeds offered in the form (shorthand, label, takes a density).
pub const SEEDS: &[(&str, &str, bool)] = &[
    ("top_center", "One cell, top centre", false),
    ("top_random", "Random top row", true),
    ("center", "One cell, centre", false),
    ("random", "Random soup", true),
    ("blob", "Random blob at the centre", true),
    ("random_states", "Random states (multi-state)", false),
    ("empty", "Empty", false),
];

/// A scene every run starts from, with one CA's rule swept.
#[derive(Clone, Debug, PartialEq)]
pub struct BaseScene {
    pub scene: Value,
    /// The CAs, as shown in the picker.
    pub cas: Vec<String>,
    /// Which CA's rule is swept (position in `cas`).
    pub vary: usize,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SweepForm {
    pub series: Series,
    pub from: u64,
    pub to: u64,
    pub template: String,
    pub birth: String,
    pub survive: String,
    /// 2 = Life-like; more = Generations.
    pub states: u8,
    /// 0 = every rule.
    pub sample: u64,
    pub list: String,
    pub tiling: TilingChoice,
    pub size: SizeChoice,
    pub boundary: Boundary,
    pub seed: String,
    pub density: f64,
    /// A start the shorthands cannot express (taken from the current world).
    pub seed_exact: Option<Value>,
    /// `""` (the rule's default), `touch`, or `edge`.
    pub neighborhood: String,
    pub until: Until,
    pub until_n: u64,
    /// Activity triggers: fraction of cells changing per generation.
    pub fraction: f64,
    pub for_gens: u64,
    pub max_period: u64,
    pub max_gens: u64,
    /// `""` = the rule's own palette.
    pub palette: String,
    pub images: bool,
    /// `None` = the screen size.
    pub image_size: Option<(u32, u32)>,
    pub compression: String,
    /// Output folder label (`""` = from the rules).
    pub label: String,
    pub skip_empty: bool,
    pub skip_uniform: bool,
    pub skip_full: bool,
    pub rng_seed: u64,
    pub base: Option<BaseScene>,
}

impl Default for SweepForm {
    fn default() -> Self {
        SweepForm {
            series: Series::HexW3,
            from: 0,
            to: 19682,
            template: "B3/S{n}".into(),
            birth: "1-8".into(),
            survive: "0-8".into(),
            states: 2,
            sample: 200,
            list: "B3/S23\nB36/S23\nB2/S\n".into(),
            tiling: TilingChoice::Id("hex".into()),
            size: SizeChoice::Screen,
            boundary: Boundary::Dead,
            seed: "top_center".into(),
            density: 0.3,
            seed_exact: None,
            neighborhood: String::new(),
            until: Until::Done,
            until_n: 500,
            fraction: 0.001,
            for_gens: 20,
            max_period: 1000,
            max_gens: 10_000,
            palette: String::new(),
            images: true,
            image_size: None,
            compression: "balanced".into(),
            label: String::new(),
            skip_empty: false,
            skip_uniform: false,
            skip_full: false,
            rng_seed: 1,
            base: None,
        }
    }
}

fn is_aperiodic(family: &str) -> bool {
    families::family(family).is_some_and(|f| f.kind == FamilyKind::Aperiodic)
}

/// A seed as the form's shorthand text, if it has one.
fn seed_text(s: &SeedSpec) -> Option<(String, f64)> {
    Some(match s {
        SeedSpec::Empty => ("empty".into(), 0.3),
        SeedSpec::Random { density } => ("random".into(), *density),
        SeedSpec::Center => ("center".into(), 0.3),
        SeedSpec::TopCenter => ("top_center".into(), 0.3),
        SeedSpec::TopRandom { density } => ("top_random".into(), *density),
        SeedSpec::Blob { radius, density } if (*radius - 0.3).abs() < 1e-9 => {
            ("blob".into(), *density)
        }
        SeedSpec::RandomStates => ("random_states".into(), 0.3),
        _ => return None,
    })
}

impl SweepForm {
    /// The form set up for a series: its range, the hexagon grid for hexagon rules, the top row
    /// and `done` for row rules.
    pub fn set_series(&mut self, s: Series) {
        let was_rows = self.series.rows();
        self.series = s;
        if let Some(last) = s.last() {
            self.from = 0;
            self.to = last;
        }
        if self.base.is_none() {
            match s {
                Series::HexW | Series::HexW3 => self.tiling = TilingChoice::Id("hex".into()),
                Series::W => self.tiling = TilingChoice::Id("square".into()),
                _ => {}
            }
        }
        if s.rows() {
            self.seed = "top_center".into();
            self.until = Until::Done;
            self.boundary = Boundary::Dead;
        } else if was_rows {
            self.seed = "random".into();
            self.density = 0.3;
            self.until = Until::Repeats;
            self.max_gens = 2000;
            self.boundary = Boundary::Wrap;
        }
    }

    /// The form started from the world on screen: its tiling, size, edges, RNG seed, and CAs.
    /// One plain CA becomes ordinary options; several CAs, drawn cells, or rule tables keep the
    /// world as a base scene and sweep one CA's rule.
    pub fn from_world(world: &World) -> SweepForm {
        let mut f = SweepForm::default();
        let scene = world.to_scene();
        let spec = world.topo.spec.clone();
        // Edits before the first step (drawn cells): they are part of the start.
        let ids: Vec<u32> = world.cas.iter().map(|c| c.id).collect();
        let start: Vec<Command> = scene
            .commands
            .iter()
            .take_while(|c| !matches!(c, Command::Step { .. }))
            .filter(|c| match c {
                Command::SetCells { id, .. }
                | Command::Seed { id, .. }
                | Command::Clear { id }
                | Command::Sandpile { id, .. } => ids.contains(id),
                Command::SetPalette { .. } | Command::AddTable { .. } => true,
                _ => false,
            })
            .cloned()
            .collect();
        let drawn = start
            .iter()
            .any(|c| !matches!(c, Command::SetPalette { .. } | Command::AddTable { .. }));
        let vary = world
            .cas
            .iter()
            .position(|c| {
                matches!(
                    c.rule,
                    Rule::Wolfram(_)
                        | Rule::HexWolfram { .. }
                        | Rule::LifeLike { .. }
                        | Rule::Generations { .. }
                )
            })
            .unwrap_or(0);
        let Some(ca) = world.cas.get(vary) else {
            f.world_shape(world, scene.rng_state);
            return f;
        };
        // The series from the swept CA's rule.
        match ca.rule {
            Rule::Wolfram(_) => f.set_series(Series::W),
            Rule::HexWolfram { states: 2, .. } => f.set_series(Series::HexW),
            Rule::HexWolfram { .. } => f.set_series(Series::HexW3),
            Rule::LifeLike { .. } => {
                f.set_series(Series::Bs);
                f.states = 2;
            }
            Rule::Generations { states, .. } => {
                f.set_series(Series::Bs);
                f.states = states;
            }
            _ => {
                f.set_series(Series::List);
                f.list = format!("{}\n", ca.notation());
                f.until = Until::Repeats;
            }
        }
        // `set_series` picks a default grid and edges; the world's own win.
        f.world_shape(world, scene.rng_state);
        if world.cas.len() == 1 && !drawn && scene.tables.is_empty() {
            // One CA: plain options.
            match seed_text(&ca.seed) {
                Some((s, d)) => {
                    f.seed = s;
                    f.density = d;
                }
                None => f.seed_exact = serde_json::to_value(&ca.seed).ok(),
            }
            f.palette = ca.palette.clone();
            f.neighborhood = match (ca.rule.nb_fixed(), ca.nb) {
                (true, _) => String::new(),
                (false, Nb::Edge) => "edge".into(),
                (false, Nb::Touch) => "touch".into(),
            };
            return f;
        }
        let mut base = Scene::new(TilingSpec {
            size: spec.size,
            ..spec.clone()
        });
        base.rng_state = scene.rng_state;
        base.tables = scene.tables.clone();
        base.palettes = scene.palettes.clone();
        base.cas = world.cas.iter().map(|c| c.spec()).collect();
        base.commands = start;
        let n_drawn: usize = base
            .commands
            .iter()
            .map(|c| match c {
                Command::SetCells { cells, .. } => cells.len(),
                _ => 0,
            })
            .sum();
        f.base = Some(BaseScene {
            scene: serde_json::to_value(&base).unwrap_or(Value::Null),
            cas: world
                .cas
                .iter()
                .enumerate()
                .map(|(i, c)| format!("{}: {} ({})", i + 1, c.name, c.notation()))
                .collect(),
            vary,
            summary: format!(
                "{} CAs{}",
                world.cas.len(),
                if n_drawn > 0 {
                    format!(", {n_drawn} drawn cells")
                } else {
                    String::new()
                }
            ),
        });
        f.seed.clear();
        f
    }

    /// The world's tiling, size, edges, and RNG seed.
    fn world_shape(&mut self, world: &World, rng: u64) {
        let spec = &world.topo.spec;
        self.tiling = tiling_choice(world, spec);
        self.rng_seed = rng;
        self.size = match spec.size {
            Some(s) => size_choice(s),
            None => SizeChoice::Screen,
        };
        self.boundary = if world.topo.wraps() {
            Boundary::Wrap
        } else if world.topo.live_edges() {
            Boundary::Live
        } else {
            Boundary::Dead
        };
    }

    /// Whether the tiling (in the plain mode) is aperiodic.
    fn aperiodic(&self) -> bool {
        match &self.tiling {
            TilingChoice::Id(id) => is_aperiodic(id),
            TilingChoice::Exact { spec, .. } => is_aperiodic(&spec.family),
        }
    }

    /// The default folder label.
    pub fn default_label(&self) -> String {
        match self.series {
            Series::W | Series::HexW | Series::HexW3 => self.series.name().into(),
            Series::Bs => "BS".into(),
            Series::Template => self
                .template
                .replace("{n}", "")
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect(),
            Series::List => "rules".into(),
        }
    }

    /// The sweep options (the map `sweep(#{...})` takes).
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        let mut put = |k: &str, v: Value| {
            m.insert(k.to_string(), v);
        };
        match self.series {
            Series::W | Series::HexW | Series::HexW3 => {
                put("rules", json!(self.series.name()));
                put("from", json!(self.from));
                put("to", json!(self.to));
            }
            Series::Bs => {
                put("rules", json!("B/S"));
                put("birth", json!(self.birth.trim()));
                put("survive", json!(self.survive.trim()));
                if self.states > 2 {
                    put("states", json!(self.states));
                }
                if self.sample > 0 {
                    put("sample", json!(self.sample));
                }
            }
            Series::Template => {
                put("rules", json!(self.template.trim()));
                put("from", json!(self.from));
                put("to", json!(self.to));
            }
            Series::List => {
                put("rules", json!(census::parse_rule_list(&self.list)));
            }
        }
        match &self.base {
            Some(b) => {
                put("scene", b.scene.clone());
                put("ca", json!(b.vary));
            }
            None => {
                let ap = self.aperiodic();
                put(
                    "tiling",
                    match &self.tiling {
                        TilingChoice::Id(id) => json!(id),
                        TilingChoice::Exact { spec, .. } => {
                            let mut t = json!({ "family": spec.family });
                            if let Some(p) = &spec.params {
                                t["params"] = p.clone();
                            }
                            if let Some(e) = &spec.edges {
                                t["edges"] = serde_json::to_value(e).unwrap_or(Value::Null);
                            }
                            t
                        }
                    },
                );
                match self.size {
                    SizeChoice::Level(l) if ap => put("level", json!(l)),
                    _ if ap => {}
                    SizeChoice::Screen | SizeChoice::Level(_) => put("size", json!("screen")),
                    SizeChoice::Rect(w, h) => put("size", json!([w, h])),
                    SizeChoice::Cells(c) => put("size", json!(c)),
                }
                if !ap {
                    put(
                        "boundary",
                        json!(match self.boundary {
                            Boundary::Dead => "dead",
                            Boundary::Wrap => "wrap",
                            Boundary::Live => "live",
                        }),
                    );
                }
                if self.rng_seed != 1 {
                    put("rng_seed", json!(self.rng_seed));
                }
            }
        }
        if let Some(s) = &self.seed_exact {
            put("seed", s.clone());
        } else if !self.seed.is_empty() {
            let takes_density = SEEDS.iter().any(|x| x.0 == self.seed && x.2);
            put(
                "seed",
                json!(if takes_density {
                    format!("{}:{}", self.seed, round3(self.density))
                } else {
                    self.seed.clone()
                }),
            );
        }
        if !self.neighborhood.is_empty() {
            put("neighborhood", json!(self.neighborhood));
        }
        if !self.palette.is_empty() {
            put("palette", json!(self.palette));
        }
        let trig = |s: &str| json!(s);
        match self.until {
            Until::Done => put("until", trig("done")),
            Until::Generations => {
                put("until", trig("generations"));
                put("n", json!(self.until_n));
            }
            Until::Repeats => {
                put("until", trig("repeats"));
                put("max_period", json!(self.max_period));
            }
            Until::Frozen => put("until", trig("frozen")),
            Until::Empty => put("until", trig("empty")),
            Until::PopBelow | Until::PopAbove => {
                put(
                    "until",
                    trig(if self.until == Until::PopBelow {
                        "population_below"
                    } else {
                        "population_above"
                    }),
                );
                put("n", json!(self.until_n));
            }
            Until::ActBelow | Until::ActAbove => {
                put(
                    "until",
                    trig(if self.until == Until::ActBelow {
                        "activity_below"
                    } else {
                        "activity_above"
                    }),
                );
                put("fraction", json!(self.fraction));
                put("for_gens", json!(self.for_gens));
            }
        }
        if self.until != Until::Generations {
            put("max_gens", json!(self.max_gens));
        }
        if !self.images {
            put("png", json!(false));
        } else if self.image_size.is_some() || self.compression != "balanced" {
            let mut p = json!({});
            if let Some((w, h)) = self.image_size {
                p["size"] = json!([w, h]);
            }
            if self.compression != "balanced" {
                p["compression"] = json!(self.compression);
            }
            put("png", p);
        }
        let skip: Vec<&str> = [
            (self.skip_empty, "empty"),
            (self.skip_uniform, "uniform"),
            (self.skip_full, "full"),
        ]
        .iter()
        .filter(|x| x.0)
        .map(|x| x.1)
        .collect();
        if !skip.is_empty() {
            put("skip", json!(skip));
        }
        put(
            "out",
            json!(if self.label.trim().is_empty() {
                self.default_label()
            } else {
                self.label.trim().to_string()
            }),
        );
        Value::Object(m)
    }

    /// Number of runs, or why the rules are not valid.
    pub fn runs(&self) -> Result<usize, String> {
        jetcarp_script::series::expand(&self.to_json()).map(|v| v.len())
    }
}

/// The world's own tiling as a choice (catalog id, family id, or the exact shape).
fn tiling_choice(world: &World, spec: &TilingSpec) -> TilingChoice {
    let catalog = (spec.family == "periodic")
        .then(|| {
            spec.params
                .as_ref()?
                .get("catalog")?
                .as_str()
                .map(String::from)
        })
        .flatten();
    match catalog {
        Some(id) if spec.edges.is_none() => TilingChoice::Id(id),
        _ if spec.params.is_none() && spec.edges.is_none() => TilingChoice::Id(spec.family.clone()),
        _ => TilingChoice::Exact {
            label: world.topo.describe(),
            spec: Box::new(TilingSpec {
                size: None,
                ..spec.clone()
            }),
        },
    }
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// What the app tells the form about the screen.
pub struct ScreenInfo {
    pub screen: jetcarp_script::Screen,
    /// The current world's tiling id and size (for "current world size").
    pub world_size: Option<SizeSpec>,
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(title).strong().size(15.0));
}

/// Draw the form. Returns true when something changed.
pub fn ui(ui: &mut egui::Ui, f: &mut SweepForm, info: &ScreenInfo) -> bool {
    let before = f.clone();
    let id = |s: &'static str| ("sweep-form", s);
    section(ui, "Rules");
    egui::Grid::new(id("rules"))
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Sweep");
            let mut s = f.series;
            egui::ComboBox::from_id_salt(id("series"))
                .selected_text(s.label())
                .width(300.0)
                .show_ui(ui, |ui| {
                    for (k, label) in Series::ALL {
                        ui.selectable_value(&mut s, k, label);
                    }
                });
            if s != f.series {
                f.set_series(s);
            }
            ui.end_row();
            match f.series {
                Series::W | Series::HexW | Series::HexW3 | Series::Template => {
                    if f.series == Series::Template {
                        ui.label("Template");
                        ui.add(
                            egui::TextEdit::singleline(&mut f.template)
                                .hint_text("B3/S{n}")
                                .desired_width(200.0),
                        )
                        .on_hover_text("{n} is replaced by each number from..to");
                        ui.end_row();
                    }
                    let last = f.series.last().unwrap_or(u64::MAX / 2);
                    ui.label("Numbers");
                    ui.horizontal(|ui| {
                        ui.label("from");
                        ui.add(egui::DragValue::new(&mut f.from).range(0..=last));
                        ui.label("to");
                        ui.add(egui::DragValue::new(&mut f.to).range(0..=last));
                        if f.series.last().is_some() && ui.small_button("All").clicked() {
                            f.from = 0;
                            f.to = last;
                        }
                    });
                    if f.from > f.to {
                        f.to = f.from;
                    }
                    ui.end_row();
                }
                Series::Bs => {
                    ui.label("Birth digits");
                    ui.add(egui::TextEdit::singleline(&mut f.birth).desired_width(80.0))
                        .on_hover_text(
                            "Every combination of these birth counts, e.g. 1-8 or 2,3,6",
                        );
                    ui.end_row();
                    ui.label("Survive digits");
                    ui.add(egui::TextEdit::singleline(&mut f.survive).desired_width(80.0));
                    ui.end_row();
                    ui.label("States");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut f.states).range(2..=32));
                        ui.label(
                            RichText::new(if f.states > 2 {
                                "Generations rules"
                            } else {
                                "Life-like rules"
                            })
                            .weak(),
                        );
                    });
                    ui.end_row();
                    ui.label("Pick");
                    ui.horizontal(|ui| {
                        let mut some = f.sample > 0;
                        ui.radio_value(&mut some, false, "every rule");
                        ui.radio_value(&mut some, true, "a random sample of");
                        if some && f.sample == 0 {
                            f.sample = 200;
                        }
                        if !some {
                            f.sample = 0;
                        }
                        ui.add_enabled(
                            some,
                            egui::DragValue::new(&mut f.sample).range(1..=1_000_000),
                        );
                    });
                    ui.end_row();
                }
                Series::List => {
                    ui.label("Rules");
                    ui.add(
                        egui::TextEdit::multiline(&mut f.list)
                            .desired_rows(4)
                            .desired_width(260.0)
                            .hint_text("one per line: B3/S23, Brian's Brain, W110, HexW3 8229"),
                    );
                    ui.end_row();
                }
            }
            if let Some(b) = &mut f.base {
                ui.label("Rule of");
                egui::ComboBox::from_id_salt(id("vary"))
                    .selected_text(b.cas.get(b.vary).cloned().unwrap_or_default())
                    .width(300.0)
                    .show_ui(ui, |ui| {
                        for (i, c) in b.cas.iter().enumerate() {
                            ui.selectable_value(&mut b.vary, i, c);
                        }
                    });
                ui.end_row();
            }
            ui.label("");
            match f.runs() {
                Ok(n) => ui.label(RichText::new(format!("{} runs", thousands(n as u64))).strong()),
                Err(e) => ui.colored_label(egui::Color32::LIGHT_RED, e),
            };
            ui.end_row();
        });

    section(ui, "World");
    if let Some(b) = &f.base {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!(
                    "Every run starts from the current world ({}): its tiling, size, edges, seeds and drawn cells.",
                    b.summary
                ))
                .weak(),
            );
        });
        if ui
            .small_button("Use an empty world with one CA instead")
            .clicked()
        {
            f.base = None;
            if f.seed.is_empty() {
                f.seed = if f.series.rows() {
                    "top_center"
                } else {
                    "random"
                }
                .into();
            }
        }
    }
    if f.base.is_none() {
        egui::Grid::new(id("world"))
            .num_columns(2)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label("Tiling");
                tiling_ui(ui, f);
                ui.end_row();
                let ap = f.aperiodic();
                ui.label("Size");
                ui.vertical(|ui| {
                    let scr = info.screen;
                    if ap {
                        let mut l = match f.size {
                            SizeChoice::Level(l) => l,
                            _ => 4,
                        };
                        ui.horizontal(|ui| {
                            ui.label("Level");
                            ui.add(egui::DragValue::new(&mut l).range(1..=8));
                        });
                        f.size = SizeChoice::Level(l);
                    } else {
                        ui.radio_value(
                            &mut f.size,
                            SizeChoice::Screen,
                            format!(
                                "Fill the screen: {} × {} px at {:.0} px tiles",
                                scr.width, scr.height, scr.cell_px
                            ),
                        )
                        .on_hover_text(
                            "The main view's size, at the Tile size set in the Tiling panel",
                        );
                        if let Some(ws) = info.world_size {
                            let (label, choice) = match ws {
                                SizeSpec::Rect { w, h } => {
                                    (format!("Current world: {w} × {h}"), SizeChoice::Rect(w, h))
                                }
                                SizeSpec::Cells(c) => (
                                    format!("Current world: about {} cells", thousands(c as u64)),
                                    SizeChoice::Cells(c),
                                ),
                                SizeSpec::Level(l) => (format!("Level {l}"), SizeChoice::Level(l)),
                            };
                            ui.radio_value(&mut f.size, choice, label);
                        }
                        ui.horizontal(|ui| {
                            let mut custom = matches!(f.size, SizeChoice::Rect(..))
                                && Some(f.size) != info.world_size.map(size_choice);
                            if ui.radio(custom, "Custom").clicked() && !custom {
                                custom = true;
                                f.size = SizeChoice::Rect(100, 100);
                            }
                            if custom && let SizeChoice::Rect(w, h) = &mut f.size {
                                ui.add(egui::DragValue::new(w).range(1..=20_000));
                                ui.label("×");
                                ui.add(egui::DragValue::new(h).range(1..=20_000));
                            }
                        });
                    }
                });
                ui.end_row();
                if !ap {
                    ui.label("Edges");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut f.boundary, Boundary::Dead, "Dead");
                        ui.radio_value(&mut f.boundary, Boundary::Wrap, "Wrap");
                        ui.radio_value(&mut f.boundary, Boundary::Live, "Live");
                    });
                    ui.end_row();
                }
                ui.label("Start");
                seed_ui(ui, f);
                ui.end_row();
                ui.label("Neighbours");
                egui::ComboBox::from_id_salt(id("nb"))
                    .selected_text(match f.neighborhood.as_str() {
                        "touch" => "Touching (edge or corner)",
                        "edge" => "Edge only",
                        _ => "Rule's default",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut f.neighborhood, String::new(), "Rule's default");
                        ui.selectable_value(
                            &mut f.neighborhood,
                            "touch".into(),
                            "Touching (edge or corner)",
                        );
                        ui.selectable_value(&mut f.neighborhood, "edge".into(), "Edge only");
                    });
                ui.end_row();
                ui.label("Random seed");
                ui.add(egui::DragValue::new(&mut f.rng_seed).range(0..=u64::MAX))
                    .on_hover_text("Every run uses this RNG seed, so results can be reproduced");
                ui.end_row();
            });
    } else {
        // Seed override for the swept CA in base mode.
        ui.horizontal(|ui| {
            let mut own = !f.seed.is_empty() || f.seed_exact.is_some();
            if ui
                .checkbox(&mut own, "Give the swept CA a different start:")
                .changed()
            {
                if own {
                    f.seed = if f.series.rows() {
                        "top_center"
                    } else {
                        "random"
                    }
                    .into();
                } else {
                    f.seed.clear();
                    f.seed_exact = None;
                }
            }
            if own {
                seed_ui(ui, f);
            }
        });
    }

    section(ui, "Run each world until");
    egui::Grid::new(id("run"))
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Stop when");
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt(id("until"))
                    .selected_text(f.until.label())
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for (k, label) in Until::ALL {
                            ui.selectable_value(&mut f.until, k, label);
                        }
                    });
                match f.until {
                    Until::Generations | Until::PopBelow | Until::PopAbove => {
                        ui.add(egui::DragValue::new(&mut f.until_n).range(0..=u64::MAX / 2));
                        if f.until == Until::Generations {
                            ui.label("generations");
                        } else {
                            ui.label("cells");
                        }
                    }
                    Until::ActBelow | Until::ActAbove => {
                        let mut pct = f.fraction * 100.0;
                        if ui
                            .add(
                                egui::DragValue::new(&mut pct)
                                    .range(0.0..=100.0)
                                    .speed(0.01)
                                    .suffix("%"),
                            )
                            .changed()
                        {
                            f.fraction = pct / 100.0;
                        }
                        ui.label("for");
                        ui.add(
                            egui::DragValue::new(&mut f.for_gens)
                                .range(1..=100_000)
                                .suffix(" gens"),
                        );
                    }
                    Until::Repeats => {
                        ui.label("period up to");
                        ui.add(egui::DragValue::new(&mut f.max_period).range(1..=100_000));
                    }
                    _ => {}
                }
            });
            ui.end_row();
            if f.until != Until::Generations {
                ui.label("At most");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut f.max_gens).range(1..=100_000_000));
                    ui.label("generations");
                });
                ui.end_row();
            }
        });

    section(ui, "Images");
    egui::Grid::new(id("img"))
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Save");
            ui.checkbox(&mut f.images, "a PNG of each run's last state");
            ui.end_row();
            if f.images {
                ui.label("Image size");
                ui.horizontal(|ui| {
                    let mut custom = f.image_size.is_some();
                    ui.radio_value(&mut custom, false, "Screen");
                    ui.radio_value(&mut custom, true, "Custom");
                    if custom {
                        let (mut w, mut h) = f.image_size.unwrap_or((800, 600));
                        ui.add(egui::DragValue::new(&mut w).range(16..=20_000));
                        ui.label("×");
                        ui.add(egui::DragValue::new(&mut h).range(16..=20_000));
                        f.image_size = Some((w, h));
                    } else {
                        f.image_size = None;
                    }
                });
                ui.end_row();
                ui.label("Compression");
                ui.horizontal(|ui| {
                    for c in ["fast", "balanced", "high"] {
                        ui.radio_value(&mut f.compression, c.to_string(), c);
                    }
                })
                .response
                .on_hover_text("fast: about 8x quicker to write, 2x larger files");
                ui.end_row();
                ui.label("Skip images of");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut f.skip_empty, "empty")
                        .on_hover_text("No live cells");
                    ui.checkbox(&mut f.skip_uniform, "uniform")
                        .on_hover_text("At most two distinct rows (blank or one repeated row); off lattices, every cell alike");
                    ui.checkbox(&mut f.skip_full, "full")
                        .on_hover_text("No cell in state 0");
                });
                ui.end_row();
            }
            ui.label("Palette");
            let mut pal = (!f.palette.is_empty()).then(|| f.palette.clone());
            if crate::palette_pick::picker_opt(ui, id("palette"), &mut pal, &[], "Rule's default") {
                f.palette = pal.unwrap_or_default();
            }
            ui.end_row();
            ui.label("Folder name");
            ui.horizontal(|ui| {
                let hint = f.default_label();
                ui.add(
                    egui::TextEdit::singleline(&mut f.label)
                        .hint_text(hint)
                        .desired_width(140.0),
                );
                ui.label(
                    RichText::new(format!(
                        "saved in <Pictures>/JETCARP/<date>_<time>_{}",
                        if f.label.trim().is_empty() { f.default_label() } else { f.label.trim().to_string() }
                    ))
                    .weak(),
                );
            });
            ui.end_row();
        });
    *f != before
}

fn size_choice(s: SizeSpec) -> SizeChoice {
    match s {
        SizeSpec::Rect { w, h } => SizeChoice::Rect(w, h),
        SizeSpec::Cells(c) => SizeChoice::Cells(c),
        SizeSpec::Level(l) => SizeChoice::Level(l),
    }
}

fn seed_ui(ui: &mut egui::Ui, f: &mut SweepForm) {
    ui.horizontal(|ui| {
        let label = match &f.seed_exact {
            Some(_) => "As in the current world".to_string(),
            None => SEEDS
                .iter()
                .find(|x| x.0 == f.seed)
                .map(|x| x.1.to_string())
                .unwrap_or_else(|| f.seed.clone()),
        };
        egui::ComboBox::from_id_salt("sweep-form-seed")
            .selected_text(label)
            .width(200.0)
            .show_ui(ui, |ui| {
                for (s, label, _) in SEEDS {
                    if ui
                        .selectable_label(f.seed_exact.is_none() && f.seed == *s, *label)
                        .clicked()
                    {
                        f.seed = s.to_string();
                        f.seed_exact = None;
                    }
                }
            });
        if f.seed_exact.is_none() && SEEDS.iter().any(|x| x.0 == f.seed && x.2) {
            let mut pct = f.density * 100.0;
            if ui
                .add(
                    egui::DragValue::new(&mut pct)
                        .range(0.0..=100.0)
                        .speed(0.5)
                        .suffix("% live"),
                )
                .changed()
            {
                f.density = pct / 100.0;
            }
        }
    });
}

fn tiling_ui(ui: &mut egui::Ui, f: &mut SweepForm) {
    ui.horizontal(|ui| {
        let current = match &f.tiling {
            TilingChoice::Id(id) => families::family(id)
                .map(|x| x.name.to_string())
                .or_else(|| catalog::get(id).map(|c| format!("{} {}", c.id, c.name)))
                .unwrap_or_else(|| id.clone()),
            TilingChoice::Exact { label, .. } => format!("Current: {label}"),
        };
        egui::ComboBox::from_id_salt("sweep-form-tiling")
            .selected_text(current)
            .width(240.0)
            .height(480.0)
            .show_ui(ui, |ui| {
                if let TilingChoice::Exact { label, .. } = &f.tiling {
                    let _ = ui.selectable_label(true, format!("Current: {label}"));
                }
                let mut group = "";
                for fam in FAMILIES.iter().filter(|x| x.id != "periodic") {
                    if fam.group != group {
                        group = fam.group;
                        ui.label(RichText::new(group).small().weak());
                    }
                    let on = f.tiling == TilingChoice::Id(fam.id.to_string());
                    if ui.selectable_label(on, fam.name).clicked() {
                        f.tiling = TilingChoice::Id(fam.id.to_string());
                        if is_aperiodic(fam.id) {
                            f.size = SizeChoice::Level(aperiodic::default_level(fam.id));
                        } else if matches!(f.size, SizeChoice::Level(_)) {
                            f.size = SizeChoice::Screen;
                        }
                    }
                }
            });
        let id = ui.make_persistent_id("sweep-form-tiling-id");
        let mut text: String = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
        let r = ui.add(
            egui::TextEdit::singleline(&mut text)
                .hint_text("or an id: k3-012, ih41")
                .desired_width(130.0),
        );
        if r.lost_focus() && !text.trim().is_empty() {
            match jetcarp_api::args::tiling_by_id(text.trim()) {
                Ok(_) => {
                    f.tiling = TilingChoice::Id(text.trim().to_ascii_lowercase());
                    text.clear();
                }
                Err(e) => {
                    r.on_hover_text(e);
                }
            }
        }
        ui.data_mut(|d| d.insert_temp(id, text));
    });
}

/// `19,683`.
pub fn thousands(n: u64) -> String {
    let d = n.to_string();
    let mut t = String::new();
    for (i, ch) in d.chars().enumerate() {
        if i > 0 && (d.len() - i).is_multiple_of(3) {
            t.push(',');
        }
        t.push(ch);
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hexw3_atlas_from_the_form() {
        // The README's atlas: every HexW3 code, top to bottom of the screen.
        let f = SweepForm::default();
        let v = f.to_json();
        assert_eq!(v["rules"], "HexW3");
        assert_eq!(v["from"], 0);
        assert_eq!(v["to"], 19682);
        assert_eq!(v["tiling"], "hex");
        assert_eq!(v["size"], "screen");
        assert_eq!(v["boundary"], "dead");
        assert_eq!(v["seed"], "top_center");
        assert_eq!(v["until"], "done");
        assert_eq!(v["out"], "HexW3");
        assert!(v.get("png").is_none() && v.get("skip").is_none());
        assert_eq!(f.runs().unwrap(), 19683);
        let script = jetcarp_script::sweep_to_script(&v);
        assert!(jetcarp_script::check(&script).is_ok(), "{script}");
    }

    #[test]
    fn options_reach_the_json() {
        let mut f = SweepForm::default();
        f.set_series(Series::Bs);
        assert_eq!(f.seed, "random");
        f.birth = "2-3".into();
        f.survive = "2-3".into();
        f.sample = 0;
        f.until = Until::ActBelow;
        f.fraction = 0.002;
        f.images = true;
        f.image_size = Some((320, 200));
        f.compression = "fast".into();
        f.skip_empty = true;
        f.skip_uniform = true;
        f.palette = "Ink".into();
        f.size = SizeChoice::Rect(64, 48);
        f.label = "my sweep".into();
        let v = f.to_json();
        assert_eq!(v["rules"], "B/S");
        assert_eq!(v["birth"], "2-3");
        assert!(v.get("sample").is_none());
        assert_eq!(v["seed"], "random:0.3");
        assert_eq!(v["until"], "activity_below");
        assert_eq!(v["fraction"], 0.002);
        assert_eq!(v["png"]["size"], json!([320, 200]));
        assert_eq!(v["png"]["compression"], "fast");
        assert_eq!(v["skip"], json!(["empty", "uniform"]));
        assert_eq!(v["palette"], "Ink");
        assert_eq!(v["size"], json!([64, 48]));
        assert_eq!(v["out"], "my sweep");
        assert_eq!(f.runs().unwrap(), 16);
        f.series = Series::List;
        f.list = "B3/S23\nBrian's Brain\n# a comment\nW30".into();
        assert_eq!(
            f.to_json()["rules"],
            json!(["B3/S23", "Brian's Brain", "W30"])
        );
        f.series = Series::Template;
        f.template = "B3/S{n}".into();
        f.from = 1;
        f.to = 8;
        assert_eq!(f.runs().unwrap(), 8);
        f.images = false;
        assert_eq!(f.to_json()["png"], json!(false));
        f.series = Series::W;
        f.to = 300;
        assert!(f.runs().is_err());
    }

    #[test]
    fn from_a_plain_world() {
        let mut w = World::new(TilingSpec::rect("square", 61, 40, Boundary::Dead), 1).unwrap();
        let mut ca = CaSpec::new("W90");
        ca.seed = Some(SeedSpec::TopCenter);
        w.apply(Command::AddCa { ca }).unwrap();
        w.apply(Command::Step { gens: 10 }).unwrap();
        let f = SweepForm::from_world(&w);
        assert!(f.base.is_none());
        assert_eq!(f.series, Series::W);
        assert_eq!((f.from, f.to), (0, 255));
        assert_eq!(f.size, SizeChoice::Rect(61, 40));
        let v = f.to_json();
        assert_eq!(v["tiling"], "square");
        assert_eq!(v["size"], json!([61, 40]));
        assert_eq!(v["seed"], "top_center");
        assert_eq!(v["boundary"], "dead");
        assert_eq!(f.runs().unwrap(), 256);
    }

    #[test]
    fn from_a_world_with_drawn_cells() {
        // Two CAs and drawn cells: the world becomes the base scene, one CA's rule is swept,
        // and a run of the sweep equals the world restarted.
        let mut w = World::new(TilingSpec::rect("hex", 40, 30, Boundary::Wrap), 5).unwrap();
        let a = w
            .apply(Command::AddCa {
                ca: CaSpec::new("B2/S/C3"),
            })
            .unwrap()
            .new_id
            .unwrap();
        let _ = a;
        let b = w
            .apply(Command::AddCa {
                ca: CaSpec::new("B2/S34H"),
            })
            .unwrap()
            .new_id
            .unwrap();
        w.apply(Command::SetCells {
            id: b,
            cells: vec![[100, 1], [101, 1], [141, 1]],
        })
        .unwrap();
        w.apply(Command::Step { gens: 7 }).unwrap();
        let f = SweepForm::from_world(&w);
        let base = f.base.as_ref().expect("base scene");
        assert_eq!(base.cas.len(), 2);
        // Brian's Brain (a Generations rule) is the first sweepable CA.
        assert_eq!(base.vary, 0);
        assert_eq!(f.series, Series::Bs);
        let mut f = f;
        // Sweep the second CA instead.
        f.base.as_mut().unwrap().vary = 1;
        f.series = Series::List;
        f.list = "B2/S34H\nB2/S3".into();
        f.until = Until::Generations;
        f.until_n = 6;
        f.images = false;
        let v = f.to_json();
        assert!(v.get("tiling").is_none());
        assert_eq!(v["ca"], 1);
        let root = std::env::temp_dir().join(format!("jetcarp-app-form-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let s = jetcarp_script::run(
            &jetcarp_script::sweep_to_script(&v),
            jetcarp_script::RunOptions {
                out_root: Some(root.clone()),
                ..Default::default()
            },
            |_| {},
            Default::default(),
        );
        assert!(s.ok, "{:?}", s.error);
        let dir = std::path::PathBuf::from(s.sweeps[0]["folder"].as_str().unwrap());
        let m: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap())
                .unwrap();
        // The B2/S34H run is the world itself restarted and run 6 generations.
        let mut sc = w.to_scene();
        sc.commands.retain(|c| !matches!(c, Command::Step { .. }));
        let mut again = World::from_scene(&sc).unwrap();
        again.apply(Command::Step { gens: 6 }).unwrap();
        assert_eq!(m["rows"][0]["hash"], json!(again.hash_hex()));
        let _ = std::fs::remove_dir_all(&root);
    }
}
