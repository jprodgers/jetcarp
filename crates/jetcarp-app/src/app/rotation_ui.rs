//! The rotation editor: in the Screensaver pane for the screensaver's rotation, and in the Desktop
//! background pane for each screen's. Saving chooses the rotation for the one it was opened from.
//! And *Show just this scene*, in the Desktop background pane: the scene open in the app saved as
//! a profile and a rotation of just that profile, chosen for a screen.

use super::*;
use jetcarp_display::host_settings::{HostSettings, ScreenSettings};
use jetcarp_display::rotation::{
    DAY_NAMES, End, Item, Look, Order, Rotation, RotationStore, Schedule, Source,
};

/// What a rotation saved in the editor is chosen for.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub(super) enum Target {
    /// The screensaver (the Screensaver pane).
    #[default]
    Saver,
    /// Screen `i` of the desktop background (the Desktop background pane).
    Host(usize),
}

/// The rotation being edited in one pane.
#[derive(Default)]
pub(super) struct RotationUi {
    /// What saving chooses it for.
    target: Target,
    draft: Option<Rotation>,
    /// The result of the last save or check.
    note: Option<(String, bool)>,
    /// The draft's script compiled last time it changed, or why not.
    script_error: Option<String>,
    quick_start_id: String,
}

impl RotationUi {
    /// The desktop background screen whose rotation is open in the editor.
    pub(super) fn host_screen(&self) -> Option<usize> {
        match (self.target, &self.draft) {
            (Target::Host(i), Some(_)) => Some(i),
            _ => None,
        }
    }

    /// Desktop background screen `i` was removed: its editor closes, and the screens after it
    /// move up.
    pub(super) fn screen_removed(&mut self, i: usize) {
        if let Target::Host(j) = self.target {
            if j == i {
                self.draft = None;
                self.script_error = None;
            } else if j > i {
                self.target = Target::Host(j - 1);
            }
        }
    }

    /// The last note, when the editor (which shows it by its buttons) is closed.
    pub(super) fn note_ui(&self, ui: &mut egui::Ui) {
        if self.draft.is_none() {
            note_ui(ui, &self.note);
        }
    }
}

pub(super) enum Do {
    Edit,
    New,
    Duplicate,
    Delete,
    Save,
    Close,
    Start,
    /// *Show just this scene* (a desktop background screen).
    Just,
}

const STARTER_SCRIPT: &str = r#"// Runs about once a second. `s` describes what is showing, `state` keeps
// anything you like between runs. Answer "next", "previous", "reseed",
// an item number, or #{item: "label"}. Anything else just continues.
if s.generation > 20000 { "next" }
"#;

/// Chooses rotation `name` for `target`: the screensaver's rotation (and *A rotation* as what it
/// shows), or desktop background screen `i`'s. Nothing else changes. True when it was for a
/// screen that exists, whose settings are then saved and told to the host (which shows the
/// rotation as saved, even under the same name).
fn choose(target: Target, name: &str, saver: &mut SaverSettings, host: &mut HostSettings) -> bool {
    match target {
        Target::Saver => {
            saver.rotation = name.to_string();
            saver.source = SaverSource::Rotation;
            false
        }
        Target::Host(i) => match host.screens.get_mut(i) {
            Some(s) => {
                s.rotation = name.to_string();
                true
            }
            None => false,
        },
    }
}

impl JetcarpApp {
    pub(super) fn rotation_store(&mut self) -> &mut RotationStore {
        if !self.display.rotations.is_loaded() {
            let dir = RotationStore::dir_for_settings(self.shell.store.path());
            self.display.rotations = RotationStore::at(dir);
        }
        &mut self.display.rotations
    }

    /// The editor's state for `target`: the Screensaver pane's or the Desktop background pane's.
    fn rotation_state(&mut self, target: Target) -> &mut RotationUi {
        match target {
            Target::Saver => &mut self.display.rotation_ui,
            Target::Host(_) => &mut self.display.host_rotation_ui,
        }
    }

    /// The rotation chosen for `target` now.
    fn rotation_chosen(&mut self, target: Target) -> String {
        match target {
            Target::Saver => self.shell.display.screensaver.rotation.clone(),
            Target::Host(i) => self
                .display
                .host
                .settings_mut()
                .screens
                .get(i)
                .map(|s| s.rotation.clone())
                .unwrap_or_default(),
        }
    }

