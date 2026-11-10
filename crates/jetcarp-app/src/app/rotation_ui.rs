//! The rotation chooser and editor in the Screensaver pane.

use super::*;
use jetcarp_display::rotation::{
    DAY_NAMES, End, Item, Look, Order, Rotation, RotationStore, Schedule, Source,
};

/// The rotation being edited.
#[derive(Default)]
pub(super) struct RotationUi {
    draft: Option<Rotation>,
    /// The result of the last save or check.
    note: Option<(String, bool)>,
    /// The draft's script compiled last time it changed, or why not.
    script_error: Option<String>,
    quick_start_id: String,
}

enum Do {
    Edit,
    New,
    Duplicate,
    Delete,
    Save,
    Close,
    Start,
}

const STARTER_SCRIPT: &str = r#"// Runs about once a second. `s` describes what is showing, `state` keeps
// anything you like between runs. Answer "next", "previous", "reseed",
// an item number, or #{item: "label"}. Anything else just continues.
if s.generation > 20000 { "next" }
"#;

impl JetcarpApp {
    pub(super) fn rotation_store(&mut self) -> &mut RotationStore {
        if !self.display.rotations.is_loaded() {
            let dir = RotationStore::dir_for_settings(self.shell.store.path());
            self.display.rotations = RotationStore::at(dir);
        }
        &mut self.display.rotations
    }

    /// Open the chosen rotation in the editor.
    pub(super) fn rotation_edit_chosen(&mut self) {
        let name = self.shell.display.screensaver.rotation.clone();
        self.display.rotation_ui.draft = self.rotation_store().get(&name);
    }

    /// Choose a rotation, and edit it.
    pub(super) fn rotation_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let current = self.shell.display.screensaver.rotation.clone();
        let store = self.rotation_store();
        let names = store.names();
        let errors = store.errors.clone();
        let chosen = store.listed(&current);
        let profile_names = self.profile_store().names();
        let c = &mut self.shell.display.screensaver;
        ui.horizontal(|ui| {
            ui.label("Rotation");
            egui::ComboBox::from_id_salt("saver-rotation")
                .selected_text(c.rotation.as_str())
                .show_ui(ui, |ui| {
                    for n in &names {
                        ui.selectable_value(&mut c.rotation, n.clone(), n);
                    }
                });
        });
        for e in &errors {
            ui.colored_label(Color32::from_rgb(255, 170, 90), e);
        }
        let can_delete = chosen.as_ref().is_some_and(|l| l.file.is_some());
        let delete_label = if chosen.as_ref().is_some_and(|l| l.builtin) {
            "Revert to built-in"
        } else {
            "Delete"
        };
        match &chosen {
            Some(l) if !l.rotation.description.is_empty() => {
                ui.label(RichText::new(&l.rotation.description).small().weak());
            }
            None => {
                ui.colored_label(
                    Color32::from_rgb(255, 170, 90),
                    format!("No rotation named {:?}: Life shows instead.", c.rotation),
                );
            }
            _ => {}
        }
        let mut action = None;
        let editing = self.display.rotation_ui.draft.is_some();
        ui.horizontal_wrapped(|ui| {
            if !editing {
                if chosen.is_some() && ui.button("Edit").clicked() {
                    action = Some(Do::Edit);
                }
                if ui.button("New").clicked() {
                    action = Some(Do::New);
                }
                if chosen.is_some() && ui.button("Duplicate").clicked() {
                    action = Some(Do::Duplicate);
                }
                if can_delete && ui.button(delete_label).clicked() {
                    action = Some(Do::Delete);
                }
            }
        });
        let state = &mut self.display.rotation_ui;
        if let Some(r) = &mut state.draft {
            ui.separator();
            let qs = &mut state.quick_start_id;
            let script_error = &mut state.script_error;
            editor(ui, r, &profile_names, qs, script_error);
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Save").strong()).clicked() {
                    action = Some(Do::Save);
                }
                if ui
                    .button("Save and start")
                    .on_hover_text("Save, then run the screensaver on this rotation")
                    .clicked()
                {
                    action = Some(Do::Start);
                }
                if ui.button("Close").on_hover_text("Discard edits").clicked() {
                    action = Some(Do::Close);
                }
            });
        }
        if let Some((t, err)) = &state.note {
            let color = if *err {
                Color32::from_rgb(255, 110, 110)
            } else {
                ui.visuals().weak_text_color()
            };
            ui.label(RichText::new(t).small().color(color));
        }
        if let Some(a) = action {
            self.rotation_do(a, chosen.map(|l| l.rotation), ctx);
        }
    }

    fn rotation_do(&mut self, a: Do, chosen: Option<Rotation>, ctx: &egui::Context) {
        let free_name = |store: &RotationStore, base: &str| {
            let names = store.names();
            if !names.iter().any(|n| n == base) {
                return base.to_string();
            }
            (2..)
                .map(|k| format!("{base} {k}"))
                .find(|n| !names.contains(n))
                .expect("a free name")
        };
        match a {
            Do::Edit => {
                self.display.rotation_ui.draft = chosen;
                self.display.rotation_ui.note = None;
            }
            Do::New => {
                let name = free_name(self.rotation_store(), "My rotation");
                self.display.rotation_ui.draft = Some(Rotation {
                    name,
                    items: vec![Item::new(Source::Profile {
                        name: "Life".into(),
                    })],
                    ..Default::default()
                });
                self.display.rotation_ui.note = None;
            }
            Do::Duplicate => {
                if let Some(mut r) = chosen {
                    r.name = free_name(self.rotation_store(), &format!("{} copy", r.name));
                    self.display.rotation_ui.draft = Some(r);
                    self.display.rotation_ui.note = None;
                }
            }
            Do::Delete => {
                let name = self.shell.display.screensaver.rotation.clone();
                let res = self.rotation_store().delete(&name);
                self.display.rotation_ui.note = res.err().map(|e| (e, true));
                if self.rotation_store().get(&name).is_none() {
                    self.shell.display.screensaver.rotation = SaverSettings::default().rotation;
                }
            }
            Do::Close => {
                self.display.rotation_ui.draft = None;
                self.display.rotation_ui.note = None;
                self.display.rotation_ui.script_error = None;
            }
            Do::Save | Do::Start => {
                let Some(r) = self.display.rotation_ui.draft.clone() else {
                    return;
                };
                match self.rotation_store().save(&r) {
                    Ok(p) => {
                        self.display.rotation_ui.note =
                            Some((format!("Saved to {}.", p.display()), false));
                        let s = &mut self.shell.display.screensaver;
                        s.rotation = r.name;
                        s.source = SaverSource::Rotation;
                        if matches!(a, Do::Start) {
                            self.saver_restart(ctx);
                        }
                    }
                    Err(e) => self.display.rotation_ui.note = Some((e, true)),
                }
            }
        }
    }
}

