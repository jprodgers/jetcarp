//! macOS's screen saver list: a `JETCARP.saver` bundle in the user's own Screen Savers folder.
//!
//! macOS runs third-party savers inside its own sandboxed process (`legacyScreenSaver`), which
//! cannot read JETCARP's settings or library. So installing bakes everything the saver shows into
//! the bundle: the rotation, every item's scene inline, and the frame rate and caption. Change the
//! rotation and install again to update it. No administrator rights are needed.

use crate::caption::CaptionSettings;
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
pub const BAKED_FORMAT: u32 = 1;
const EXECUTABLE: &str = "JETCARP";

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
/// be (those show Life, with a note in the log, as they would in the host).
pub fn bake(
    name: &str,
    settings: &HostSettings,
    lookup: &dyn Fn(&str) -> Option<Scene>,
) -> Result<(Baked, Vec<String>), String> {
    let (rotation, mut slots, mut warnings) = rotation::load_stored(name)?;
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
    let baked = Baked {
        format: BAKED_FORMAT,
        rotation,
        slots,
        fps: settings.fps,
        caption: settings.caption,
        max_cells: settings.max_cells,
        gpu: settings.gpu,
    };
    Ok((baked, warnings))
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
fn sign(bundle: &Path) -> Result<(), String> {
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

/// The built saver code, beside this program: beside the file it really is when it was started
/// through a link (Homebrew, pipx and the shell installer put one on the `PATH`), else beside the
/// link.
pub fn built_dylib() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let real = std::fs::canonicalize(&exe).unwrap_or_else(|_| exe.clone());
    let path = real.with_file_name(DYLIB);
    if path.exists() {
        Ok(path)
    } else if exe.with_file_name(DYLIB).exists() {
        Ok(exe.with_file_name(DYLIB))
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
    let (baked, warnings) = bake(&name, &settings, &*lookup)?;
    let dest = bundle_path()?;
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

/// Deletes the bundle. macOS then goes back to one of its own savers.
pub fn remove() -> Result<(), String> {
    let dest = bundle_path()?;
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    Ok(())
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
        let (baked, _) = bake(&name, &settings, &*lookup).unwrap();
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
}
