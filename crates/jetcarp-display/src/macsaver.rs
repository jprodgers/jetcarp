//! macOS's screen saver list: a `JETCARP.saver` bundle in the user's own Screen Savers folder.
//!
//! macOS runs third-party savers inside its own sandboxed process (`legacyScreenSaver`), which
//! cannot read JETCARP's settings or library. So installing bakes everything the saver shows into
//! the bundle: the rotation, every item's scene inline, the frame rate and caption, and the
//! caption's font (a font file chosen in the app is copied in). Change the rotation or the font and
//! install again to update it. No administrator rights are needed.

use crate::caption::{CaptionFont, CaptionSettings, LastResort};
use crate::fonts::{Fallbacks, FontChoice};
use crate::host_settings::{HostSettings, ScreenSettings};
use crate::profiles::{self, Show};
use crate::rotation::{self, Rotation, Slot};
use crate::simgpu::SimBackend;
use crate::stage::{self, Stage};
use jetcarp_core::Scene;
use jetcarp_core::media::encode;
use jetcarp_core::render::{self, RenderOptions};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[cfg(feature = "library")]
use std::process::Command;
use std::time::{Duration, Instant};

/// The picture System Settings shows for the saver in its list, in points: `thumbnail.png` at this
/// size and `thumbnail@2x.png` at twice it. Not documented by Apple, but what savers use.
pub const THUMBNAIL: [u32; 2] = [90, 58];
/// Generations run before the picture is taken, so it shows the rule at work and not its seed.
const THUMBNAIL_GENERATIONS: u64 = 60;
/// Making the picture gives up after this long, and the bundle goes without one.
const THUMBNAIL_TIME: Duration = Duration::from_secs(3);

/// The name System Settings shows comes from the bundle's.
pub const BUNDLE: &str = "JETCARP.saver";
/// The bundle's identifier, by which its code finds the bundle again.
pub const BUNDLE_ID: &str = "com.jimmieprodgers.jetcarp.saver";
/// The Objective-C class the bundle's code registers, named in its `Info.plist`.
pub const PRINCIPAL_CLASS: &str = "JETCARPSaverView";
/// What the bundle's code is built as (`cargo build --release -p jetcarp-saver`).
pub const DYLIB: &str = "libjetcarp_saver.dylib";
/// The baked settings, in the bundle's Resources.
pub const BAKED_FILE: &str = "jetcarp-saver.json";
/// 2 added the caption's font ([`Baked::font`]); a bundle of format 1 has none, and draws its
/// caption in Atkinson Hyperlegible Next.
pub const BAKED_FORMAT: u32 = 2;
const EXECUTABLE: &str = "JETCARP";
/// The copy of the caption's font in the bundle's Resources, with the file's own extension.
const FONT_STEM: &str = "caption-font";
/// The copies of installed fonts for its last resort, numbered from 1.
const FALLBACK_STEM: &str = "caption-fallback";
/// Larger files are not copied in (as the app's last resort reads none larger).
const MAX_FALLBACK_FILE: u64 = 256 << 20;

/// The caption's font in a bundle, as the app's Font setting named it at install (#273).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BakedFont {
    /// Atkinson Hyperlegible Next, which the saver's code has built in.
    #[default]
    Atkinson,
    /// egui's original, Ubuntu Light, also built in.
    Egui,
    /// An installed font or a font file, copied into the bundle's Resources as `file`; `index` is
    /// its face in a collection (`.ttc`).
    File {
        family: String,
        file: String,
        #[serde(default)]
        index: u32,
    },
}

impl BakedFont {
    /// In words, for the saver's log.
    pub fn label(&self) -> String {
        match self {
            BakedFont::Atkinson => FontChoice::Atkinson.label(false),
            BakedFont::Egui => FontChoice::Egui.label(false),
            BakedFont::File { family, .. } => family.clone(),
        }
    }
}

/// An installed font copied into the bundle for characters none of the caption's fonts has (the
/// caption's last resort, [`LastResort`]), as `file` in its Resources.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BakedFallback {
    pub family: String,
    pub file: String,
    /// The file, read when baked, for [`write_bundle`] to copy in.
    #[serde(skip)]
    pub bytes: FontBytes,
}

/// A font file's bytes, read when the bundle is baked, for [`write_bundle`] to copy in.
#[derive(Clone, Default, PartialEq)]
pub struct FontBytes(pub std::sync::Arc<Vec<u8>>);

impl std::fmt::Debug for FontBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} bytes", self.0.len())
    }
}

/// Everything the saver shows, so it needs nothing outside its bundle.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Baked {
    pub format: u32,
    pub rotation: Rotation,
    /// The rotation's items, each with its scene inline.
    pub slots: Vec<Slot>,
    pub fps: f32,
    pub caption: CaptionSettings,
    pub max_cells: u64,
    pub gpu: bool,
    /// The strobe guard (#97), as the setting for the desktop background and the screen saver
    /// was when the bundle was built: the sandboxed saver cannot read it. On when missing.
    #[serde(default = "guard_default")]
    pub strobe_guard: bool,
    /// The saved answer to the seizure warning was *at risk* when the bundle was built: the
    /// guard's stricter stripe rule ([`jetcarp_core::strobe::similar`]).
    #[serde(default)]
    pub strobe_at_risk: bool,
    /// The caption's font (#273), as the app's Font setting named it when the bundle was built:
    /// the sandboxed saver can't read the setting, nor be counted on to read the user's font
    /// folders, so a font file chosen there is copied in. Atkinson when missing (format 1).
    #[serde(default)]
    pub font: BakedFont,
    /// The file `font` names, read when baked, for [`write_bundle`] to copy in.
    #[serde(skip)]
    pub font_file: Option<FontBytes>,
    /// The installed fonts copied in for the characters of the caption's lines (its rotation's
    /// name, the rules' names, the grids') that none of its fonts has.
    #[serde(default)]
    pub fallbacks: Vec<BakedFallback>,
}

