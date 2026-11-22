//! Settings → Accessibility: the palette scenes that name none are drawn in (#217), reduced
//! motion (#218), the high-contrast theme with light or dark (#220), and the fonts (Atkinson
//! Hyperlegible Next and Mono, egui's original, or any installed font: [`crate::fonts`]). Saved
//! with the app's other settings (`accessibility` in `app.json`).
//!
//! Reduced motion and high contrast follow the system's settings ([`crate::a11y`]) unless the
//! app's own setting says otherwise. Each frame the app reads where they stand and, when that
//! changed, puts it in place: egui's animations, the theme, and the Quick Start's *Live*.
//!
//! The strobe guard is not here: it acts on what is drawn, whatever these settings say, and its
//! switches are in Settings → Safety.

use super::menu::ToolWin;
use super::*;
use crate::a11y::{Follow, Watch};
use crate::fonts::{Face, FontChoice};
use crate::theme::{self, Brightness};
use jetcarp_core::cvd;
use serde::{Deserialize, Serialize};

/// The note a scene opened paused for reduced motion carries.
pub(super) const PAUSED_NOTE: &str = "Paused because your system asks for reduced motion. Space runs it; Settings → Accessibility changes this.";

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub(super) struct AccessibilitySettings {
    /// Follow the system's request for reduced motion (the default), or on or off.
    pub reduced_motion: Follow,
    /// The high-contrast theme: as the system asks (the default), or on or off.
    pub high_contrast: Follow,
    /// Light or dark.
    pub brightness: Brightness,
    /// The palette of automata whose scene names none (`None`: Ember, JETCARP's own).
    pub palette: Option<String>,
    /// JACA's +1 and −1 palettes when its scene names none (`None`: Tide and Koi).
    pub jaca_pos: Option<String>,
    pub jaca_neg: Option<String>,
    /// The font of the app's words (Atkinson Hyperlegible Next by default).
    pub font: FontChoice,
    /// The font of monospaced text: rules, scripts, numbers in tables (Atkinson Hyperlegible Mono
    /// by default).
    pub mono_font: FontChoice,
}

impl AccessibilitySettings {
    /// Unknown palette names (a hand-edited file, a palette since removed) go back to JETCARP's.
    pub fn sanitize(mut self) -> Self {
        for p in [&mut self.palette, &mut self.jaca_pos, &mut self.jaca_neg] {
            if p.as_deref()
                .is_some_and(|n| render::Palette::builtin(n).is_none())
            {
                *p = None;
            }
        }
        self
    }

    /// The palettes chosen for colour-vision deficiency are the defaults.
    pub fn chosen_for_cvd(&self) -> bool {
        self.palette.as_deref() == Some(cvd::CHOSEN.palette)
            && self.jaca_pos.as_deref() == Some(cvd::CHOSEN.jaca_pos)
            && self.jaca_neg.as_deref() == Some(cvd::CHOSEN.jaca_neg)
    }

    /// Give the automata of `scene` that name no palette the default ones. True when any did.
    pub fn fill_palettes(&self, scene: &mut Scene) -> bool {
        let mut changed = false;
        for ca in &mut scene.cas {
            let jaca = rules::Rule::parse(&ca.rule).is_ok_and(|(r, _)| matches!(r, Rule::Jaca(_)));
            if jaca {
                if ca.palette.is_none() && self.jaca_pos.is_some() {
                    ca.palette = self.jaca_pos.clone();
                    changed = true;
                }
                if ca.palette_neg.is_none() && self.jaca_neg.is_some() {
                    ca.palette_neg = self.jaca_neg.clone();
                    changed = true;
                }
            } else if ca.palette.is_none() && self.palette.is_some() {
                ca.palette = self.palette.clone();
                changed = true;
            }
        }
        changed
    }
}

/// What is not saved: the system's settings, the window, what is in place.
pub(super) struct A11yRt {
    pub settings: AccessibilitySettings,
    pub watch: Watch,
    pub window_open: bool,
    /// In place now: (reduced motion, high contrast, light or dark).
    applied: Option<(bool, bool, Brightness)>,
    /// The scene being opened was paused for reduced motion (its note is shown once it is in
    /// place).
    pub paused_open: bool,
    /// The fonts: what is in place, the installed ones, a file refused.
    pub fonts: FontsRt,
}

