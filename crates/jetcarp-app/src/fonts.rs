//! The app's fonts (Settings → Accessibility → Font). By default its words are in Atkinson
//! Hyperlegible Next and its monospaced text in Atkinson Hyperlegible Mono: fonts whose letters
//! are easier to tell apart, designed by the Braille Institute for low-vision readers
//! ([`jetcarp_display::fonts`]). egui's original fonts can be chosen instead, or any font
//! installed on the system ([`scan`]), or a font file. The setting and the scan of installed
//! fonts are the display crate's, which the desktop background's caption shares.
//!
//! Whatever is chosen comes first, and egui's built-in fonts stay behind it for any character it
//! lacks: Ubuntu Light (Greek, Cyrillic and more Latin), egui's icons, then Hack (arrows, boxes and
//! dots). With Atkinson, headings are in Atkinson Hyperlegible Next Bold.

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The names the fonts go by in egui's definitions.
pub const NEXT: &str = jetcarp_display::fonts::NEXT_NAME;
pub const NEXT_BOLD: &str = "Atkinson Hyperlegible Next Bold";
pub const MONO: &str = jetcarp_display::fonts::MONO_NAME;
/// A font chosen from the system or a file, for text and for monospaced text.
pub const CHOSEN: &str = "chosen font";
pub const CHOSEN_MONO: &str = "chosen monospace font";

/// egui's built-in fonts, behind every choice, in egui's own order (Hack after the proportional
/// font, as the app has always had it, for arrows and dots).
pub const BEHIND_TEXT: [&str; 3] = ["Ubuntu-Light", "egui-icons", "Hack"];
pub const BEHIND_MONO: [&str; 3] = ["Hack", "Ubuntu-Light", "egui-icons"];

/// The family headings are drawn in ([`egui::TextStyle::Heading`]).
const HEADING: &str = "heading";

pub fn heading_family() -> FontFamily {
    FontFamily::Name(HEADING.into())
}

pub use jetcarp_display::fonts::{FontChoice, Installed, scan, system_dirs};

/// A choice ready to draw with.
#[derive(Clone)]
pub enum Face {
    Atkinson,
    Egui,
    Loaded(Arc<FontData>),
}

/// egui's font definitions for text in `text` and monospaced text in `mono`, egui's built-in
/// fonts behind each.
pub fn definitions(text: &Face, mono: &Face) -> FontDefinitions {
    use jetcarp_display::fonts as f;
    let mut d = FontDefinitions::default();
    let mut add = |name: &str, data: Arc<FontData>| {
        d.font_data.insert(name.to_owned(), data);
    };
    let (first, heading): (&str, &str) = match text {
        Face::Atkinson => {
            add(NEXT, Arc::new(FontData::from_static(f::NEXT_REGULAR)));
            add(NEXT_BOLD, Arc::new(FontData::from_static(f::NEXT_BOLD)));
            (NEXT, NEXT_BOLD)
        }
        Face::Egui => ("", ""),
        Face::Loaded(data) => {
            add(CHOSEN, data.clone());
            (CHOSEN, CHOSEN)
        }
    };
    let first_mono = match mono {
        Face::Atkinson => {
            add(MONO, Arc::new(FontData::from_static(f::MONO_REGULAR)));
            MONO
        }
        Face::Egui => "",
        Face::Loaded(data) => {
            add(CHOSEN_MONO, data.clone());
            CHOSEN_MONO
        }
    };
    let chain = |first: &str, behind: &[&str]| -> Vec<String> {
        std::iter::once(first)
            .filter(|n| !n.is_empty())
            .chain(behind.iter().copied())
            .map(str::to_owned)
            .collect()
    };
    d.families
        .insert(FontFamily::Proportional, chain(first, &BEHIND_TEXT));
    d.families
        .insert(heading_family(), chain(heading, &BEHIND_TEXT));
    d.families
        .insert(FontFamily::Monospace, chain(first_mono, &BEHIND_MONO));
    d
}

/// Put these fonts in place in `ctx` (from its next frame), headings in their own family.
pub fn install(ctx: &egui::Context, text: &Face, mono: &Face) {
    ctx.set_fonts(definitions(text, mono));
    ctx.all_styles_mut(|s| {
        if let Some(h) = s.text_styles.get_mut(&egui::TextStyle::Heading) {
            h.family = heading_family();
        }
    });
}