fn guard_default() -> bool {
    true
}

/// The rotation a screen saver runs: its own choice, else the first screen's, else the default
/// one when the choice no longer exists.
pub fn chosen_rotation(settings: &HostSettings) -> String {
    let name = settings
        .saver_rotation
        .clone()
        .or_else(|| settings.screens.first().map(|s| s.rotation.clone()))
        .unwrap_or_else(|| ScreenSettings::default().rotation);
    if rotation::load_stored(&name).is_ok() {
        name
    } else {
        ScreenSettings::default().rotation
    }
}

/// Rotation `name` with every item's scene looked up now, with warnings for items that could not
/// be (those show Life, with a note in the log, as they would in the host). `font` is the app's
/// Font setting, for the caption; `installed` the folders installed fonts are in, for the
/// caption's last resort ([`crate::fonts::system_dirs`]).
pub fn bake(
    name: &str,
    settings: &HostSettings,
    font: &FontChoice,
    installed: &[PathBuf],
    lookup: &dyn Fn(&str) -> Option<Scene>,
) -> Result<(Baked, Vec<String>), String> {
    let loaded = rotation::load_stored(name)?;
    Ok(bake_loaded(loaded, settings, font, installed, lookup))
}

/// [`bake`], for a rotation loaded already.
fn bake_loaded(
    (rotation, mut slots, mut warnings): (Rotation, Vec<Slot>, Vec<String>),
    settings: &HostSettings,
    font: &FontChoice,
    installed: &[PathBuf],
    lookup: &dyn Fn(&str) -> Option<Scene>,
) -> (Baked, Vec<String>) {
    for s in &mut slots {
        match profiles::source_scene(&s.profile, lookup) {
            Ok(scene) => {
                s.profile.show = Show::Scene {
                    scene: Box::new(scene),
                }
            }
            Err(e) => warnings.push(format!("{}: {e}", s.label)),
        }
    }
    let choice = font;
    let (font, font_file) = bake_font(choice).unwrap_or_else(|w| {
        warnings.push(w);
        (BakedFont::Atkinson, None)
    });
    // The fonts the saver will draw in, for what they lack.
    let drawn = match &font {
        BakedFont::File { .. } => CaptionFont::for_choice(choice).unwrap_or_default(),
        BakedFont::Egui => CaptionFont::for_choice(&FontChoice::Egui).unwrap_or_default(),
        BakedFont::Atkinson => CaptionFont::default(),
    };
    let chars = caption_chars(&rotation.name, &slots);
    let fallbacks = bake_fallbacks(&chars, &drawn, installed, &mut warnings);
    let baked = Baked {
        format: BAKED_FORMAT,
        rotation,
        slots,
        fps: settings.fps,
        caption: settings.caption,
        max_cells: settings.cell_limit(),
        // The sandboxed saver cannot read the GPU settings: the desktop background's choice now.
        gpu: settings.gpu_policy().mode != jetcarp_gpu::select::Mode::Off,
        strobe_guard: jetcarp_core::photosensitivity::guard_on(
            jetcarp_core::photosensitivity::GuardPlace::Desktop,
        ),
        strobe_at_risk: jetcarp_core::photosensitivity::at_risk(),
        font,
        font_file,
        fallbacks,
    };
    (baked, warnings)
}

/// Every character the caption could show for a rotation named `name` of `slots`, their scenes
/// baked in: the name, each scene's rules (notations and names) and grid, and the words, digits
/// and signs the caption adds ([`crate::caption::lines`]). More than it will show, never less.
fn caption_chars(name: &str, slots: &[Slot]) -> std::collections::BTreeSet<char> {
    use jetcarp_core::rules::{self, Rule};
    let mut text = String::from(name);
    text.push_str(
        "0123456789,·×…+ No rule running Rule more Square Hexagon Triangle grid cells wraps dead \
         live edges of generation",
    );
    for s in slots {
        let Show::Scene { scene } = &s.profile.show else {
            continue;
        };
        text.push_str(&scene.tiling.family);
        if let Some(f) = jetcarp_core::families::family(&scene.tiling.family) {
            text.push_str(f.name);
        }
        for ca in &scene.cas {
            text.push_str(&ca.rule);
            let Ok((rule, _)) = Rule::parse(&ca.rule) else {
                continue;
            };
            let n = rule.notation();
            text.push_str(&n);
            // The catalog's names on any neighbourhood (a suffix such as H), and named rules.
            for e in rules::catalog() {
                let on = e.notation.strip_prefix(n.as_str());
                if on.is_some_and(|rest| rest.chars().all(|c| c.is_ascii_alphabetic())) {
                    text.push_str(e.name);
                }
            }
            for r in rules::named_rules().iter().filter(|r| r.notation == n) {
                text.push_str(&r.name);
            }
        }
    }
    text.chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect()
}