/// The fonts as they are now (not saved).
#[derive(Default)]
pub(super) struct FontsRt {
    /// The choices in place (text, monospaced).
    pub applied: Option<(FontChoice, FontChoice)>,
    /// The fonts installed on the system, found when the window first shows.
    pub scanner: crate::fonts::Scanner,
    /// Why the last font chosen couldn't be used, shown in the window until another is chosen.
    pub refused: Option<String>,
    /// The installed fonts a character no font in place has is drawn in, read when the app
    /// starts.
    pub fallback: crate::fonts::FallbackScan,
}

impl Default for A11yRt {
    fn default() -> Self {
        // Tests and screenshot runs read nothing from this machine's desktop, so they look the
        // same everywhere; their own tests stand a reading in.
        let watch = if cfg!(test) || std::env::var_os("JETCARP_SCREENSHOT").is_some() {
            Watch::new(Box::new(crate::a11y::Fixed::default()))
        } else {
            Watch::os()
        };
        // Nor do they look for this machine's fonts, unless they say where.
        let font_dirs = if cfg!(test) || std::env::var_os("JETCARP_SCREENSHOT").is_some() {
            vec![]
        } else {
            crate::fonts::system_dirs()
        };
        A11yRt {
            settings: Default::default(),
            watch,
            window_open: false,
            applied: None,
            paused_open: false,
            // The app starts with JETCARP's fonts in place (`JetcarpApp::new`).
            fonts: FontsRt {
                applied: Some((FontChoice::Atkinson, FontChoice::Atkinson)),
                scanner: crate::fonts::Scanner::new(font_dirs.clone()),
                refused: None,
                fallback: crate::fonts::FallbackScan::new(font_dirs),
            },
        }
    }
}

impl JetcarpApp {
    /// Reduced motion is on: the app's setting, or the system's when it follows the system.
    pub(super) fn motion_reduced(&self) -> bool {
        self.a11y
            .settings
            .reduced_motion
            .resolve(self.a11y.watch.reading.reduce_motion)
    }

    /// The high-contrast theme is on.
    pub(super) fn contrast_high(&self) -> bool {
        self.a11y
            .settings
            .high_contrast
            .resolve(self.a11y.watch.reading.high_contrast)
    }

    /// Once a frame (and at start): read the system now and then, and put in place what
    /// changed. Reduced motion stops egui's animations and sets the Quick Start's *Live* to
    /// match (the person can tick it again).
    pub(super) fn a11y_frame(&mut self, ctx: &egui::Context) {
        self.fonts_frame(ctx);
        let now = self.started.elapsed().as_secs_f64();
        self.a11y.watch.poll(now);
        let want = (
            self.motion_reduced(),
            self.contrast_high(),
            brightness_shown(
                self.a11y.settings.brightness,
                self.a11y.watch.reading.dark,
                ctx.system_theme(),
            ),
        );
        if self.a11y.applied == Some(want) {
            return;
        }
        let motion_changed = self.a11y.applied.is_none_or(|a| a.0 != want.0);
        crate::a11y::set_motion_reduced(ctx, want.0);
        theme::apply(ctx, want.1, want.2);
        if motion_changed {
            self.menu.view.live = !want.0;
        }
        self.a11y.applied = Some(want);
    }

    /// Put the fonts chosen in place when the choice changed (at once: egui draws with them from
    /// the next frame). A font file that can't be used is refused: the setting goes back to
    /// JETCARP's font, and the window and the status line say why.
    pub(super) fn fonts_frame(&mut self, ctx: &egui::Context) {
        // The installed fonts that draw what no font in place has, once read.
        self.a11y.fonts.fallback.poll(ctx);
        let s = &mut self.a11y.settings;
        let want = (s.font.clone(), s.mono_font.clone());
        if self.a11y.fonts.applied.as_ref() == Some(&want) {
            return;
        }
        let mut refused = None;
        let mut face = |c: &mut FontChoice, what: &str| match c {
            FontChoice::Atkinson => Face::Atkinson,
            FontChoice::Egui => Face::Egui,
            FontChoice::File { path, index, .. } => match crate::fonts::load(path, Some(*index)) {
                Ok(l) => Face::Loaded(l.data),
                Err(e) => {
                    refused = Some(format!(
                        "{e} The {what} is JETCARP's own again (Settings → Accessibility)."
                    ));
                    *c = FontChoice::Atkinson;
                    Face::Atkinson
                }
            },
        };
        let text = face(&mut s.font, "font");
        let mono = face(&mut s.mono_font, "monospace font");
        crate::fonts::install(ctx, &text, &mono);
        self.a11y.fonts.applied = Some((s.font.clone(), s.mono_font.clone()));
        if let Some(e) = refused {
            self.message = Some((e.clone(), true));
            self.a11y.fonts.refused = Some(e);
        }
    }