/// JETCARP's fonts in `ctx` (the offscreen pictures' contexts, and the app's at start).
pub fn install_default(ctx: &egui::Context) {
    install(ctx, &Face::Atkinson, &Face::Atkinson);
}

/// A font file read and checked, ready to draw with.
pub struct Loaded {
    pub family: String,
    pub index: u32,
    pub data: Arc<FontData>,
}

/// Read the font file at `path` and check that egui can draw JETCARP's words in it
/// ([`jetcarp_display::fonts::load`]). The error is a plain sentence.
pub fn load(path: &Path, index: Option<u32>) -> Result<Loaded, String> {
    let f = jetcarp_display::fonts::load(path, index)?;
    Ok(Loaded {
        family: f.family,
        index: f.index,
        data: Arc::new(FontData::from_blob(Arc::new(f.bytes), f.index)),
    })
}

/// The installed fonts, found once on a thread of their own when the setting is first shown.
#[derive(Default)]
pub struct Scanner {
    /// The folders looked in: the system's ([`system_dirs`]), none in tests unless they say.
    pub dirs: Vec<PathBuf>,
    pub found: Option<Arc<Vec<Installed>>>,
    rx: Option<std::sync::mpsc::Receiver<Vec<Installed>>>,
}

impl Scanner {
    pub fn new(dirs: Vec<PathBuf>) -> Scanner {
        Scanner {
            dirs,
            found: None,
            rx: None,
        }
    }

    /// Start looking, unless it has (`ctx` is asked to draw again when done).
    pub fn start(&mut self, ctx: &egui::Context) {
        if self.found.is_some() || self.rx.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let dirs = self.dirs.clone();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("font scan".into())
            .spawn(move || {
                let _ = tx.send(scan(&dirs));
                ctx.request_repaint();
            });
        if spawned.is_ok() {
            self.rx = Some(rx);
        } else {
            self.found = Some(Arc::new(scan(&self.dirs)));
        }
    }

    /// The fonts, once found.
    pub fn poll(&mut self) -> Option<Arc<Vec<Installed>>> {
        if let Some(rx) = &self.rx
            && let Ok(v) = rx.try_recv()
        {
            self.found = Some(Arc::new(v));
            self.rx = None;
        }
        self.found.clone()
    }