/// The installed fonts in `installed` that draw the characters of `chars` that `drawn` lacks, read
/// to be copied in, as the app's last resort picks them (the upright regular face in the smallest
/// file); a warning for one that can't be read. Installed fonts are looked at only when a
/// character is missing.
fn bake_fallbacks(
    chars: &std::collections::BTreeSet<char>,
    drawn: &CaptionFont,
    installed: &[PathBuf],
    warnings: &mut Vec<String>,
) -> Vec<BakedFallback> {
    let missing: Vec<char> = chars
        .iter()
        .copied()
        .filter(|&c| drawn.glyph(c).1.0 == 0)
        .collect();
    if missing.is_empty() || installed.is_empty() {
        return vec![];
    }
    let index = Fallbacks::find(installed);
    let mut files: Vec<(PathBuf, String)> = vec![];
    for c in missing {
        if let Some(f) = index.for_char(c)
            && !files.iter().any(|(p, _)| *p == f.path)
        {
            files.push((f.path.clone(), f.family.clone()));
        }
    }
    let mut out = vec![];
    for (path, family) in files {
        let read = std::fs::metadata(&path)
            .map_err(|e| e.to_string())
            .and_then(|m| {
                if m.len() > MAX_FALLBACK_FILE {
                    Err("it is too large".into())
                } else {
                    std::fs::read(&path).map_err(|e| e.to_string())
                }
            });
        match read {
            Ok(bytes) => out.push(BakedFallback {
                family,
                file: format!("{FALLBACK_STEM}-{}.{}", out.len() + 1, font_ext(&path)),
                bytes: FontBytes(std::sync::Arc::new(bytes)),
            }),
            Err(e) => {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                warnings.push(format!(
                    "{family} ({name}), which has characters of the caption that its font lacks, \
                     can't be copied into the screen saver ({e}); they are boxes there"
                ));
            }
        }
    }
    out
}

/// A font file's type, for its copy's name: `ttf` when it has none JETCARP knows.
fn font_ext(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .filter(|e| ["ttf", "otf", "ttc", "otc"].contains(&e.as_str()))
        .unwrap_or_else(|| "ttf".into())
}

/// The caption's last resort in a bundle whose Resources are at `resources`: the installed fonts
/// copied in, `fallbacks`, ready at once (their character maps are read now).
pub fn last_resort(fallbacks: &[BakedFallback], resources: &Path) -> std::sync::Arc<LastResort> {
    let files: Vec<PathBuf> = fallbacks
        .iter()
        .map(|f| resources.join(Path::new(&f.file).file_name().unwrap_or_default()))
        .collect();
    LastResort::ready(Fallbacks::of_files(&files))
}

/// The caption's font to bake in for the app's Font setting `choice`, and the file to copy in.
/// The saver's code has Atkinson and egui's fonts built in. An installed font or a font file is
/// read now, whole (a collection too, its face recorded), and checked as the saver will read it;
/// one that can't be is a warning for the install's report, and the caption is in Atkinson.
fn bake_font(choice: &FontChoice) -> Result<(BakedFont, Option<FontBytes>), String> {
    let FontChoice::File {
        family,
        path,
        index,
    } = choice
    else {
        let font = match choice {
            FontChoice::Egui => BakedFont::Egui,
            _ => BakedFont::Atkinson,
        };
        return Ok((font, None));
    };
    let warn = |e: String| {
        format!(
            "the caption's font, {family}, can't be copied into the screen saver ({}); its caption \
             is in Atkinson Hyperlegible Next",
            e.trim_end_matches('.')
        )
    };
    let file = crate::fonts::load(path, Some(*index)).map_err(warn)?;
    if ab_glyph::FontRef::try_from_slice_and_index(&file.bytes, file.index).is_err() {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        return Err(warn(format!("{name} isn't a font JETCARP can read")));
    }
    let font = BakedFont::File {
        family: file.family,
        file: format!("{FONT_STEM}.{}", font_ext(path)),
        index: file.index,
    };
    Ok((font, Some(FontBytes(std::sync::Arc::new(file.bytes)))))
}

/// The baked settings from `text`, a bundle's [`BAKED_FILE`]: of this format or an earlier one,
/// whose missing settings take their defaults (format 1's caption is in Atkinson).
pub fn parse_baked(text: &str) -> Result<Baked, String> {
    let baked: Baked = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if baked.format > BAKED_FORMAT {
        return Err(format!(
            "it is format {}, newer than this code reads ({BAKED_FORMAT}); install the saver again",
            baked.format
        ));
    }
    Ok(baked)
}

/// The caption's fonts for the `font` baked into a bundle whose Resources are at `resources`, and
/// a note when the bundle's copy of a font file is missing or can't be used: the caption is then in
/// Atkinson Hyperlegible Next, as on the desktop background.
pub fn caption_font(font: &BakedFont, resources: &Path) -> (CaptionFont, Option<String>) {
    let choice = match font {
        BakedFont::Atkinson => FontChoice::Atkinson,
        BakedFont::Egui => FontChoice::Egui,
        BakedFont::File {
            family,
            file,
            index,
        } => FontChoice::File {
            family: family.clone(),
            // A file in Resources itself, whatever the name says.
            path: resources.join(Path::new(file).file_name().unwrap_or_default()),
            index: *index,
        },
    };
    match CaptionFont::for_choice(&choice) {
        Ok(f) => (f, None),
        Err(e) => (
            CaptionFont::default(),
            Some(format!(
                "the caption can't be drawn in {}, the font copied in when the screen saver was \
                 installed ({}); it is in Atkinson Hyperlegible Next",
                font.label(),
                e.trim_end_matches('.')
            )),
        ),
    }
}

/// `~/Library/Screen Savers/JETCARP.saver`.
pub fn bundle_path() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("no home folder")?
        .join("Library/Screen Savers")
        .join(BUNDLE))
}

fn info_plist() -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleExecutable</key>
	<string>{EXECUTABLE}</string>
	<key>CFBundleIdentifier</key>
	<string>{BUNDLE_ID}</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>JETCARP</string>
	<key>CFBundlePackageType</key>
	<string>BNDL</string>
	<key>CFBundleShortVersionString</key>
	<string>{version}</string>
	<key>CFBundleVersion</key>
	<string>{version}</string>
	<key>LSMinimumSystemVersion</key>
	<string>11.0</string>
	<key>NSPrincipalClass</key>
	<string>{PRINCIPAL_CLASS}</string>
</dict>
</plist>
"#
    )
}