    /// Open the screensaver's rotation in the editor.
    pub(super) fn rotation_edit_chosen(&mut self) {
        let name = self.shell.display.screensaver.rotation.clone();
        self.display.rotation_ui.draft = self.rotation_store().get(&name);
    }

    /// The Screensaver pane's rotation: choose one, and edit it.
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
        if let Some(a) = editor_block(
            ui,
            state,
            &profile_names,
            "Save it, and choose it for the screensaver",
        ) {
            action = Some(a);
        }
        state.note_ui(ui);
        if let Some(a) = action {
            self.rotation_do(Target::Saver, a, ctx);
        }
    }

    /// Does an editor action for `target`. True when it chose a rotation for a desktop
    /// background screen: the Desktop background pane then saves its settings and tells the host.
    pub(super) fn rotation_do(&mut self, target: Target, a: Do, ctx: &egui::Context) -> bool {
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
        let chosen_name = self.rotation_chosen(target);
        let chosen = self
            .rotation_store()
            .listed(&chosen_name)
            .map(|l| l.rotation);
        let open = |s: &mut RotationUi, draft: Option<Rotation>| {
            s.target = target;
            s.draft = draft;
            s.note = None;
            s.script_error = None;
        };
        match a {
            Do::Edit => open(self.rotation_state(target), chosen),
            Do::New => {
                let name = free_name(self.rotation_store(), "My rotation");
                let r = Rotation {
                    name,
                    items: vec![Item::new(Source::Profile {
                        name: "Life".into(),
                    })],
                    ..Default::default()
                };
                open(self.rotation_state(target), Some(r));
            }
            Do::Duplicate => {
                if let Some(mut r) = chosen {
                    r.name = free_name(self.rotation_store(), &format!("{} copy", r.name));
                    open(self.rotation_state(target), Some(r));
                }
            }
            Do::Delete => {
                let res = self.rotation_store().delete(&chosen_name);
                self.rotation_state(target).note = res.err().map(|e| (e, true));
                if self.rotation_store().get(&chosen_name).is_none() {
                    match target {
                        Target::Saver => {
                            self.shell.display.screensaver.rotation =
                                SaverSettings::default().rotation;
                        }
                        Target::Host(_) => {
                            let s = &mut self.shell.display.screensaver;
                            let default = ScreenSettings::default().rotation;
                            return choose(target, &default, s, self.display.host.settings_mut());
                        }
                    }
                }
            }
            Do::Close => {
                let s = self.rotation_state(target);
                s.draft = None;
                s.note = None;
                s.script_error = None;
            }
            Do::Save | Do::Start => {
                let Some(r) = self.rotation_state(target).draft.clone() else {
                    return false;
                };
                match self.rotation_store().save(&r) {
                    Ok(p) => {
                        self.rotation_state(target).note =
                            Some((format!("Saved to {}.", p.display()), false));
                        let s = &mut self.shell.display.screensaver;
                        let changed = choose(target, &r.name, s, self.display.host.settings_mut());
                        if matches!(a, Do::Start) && target == Target::Saver {
                            self.saver_restart(ctx);
                        }
                        return changed;
                    }
                    Err(e) => self.rotation_state(target).note = Some((e, true)),
                }
            }
            Do::Just => {
                let Target::Host(i) = target else {
                    return false;
                };
                let done = self.show_just_this_scene(i);
                let ok = done.is_ok();
                self.rotation_state(target).note = Some(match done {
                    Ok(t) => (t, false),
                    Err(e) => (e, true),
                });
                return ok;
            }
        }
        false
    }

    /// *Show just this scene* for desktop background screen `i`: the world open in the app saved
    /// as a profile named after its scene (as *Save current world as profile* does; a profile of
    /// your own of that name, saved by an earlier press, is updated from the world instead, as
    /// *Update from the current world* does), a rotation of just that profile (likewise made once,
    /// then kept), and that rotation chosen for the screen. Returns what it did, in words.
    pub(super) fn show_just_this_scene(&mut self, i: usize) -> Result<String, String> {
        if self.screen != Screen::Sim || self.world.cas.is_empty() {
            return Err("Open a scene first: this shows the scene open in the app.".into());
        }
        let base = self.profile_base_name();
        self.saver_ui_flush(true);
        let name = self.profile_store().own_or_free_name(&base);
        if self.profile_store().listed(&name).is_some() {
            self.profile_update_from_world(&name)?;
        } else {
            let p = self.profile_from_world(&name);
            self.profile_store().save(&p)?;
        }
        let (r, _) = self.rotation_store().save_just(&name, &name)?;
        let host = self.display.host.settings_mut();
        let count = host.screens.len();
        choose(
            Target::Host(i),
            &r.name,
            &mut self.shell.display.screensaver,
            host,
        );
        let screen = if count > 1 {
            format!("screen {}", i + 1)
        } else {
            "the desktop background".into()
        };
        let mut t = if r.name == name {
            format!(
                "Saved the scene as the profile and the rotation {name:?}, and chose it for {screen}."
            )
        } else {
            format!(
                "Saved the scene as the profile {name:?} and the rotation {:?}, and chose it for \
                 {screen}.",
                r.name
            )
        };
        if self.display.host.running().is_none() {
            t.push_str(" Press Start to show it.");
        }
        Ok(t)
    }
}