use super::display::{SaverSettings, SaverSource};

fn editor(
    ui: &mut egui::Ui,
    r: &mut Rotation,
    profile_names: &[String],
    quick_start_id: &mut String,
    script_error: &mut Option<String>,
) {
    ui.horizontal(|ui| {
        ui.label("Name");
        ui.text_edit_singleline(&mut r.name);
    });
    ui.horizontal(|ui| {
        ui.label("Order");
        egui::ComboBox::from_id_salt("rotation-order")
            .selected_text(r.order.label())
            .show_ui(ui, |ui| {
                for o in Order::ALL {
                    ui.selectable_value(&mut r.order, o, o.label());
                }
            });
        let mut limit = r.default_minutes.is_some();
        if ui
            .checkbox(&mut limit, "Default")
            .on_hover_text("Minutes for items with no ending of their own")
            .changed()
        {
            r.default_minutes = limit.then_some(5.0);
        }
        if let Some(m) = &mut r.default_minutes {
            ui.add(
                egui::DragValue::new(m)
                    .range(0.05..=10080.0)
                    .speed(0.1)
                    .suffix(" min"),
            );
        }
    });
    look_ui(ui, "rotation-look", &mut r.look, "Settings for every item");

    let mut remove = None;
    let mut swap = None;
    let n = r.items.len();
    for (i, it) in r.items.iter_mut().enumerate() {
        ui.push_id(i, |ui| {
            ui.horizontal(|ui| {
                if ui.small_button("×").on_hover_text("Remove").clicked() {
                    remove = Some(i);
                }
                if ui
                    .add_enabled(i > 0, egui::Button::new("↑").small())
                    .clicked()
                {
                    swap = Some((i - 1, i));
                }
                if ui
                    .add_enabled(i + 1 < n, egui::Button::new("↓").small())
                    .clicked()
                {
                    swap = Some((i, i + 1));
                }
                let missing = matches!(&it.source, Source::Profile { name }
                    if !profile_names.contains(name));
                let text = RichText::new(it.source.describe());
                let text = if missing {
                    text.color(Color32::from_rgb(255, 170, 90))
                } else {
                    text
                };
                egui::CollapsingHeader::new(text)
                    .id_salt("item")
                    .show(ui, |ui| item_ui(ui, it));
            });
        });
    }
    if let Some(i) = remove {
        r.items.remove(i);
    }
    if let Some((a, b)) = swap {
        r.items.swap(a, b);
    }
    ui.horizontal_wrapped(|ui| {
        ui.menu_button("Add profile", |ui| {
            for n in profile_names {
                if ui.button(n).clicked() {
                    r.items.push(Item::new(Source::Profile { name: n.clone() }));
                    ui.close();
                }
            }
        });
        if ui.button("Add scene…").clicked()
            && let Some(ps) = rfd::FileDialog::new()
                .add_filter("JETCARP scene", &["json"])
                .pick_files()
        {
            for p in ps {
                r.items.push(Item::new(Source::Scene {
                    path: p.display().to_string(),
                }));
            }
        }
        if ui
            .button("Add series")
            .on_hover_text("One item per rule, for example every Wolfram rule")
            .clicked()
        {
            r.items.push(
                Item::new(Source::Series {
                    rules: "W".into(),
                    from: None,
                    to: None,
                    family: "square".into(),
                    boundary: jetcarp_core::Boundary::Dead,
                    seed: String::new(),
                })
                .end(End::Done)
                .hold(3.0),
            );
        }
        let e = ui.add(
            egui::TextEdit::singleline(quick_start_id)
                .desired_width(120.0)
                .hint_text("Quick Start id"),
        );
        if e.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && !quick_start_id.trim().is_empty()
        {
            r.items.push(Item::new(Source::QuickStart {
                id: quick_start_id.trim().to_string(),
            }));
            quick_start_id.clear();
        }
    });

    egui::CollapsingHeader::new("Script")
        .id_salt("rotation-script")
        .default_open(!r.script.is_empty())
        .show(ui, |ui| {
            if r.script.is_empty() {
                if ui.button("Add a script").clicked() {
                    r.script = STARTER_SCRIPT.into();
                }
                return;
            }
            let resp = crate::rhai_edit::editor(ui, &mut r.script, None, 130.0);
            if resp.changed() {
                let check = Rotation {
                    name: "check".into(),
                    items: vec![Item::new(Source::Scene { path: "x".into() })],
                    script: r.script.clone(),
                    ..Default::default()
                };
                *script_error = check.validate().err();
            }
            if let Some(e) = script_error.as_deref() {
                ui.colored_label(Color32::from_rgb(255, 110, 110), e);
            }
            ui.label(
                RichText::new(
                    "s: index, count, label, n, generation, seconds, population, cells, activity, \
                     done, settled, hour, minute, weekday (0 is Monday)",
                )
                .small()
                .weak(),
            );
        });
}