/// Writes the bundle at `dest` from the built `dylib` and the `baked` settings.
pub fn write_bundle(dest: &Path, dylib: &Path, baked: &Baked) -> Result<(), String> {
    let err = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    // A fresh folder, not files overwritten in place: macOS may still have the old code mapped,
    // and changing a mapped, signed file makes the process that has it crash.
    if dest.exists() {
        std::fs::remove_dir_all(dest).map_err(|e| err(dest, e))?;
    }
    let contents = dest.join("Contents");
    let (macos, resources) = (contents.join("MacOS"), contents.join("Resources"));
    for d in [&macos, &resources] {
        std::fs::create_dir_all(d).map_err(|e| err(d, e))?;
    }
    let plist = contents.join("Info.plist");
    std::fs::write(&plist, info_plist()).map_err(|e| err(&plist, e))?;
    let exe = macos.join(EXECUTABLE);
    std::fs::copy(dylib, &exe).map_err(|e| err(&exe, e))?;
    let file = resources.join(BAKED_FILE);
    let text = serde_json::to_string(baked).map_err(|e| e.to_string())?;
    std::fs::write(&file, text).map_err(|e| err(&file, e))?;
    if let BakedFont::File { file, .. } = &baked.font {
        let bytes = baked
            .font_file
            .as_ref()
            .ok_or_else(|| format!("{file}: the caption's font wasn't read when it was baked"))?;
        let path = resources.join(file);
        std::fs::write(&path, &*bytes.0).map_err(|e| err(&path, e))?;
    }
    for f in &baked.fallbacks {
        if f.bytes.0.is_empty() {
            return Err(format!(
                "{}: the font wasn't read when it was baked",
                f.file
            ));
        }
        let path = resources.join(&f.file);
        std::fs::write(&path, &*f.bytes.0).map_err(|e| err(&path, e))?;
    }
    if let Some(pictures) = thumbnails(baked) {
        for (name, png) in ["thumbnail.png", "thumbnail@2x.png"]
            .into_iter()
            .zip(pictures)
        {
            let path = resources.join(name);
            std::fs::write(&path, png).map_err(|e| err(&path, e))?;
        }
    }
    Ok(())
}

/// The saver's first scene a little way in, as PNGs at [`THUMBNAIL`] and twice it. None when no
/// world was ready in time.
pub fn thumbnails(baked: &Baked) -> Option<[Vec<u8>; 2]> {
    let [w, h] = THUMBNAIL;
    let mut stage = Stage::new(
        &baked.rotation,
        baked.slots.clone(),
        1,
        [w as f64, h as f64],
        stage::no_lookup(),
        baked.max_cells,
    )
    .ok()?;
    let mut sim = SimBackend::unavailable("a still picture".into());
    let began = Instant::now();
    let mut now = 0.0;
    // The world is built on a thread, then stepped as fast as each tick allows.
    while began.elapsed() < THUMBNAIL_TIME {
        stage.tick(now, &mut sim, Duration::from_millis(20));
        if stage
            .world()
            .is_some_and(|w| w.generation >= THUMBNAIL_GENERATIONS)
        {
            break;
        }
        now += 1.0 / 30.0;
        std::thread::sleep(Duration::from_millis(1));
    }
    let world = stage.world()?;
    let opts = RenderOptions::default();
    let one = encode::png(&render::render_fitted(world, w, h, &opts)).ok()?;
    let two = encode::png(&render::render_fitted(world, w * 2, h * 2, &opts)).ok()?;
    Some([one, two])
}

/// Signs the bundle for this machine only (ad hoc), which Apple silicon needs before it runs code.
#[cfg(feature = "library")]
pub(crate) fn sign(bundle: &Path) -> Result<(), String> {
    let out = Command::new("codesign")
        .args(["--force", "--sign", "-"])
        .arg(bundle)
        .output()
        .map_err(|e| format!("codesign: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "codesign: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// The built saver code: beside this program (a build folder, a release archive, the shell
/// installer's folder), else in `../lib/jetcarp/` from it (Homebrew, and the wheel's data in a
/// Python environment). The program is the file it really is when it was started through a link
/// on the `PATH`.
pub fn built_dylib() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let real = std::fs::canonicalize(&exe).unwrap_or_else(|_| exe.clone());
    let path = real.with_file_name(DYLIB);
    let mut places = vec![path.clone()];
    if let Some(prefix) = real.parent().and_then(Path::parent) {
        places.push(prefix.join("lib").join("jetcarp").join(DYLIB));
    }
    places.push(exe.with_file_name(DYLIB));
    if let Some(found) = places.into_iter().find(|p| p.is_file()) {
        Ok(found)
    } else {
        Err(format!(
            "{} was not found. Build it with `cargo build --release -p jetcarp-saver`.",
            path.display()
        ))
    }
}

/// Builds the bundle from the current settings and opens System Settings where savers are chosen. Returns
/// where it went, and warnings about items that will show Life instead.
#[cfg(feature = "library")]
pub fn install() -> Result<(PathBuf, Vec<String>), String> {
    if !cfg!(target_os = "macos") {
        return Err("`.saver` screen savers exist only on macOS".into());
    }
    let dylib = built_dylib()?;
    let settings = HostSettings::load().0;
    let name = chosen_rotation(&settings);
    let lookup = crate::stage::library_lookup();
    let font = crate::host_settings::config_dir()
        .map(|d| crate::fonts::saved_in(&d).0)
        .unwrap_or_default();
    let installed = crate::fonts::system_dirs();
    let (baked, warnings) = bake(&name, &settings, &font, &installed, &*lookup)?;
    let dest = bundle_path()?;
    // What the saver will replace once chosen, to put back when it is removed. Without a record,
    // removing shows the desktop picture as the screen saver instead.
    let _ = record_choices();
    write_bundle(&dest, &dylib, &baked)?;
    sign(&dest)?;
    let _ = Command::new("open")
        .arg(format!("x-apple.systempreferences:{}", settings_pane()))
        .spawn();
    Ok((dest, warnings))
}

/// System Settings' own pane for screen savers, which macOS 14 and 15 have.
const SAVER_PANE: &str = "com.apple.ScreenSaver-Settings.extension";
/// Where the screen savers are when that pane is gone (macOS 27): Wallpaper, "Screen Saver…".
#[cfg(feature = "library")]
const WALLPAPER_PANE: &str = "com.apple.Wallpaper-Settings.extension";

/// Whether this macOS still has the Screen Saver pane, from the panes System Settings loads.
fn has_saver_pane() -> bool {
    let Ok(dir) = std::fs::read_dir("/System/Library/ExtensionKit/Extensions") else {
        return false;
    };
    let wanted = SAVER_PANE.as_bytes();
    dir.flatten().any(|e| {
        std::fs::read(e.path().join("Contents/Info.plist"))
            .is_ok_and(|b| b.windows(wanted.len()).any(|w| w == wanted))
    })
}

#[cfg(feature = "library")]
fn settings_pane() -> &'static str {
    if has_saver_pane() {
        SAVER_PANE
    } else {
        WALLPAPER_PANE
    }
}

/// Where people choose a screen saver, for messages.
pub fn settings_place() -> &'static str {
    if has_saver_pane() {
        "System Settings, Screen Saver"
    } else {
        "System Settings, Wallpaper, Screen Saver…"
    }
}