/// A desktop background screen's buttons, under its rotation: *Edit* (when `listed`, the
/// rotation exists) and *New* while the editor is closed, and *Show just this scene* (`scene`: the
/// scene open in the app, if any).
pub(super) fn host_rotation_buttons(
    ui: &mut egui::Ui,
    listed: bool,
    scene: Option<&str>,
    editing: bool,
) -> Option<Do> {
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        if !editing {
            if ui
                .add_enabled(listed, egui::Button::new("Edit").small())
                .on_hover_text("Edit this rotation here. Save shows it on this screen")
                .clicked()
            {
                action = Some(Do::Edit);
            }
            if ui
                .small_button("New")
                .on_hover_text("Make a rotation here. Save shows it on this screen")
                .clicked()
            {
                action = Some(Do::New);
            }
        }
        let tip = format!(
            "Show {:?}, the scene open in the app, and nothing else on this screen: it is saved \
             as a profile and as a rotation of just that profile, which this screen then runs. \
             Pressing it again for the same scene updates them",
            scene.unwrap_or_default()
        );
        if ui
            .add_enabled(
                scene.is_some(),
                egui::Button::new("Show just this scene").small(),
            )
            .on_hover_text(tip)
            .on_disabled_hover_text("Open a scene in the app first: this shows it on this screen")
            .clicked()
        {
            action = Some(Do::Just);
        }
    });
    action
}

/// The editor, with *Save* and *Close* both above and below it so they show without scrolling
/// down a long rotation, and the last note under each. Nothing while no rotation is open.
pub(super) fn editor_block(
    ui: &mut egui::Ui,
    state: &mut RotationUi,
    profile_names: &[String],
    save_tip: &str,
) -> Option<Do> {
    let RotationUi {
        target,
        draft,
        note,
        script_error,
        quick_start_id,
    } = state;
    let r = draft.as_mut()?;
    let target = *target;
    let mut action = None;
    ui.push_id(("rotation-editor", target), |ui| {
        ui.separator();
        if let Some(a) = editor_buttons(ui, target, save_tip) {
            action = Some(a);
        }
        note_ui(ui, note);
        editor(ui, r, profile_names, quick_start_id, script_error);
        if let Some(a) = editor_buttons(ui, target, save_tip) {
            action = Some(a);
        }
        note_ui(ui, note);
    });
    action
}

