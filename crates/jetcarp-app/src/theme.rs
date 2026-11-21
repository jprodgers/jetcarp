//! The app's look: egui's own dark and light themes, or JETCARP's high-contrast theme in a dark
//! and a light form (#220).
//!
//! The high-contrast theme keeps text at 7:1 or more against what it is drawn on (WCAG 2.2's
//! AAA level, 1.4.6), and controls' outlines, the focus ring and the boxes and rails of check
//! boxes and sliders at 3:1 or more (1.4.11), drawn thicker than egui's. Black on white or white
//! on black, a gold (dark) or navy (light) ring around whatever has the keyboard, and outlines
//! on every button and field. The tests below compute every one of those contrasts.
//!
//! The app's own coloured words (an error in red, a note in amber, "valid" in green) go through
//! [`ink`]: unchanged in the standard theme; in the high-contrast one, the theme's colour of the
//! same kind, which keeps its meaning and is as legible as the text around it. Their meaning is
//! in the words too, never in the colour alone.

use eframe::egui::{self, Color32, Stroke};
use serde::{Deserialize, Serialize};

/// Light or dark: as the system says (the default), or one of them whatever it says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Brightness {
    #[default]
    System,
    Dark,
    Light,
}

impl Brightness {
    pub const ALL: [Brightness; 3] = [Brightness::System, Brightness::Dark, Brightness::Light];

    pub fn label(self) -> &'static str {
        match self {
            Brightness::System => "Follow the system",
            Brightness::Dark => "Dark",
            Brightness::Light => "Light",
        }
    }

    pub fn preference(self) -> egui::ThemePreference {
        match self {
            Brightness::System => egui::ThemePreference::System,
            Brightness::Dark => egui::ThemePreference::Dark,
            Brightness::Light => egui::ThemePreference::Light,
        }
    }
}

/// Put the theme in place: egui's standard visuals, or the high-contrast ones, in both the dark
/// and the light form (egui shows the one the system, or [`Brightness`], asks for).
pub fn apply(ctx: &egui::Context, high: bool, brightness: Brightness) {
    ctx.set_visuals_of(egui::Theme::Dark, visuals(true, high));
    ctx.set_visuals_of(egui::Theme::Light, visuals(false, high));
    ctx.set_theme(brightness.preference());
    crate::a11y::set_high_contrast_flag(ctx, high);
}

/// The visuals of one form of a theme.
pub fn visuals(dark: bool, high: bool) -> egui::Visuals {
    match (dark, high) {
        (true, false) => egui::Visuals::dark(),
        (false, false) => egui::Visuals::light(),
        (true, true) => high_contrast(&DARK),
        (false, true) => high_contrast(&LIGHT),
    }
}

/// The colours of one form of the high-contrast theme.
pub struct Scheme {
    pub dark: bool,
    /// Panels and windows.
    pub background: Color32,
    /// Striped rows.
    pub faint: Color32,
    /// Behind monospaced code.
    pub code: Color32,
    pub text: Color32,
    /// Weak (secondary) text: still at 7:1.
    pub weak: Color32,
    /// Separators and frames.
    pub rule: Color32,
    /// Buttons' and fields' outlines.
    pub outline: Color32,
    /// Outlines under the pointer.
    pub hover_outline: Color32,
    /// Buttons under the pointer.
    pub hover_fill: Color32,
    /// Check boxes' and radio buttons' boxes, sliders' rails and handles, scroll bars.
    pub control: Color32,
    pub control_hover: Color32,
    pub control_active: Color32,
    /// The ring around whatever has the keyboard, and around a field being typed in.
    pub focus: Color32,
    /// Behind selected text and selected items, whose text is then [`Scheme::selected_text`].
    pub selection: Color32,
    pub selected_text: Color32,
    pub link: Color32,
    /// The app's coloured words, by kind ([`ink`]).
    pub red: Color32,
    pub amber: Color32,
    pub green: Color32,
    pub blue: Color32,
    pub purple: Color32,
}

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