/// Deletes the bundle, and puts back the screen saver macOS had chosen before JETCARP was
/// installed wherever the choice still names JETCARP's (see [`crate::wallpaper_store`]). Returns
/// a note for people on what became of the choice.
pub fn remove() -> Result<Option<String>, String> {
    let dest = bundle_path()?;
    let had = dest.exists();
    if had {
        std::fs::remove_dir_all(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    if !cfg!(target_os = "macos") {
        return Ok(None);
    }
    let mut notes = Vec::new();
    match restore_choices() {
        Ok(Some(n)) => notes.push(n),
        Ok(None) => {}
        Err(e) => notes.push(format!(
            "the screen saver choice could not be put back ({e}); choose another in {}",
            settings_place()
        )),
    }
    if had {
        notes.push(
            "System Settings, if it is open, may keep listing JETCARP until you quit and reopen it"
                .into(),
        );
    }
    Ok((!notes.is_empty()).then(|| notes.join("; ")))
}

/// Where installing records the screen saver choices it replaces: in JETCARP's settings folder.
pub const CHOICES_RECORD: &str = "screen-saver-before.plist";
/// A copy of the wallpaper store as it was before removing changed it, in the settings folder.
const STORE_COPY: &str = "wallpaper-store-before-saver-remove.plist";

/// macOS's wallpaper store, which holds the screen saver chosen for every display and Space.
fn store_path() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("no home folder")?
        .join("Library/Application Support/com.apple.wallpaper/Store/Index.plist"))
}

fn record_path() -> Result<PathBuf, String> {
    Ok(crate::host_settings::config_dir()
        .ok_or("no settings folder")?
        .join(CHOICES_RECORD))
}

/// The store (a binary property list) as a tree, through `plutil`.
fn read_store(path: &Path) -> Result<crate::wallpaper_store::Value, String> {
    let out = std::process::Command::new("/usr/bin/plutil")
        .args(["-convert", "xml1", "-o", "-"])
        .arg(path)
        .output()
        .map_err(|e| format!("plutil: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "plutil could not read {}: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    crate::wallpaper_store::parse(&String::from_utf8_lossy(&out.stdout))
}

/// Writes the store back as a binary property list, in place of the old one at once, and
/// restarts macOS's wallpaper agent (launchd starts it again), which reads the store as it starts.
fn write_store(path: &Path, v: &crate::wallpaper_store::Value) -> Result<(), String> {
    let tmp = path.with_extension("plist.jetcarp");
    std::fs::write(&tmp, crate::wallpaper_store::write(v))
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
    let ok = std::process::Command::new("/usr/bin/plutil")
        .args(["-convert", "binary1"])
        .arg(&tmp)
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        let _ = std::fs::remove_file(&tmp);
        return Err("plutil could not write the store".into());
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))?;
    let _ = std::process::Command::new("/usr/bin/killall")
        .arg("WallpaperAgent")
        .status();
    Ok(())
}

/// Records the screen saver choices that choosing JETCARP's saver would replace (keeping an
/// earlier record for places that choose it already).
#[cfg_attr(not(feature = "library"), allow(dead_code))]
fn record_choices() -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    let store = read_store(&store_path()?)?;
    let file = record_path()?;
    let old = std::fs::read_to_string(&file)
        .ok()
        .and_then(|t| crate::wallpaper_store::parse(&t).ok());
    let rec = crate::wallpaper_store::record(&store, old.as_ref(), BUNDLE);
    if rec == crate::wallpaper_store::Value::Array(vec![]) {
        return Ok(());
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&file, crate::wallpaper_store::write(&rec))
        .map_err(|e| format!("{}: {e}", file.display()))
}

/// Puts back the choices that name JETCARP's saver, from the record (then deleted), else the
/// desktop picture. A note on what it did, if anything.
fn restore_choices() -> Result<Option<String>, String> {
    let store_file = store_path()?;
    let record_file = record_path().ok();
    let record = record_file
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| crate::wallpaper_store::parse(&t).ok());
    if !store_file.exists() {
        return Ok(None);
    }
    let mut store = read_store(&store_file)?;
    let r = crate::wallpaper_store::restore(&mut store, record.as_ref(), BUNDLE);
    if r.changed() {
        if let Some(dir) = crate::host_settings::config_dir()
            && std::fs::create_dir_all(&dir).is_ok()
        {
            let _ = std::fs::copy(&store_file, dir.join(STORE_COPY));
        }
        write_store(&store_file, &store)?;
    }
    if let Some(p) = record_file {
        let _ = std::fs::remove_file(p);
    }
    let mut said = Vec::new();
    if r.recorded > 0 {
        said.push("put back the screen saver macOS had chosen before JETCARP was installed".into());
    }
    if r.desktop > 0 {
        said.push(
            "with no earlier choice recorded, macOS now shows your desktop picture as the screen saver"
                .to_string(),
        );
    }
    if r.left > 0 {
        said.push(format!(
            "choose another screen saver in {}: macOS still chooses JETCARP's in {} place(s)",
            settings_place(),
            r.left
        ));
    }
    Ok((!said.is_empty()).then(|| said.join("; ")))
}