fn editor_buttons(ui: &mut egui::Ui, target: Target, save_tip: &str) -> Option<Do> {
    let mut action = None;
    ui.horizontal(|ui| {
        if ui
            .button(RichText::new("Save").strong())
            .on_hover_text(save_tip)
            .clicked()
        {
            action = Some(Do::Save);
        }
        if target == Target::Saver
            && ui
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
    action
}

fn note_ui(ui: &mut egui::Ui, note: &Option<(String, bool)>) {
    if let Some((t, err)) = note {
        let color = if *err {
            Color32::from_rgb(255, 110, 110)
        } else {
            ui.visuals().weak_text_color()
        };
        ui.label(RichText::new(t).small().color(color));
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
        let clicked = ui.button("Add scene…").clicked();
        if let Some(ps) =
            crate::file_dialog::on_click(ui.ctx(), "rotation.add_scene", clicked, || {
                rfd::FileDialog::new()
                    .add_filter("JETCARP scene", &["json"])
                    .pick_files()
            })
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
        let e = ui
            .add(
                egui::TextEdit::singleline(quick_start_id)
                    .desired_width(180.0)
                    .hint_text("Add Quick Start: search"),
            )
            .on_hover_text(
                "Type part of a Quick Start entry's title, rule or id, and pick it below; or type \
                 an id (as `jetcarp quickstart` lists them) and press Enter",
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
    // The Quick Start picker (#74): the entries matching what is typed, to click instead of
    // knowing an id, which `jetcarp quickstart` alone listed.
    let q = quick_start_id.trim().to_lowercase();
    if !q.is_empty() {
        let hits: Vec<&(String, String, String)> = quick_start_list()
            .iter()
            .filter(|(id, title, rule)| {
                id.to_lowercase().contains(&q)
                    || title.to_lowercase().contains(&q)
                    || rule.to_lowercase().contains(&q)
            })
            .take(12)
            .collect();
        egui::Frame::group(ui.style()).show(ui, |ui| {
            if hits.is_empty() {
                ui.label(RichText::new("No Quick Start entry matches").weak());
            }
            for (id, title, rule) in hits {
                let label = if title.is_empty() {
                    id.clone()
                } else {
                    format!("{title}  ({rule})")
                };
                if ui
                    .selectable_label(false, label)
                    .on_hover_text(id.as_str())
                    .clicked()
                {
                    r.items
                        .push(Item::new(Source::QuickStart { id: id.clone() }));
                    quick_start_id.clear();
                }
            }
        });
    }

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

/// The built-in Quick Start entries as the rotation editor's picker lists them: id, title and
/// rule, read once (#74).
fn quick_start_list() -> &'static [(String, String, String)] {
    static LIST: std::sync::OnceLock<Vec<(String, String, String)>> = std::sync::OnceLock::new();
    LIST.get_or_init(|| {
        jetcarp_core::quickstart::entries()
            .into_iter()
            .map(|q| (q.id, q.scene.name, q.rule))
            .collect()
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Saving in the Desktop background pane chooses the rotation for that screen and leaves the
    /// screensaver alone; saving in the Screensaver pane chooses it for the screensaver and
    /// leaves the desktop background alone (issue #41: it set the screensaver's either way).
    #[test]
    fn saving_chooses_the_rotation_for_where_it_was_edited_only() {
        let saver = SaverSettings {
            source: SaverSource::Profile,
            rotation: "Wolfram series".into(),
            ..Default::default()
        };
        let host = HostSettings {
            screens: vec![ScreenSettings::default(), ScreenSettings::default()],
            ..Default::default()
        };

        let (mut s, mut h) = (saver.clone(), host.clone());
        assert!(choose(Target::Host(1), "Mine", &mut s, &mut h));
        assert_eq!(s, saver, "the screensaver's settings are untouched");
        assert_eq!(h.screens[1].rotation, "Mine");
        assert_eq!(h.screens[0], host.screens[0], "and the other screen's");
        assert!(
            !choose(Target::Host(2), "Mine", &mut s, &mut h),
            "no such screen"
        );
        assert_eq!((&s, h.screens.len()), (&saver, 2));

        let (mut s, mut h) = (saver.clone(), host.clone());
        assert!(!choose(Target::Saver, "Mine", &mut s, &mut h));
        assert_eq!(s.rotation, "Mine");
        assert_eq!(s.source, SaverSource::Rotation);
        assert_eq!(h, host, "the desktop background's settings are untouched");
    }

    #[test]
    fn removing_a_screen_closes_its_editor_and_moves_later_ones_up() {
        let open = |i| RotationUi {
            target: Target::Host(i),
            draft: Some(Rotation::just("Mine", "Life")),
            ..Default::default()
        };
        let mut ui = open(1);
        ui.screen_removed(2);
        assert_eq!(ui.host_screen(), Some(1));
        ui.screen_removed(0);
        assert_eq!(ui.host_screen(), Some(0));
        ui.screen_removed(0);
        assert_eq!(ui.host_screen(), None, "closed");
        assert_eq!(RotationUi::default().host_screen(), None);
        let mut saver = RotationUi {
            draft: Some(Rotation::just("Mine", "Life")),
            ..Default::default()
        };
        saver.screen_removed(0);
        assert!(
            saver.draft.is_some(),
            "the Screensaver pane's is not a screen's"
        );
    }
}