pub const DARK: Scheme = Scheme {
    dark: true,
    background: hex(0x000000),
    faint: hex(0x1a1a1a),
    code: hex(0x1a1a1a),
    text: hex(0xffffff),
    weak: hex(0xd9d9d9),
    rule: hex(0xb3b3b3),
    outline: hex(0xe0e0e0),
    hover_outline: hex(0xffffff),
    hover_fill: hex(0x1f1f1f),
    control: hex(0x707070),
    control_hover: hex(0x7f7f7f),
    control_active: hex(0x8c8c8c),
    focus: hex(0xffd700),
    selection: hex(0x102a80),
    selected_text: hex(0xffd700),
    link: hex(0x66ccff),
    red: hex(0xff9999),
    amber: hex(0xffcc00),
    green: hex(0x7cf07c),
    blue: hex(0x66ccff),
    purple: hex(0xff99ff),
};

pub const LIGHT: Scheme = Scheme {
    dark: false,
    background: hex(0xffffff),
    faint: hex(0xebebeb),
    code: hex(0xebebeb),
    text: hex(0x000000),
    weak: hex(0x333333),
    rule: hex(0x595959),
    outline: hex(0x1a1a1a),
    hover_outline: hex(0x000000),
    hover_fill: hex(0xe6e6e6),
    control: hex(0x858585),
    control_hover: hex(0x7a7a7a),
    control_active: hex(0x6b6b6b),
    focus: hex(0x00268c),
    selection: hex(0xc6dbff),
    selected_text: hex(0x00268c),
    link: hex(0x003399),
    red: hex(0x8c0000),
    amber: hex(0x5c3a00),
    green: hex(0x004d00),
    blue: hex(0x003399),
    purple: hex(0x6b0080),
};

/// Stroke widths of the high-contrast theme, thicker than egui's (1.0, or none for buttons at
/// rest).
pub const RULE_WIDTH: f32 = 1.5;
pub const OUTLINE_WIDTH: f32 = 1.5;
pub const HOVER_WIDTH: f32 = 2.0;
pub const FOCUS_WIDTH: f32 = 3.0;
pub const FIELD_FOCUS_WIDTH: f32 = 2.5;
pub const WINDOW_WIDTH: f32 = 2.0;

fn high_contrast(s: &Scheme) -> egui::Visuals {
    let mut v = if s.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.override_text_color = None;
    v.weak_text_color = Some(s.weak);
    v.hyperlink_color = s.link;
    v.faint_bg_color = s.faint;
    v.extreme_bg_color = s.background;
    v.text_edit_bg_color = Some(s.background);
    v.code_bg_color = s.code;
    v.warn_fg_color = s.amber;
    v.error_fg_color = s.red;
    v.window_fill = s.background;
    v.panel_fill = s.background;
    v.window_stroke = Stroke::new(WINDOW_WIDTH, s.outline);
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;
    v.selection.bg_fill = s.selection;
    v.selection.stroke = Stroke::new(FIELD_FOCUS_WIDTH, s.selected_text);
    v.text_cursor.stroke = Stroke::new(2.0, s.text);
    // Disabled controls stay readable, if fainter (WCAG asks no contrast of them).
    v.disabled_alpha = 0.6;
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = s.background;
    w.noninteractive.weak_bg_fill = s.background;
    w.noninteractive.bg_stroke = Stroke::new(RULE_WIDTH, s.rule);
    w.noninteractive.fg_stroke = Stroke::new(1.5, s.text);
    w.inactive.bg_fill = s.control;
    w.inactive.weak_bg_fill = s.background;
    w.inactive.bg_stroke = Stroke::new(OUTLINE_WIDTH, s.outline);
    w.inactive.fg_stroke = Stroke::new(1.5, s.text);
    w.hovered.bg_fill = s.control_hover;
    w.hovered.weak_bg_fill = s.hover_fill;
    w.hovered.bg_stroke = Stroke::new(HOVER_WIDTH, s.hover_outline);
    w.hovered.fg_stroke = Stroke::new(2.0, s.text);
    // Pressed, and with the keyboard (egui draws a focused widget as active): the ring.
    w.active.bg_fill = s.control_active;
    w.active.weak_bg_fill = s.background;
    w.active.bg_stroke = Stroke::new(FOCUS_WIDTH, s.focus);
    w.active.fg_stroke = Stroke::new(2.0, s.text);
    w.active.expansion = 1.0;
    w.open.bg_fill = s.control_hover;
    w.open.weak_bg_fill = s.hover_fill;
    w.open.bg_stroke = Stroke::new(HOVER_WIDTH, s.hover_outline);
    w.open.fg_stroke = Stroke::new(1.5, s.text);
    v
}