    /// Wait for the scan (tests).
    #[cfg(test)]
    pub fn wait(&mut self) -> Arc<Vec<Installed>> {
        if let Some(rx) = self.rx.take() {
            self.found = Some(Arc::new(rx.recv().unwrap_or_default()));
        }
        self.found.clone().expect("a scan was started")
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// egui's own bytes of one of its built-in fonts.
    pub(crate) fn builtin(name: &str) -> Vec<u8> {
        FontDefinitions::default().font_data[name].bytes().to_vec()
    }

    fn ctx_with(text: &Face, mono: &Face) -> egui::Context {
        let ctx = egui::Context::default();
        install(&ctx, text, mono);
        ctx.run_pass(Default::default(), |_| {})
            .textures_delta
            .clear();
        ctx
    }

    #[test]
    fn the_default_fonts_are_atkinson() {
        let d = definitions(&Face::Atkinson, &Face::Atkinson);
        let p = &d.families[&FontFamily::Proportional];
        let m = &d.families[&FontFamily::Monospace];
        assert_eq!(p[0], NEXT, "{p:?}");
        assert_eq!(m[0], MONO, "{m:?}");
        assert_eq!(d.families[&heading_family()][0], NEXT_BOLD);
        assert_eq!(
            d.font_data[NEXT].bytes(),
            jetcarp_display::fonts::NEXT_REGULAR
        );
        assert_eq!(
            d.font_data[MONO].bytes(),
            jetcarp_display::fonts::MONO_REGULAR
        );
        assert_eq!(
            d.font_data[NEXT_BOLD].bytes(),
            jetcarp_display::fonts::NEXT_BOLD
        );
        // egui's fonts stay behind them, in egui's order.
        assert_eq!(&p[1..], &BEHIND_TEXT.map(String::from));
        assert_eq!(&m[1..], &BEHIND_MONO.map(String::from));
        // In a context: what egui draws with.
        let ctx = ctx_with(&Face::Atkinson, &Face::Atkinson);
        let in_ctx = ctx.fonts(|f| f.definitions().clone());
        assert_eq!(in_ctx.families[&FontFamily::Proportional][0], NEXT);
        assert_eq!(in_ctx.families[&FontFamily::Monospace][0], MONO);
        let style = ctx.global_style();
        assert_eq!(
            style.text_styles[&egui::TextStyle::Heading].family,
            heading_family()
        );
    }

    #[test]
    fn egui_s_original_fonts_are_as_they_were() {
        let d = definitions(&Face::Egui, &Face::Egui);
        assert_eq!(
            d.families[&FontFamily::Proportional],
            BEHIND_TEXT.map(String::from)
        );
        assert_eq!(
            d.families[&FontFamily::Monospace],
            BEHIND_MONO.map(String::from)
        );
        // As the app had them before Atkinson: egui's defaults with Hack after Ubuntu Light.
        let mut before = FontDefinitions::default();
        before
            .families
            .get_mut(&FontFamily::Proportional)
            .unwrap()
            .push("Hack".into());
        assert_eq!(
            d.families[&FontFamily::Proportional],
            before.families[&FontFamily::Proportional]
        );
        assert_eq!(
            d.families[&FontFamily::Monospace],
            before.families[&FontFamily::Monospace]
        );
    }

    /// Every character in the app's strings: string literals in its source and the display
    /// crate's (the caption), non-ASCII ones.
    fn interface_characters() -> std::collections::BTreeSet<char> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut set = std::collections::BTreeSet::new();
        let mut stack = vec![root.join("src"), root.join("../jetcarp-display/src")];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    let text = std::fs::read_to_string(&p).unwrap();
                    // Not tests' strings, nor comments: only what is shown.
                    let shown = text.split("#[cfg(test)]").next().unwrap_or("");
                    for line in shown.lines() {
                        let code = line.split("//").next().unwrap_or("");
                        let mut inside = false;
                        let mut escaped = false;
                        for c in code.chars() {
                            if inside && !escaped && c == '"' {
                                inside = false;
                            } else if !inside && c == '"' {
                                inside = true;
                            } else if inside && !c.is_ascii() {
                                set.insert(c);
                            }
                            escaped = inside && c == '\\' && !escaped;
                        }
                    }
                }
            }
        }
        set
    }

    /// Characters the app shows that egui's own fonts never drew either (a font of the system's
    /// can be chosen for them): the macOS key symbols.
    const NEVER_DRAWN: &str = "⌃⌘⌥⌦⌫";

    #[test]
    fn the_fallback_draws_every_interface_symbol() {
        let chars = interface_characters();
        assert!(chars.contains(&'×') && chars.contains(&'→'), "{chars:?}");
        let atkinson = ctx_with(&Face::Atkinson, &Face::Atkinson);
        let original = ctx_with(&Face::Egui, &Face::Egui);
        let draws = |ctx: &egui::Context, family: FontFamily, c: char| {
            ctx.fonts_mut(|f| f.has_glyph(&egui::FontId::new(14.0, family), c))
        };
        let mut lost = vec![];
        let mut undrawn = vec![];
        for &c in &chars {
            for family in [
                FontFamily::Proportional,
                FontFamily::Monospace,
                heading_family(),
            ] {
                let before = draws(&original, family.clone(), c);
                let now = draws(&atkinson, family.clone(), c);
                if before && !now {
                    lost.push(format!("{c} (U+{:04X}) in {family:?}", c as u32));
                }
                if !now && !NEVER_DRAWN.contains(c) {
                    undrawn.push(format!("{c} (U+{:04X}) in {family:?}", c as u32));
                }
            }
        }
        assert!(
            lost.is_empty(),
            "egui drew these and Atkinson's set doesn't: {lost:?}"
        );
        assert!(undrawn.is_empty(), "not drawn: {undrawn:?}");
        // Greek and more of Latin come from Ubuntu Light behind Atkinson; CJK is drawn by none
        // of the built-in fonts (a system font can be chosen).
        for c in "ΔΣλπφαβγΩμ ÆŒçéüñßøå°±–—“”€".chars() {
            assert!(draws(&atkinson, FontFamily::Proportional, c), "{c}");
            assert!(draws(&atkinson, FontFamily::Monospace, c), "{c}");
        }
        for c in "中文日本語".chars() {
            assert_eq!(
                draws(&atkinson, FontFamily::Proportional, c),
                draws(&original, FontFamily::Proportional, c),
                "{c}"
            );
        }
    }

    pub(crate) fn scratch(name: &str) -> jetcarp_core::scratch::ScratchDir {
        jetcarp_core::scratch::ScratchDir::new(&format!("fonts-{name}"))
    }

    #[test]
    fn installed_fonts_are_found_by_family_and_unusable_files_are_left_out() {
        let dir = scratch("scan");
        let fonts = dir.join("share/fonts");
        std::fs::create_dir_all(fonts.join("truetype/hack")).unwrap();
        std::fs::write(
            fonts.join("truetype/hack/Hack-Regular.ttf"),
            builtin("Hack"),
        )
        .unwrap();
        std::fs::write(fonts.join("Ubuntu-Light.TTF"), builtin("Ubuntu-Light")).unwrap();
        // Not fonts, or no Latin: left out.
        std::fs::write(fonts.join("broken.ttf"), b"not a font at all").unwrap();
        std::fs::write(fonts.join("icons.ttf"), builtin("egui-icons")).unwrap();
        std::fs::write(fonts.join("notes.txt"), b"hello").unwrap();
        let found = scan(&[fonts.clone(), dir.join("nowhere")]);
        let names: Vec<&str> = found.iter().map(|f| f.family.as_str()).collect();
        assert_eq!(names, ["Hack", "Ubuntu"], "{found:?}");
        assert!(found[0].path.ends_with("truetype/hack/Hack-Regular.ttf"));
        // A scan on its own thread finds the same.
        let mut s = Scanner::new(vec![fonts]);
        s.start(&egui::Context::default());
        assert_eq!(*s.wait(), found);
    }

    #[test]
    fn a_bad_font_file_is_refused_with_a_plain_message() {
        let dir = scratch("bad");
        std::fs::create_dir_all(&*dir).unwrap();
        let junk = dir.join("junk.ttf");
        std::fs::write(&junk, b"\0\x01\0\0 this is not a font").unwrap();
        let e = load(&junk, None).err().expect("refused");
        assert!(e.starts_with("junk.ttf isn't a font"), "{e}");
        let icons = dir.join("icons.ttf");
        std::fs::write(&icons, builtin("egui-icons")).unwrap();
        let e = load(&icons, None).err().expect("refused");
        assert!(e.contains("no basic Latin"), "{e}");
        let e = load(&dir.join("gone.ttf"), None).err().expect("refused");
        assert!(e.starts_with("JETCARP couldn't read gone.ttf"), "{e}");
        // No local path in what is shown.
        for p in [&junk, &icons] {
            let e = load(p, None).err().unwrap();
            assert!(!e.contains(&*dir.to_string_lossy()), "{e}");
        }
        let ok = load(
            &{
                let p = dir.join("Hack-Regular.ttf");
                std::fs::write(&p, builtin("Hack")).unwrap();
                p
            },
            None,
        )
        .expect("Hack is a font");
        assert_eq!(ok.family, "Hack");
        assert_eq!(ok.index, 0);
    }

    #[test]
    fn a_chosen_font_comes_first_with_egui_s_behind_it() {
        let data = Arc::new(FontData::from_owned(builtin("Hack")));
        let d = definitions(&Face::Loaded(data.clone()), &Face::Loaded(data));
        assert_eq!(d.families[&FontFamily::Proportional][0], CHOSEN);
        assert_eq!(
            &d.families[&FontFamily::Proportional][1..],
            &BEHIND_TEXT.map(String::from)
        );
        assert_eq!(d.families[&FontFamily::Monospace][0], CHOSEN_MONO);
        assert_eq!(d.families[&heading_family()][0], CHOSEN);
        assert_eq!(d.font_data[CHOSEN].bytes(), &builtin("Hack")[..]);
    }

    #[test]
    fn font_choices_are_saved_by_name() {
        let c = FontChoice::File {
            family: "Hack".into(),
            path: "/usr/share/fonts/hack/Hack-Regular.ttf".into(),
            index: 0,
        };
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(
            json,
            r#"{"file":{"family":"Hack","path":"/usr/share/fonts/hack/Hack-Regular.ttf","index":0}}"#
        );
        assert_eq!(serde_json::from_str::<FontChoice>(&json).unwrap(), c);
        assert_eq!(
            serde_json::to_string(&FontChoice::Atkinson).unwrap(),
            r#""atkinson""#
        );
        assert_eq!(
            serde_json::to_string(&FontChoice::Egui).unwrap(),
            r#""egui""#
        );
    }
}