    /// Whether a scene being opened starts running: as `autoplay` says, unless reduced motion is
    /// on; then it opens paused, with a note saying why ([`PAUSED_NOTE`]).
    pub(super) fn opens_running(&mut self) -> bool {
        self.a11y.paused_open = self.autoplay && self.motion_reduced();
        self.autoplay && !self.a11y.paused_open
    }

    /// A scene as the app opens it: automata that name no palette take the defaults chosen in
    /// Settings → Accessibility.
    pub(super) fn with_default_palettes<'a>(
        &self,
        scene: &'a Scene,
    ) -> std::borrow::Cow<'a, Scene> {
        let mut s = scene.clone();
        if self.a11y.settings.fill_palettes(&mut s) {
            std::borrow::Cow::Owned(s)
        } else {
            std::borrow::Cow::Borrowed(scene)
        }
    }

    /// Settings → Accessibility.
    pub(super) fn accessibility_window(&mut self, ctx: &egui::Context) {
        if !self.a11y.window_open {
            return;
        }
        let mut open = self.a11y.window_open;
        let reading = self.a11y.watch.reading;
        let (motion, high) = (self.motion_reduced(), self.contrast_high());
        let s = &mut self.a11y.settings;
        let fonts = &mut self.a11y.fonts;
        fonts.scanner.start(ctx);
        crate::popout::tool_window(
            ctx,
            crate::popout::Spec::new(ToolWin::Accessibility.key(), "Accessibility")
                .size(540.0, 560.0),
            &mut open,
            |ui| settings_ui(ui, s, fonts, reading, motion, high),
        );
        self.a11y.window_open = open;
    }
}

/// What the system says, in words.
/// Light or dark as shown: the choice, except that *Follow the system* takes the style the app
/// read itself (Linux, #137) where egui has none from winit.
fn brightness_shown(
    chosen: theme::Brightness,
    read: Option<bool>,
    from_winit: Option<egui::Theme>,
) -> theme::Brightness {
    match (chosen, read, from_winit) {
        (theme::Brightness::System, Some(true), None) => theme::Brightness::Dark,
        (theme::Brightness::System, Some(false), None) => theme::Brightness::Light,
        (b, _, _) => b,
    }
}

fn system_word(v: Option<bool>, on: &str, off: &str) -> String {
    match v {
        Some(true) => format!("Your system asks for {on}."),
        Some(false) => format!("Your system {off}."),
        None => "JETCARP can't read your system's setting here.".into(),
    }
}

/// Follow the system, or one of two choices that override it, as radio buttons.
fn follow_choice(ui: &mut egui::Ui, f: &mut Follow, on: &str, off: &str) {
    ui.horizontal(|ui| {
        for c in Follow::ALL {
            let label = match c {
                Follow::System => c.label(),
                Follow::On => on,
                Follow::Off => off,
            };
            let hover = match c {
                Follow::System => "As your system's setting says, now and when it changes",
                _ => "Whatever your system's setting says",
            };
            ui.radio_value(f, c, label).on_hover_text(hover);
        }
    });
}