/// The rotation in the installed bundle, if there is one that can be read.
pub fn installed_rotation() -> Option<String> {
    let file = bundle_path()
        .ok()?
        .join("Contents/Resources")
        .join(BAKED_FILE);
    let baked: Baked = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
    Some(baked.rotation.name)
}

/// One line for people. macOS keeps which saver is chosen where other programs can't read it
/// reliably, so this says only whether JETCARP's is there.
pub fn describe() -> String {
    let Ok(path) = bundle_path() else {
        return "No home folder.".into();
    };
    if !path.exists() {
        return "JETCARP's screen saver is not installed.".into();
    }
    match installed_rotation() {
        Some(r) => format!(
            "JETCARP's screen saver is at {}, showing {r:?}. Choose it in {}.",
            path.display(),
            settings_place()
        ),
        None => format!(
            "{} is there but can't be read; install it again.",
            path.display()
        ),
    }
}

#[cfg(all(test, feature = "library"))]
mod tests {
    use super::*;

    #[test]
    fn a_baked_rotation_needs_no_lookup_and_survives_a_round_trip() {
        let settings = HostSettings::default();
        let lookup = crate::stage::library_lookup();
        let name = ScreenSettings::default().rotation;
        let (baked, _) = bake(&name, &settings, &FontChoice::Atkinson, &[], &*lookup).unwrap();
        assert!(!baked.slots.is_empty());
        assert!(
            baked
                .slots
                .iter()
                .all(|s| matches!(s.profile.show, Show::Scene { .. }))
        );
        let text = serde_json::to_string(&baked).unwrap();
        let back: Baked = serde_json::from_str(&text).unwrap();
        assert_eq!(back.slots.len(), baked.slots.len());
        // The stage runs it with nothing to look up.
        let stage = crate::stage::Stage::new(
            &back.rotation,
            back.slots,
            1,
            [320.0, 200.0],
            crate::stage::no_lookup(),
            back.max_cells,
        );
        assert!(stage.is_ok());
    }

    #[test]
    fn the_bundle_names_its_class_and_code() {
        let dir = std::env::temp_dir().join(format!("jetcarp-macsaver-{}", std::process::id()));
        let dylib = dir.join("fake.dylib");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&dylib, b"code").unwrap();
        let dest = dir.join(BUNDLE);
        let (baked, _) = bake(
            &ScreenSettings::default().rotation,
            &HostSettings::default(),
            &FontChoice::Atkinson,
            &[],
            &*crate::stage::library_lookup(),
        )
        .unwrap();
        write_bundle(&dest, &dylib, &baked).unwrap();
        // Twice, as a reinstall does.
        write_bundle(&dest, &dylib, &baked).unwrap();
        let plist = std::fs::read_to_string(dest.join("Contents/Info.plist")).unwrap();
        assert!(plist.contains("<string>JETCARPSaverView</string>"));
        assert!(plist.contains("<string>BNDL</string>"));
        assert_eq!(
            std::fs::read(dest.join("Contents/MacOS/JETCARP")).unwrap(),
            b"code"
        );
        assert!(dest.join("Contents/Resources").join(BAKED_FILE).exists());
        let thumb = std::fs::read(dest.join("Contents/Resources/thumbnail@2x.png")).unwrap();
        let img = jetcarp_core::media::encode::decode_png(&thumb).unwrap();
        assert_eq!(
            [img.width, img.height],
            [THUMBNAIL[0] * 2, THUMBNAIL[1] * 2]
        );
        assert!(dest.join("Contents/Resources/thumbnail.png").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The text the font tests measure.
    const TEXT: &str = "Square grid 480 270";

    /// The caption's width in `font`, at 20 pixels.
    fn width(font: &CaptionFont) -> f32 {
        font.line_width(TEXT, 20.0)
    }

    /// Two stand-ins for installed fonts, which tests can't count on: every basic Latin letter
    /// and digit drawn as egui's Hack draws X, and as Ubuntu Light draws W.
    fn stand_ins() -> (Vec<u8>, Vec<u8>) {
        use crate::fonts::stand_in::font_with;
        let latin: String = ('A'..='Z').chain('a'..='z').chain('0'..='9').collect();
        (
            font_with(
                epaint_default_fonts::HACK_REGULAR,
                &latin,
                'X',
                "Stand-in Sans",
            ),
            font_with(
                epaint_default_fonts::UBUNTU_LIGHT,
                &latin,
                'W',
                "Stand-in Wide",
            ),
        )
    }

    /// The bundle baked with `font` and written in `dir`: what was baked, the install's warnings,
    /// and the bundle's Resources.
    fn bundle_with(font: &FontChoice, dir: &Path) -> (Baked, Vec<String>, PathBuf) {
        let (baked, warnings) = bake(
            &ScreenSettings::default().rotation,
            &HostSettings::default(),
            font,
            &[],
            &*crate::stage::library_lookup(),
        )
        .unwrap();
        let dylib = dir.join("fake.dylib");
        std::fs::write(&dylib, b"code").unwrap();
        let dest = dir.join(BUNDLE);
        write_bundle(&dest, &dylib, &baked).unwrap();
        (baked, warnings, dest.join("Contents/Resources"))
    }

    /// The fonts copied into a bundle's Resources.
    fn copied(resources: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(resources)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("caption-font"))
            .collect();
        v.sort();
        v
    }