fn item_ui(ui: &mut egui::Ui, it: &mut Item) {
    if let Source::Series {
        rules,
        from,
        to,
        family,
        seed,
        ..
    } = &mut it.source
    {
        ui.horizontal(|ui| {
            ui.label("Rules");
            ui.add(
                egui::TextEdit::singleline(rules)
                    .desired_width(110.0)
                    .hint_text("W, HexW, or a list"),
            );
            ui.label("Tiling");
            ui.add(egui::TextEdit::singleline(family).desired_width(90.0));
        });
        ui.horizontal(|ui| {
            optional_number(ui, "From", from);
            optional_number(ui, "To", to);
            ui.label("Seed");
            ui.add(
                egui::TextEdit::singleline(seed)
                    .desired_width(90.0)
                    .hint_text("the rule's own"),
            );
        });
    }
    ui.label(RichText::new("Ends (any of these)").small().strong());
    let mut remove = None;
    for (i, e) in it.end.iter_mut().enumerate() {
        ui.push_id(i, |ui| {
            ui.horizontal(|ui| {
                if ui.small_button("×").clicked() {
                    remove = Some(i);
                }
                end_ui(ui, e);
            });
        });
    }
    if let Some(i) = remove {
        it.end.remove(i);
    }
    ui.horizontal(|ui| {
        ui.menu_button("Add ending", |ui| {
            for (label, e) in end_kinds() {
                if ui.button(label).clicked() {
                    it.end.push(e);
                    ui.close();
                }
            }
        });
        ui.label("Hold");
        ui.add(
            egui::DragValue::new(&mut it.hold_s)
                .range(0.0..=3600.0)
                .speed(0.1)
                .suffix(" s"),
        )
        .on_hover_text("Keep showing the finished world this long before moving on");
    });
    if it.end.is_empty() {
        ui.label(
            RichText::new("No ending: the profile's minutes, else the rotation's default.")
                .small()
                .weak(),
        );
    }
    let mut scheduled = it.schedule.is_some();
    if ui
        .checkbox(&mut scheduled, "Only at certain times")
        .changed()
    {
        it.schedule = scheduled.then(Schedule::default);
    }
    if let Some(s) = &mut it.schedule {
        schedule_ui(ui, s);
    }
    look_ui(ui, "item-look", &mut it.look, "Settings for this item");
}

fn end_kinds() -> Vec<(&'static str, End)> {
    vec![
        ("After minutes", End::Minutes { minutes: 10.0 }),
        ("After generations", End::Generations { gens: 10_000 }),
        ("When settled", End::Settled { latest: None }),
        (
            "On low activity",
            End::LowActivity {
                below: 0.005,
                for_gens: 120,
            },
        ),
        ("When finished", End::Done),
        ("At a time of day", End::Clock { at: "08:00".into() }),
    ]
}