/// One font setting's list: JETCARP's, egui's original, the installed fonts, and a font file.
fn font_choice(
    ui: &mut egui::Ui,
    id: &'static str,
    c: &mut FontChoice,
    mono: bool,
    installed: Option<&[crate::fonts::Installed]>,
    refused: &mut Option<String>,
) {
    let before = c.clone();
    let dialog_key = if mono {
        "fonts.mono_file"
    } else {
        "fonts.file"
    };
    let mut pick_file = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(c.label(mono))
        .height(320.0)
        .width(280.0)
        .show_ui(ui, |ui| {
            for b in [FontChoice::Atkinson, FontChoice::Egui] {
                let label = b.label(mono);
                ui.selectable_value(c, b, label);
            }
            ui.separator();
            match installed {
                None => {
                    ui.label(RichText::new("Looking for installed fonts…").weak());
                }
                Some([]) => {
                    ui.label(RichText::new("No installed fonts found.").weak());
                }
                Some(list) => {
                    let listed = list.iter().any(|f| f.choice() == *c);
                    if let FontChoice::File { family, .. } = &*c
                        && !listed
                    {
                        let label = format!("{family} (a font file)");
                        let mine = c.clone();
                        ui.selectable_value(c, mine, label);
                    }
                    for f in list {
                        ui.selectable_value(c, f.choice(), &f.family);
                    }
                }
            }
            ui.separator();
            pick_file = ui
                .button("Choose a font file…")
                .on_hover_text("A TrueType or OpenType font file (.ttf, .otf or .ttc)")
                .clicked();
        });
    if let Some(path) = crate::file_dialog::on_click(ui.ctx(), dialog_key, pick_file, || {
        rfd::FileDialog::new()
            .add_filter("Font", &["ttf", "otf", "ttc", "otc", "TTF", "OTF", "TTC"])
            .pick_file()
    }) {
        match crate::fonts::load(&path, None) {
            Ok(l) => {
                *c = FontChoice::File {
                    family: l.family,
                    path,
                    index: l.index,
                };
            }
            Err(e) => *refused = Some(e),
        }
    }
    if *c != before {
        *refused = None;
    }
}