    /// A scratch folder with the stand-ins in `fonts/`: one font file, and a collection of the
    /// two.
    fn scratch_fonts(name: &str) -> (jetcarp_core::scratch::ScratchDir, PathBuf, PathBuf) {
        let dir = jetcarp_core::scratch::ScratchDir::new(name);
        let fonts = dir.join("fonts");
        std::fs::create_dir_all(&fonts).unwrap();
        let (one, two) = stand_ins();
        let single = fonts.join("StandIn-Regular.ttf");
        let collection = fonts.join("StandIns.ttc");
        std::fs::write(&single, &one).unwrap();
        std::fs::write(&collection, crate::fonts::stand_in::collection(&[one, two])).unwrap();
        (dir, single, collection)
    }

    /// Installing with an installed font (or a font file) chosen in the app records the choice
    /// and copies the file into the bundle's Resources, a collection whole with its face recorded,
    /// since the sandboxed saver can't be counted on to read the user's font folders. Atkinson and
    /// egui's original, which the saver has built in, are not copied.
    #[test]
    fn baking_a_chosen_font_records_it_and_copies_the_file() {
        let (dir, single, collection) = scratch_fonts("macsaver-font-bake");
        let (_, base, _) = bundle_with(&FontChoice::Atkinson, &dir);

        let choice = FontChoice::File {
            family: "Stand-in Sans".into(),
            path: single.clone(),
            index: 0,
        };
        let (baked, warnings, resources) = bundle_with(&choice, &dir);
        assert_eq!(warnings, base);
        assert_eq!(
            baked.font,
            BakedFont::File {
                family: "Stand-in Sans".into(),
                file: "caption-font.ttf".into(),
                index: 0
            },
            "the choice is recorded"
        );
        assert_eq!(copied(&resources), ["caption-font.ttf"]);
        assert_eq!(
            std::fs::read(resources.join("caption-font.ttf")).unwrap(),
            std::fs::read(&single).unwrap(),
            "the file is copied whole"
        );
        let json = std::fs::read_to_string(resources.join(BAKED_FILE)).unwrap();
        assert_eq!(parse_baked(&json).unwrap().font, baked.font);
        let folder = single.parent().unwrap().to_string_lossy().into_owned();
        assert!(
            !json.contains(&folder),
            "no folder of the user's in the bundle"
        );

        // Face 1 of a collection: the whole file, and which face.
        let choice = FontChoice::File {
            family: "Stand-in Wide".into(),
            path: collection.clone(),
            index: 1,
        };
        let (baked, warnings, resources) = bundle_with(&choice, &dir);
        assert_eq!(warnings, base);
        assert_eq!(
            baked.font,
            BakedFont::File {
                family: "Stand-in Wide".into(),
                file: "caption-font.ttc".into(),
                index: 1
            }
        );
        assert_eq!(
            copied(&resources),
            ["caption-font.ttc"],
            "installing again leaves no earlier copy"
        );
        assert_eq!(
            std::fs::read(resources.join("caption-font.ttc")).unwrap(),
            std::fs::read(&collection).unwrap()
        );

        // The saver's own fonts are not copied.
        for (choice, want) in [
            (FontChoice::Atkinson, BakedFont::Atkinson),
            (FontChoice::Egui, BakedFont::Egui),
        ] {
            let (baked, warnings, resources) = bundle_with(&choice, &dir);
            assert_eq!(warnings, base);
            assert_eq!(baked.font, want);
            assert!(copied(&resources).is_empty(), "{want:?}");
        }
    }

    /// A bundle of format 1, from before the saver followed the Font setting, still loads, its
    /// caption in Atkinson Hyperlegible Next. A format newer than this code reads is refused.
    #[test]
    fn a_format_1_bundle_still_loads_in_atkinson() {
        let (baked, _) = bake(
            &ScreenSettings::default().rotation,
            &HostSettings::default(),
            &FontChoice::Atkinson,
            &[],
            &*crate::stage::library_lookup(),
        )
        .unwrap();
        // As format 1 wrote it: every setting of today's but the font.
        let mut v = serde_json::to_value(&baked).unwrap();
        let fields = v.as_object_mut().unwrap();
        fields.remove("font");
        fields.insert("format".into(), 1.into());
        let old = parse_baked(&v.to_string()).expect("a format-1 bundle loads");
        assert_eq!(old.format, 1);
        assert_eq!(old.font, BakedFont::Atkinson);
        assert_eq!(old.slots.len(), baked.slots.len());
        let (font, note) = caption_font(&old.font, Path::new("nowhere"));
        assert_eq!(note, None);
        assert_eq!(width(&font), width(&CaptionFont::default()));

        v["format"] = (BAKED_FORMAT + 1).into();
        let e = parse_baked(&v.to_string()).unwrap_err();
        assert!(e.contains("install the saver again"), "{e}");
    }

    /// A chosen font that can't be read at install (gone, or not a font) gives Atkinson
    /// Hyperlegible Next in the bundle, nothing copied, and a warning in the install's report that
    /// names the file and no folder.
    #[test]
    fn a_font_that_can_t_be_read_at_install_gives_atkinson_and_a_warning() {
        let (dir, single, _) = scratch_fonts("macsaver-font-gone");
        let (_, base, _) = bundle_with(&FontChoice::Atkinson, &dir);
        let fonts = single.parent().unwrap().to_path_buf();
        std::fs::write(fonts.join("junk.ttf"), b"not a font").unwrap();
        for file in ["Gone-Regular.ttf", "junk.ttf"] {
            let choice = FontChoice::File {
                family: "Gone Sans".into(),
                path: fonts.join(file),
                index: 0,
            };
            let (baked, warnings, resources) = bundle_with(&choice, &dir);
            assert_eq!(baked.font, BakedFont::Atkinson, "{file}");
            assert!(copied(&resources).is_empty(), "{file}");
            assert_eq!(warnings.len(), base.len() + 1, "a warning for {file}");
            let w = warnings.last().unwrap();
            assert!(w.contains(file), "{w}");
            assert!(w.contains("Atkinson Hyperlegible Next"), "{w}");
            assert!(!w.contains(&*fonts.to_string_lossy()), "{w}");
        }
    }