fn end_ui(ui: &mut egui::Ui, e: &mut End) {
    match e {
        End::Minutes { minutes } => {
            ui.label("After");
            ui.add(
                egui::DragValue::new(minutes)
                    .range(0.05..=10080.0)
                    .speed(0.1)
                    .suffix(" min"),
            );
        }
        End::Generations { gens } => {
            ui.label("After");
            ui.add(
                egui::DragValue::new(gens)
                    .range(1..=u64::MAX)
                    .speed(10.0)
                    .suffix(" generations"),
            );
        }
        End::Settled { latest } => {
            ui.label("When settled");
            optional_number(ui, "at most", latest);
        }
        End::LowActivity { below, for_gens } => {
            ui.label("Under");
            let mut pct = *below * 100.0;
            if ui
                .add(
                    egui::DragValue::new(&mut pct)
                        .range(0.0..=100.0)
                        .speed(0.01)
                        .suffix("%"),
                )
                .changed()
            {
                *below = pct / 100.0;
            }
            ui.label("changing for");
            ui.add(
                egui::DragValue::new(for_gens)
                    .range(1..=1_000_000)
                    .suffix(" generations"),
            );
        }
        End::Done => {
            ui.label("When finished")
                .on_hover_text("A rows rule reaches the bottom, or nothing can change any more");
        }
        End::Clock { at } => {
            ui.label("At");
            ui.add(egui::TextEdit::singleline(at).desired_width(50.0));
        }
    }
}

fn optional_number(ui: &mut egui::Ui, label: &str, v: &mut Option<u64>) {
    let mut on = v.is_some();
    if ui.checkbox(&mut on, label).changed() {
        *v = on.then_some(0);
    }
    if let Some(x) = v {
        ui.add(egui::DragValue::new(x).speed(1.0));
    }
}

fn schedule_ui(ui: &mut egui::Ui, s: &mut Schedule) {
    ui.horizontal_wrapped(|ui| {
        for d in DAY_NAMES {
            let mut on = s.days.iter().any(|x| x == d);
            if ui.checkbox(&mut on, d).changed() {
                s.days.retain(|x| x != d);
                if on {
                    s.days.push(d.to_string());
                }
            }
        }
    });
    let mut hours = s.from.is_some();
    ui.horizontal(|ui| {
        if ui.checkbox(&mut hours, "Between").changed() {
            s.from = hours.then(|| "09:00".to_string());
            s.to = hours.then(|| "17:00".to_string());
        }
        if let (Some(a), Some(b)) = (&mut s.from, &mut s.to) {
            ui.add(egui::TextEdit::singleline(a).desired_width(50.0));
            ui.label("and");
            ui.add(egui::TextEdit::singleline(b).desired_width(50.0));
        }
    });
}

fn look_ui(ui: &mut egui::Ui, id: &str, l: &mut Look, title: &str) {
    egui::CollapsingHeader::new(RichText::new(title).small())
        .id_salt(id)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut on = l.cell_px.is_some();
                if ui.checkbox(&mut on, "Cell size").changed() {
                    l.cell_px = on.then_some(4.0);
                }
                if let Some(x) = &mut l.cell_px {
                    ui.add(
                        egui::DragValue::new(x)
                            .range(profiles::CELL_PX_RANGE)
                            .speed(0.1)
                            .suffix(" px"),
                    );
                }
                let mut on = l.speed.is_some();
                if ui.checkbox(&mut on, "Speed").changed() {
                    l.speed = on.then_some(30.0);
                }
                if let Some(x) = &mut l.speed {
                    ui.add(
                        egui::DragValue::new(x)
                            .range(0.0..=100_000.0)
                            .speed(1.0)
                            .suffix(" gen/s"),
                    )
                    .on_hover_text("0 runs as fast as possible");
                }
            });
            ui.horizontal(|ui| {
                let mut on = l.palette.is_some();
                if ui.checkbox(&mut on, "Palette").changed() {
                    l.palette = on.then(|| "Ember".to_string());
                }
                if let Some(x) = &mut l.palette {
                    ui.add(egui::TextEdit::singleline(x).desired_width(90.0));
                }
                let mut on = l.refill.is_some();
                if ui
                    .checkbox(&mut on, "Set fill")
                    .on_hover_text("Choose whether periodic tilings are rebuilt to fill the screen")
                    .changed()
                {
                    l.refill = on.then_some(true);
                }
                if let Some(x) = &mut l.refill {
                    ui.checkbox(x, "Fill the screen");
                }
                let mut none = l.reseed.as_ref().is_some_and(|r| r.is_empty());
                if ui.checkbox(&mut none, "No reseeding").changed() {
                    l.reseed = none.then(Vec::new);
                }
            });
        });
}