fn settings_ui(
    ui: &mut egui::Ui,
    s: &mut AccessibilitySettings,
    fonts: &mut FontsRt,
    reading: crate::a11y::Reading,
    motion: bool,
    high: bool,
) {
    ui.label(
        RichText::new(
            "Saved with the app's other settings. The strobe guard is in Settings → Safety: it acts \
             on what is drawn whatever these say.",
        )
        .small()
        .weak(),
    );
    ui.add_space(6.0);
    ui.heading("Reduced motion");
    ui.label(system_word(
        reading.reduce_motion,
        "reduced motion",
        "doesn't ask for reduced motion",
    ));
    follow_choice(ui, &mut s.reduced_motion, "Reduce motion", "Don't");
    ui.label(
        RichText::new(if motion {
            "On: the Quick Start's cards and the small animations in other windows show a still \
             until you choose one (tick Live, or click a picture); a scene opens paused, with a \
             note; and the app's own animations are off."
        } else {
            "Off: cards and previews run, and scenes start running when opened."
        })
        .small(),
    );
    ui.add_space(8.0);
    ui.separator();
    ui.heading("Theme");
    ui.label(system_word(
        reading.high_contrast,
        "high contrast",
        "doesn't ask for high contrast",
    ));
    follow_choice(ui, &mut s.high_contrast, "High contrast", "Standard");
    ui.horizontal(|ui| {
        ui.label("Light or dark");
        egui::ComboBox::from_id_salt("a11y-brightness")
            .selected_text(s.brightness.label())
            .show_ui(ui, |ui| {
                for b in Brightness::ALL {
                    ui.selectable_value(&mut s.brightness, b, b.label());
                }
            });
    });
    ui.label(
        RichText::new(if high {
            "High contrast: text at 7:1 or more, thick outlines, and a ring around whatever has \
             the keyboard."
        } else {
            "Standard: egui's own light and dark looks."
        })
        .small(),
    );
    ui.add_space(8.0);
    ui.separator();
    ui.heading("Font");
    ui.label(
        RichText::new(
            "JETCARP's words are in Atkinson Hyperlegible Next, a font whose letters are easier to \
             tell apart, designed by the Braille Institute for low-vision readers. You can choose \
             any font installed on this computer instead, or a font file. A character the font \
             lacks is drawn in egui's own fonts.",
        )
        .small(),
    );
    let installed = fonts.scanner.poll();
    let listed = installed.as_deref().map(|v| v.as_slice());
    egui::Grid::new("a11y-fonts")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Font");
            font_choice(
                ui,
                "a11y-font",
                &mut s.font,
                false,
                listed,
                &mut fonts.refused,
            );
            ui.end_row();
            ui.label("Monospace font");
            font_choice(
                ui,
                "a11y-mono-font",
                &mut s.mono_font,
                true,
                listed,
                &mut fonts.refused,
            );
            ui.end_row();
        });
    if let Some(e) = &fonts.refused {
        ui.add(
            egui::Label::new(RichText::new(e).color(theme::ink(ui, egui::Color32::LIGHT_RED)))
                .wrap(),
        );
    }
    ui.add_space(8.0);
    ui.separator();
    ui.heading("Palettes for scenes that name none");
    ui.label(
        RichText::new(
            "A scene's automata are drawn in these when the scene doesn't choose a palette. \
             Scenes you open after a change use them; a scene that names its palettes keeps them.",
        )
        .small(),
    );
    let use_chosen = !s.chosen_for_cvd()
        && ui
            .button(format!(
                "Use the palettes chosen for colour-vision deficiency ({}; JACA: {} and {})",
                cvd::CHOSEN.palette,
                cvd::CHOSEN.jaca_pos,
                cvd::CHOSEN.jaca_neg
            ))
            .on_hover_text(
                "Chosen so that neighbouring colours, and every colour and the background, stay \
                 apart for protanopia, deuteranopia, tritanopia and their milder forms",
            )
            .clicked();
    if use_chosen {
        s.palette = Some(cvd::CHOSEN.palette.into());
        s.jaca_pos = Some(cvd::CHOSEN.jaca_pos.into());
        s.jaca_neg = Some(cvd::CHOSEN.jaca_neg.into());
    }
    egui::Grid::new("a11y-palettes")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Most rules");
            crate::palette_pick::picker_opt(
                ui,
                "a11y-pal",
                &mut s.palette,
                &[],
                "Ember (JETCARP's)",
            );
            ui.end_row();
            ui.label("JACA +1 cells");
            crate::palette_pick::picker_opt(
                ui,
                "a11y-pal-pos",
                &mut s.jaca_pos,
                &[],
                "Tide (JETCARP's)",
            );
            ui.end_row();
            ui.label("JACA −1 cells");
            crate::palette_pick::picker_opt(
                ui,
                "a11y-pal-neg",
                &mut s.jaca_neg,
                &[],
                "Koi (JETCARP's)",
            );
            ui.end_row();
        });
    let passing: Vec<&str> = render::PALETTES
        .iter()
        .filter(|b| {
            let p = render::Palette::builtin(b.name).unwrap();
            let bg = p.background_rgba();
            cvd::check(&p.stops, [bg[0], bg[1], bg[2]]).passes()
        })
        .map(|b| b.name)
        .collect();
    let pairs: Vec<String> = cvd::JACA_PAIRS
        .iter()
        .map(|(p, n)| format!("{p} and {n}"))
        .collect();
    ui.add(
        egui::Label::new(
            RichText::new(format!(
                "{} of the {} built-in palettes pass JETCARP's colour-vision check (the book's \
                 palettes reference marks them). For JACA, these pairs stay apart: {}.",
                passing.len(),
                render::PALETTES.len(),
                pairs.join("; ")
            ))
            .small()
            .weak(),
        )
        .wrap(),
    );
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::a11y::{Fixed, Reading};

    /// The whole app with no window and no GPU, its system settings stood in by `sys`.
    pub(in crate::app) fn headless(sys: &Fixed) -> JetcarpApp {
        let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
        let mut app = JetcarpApp::new(&cc, crate::cli::LaunchArgs::default());
        app.a11y.watch = Watch::new(Box::new(sys.clone()));
        app.a11y.applied = None;
        app.a11y_frame(&cc.egui_ctx);
        app
    }

    fn reduce(on: Option<bool>) -> Reading {
        Reading {
            reduce_motion: on,
            high_contrast: None,
            dark: None,
        }
    }

    fn life() -> Scene {
        Scene::from_json(
            r#"{"tiling": {"family": "square", "size": {"cells": 400}}, "cas": [{"rule": "B3/S23"}]}"#,
        )
        .unwrap()
    }

    #[test]
    fn a_scene_opens_paused_with_a_note_when_the_system_asks_for_reduced_motion() {
        let sys = Fixed::new(reduce(Some(true)));
        let mut app = headless(&sys);
        app.load_scene(&life());
        assert!(!app.playing, "paused");
        let (note, err) = app.message.clone().expect("a note");
        assert!(!err);
        assert!(note.contains("reduced motion"), "{note}");
        // The app's own setting overrides the system's: off runs it.
        app.a11y.settings.reduced_motion = Follow::Off;
        app.load_scene(&life());
        assert!(app.playing, "runs when the app says no reduced motion");
        assert!(app.message.is_none(), "{:?}", app.message);
    }

    #[test]
    fn a_scene_runs_when_nothing_asks_and_pauses_when_the_app_asks() {
        let sys = Fixed::new(reduce(Some(false)));
        let mut app = headless(&sys);
        app.load_scene(&life());
        assert!(app.playing);
        app.a11y.settings.reduced_motion = Follow::On;
        app.load_scene(&life());
        assert!(!app.playing);
        assert!(app.message.as_ref().unwrap().0.contains("reduced motion"));
        // A system that can't be read asks for nothing.
        sys.set(reduce(None));
        app.a11y.settings.reduced_motion = Follow::System;
        app.a11y.watch.poll(1e9);
        app.load_scene(&life());
        assert!(app.playing);
    }

    #[test]
    fn the_app_follows_a_change_in_the_system_while_it_runs() {
        let sys = Fixed::new(reduce(Some(false)));
        let mut app = headless(&sys);
        let ctx = app.ctx.clone().unwrap();
        assert!(!crate::a11y::motion_reduced(&ctx));
        assert!(app.menu.view.live, "the Quick Start's cards run");
        sys.set(Reading {
            reduce_motion: Some(true),
            high_contrast: Some(true),
            dark: None,
        });
        // Read again after a couple of seconds of the app's clock.
        app.started -= std::time::Duration::from_secs(5);
        app.a11y_frame(&ctx);
        assert!(crate::a11y::motion_reduced(&ctx));
        assert!(!app.menu.view.live, "the cards show stills");
        assert_eq!(
            ctx.global_style().animation_time,
            0.0,
            "egui's animations are off"
        );
        assert!(
            crate::a11y::high_contrast(&ctx),
            "the high-contrast theme follows"
        );
        // The app's setting wins over the system's, either way.
        app.a11y.settings.high_contrast = Follow::Off;
        app.a11y.settings.reduced_motion = Follow::Off;
        app.a11y_frame(&ctx);
        assert!(!crate::a11y::high_contrast(&ctx));
        assert!(!crate::a11y::motion_reduced(&ctx));
        assert!(app.menu.view.live);
        assert!(ctx.global_style().animation_time > 0.0);
    }

    #[test]
    fn the_cards_choice_is_the_person_s_until_the_setting_changes() {
        let sys = Fixed::new(reduce(Some(true)));
        let mut app = headless(&sys);
        let ctx = app.ctx.clone().unwrap();
        assert!(!app.menu.view.live);
        // Ticking Live is a choice; frames don't undo it.
        app.menu.view.live = true;
        app.started -= std::time::Duration::from_secs(5);
        app.a11y_frame(&ctx);
        assert!(app.menu.view.live);
    }

    #[test]
    fn scenes_that_name_no_palette_take_the_defaults() {
        let sys = Fixed::new(reduce(Some(false)));
        let mut app = headless(&sys);
        app.a11y.settings.reduced_motion = Follow::Off;
        // JETCARP's own defaults until a choice is made.
        app.load_scene(&life());
        assert_eq!(app.world.cas[0].palette, "Ember");
        app.a11y.settings.palette = Some(cvd::CHOSEN.palette.into());
        app.a11y.settings.jaca_pos = Some(cvd::CHOSEN.jaca_pos.into());
        app.a11y.settings.jaca_neg = Some(cvd::CHOSEN.jaca_neg.into());
        assert!(app.a11y.settings.chosen_for_cvd());
        app.load_scene(&life());
        assert_eq!(app.world.cas[0].palette, cvd::CHOSEN.palette);
        // JACA takes its pair.
        let jaca = Scene::from_json(
            r#"{"tiling": {"family": "square", "size": {"cells": 400}}, "cas": [{"rule": "JACA B3/S23"}]}"#,
        )
        .unwrap();
        app.load_scene(&jaca);
        let ca = &app.world.cas[0];
        assert!(matches!(ca.rule, Rule::Jaca(_)), "{:?}", ca.rule);
        assert_eq!(ca.palette, cvd::CHOSEN.jaca_pos);
        assert_eq!(ca.palette_neg, cvd::CHOSEN.jaca_neg);
        // A scene that names its palette keeps it.
        let named = Scene::from_json(
            r#"{"tiling": {"family": "square", "size": {"cells": 400}}, "cas": [{"rule": "B3/S23", "palette": "Koi"}]}"#,
        )
        .unwrap();
        app.load_scene(&named);
        assert_eq!(app.world.cas[0].palette, "Koi");
        // Saving the world names the palette it shows.
        let saved = app.current_scene();
        assert_eq!(saved.cas[0].palette.as_deref(), Some("Koi"));
    }

    #[test]
    fn accessibility_settings_round_trip_and_unknown_palettes_are_dropped() {
        use super::super::settings::AppSettings;
        let mut s = AppSettings::default();
        assert_eq!(s.accessibility, AccessibilitySettings::default());
        s.accessibility = AccessibilitySettings {
            reduced_motion: Follow::On,
            high_contrast: Follow::Off,
            brightness: Brightness::Light,
            palette: Some("Lantern".into()),
            jaca_pos: Some("Cobalt".into()),
            jaca_neg: Some("Saffron".into()),
            font: FontChoice::Egui,
            mono_font: FontChoice::File {
                family: "Hack".into(),
                path: "/usr/share/fonts/hack/Hack-Regular.ttf".into(),
                index: 0,
            },
        };
        let json = s.to_json();
        assert!(json.contains(r#""reduced_motion": "on""#), "{json}");
        let back = AppSettings::from_json(&json);
        assert_eq!(back.accessibility, s.accessibility);
        // A file from before the section, or with a broken one, keeps everything else.
        let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
        v["menu_autohide"] = serde_json::json!(true);
        v["accessibility"] = serde_json::json!({"reduced_motion": "sometimes"});
        let bad = AppSettings::from_json(&v.to_string());
        assert_eq!(bad.accessibility, AccessibilitySettings::default());
        assert!(bad.menu_autohide);
        let odd = AccessibilitySettings {
            palette: Some("Gone".into()),
            jaca_neg: Some("Saffron".into()),
            ..Default::default()
        }
        .sanitize();
        assert_eq!(odd.palette, None);
        assert_eq!(odd.jaca_neg.as_deref(), Some("Saffron"));
    }

    /// What egui draws text and monospaced text with in the app's context.
    fn fonts_in(app: &JetcarpApp) -> (Vec<String>, Vec<String>, egui::FontDefinitions) {
        let ctx = app.ctx.clone().unwrap();
        // set_fonts takes effect at the next frame.
        ctx.run_pass(Default::default(), |_| {})
            .textures_delta
            .clear();
        let d = ctx.fonts(|f| f.definitions().clone());
        (
            d.families[&egui::FontFamily::Proportional].clone(),
            d.families[&egui::FontFamily::Monospace].clone(),
            d,
        )
    }

    #[test]
    fn the_app_starts_in_atkinson_hyperlegible() {
        let app = headless(&Fixed::new(reduce(Some(false))));
        let (text, mono, d) = fonts_in(&app);
        assert_eq!(text[0], crate::fonts::NEXT, "{text:?}");
        assert_eq!(mono[0], crate::fonts::MONO, "{mono:?}");
        assert_eq!(
            d.font_data[crate::fonts::NEXT].bytes(),
            jetcarp_display::fonts::NEXT_REGULAR
        );
        // egui's fonts behind, for what Atkinson lacks.
        assert!(text.ends_with(&crate::fonts::BEHIND_TEXT.map(String::from)));
        assert!(mono.ends_with(&crate::fonts::BEHIND_MONO.map(String::from)));
        assert_eq!(AccessibilitySettings::default().font, FontChoice::Atkinson);
        assert_eq!(
            AccessibilitySettings::default().mono_font,
            FontChoice::Atkinson
        );
    }

    #[test]
    fn a_chosen_installed_font_is_used_saved_and_restored() {
        use super::super::settings::AppSettings;
        let dir = crate::fonts::tests::scratch("chosen");
        let share = dir.join("fonts");
        std::fs::create_dir_all(&share).unwrap();
        let hack = crate::fonts::tests::builtin("Hack");
        std::fs::write(share.join("Hack-Regular.ttf"), &hack).unwrap();
        let mut app = headless(&Fixed::new(reduce(Some(false))));
        app.a11y.fonts.scanner = crate::fonts::Scanner::new(vec![share.clone()]);
        // Opening the window looks for the installed fonts.
        app.a11y.window_open = true;
        let ctx = app.ctx.clone().unwrap();
        ctx.run_pass(Default::default(), |ctx| app.accessibility_window(ctx))
            .textures_delta
            .clear();
        let found = app.a11y.fonts.scanner.wait();
        let hack_font = found
            .iter()
            .find(|f| f.family == "Hack")
            .unwrap_or_else(|| panic!("{found:?}"));
        // Chosen for both: in place at once, egui's fonts behind it.
        app.a11y.settings.font = hack_font.choice();
        app.a11y.settings.mono_font = hack_font.choice();
        app.a11y_frame(&ctx);
        let (text, mono, d) = fonts_in(&app);
        assert_eq!(text[0], crate::fonts::CHOSEN);
        assert_eq!(mono[0], crate::fonts::CHOSEN_MONO);
        assert_eq!(d.font_data[crate::fonts::CHOSEN].bytes(), &hack[..]);
        assert!(text.ends_with(&crate::fonts::BEHIND_TEXT.map(String::from)));
        // Saved in app.json, and put back in place by the next start.
        let json = app.settings_snapshot(&ctx).to_json();
        assert!(json.contains(r#""family": "Hack""#), "{json}");
        let back = AppSettings::from_json(&json);
        assert_eq!(back.accessibility.font, hack_font.choice());
        // Where the desktop background's caption reads it.
        std::fs::write(dir.join("app.json"), &json).unwrap();
        assert_eq!(
            jetcarp_display::fonts::saved_in(&dir),
            (hack_font.choice(), hack_font.choice())
        );
        let mut next = headless(&Fixed::new(reduce(Some(false))));
        let next_ctx = next.ctx.clone().unwrap();
        next.apply_settings(&next_ctx, back);
        next.a11y_frame(&next_ctx);
        let (text, mono, d) = fonts_in(&next);
        assert_eq!(text[0], crate::fonts::CHOSEN);
        assert_eq!(mono[0], crate::fonts::CHOSEN_MONO);
        assert_eq!(d.font_data[crate::fonts::CHOSEN_MONO].bytes(), &hack[..]);
        // And back to JETCARP's.
        next.a11y.settings.font = FontChoice::Atkinson;
        next.a11y.settings.mono_font = FontChoice::Egui;
        next.a11y_frame(&next_ctx);
        let (text, mono, _) = fonts_in(&next);
        assert_eq!(text[0], crate::fonts::NEXT);
        assert_eq!(mono, crate::fonts::BEHIND_MONO.map(String::from));
    }

    #[test]
    fn a_font_file_that_can_t_be_used_is_refused_and_the_setting_goes_back() {
        let dir = crate::fonts::tests::scratch("refused");
        std::fs::create_dir_all(&*dir).unwrap();
        let junk = dir.join("Junk.ttf");
        std::fs::write(&junk, b"not a font").unwrap();
        let mut app = headless(&Fixed::new(reduce(Some(false))));
        let ctx = app.ctx.clone().unwrap();
        // As a hand-edited app.json, or a font removed since it was chosen, would ask.
        app.a11y.settings.font = FontChoice::File {
            family: "Junk".into(),
            path: junk,
            index: 0,
        };
        app.a11y_frame(&ctx);
        assert_eq!(app.a11y.settings.font, FontChoice::Atkinson);
        let e = app.a11y.fonts.refused.clone().expect("a reason");
        assert!(e.starts_with("Junk.ttf isn't a font"), "{e}");
        assert!(!e.contains(&*dir.to_string_lossy()), "no local path: {e}");
        let (note, err) = app.message.clone().expect("said on the status line");
        assert!(err && note == e, "{note}");
        let (text, _, _) = fonts_in(&app);
        assert_eq!(text[0], crate::fonts::NEXT);
    }

    /// On Linux, where winit says nothing of the style, *Follow the system* follows what the app
    /// read from the portal (#137); a choice, or winit's own answer, wins.
    #[test]
    fn follow_the_system_takes_the_style_read_on_linux() {
        use crate::theme::Brightness::*;
        use eframe::egui::Theme;
        assert_eq!(brightness_shown(System, Some(false), None), Light);
        assert_eq!(brightness_shown(System, Some(true), None), Dark);
        assert_eq!(brightness_shown(System, None, None), System);
        assert_eq!(
            brightness_shown(System, Some(false), Some(Theme::Dark)),
            System
        );
        assert_eq!(brightness_shown(Dark, Some(false), None), Dark);
        assert_eq!(brightness_shown(Light, Some(true), None), Light);
    }
}