    /// Installing copies in the installed fonts that draw the characters of the caption's lines
    /// that none of its fonts has (here a rotation's name in Chinese), so the sandboxed saver
    /// draws them from its bundle alone; nothing when its fonts have every character.
    #[test]
    fn the_bake_copies_the_fallback_fonts_the_caption_needs() {
        let dir = jetcarp_core::scratch::ScratchDir::new("macsaver-fallbacks");
        let installed = dir.join("installed");
        std::fs::create_dir_all(&installed).unwrap();
        let hack = epaint_default_fonts::HACK_REGULAR;
        let cjk = crate::fonts::stand_in::font_with(hack, "生命中文", 'X', "Stand-in CJK");
        std::fs::write(installed.join("StandInCJK.ttf"), &cjk).unwrap();
        // One the caption needs nothing from: not copied.
        let keys = crate::fonts::stand_in::font_with(hack, "⌘⌥", 'X', "Stand-in Keys");
        std::fs::write(installed.join("StandInKeys.ttf"), keys).unwrap();
        let lookup = crate::stage::library_lookup();
        let bake_named = |name: &str| {
            let (mut rotation, slots, warnings) =
                rotation::load_stored(&ScreenSettings::default().rotation).unwrap();
            rotation.name = name.into();
            let dirs = [installed.clone()];
            let settings = HostSettings::default();
            bake_loaded(
                (rotation, slots, warnings),
                &settings,
                &FontChoice::Atkinson,
                &dirs,
                &*lookup,
            )
            .0
        };

        let baked = bake_named("生命 rotation");
        let named: Vec<(&str, &str)> = baked
            .fallbacks
            .iter()
            .map(|f| (f.family.as_str(), f.file.as_str()))
            .collect();
        assert_eq!(named, [("Stand-in CJK", "caption-fallback-1.ttf")]);
        let dylib = dir.join("fake.dylib");
        std::fs::write(&dylib, b"code").unwrap();
        let dest = dir.join(BUNDLE);
        write_bundle(&dest, &dylib, &baked).unwrap();
        let resources = dest.join("Contents/Resources");
        assert_eq!(
            std::fs::read(resources.join("caption-fallback-1.ttf")).unwrap(),
            cjk
        );
        // The saver, from its bundle alone.
        let json = std::fs::read_to_string(resources.join(BAKED_FILE)).unwrap();
        let back = parse_baked(&json).unwrap();
        let font = caption_font(&back.font, &resources)
            .0
            .with_last_resort(last_resort(&back.fallbacks, &resources));
        assert_eq!(font.glyph('生').0, 2, "生 from the copy");
        assert_ne!(font.glyph('生').1.0, 0);

        // A caption its fonts draw whole needs nothing copied.
        assert!(bake_named("A classic a day").fallbacks.is_empty());
    }

    /// The saver draws its caption in the font baked into its bundle (a font file, a collection's
    /// face, egui's original), and, as the desktop background does, in Atkinson Hyperlegible Next
    /// with a note naming the file when the bundle's copy is broken or gone.
    #[test]
    fn the_saver_draws_in_the_baked_font_and_in_atkinson_without_it() {
        let (dir, single, collection) = scratch_fonts("macsaver-font-load");
        let atkinson = width(&CaptionFont::default());
        let ubuntu = width(&CaptionFont::for_choice(&FontChoice::Egui).unwrap());
        let in_file = |path: &Path, index| {
            let choice = FontChoice::File {
                family: String::new(),
                path: path.into(),
                index,
            };
            width(&CaptionFont::for_choice(&choice).unwrap())
        };
        let (sans, wide) = (in_file(&single, 0), in_file(&collection, 1));
        for (a, b) in [(sans, atkinson), (wide, atkinson), (sans, wide)] {
            assert!(
                (a - b).abs() > 1.0,
                "{a} vs {b}: the fonts can be told apart"
            );
        }

        let (font, note) = caption_font(&BakedFont::Egui, &dir);
        assert_eq!((width(&font), note), (ubuntu, None));

        let choice = FontChoice::File {
            family: "Stand-in Sans".into(),
            path: single,
            index: 0,
        };
        let (_, _, resources) = bundle_with(&choice, &dir);
        let json = std::fs::read_to_string(resources.join(BAKED_FILE)).unwrap();
        let baked = parse_baked(&json).unwrap();
        let (font, note) = caption_font(&baked.font, &resources);
        assert_eq!(note, None);
        assert_eq!(width(&font), sans, "the baked font is drawn");

        let choice = FontChoice::File {
            family: "Stand-in Wide".into(),
            path: collection,
            index: 1,
        };
        let (baked, _, resources) = bundle_with(&choice, &dir);
        let (font, note) = caption_font(&baked.font, &resources);
        assert_eq!((width(&font), note), (wide, None), "the collection's face");

        // The bundle's copy broken, then gone: Atkinson, and a note naming the file.
        let copy = resources.join("caption-font.ttc");
        std::fs::write(&copy, b"broken").unwrap();
        for _ in ["broken", "gone"] {
            let (font, note) = caption_font(&baked.font, &resources);
            assert_eq!(width(&font), atkinson);
            let note = note.expect("a note that the font can't be used");
            assert!(note.contains("caption-font.ttc"), "{note}");
            assert!(note.contains("Atkinson Hyperlegible Next"), "{note}");
            assert!(!note.contains(&*resources.to_string_lossy()), "{note}");
            let _ = std::fs::remove_file(&copy);
        }
    }
}