/// The colour to draw one of the app's coloured words in: `c` itself in the standard theme; in
/// the high-contrast theme, the theme's colour of the same kind (red, amber, green, blue, purple,
/// or plain or weak text for greys).
pub fn ink(ui: &egui::Ui, c: Color32) -> Color32 {
    if !crate::a11y::high_contrast(ui.ctx()) {
        return c;
    }
    ink_in(
        if ui.visuals().dark_mode {
            &DARK
        } else {
            &LIGHT
        },
        c,
    )
}

/// [`ink`] in one scheme.
pub fn ink_in(s: &Scheme, c: Color32) -> Color32 {
    match kind(c) {
        Kind::Plain => s.text,
        Kind::Weak => s.weak,
        Kind::Red => s.red,
        Kind::Amber => s.amber,
        Kind::Green => s.green,
        Kind::Blue => s.blue,
        Kind::Purple => s.purple,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Plain,
    Weak,
    Red,
    Amber,
    Green,
    Blue,
    Purple,
}

/// What kind of colour `c` is: a grey (bright, or dim), or a hue.
fn kind(c: Color32) -> Kind {
    let [r, g, b, _] = c.to_srgba_unmultiplied().map(|x| x as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let sat = if max > 0.0 { (max - min) / max } else { 0.0 };
    if sat < 0.25 {
        // A grey: the standard dark theme's text is about 0.55 to 0.85; dimmer greys are weak.
        return if max >= 0.7 || max <= 0.3 {
            Kind::Plain
        } else {
            Kind::Weak
        };
    }
    let d = max - min;
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    match h {
        h if !(20.0..330.0).contains(&h) => Kind::Red,
        h if h < 70.0 => Kind::Amber,
        h if h < 170.0 => Kind::Green,
        h if h < 260.0 => Kind::Blue,
        _ => Kind::Purple,
    }
}

/// WCAG 2.2's relative luminance of an sRGB colour.
#[cfg(test)]
pub fn luminance(c: Color32) -> f64 {
    let f = |x: u8| {
        let x = x as f64 / 255.0;
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
}

/// WCAG 2.2's contrast ratio of two opaque colours (1 to 21).
#[cfg(test)]
pub fn contrast(a: Color32, b: Color32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2.2: text at 7:1 (1.4.6, AAA); outlines and the parts of controls that show their
    /// state at 3:1 (1.4.11).
    const TEXT: f64 = 7.0;
    const OUTLINE: f64 = 3.0;

    /// Every background text is drawn on, named.
    fn text_backgrounds(v: &egui::Visuals) -> Vec<(&'static str, Color32)> {
        let w = &v.widgets;
        vec![
            ("panel", v.panel_fill),
            ("window", v.window_fill),
            ("striped row", v.faint_bg_color),
            ("text field", v.text_edit_bg_color()),
            ("extreme", v.extreme_bg_color),
            ("code", v.code_bg_color),
            ("button", w.inactive.weak_bg_fill),
            ("button under the pointer", w.hovered.weak_bg_fill),
            ("button pressed or focused", w.active.weak_bg_fill),
            ("open menu button", w.open.weak_bg_fill),
            ("label", w.noninteractive.weak_bg_fill),
        ]
    }

    /// Every text colour the theme draws on those backgrounds, named.
    fn text_colours(v: &egui::Visuals, s: &Scheme) -> Vec<(String, Color32)> {
        let w = &v.widgets;
        let mut t = vec![
            ("text".to_string(), v.text_color()),
            ("weak text".into(), v.weak_text_color()),
            ("strong text".into(), v.strong_text_color()),
            ("label text".into(), w.noninteractive.fg_stroke.color),
            ("button text".into(), w.inactive.fg_stroke.color),
            (
                "button text under the pointer".into(),
                w.hovered.fg_stroke.color,
            ),
            (
                "button text pressed or focused".into(),
                w.active.fg_stroke.color,
            ),
            ("open menu button text".into(), w.open.fg_stroke.color),
            ("link".into(), v.hyperlink_color),
            ("warning".into(), v.warn_fg_color),
            ("error".into(), v.error_fg_color),
        ];
        // The app's coloured words, as `ink` maps them, from colours the app uses.
        for c in [
            Color32::LIGHT_RED,
            Color32::LIGHT_GREEN,
            Color32::YELLOW,
            Color32::GRAY,
            Color32::from_gray(150),
            Color32::from_gray(110),
            Color32::from_rgb(255, 110, 110),
            Color32::from_rgb(255, 170, 90),
            Color32::from_rgb(0xff, 0xc8, 0x3c),
            Color32::from_rgb(0x58, 0xc8, 0xff),
            Color32::from_rgb(0xb4, 0xa0, 0xff),
            Color32::from_rgb(110, 200, 120),
            Color32::WHITE,
            Color32::BLACK,
        ] {
            t.push((format!("the app's {c:?}"), ink_in(s, c)));
        }
        t
    }

    fn check(dark: bool, s: &Scheme) -> Vec<String> {
        let v = visuals(dark, true);
        let mut fails = vec![];
        let mut need = |what: String, a: Color32, b: Color32, min: f64| {
            let r = contrast(a, b);
            if r < min {
                fails.push(format!("{what}: {r:.2}:1, under {min}:1"));
            }
        };
        for (tn, tc) in text_colours(&v, s) {
            for (bn, bc) in text_backgrounds(&v) {
                need(format!("{tn} on {bn}"), tc, bc, TEXT);
            }
        }
        // Selected text and selected items.
        need(
            "selected text on the selection".into(),
            v.selection.stroke.color,
            v.selection.bg_fill,
            TEXT,
        );
        let w = &v.widgets;
        let surfaces = [
            ("panel", v.panel_fill),
            ("window", v.window_fill),
            ("striped row", v.faint_bg_color),
        ];
        for (sn, sc) in surfaces {
            for (on, oc) in [
                ("separator", w.noninteractive.bg_stroke.color),
                ("button outline", w.inactive.bg_stroke.color),
                ("outline under the pointer", w.hovered.bg_stroke.color),
                ("focus ring", w.active.bg_stroke.color),
                ("open menu button outline", w.open.bg_stroke.color),
                ("window outline", v.window_stroke.color),
                ("check box or slider rail", w.inactive.bg_fill),
                ("check box or slider under the pointer", w.hovered.bg_fill),
                ("check box or slider pressed or focused", w.active.bg_fill),
            ] {
                need(format!("{on} on {sn}"), oc, sc, OUTLINE);
            }
        }
        // A field being typed in is ringed in the selection's stroke.
        need(
            "a focused text field's ring".into(),
            v.selection.stroke.color,
            v.text_edit_bg_color(),
            OUTLINE,
        );
        // The ring stands out from the button inside it too.
        need(
            "focus ring against its button".into(),
            w.active.bg_stroke.color,
            w.active.weak_bg_fill,
            OUTLINE,
        );
        // Check marks and slider handles' marks on their boxes.
        for (n, st) in [
            ("inactive", &w.inactive),
            ("hovered", &w.hovered),
            ("active", &w.active),
        ] {
            need(
                format!("{n} check mark on its box"),
                st.fg_stroke.color,
                st.bg_fill,
                OUTLINE,
            );
        }
        fails
    }

    #[test]
    fn the_high_contrast_theme_meets_its_contrasts_in_both_forms() {
        for (dark, s) in [(true, &DARK), (false, &LIGHT)] {
            let fails = check(dark, s);
            assert!(
                fails.is_empty(),
                "{}: {}",
                if dark { "dark" } else { "light" },
                fails.join("\n")
            );
        }
    }

    /// The same check fails egui's standard dark theme: its weak text and outlines are the
    /// faint ones the high-contrast theme replaces.
    #[test]
    fn the_standard_theme_does_not_and_the_check_says_why() {
        let v = visuals(true, false);
        let weak = contrast(v.weak_text_color(), v.panel_fill);
        assert!(weak < TEXT, "{weak}");
        let outline = contrast(v.widgets.noninteractive.bg_stroke.color, v.panel_fill);
        assert!(outline < OUTLINE, "{outline}");
    }

    #[test]
    fn high_contrast_strokes_are_thicker_than_egui_s() {
        for dark in [true, false] {
            let std = visuals(dark, false);
            let hc = visuals(dark, true);
            let (s, h) = (&std.widgets, &hc.widgets);
            for (n, a, b) in [
                (
                    "separator",
                    s.noninteractive.bg_stroke,
                    h.noninteractive.bg_stroke,
                ),
                ("button outline", s.inactive.bg_stroke, h.inactive.bg_stroke),
                ("hovered outline", s.hovered.bg_stroke, h.hovered.bg_stroke),
                ("focus ring", s.active.bg_stroke, h.active.bg_stroke),
                ("window", std.window_stroke, hc.window_stroke),
                ("field focus", std.selection.stroke, hc.selection.stroke),
            ] {
                assert!(b.width >= a.width + 0.5, "{n}: {} vs {}", b.width, a.width);
            }
            assert!(h.active.bg_stroke.width >= 2.5, "the focus ring is thick");
        }
    }

    #[test]
    fn ink_keeps_the_kind_of_colour_and_leaves_the_standard_theme_alone() {
        assert_eq!(kind(Color32::LIGHT_RED), Kind::Red);
        assert_eq!(kind(Color32::from_rgb(255, 110, 110)), Kind::Red);
        assert_eq!(kind(Color32::YELLOW), Kind::Amber);
        assert_eq!(kind(Color32::from_rgb(255, 170, 90)), Kind::Amber);
        assert_eq!(kind(Color32::LIGHT_GREEN), Kind::Green);
        assert_eq!(kind(Color32::from_rgb(0x58, 0xc8, 0xff)), Kind::Blue);
        assert_eq!(kind(Color32::from_rgb(0xc0, 0x80, 0xff)), Kind::Purple);
        assert_eq!(kind(Color32::WHITE), Kind::Plain);
        assert_eq!(kind(Color32::from_gray(150)), Kind::Weak);
        let ctx = egui::Context::default();
        let frame = |ctx: &egui::Context| {
            let mut got = (Color32::TRANSPARENT, Color32::TRANSPARENT);
            let _ = ctx.run_ui(Default::default(), |ui| {
                got = (
                    ink(ui, Color32::LIGHT_RED),
                    ink(ui, Color32::from_gray(150)),
                );
            });
            got
        };
        apply(&ctx, false, Brightness::Dark);
        assert_eq!(frame(&ctx), (Color32::LIGHT_RED, Color32::from_gray(150)));
        apply(&ctx, true, Brightness::Dark);
        assert_eq!(frame(&ctx), (DARK.red, DARK.weak));
        apply(&ctx, true, Brightness::Light);
        assert_eq!(frame(&ctx), (LIGHT.red, LIGHT.weak));
    }

    #[test]
    fn applying_puts_both_forms_in_place_and_the_brightness_chosen() {
        let ctx = egui::Context::default();
        apply(&ctx, true, Brightness::Light);
        assert_eq!(
            ctx.style_of(egui::Theme::Dark).visuals.panel_fill,
            DARK.background
        );
        assert_eq!(
            ctx.style_of(egui::Theme::Light).visuals.panel_fill,
            LIGHT.background
        );
        assert_eq!(
            ctx.options(|o| o.theme_preference),
            egui::ThemePreference::Light
        );
        assert!(crate::a11y::high_contrast(&ctx));
        apply(&ctx, false, Brightness::System);
        assert_eq!(
            ctx.style_of(egui::Theme::Dark).visuals,
            egui::Visuals::dark(),
            "the standard theme is egui's own"
        );
        assert!(!crate::a11y::high_contrast(&ctx));
        assert_eq!(
            ctx.options(|o| o.theme_preference),
            egui::ThemePreference::System
        );
    }
}
