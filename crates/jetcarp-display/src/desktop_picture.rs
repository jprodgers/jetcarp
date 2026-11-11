//! The desktop's own picture, set through each system's documented setting: GNOME's
//! `org.gnome.desktop.background`, Windows' `SPI_SETDESKWALLPAPER`, and macOS's `NSWorkspace`.
//!
//! The host leaves a still of its scene there when it stops, or while the live wallpaper can't be
//! shown, and puts the user's own picture back when the still is no longer wanted. The user's
//! picture is saved beside the settings the first time, so it survives the host stopping. If the
//! user has chosen another picture since, that one is left alone. `jetcarp uninstall` puts it back
//! as well ([`restore`]), and keeps whatever file the desktop still shows ([`shown_files`]) when it
//! deletes the settings folder.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The user's picture, saved while a still is up.
const SAVED: &str = "desktop-picture.json";
/// Where the stills are written. Each has a new name, since GNOME keeps showing a file it has
/// already loaded.
const FOLDER: &str = "desktop-pictures";

#[derive(Serialize, Deserialize)]
struct Saved {
    /// The system's own description of the user's picture.
    original: Value,
    /// The same description just after the still was set, to tell whether the user has changed
    /// the picture since.
    after: Value,
}

/// What putting the picture back did.
#[derive(Debug, PartialEq, Eq)]
pub enum Restored {
    /// No still was up.
    Nothing,
    /// The user's picture is back.
    PutBack,
    /// The user had chosen another picture since, so it was left as it is.
    LeftAlone,
}

/// One system's desktop picture setting.
trait Desktop {
    /// A description of the picture showing now, which [`Desktop::put_back`] can restore.
    fn current(&self) -> Result<Value, String>;
    fn set_file(&self, path: &Path) -> Result<(), String>;
    fn put_back(&self, original: &Value) -> Result<(), String>;
    /// What [`Desktop::current`] will say once `path` is showing. Reading it back is right where
    /// the setting takes at once; macOS overrides this, since there the change goes through the
    /// wallpaper agent and the process that made it keeps reading the old picture for a while.
    fn after_set(&self, _path: &Path) -> Value {
        self.current().unwrap_or(Value::Null)
    }
    /// The user's picture as it should be saved, before the still replaces it. Windows overrides
    /// this, since there the picture may be a file Windows rewrites when the still is set.
    fn keep(&self, original: Value, _folder: &Path) -> Value {
        original
    }
}

/// Copies of the user's own picture start with this, and stills with `still-`.
#[cfg_attr(not(windows), allow(dead_code))]
const ORIGINAL: &str = "original-";

fn millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

fn dir() -> Result<PathBuf, String> {
    crate::host_settings::config_dir()
        .ok_or_else(|| "no configuration folder on this system".into())
}

/// Sets `png` as the desktop picture, saving the user's own first. Returns the still's path.
pub fn show(png: &[u8]) -> Result<PathBuf, String> {
    show_on(&*system()?, &dir()?, png)
}

/// Puts the user's own picture back, if a still is up.
pub fn restore() -> Result<Restored, String> {
    let dir = dir()?;
    if !dir.join(SAVED).exists() {
        return Ok(Restored::Nothing);
    }
    restore_on(&*system()?, &dir)
}

/// Whether `settings` want a still standing in for the live wallpaper.
pub fn wanted(settings: &crate::host_settings::HostSettings) -> bool {
    settings.still_picture
        && settings
            .screens
            .iter()
            .any(|s| s.place == crate::host_settings::Place::Wallpaper)
}

/// Puts the user's picture back if a still is up and `settings` no longer want one.
pub fn settle(settings: &crate::host_settings::HostSettings) -> Result<Restored, String> {
    if wanted(settings) || !showing() {
        return Ok(Restored::Nothing);
    }
    restore()
}

/// Whether a still is up (the user's picture is saved).
pub fn showing() -> bool {
    dir().is_ok_and(|d| d.join(SAVED).exists())
}

/// The picture files the desktop shows now (every screen, light and dark), as far as the
/// system says: uninstalling keeps these when it deletes the settings folder, since a desktop
/// whose picture file has gone shows a blank or default background at the next login.
pub fn shown_files() -> Vec<PathBuf> {
    system()
        .and_then(|d| d.current())
        .map(|v| files_named(&v))
        .unwrap_or_default()
}

/// The local files a [`Desktop::current`] description names: GNOME's `'file:///…'` URIs (as
/// `gsettings` prints them, quoted and percent-escaped), Windows' path, or macOS's paths per
/// screen. Anything else in it (GNOME's `'zoom'`, a plain colour's empty path) is skipped.
fn files_named(v: &Value) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut add = |s: &str| {
        let s = s.trim().trim_matches('\'');
        let path = match s.strip_prefix("file://") {
            Some(rest) => PathBuf::from(unescape(rest)),
            None => PathBuf::from(s),
        };
        if path.is_absolute() && !out.contains(&path) {
            out.push(path);
        }
    };
    match v {
        Value::String(s) => add(s),
        Value::Array(a) => a.iter().filter_map(Value::as_str).for_each(&mut add),
        Value::Object(m) => m.values().filter_map(Value::as_str).for_each(&mut add),
        _ => {}
    }
    out
}

/// A URI path with its `%XX` escapes decoded (invalid escapes are kept as written).
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn show_on(desktop: &dyn Desktop, dir: &Path, png: &[u8]) -> Result<PathBuf, String> {
    let err = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    let folder = dir.join(FOLDER);
    std::fs::create_dir_all(&folder).map_err(|e| err(&folder, e))?;
    let path = folder.join(format!("still-{}.png", millis()));
    std::fs::write(&path, png).map_err(|e| err(&path, e))?;
    let saved_file = dir.join(SAVED);
    let original = match read_saved(&saved_file) {
        Some(s) => s.original,
        None => desktop.keep(desktop.current()?, &folder),
    };
    desktop.set_file(&path)?;
    let after = desktop.after_set(&path);
    let text =
        serde_json::to_string_pretty(&Saved { original, after }).map_err(|e| e.to_string())?;
    std::fs::write(&saved_file, text).map_err(|e| err(&saved_file, e))?;
    remove_stills(&folder, Some(&path));
    Ok(path)
}

fn restore_on(desktop: &dyn Desktop, dir: &Path) -> Result<Restored, String> {
    let saved_file = dir.join(SAVED);
    let Some(saved) = read_saved(&saved_file) else {
        return Ok(Restored::Nothing);
    };
    let unchanged = desktop.current().is_ok_and(|now| now == saved.after);
    let result = if unchanged {
        desktop.put_back(&saved.original)?;
        Restored::PutBack
    } else {
        Restored::LeftAlone
    };
    let _ = std::fs::remove_file(&saved_file);
    remove_stills(&dir.join(FOLDER), None);
    Ok(result)
}

fn read_saved(file: &Path) -> Option<Saved> {
    serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()
}

fn remove_stills(folder: &Path, keep: Option<&Path>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let ours = p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("still-") && n.ends_with(".png"));
        if ours && Some(p.as_path()) != keep {
            let _ = std::fs::remove_file(&p);
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn system() -> Result<Box<dyn Desktop>, String> {
    Ok(Box::new(gnome::Gnome))
}

#[cfg(windows)]
fn system() -> Result<Box<dyn Desktop>, String> {
    Ok(Box::new(windows::Windows))
}

#[cfg(target_os = "macos")]
fn system() -> Result<Box<dyn Desktop>, String> {
    Ok(Box::new(mac::Mac))
}

#[cfg(not(any(unix, windows)))]
fn system() -> Result<Box<dyn Desktop>, String> {
    Err("this system has no desktop picture setting JETCARP knows".into())
}

#[cfg(all(unix, not(target_os = "macos")))]
mod gnome {
    use super::Desktop;
    use serde_json::{Value, json};
    use std::path::Path;
    use std::process::Command;

    const SCHEMA: &str = "org.gnome.desktop.background";
    const KEYS: [&str; 3] = ["picture-uri", "picture-uri-dark", "picture-options"];

    pub struct Gnome;

    fn get(key: &str) -> Result<String, String> {
        let out = Command::new("gsettings")
            .args(["get", SCHEMA, key])
            .output()
            .map_err(|e| format!("gsettings: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "gsettings: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// `value` in GVariant text, as `gsettings get` prints it.
    fn set(key: &str, value: &str) -> Result<(), String> {
        let out = Command::new("gsettings")
            .args(["set", SCHEMA, key, value])
            .output()
            .map_err(|e| format!("gsettings: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "gsettings: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }

    /// A `file://` URI, with everything but unreserved characters and `/` escaped.
    fn file_uri(path: &Path) -> String {
        let mut uri = String::from("file://");
        for b in path.to_string_lossy().bytes() {
            if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
                uri.push(b as char);
            } else {
                uri.push_str(&format!("%{b:02X}"));
            }
        }
        uri
    }

    impl Desktop for Gnome {
        fn current(&self) -> Result<Value, String> {
            let mut v = serde_json::Map::new();
            for k in KEYS {
                v.insert(k.into(), json!(get(k)?));
            }
            Ok(Value::Object(v))
        }

        fn set_file(&self, path: &Path) -> Result<(), String> {
            let uri = format!("'{}'", file_uri(path));
            set("picture-uri", &uri)?;
            set("picture-uri-dark", &uri)?;
            set("picture-options", "'zoom'")
        }

        fn put_back(&self, original: &Value) -> Result<(), String> {
            for k in KEYS {
                if let Some(v) = original[k].as_str() {
                    set(k, v)?;
                }
            }
            Ok(())
        }
    }

    #[cfg(test)]
    #[test]
    fn a_path_becomes_a_file_uri() {
        assert_eq!(
            file_uri(Path::new("/home/a b/still-1.png")),
            "file:///home/a%20b/still-1.png"
        );
        assert_eq!(file_uri(Path::new("/x/'q'")), "file:///x/%27q%27");
    }
}

#[cfg(windows)]
mod windows {
    use super::Desktop;
    use serde_json::Value;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETDESKWALLPAPER, SPI_SETDESKWALLPAPER, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE,
        SystemParametersInfoW,
    };

    pub struct Windows;

    fn set_path(path: &std::ffi::OsStr) -> Result<(), String> {
        let mut wide: Vec<u16> = path.encode_wide().chain(Some(0)).collect();
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_SETDESKWALLPAPER,
                0,
                wide.as_mut_ptr().cast(),
                SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
            )
        };
        if ok != 0 {
            Ok(())
        } else {
            Err(format!(
                "the desktop picture could not be set: {}",
                std::io::Error::last_os_error()
            ))
        }
    }

    impl Desktop for Windows {
        fn current(&self) -> Result<Value, String> {
            let mut buf = [0u16; 1024];
            let ok = unsafe {
                SystemParametersInfoW(
                    SPI_GETDESKWALLPAPER,
                    buf.len() as u32,
                    buf.as_mut_ptr().cast(),
                    0,
                )
            };
            if ok == 0 {
                return Err("the desktop picture could not be read".into());
            }
            let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Ok(Value::String(String::from_utf16_lossy(&buf[..n])))
        }

        fn set_file(&self, path: &Path) -> Result<(), String> {
            set_path(path.as_os_str())
        }

        fn put_back(&self, original: &Value) -> Result<(), String> {
            // An empty path is a plain colour, which setting it back restores too.
            let p = original.as_str().unwrap_or("");
            set_path(std::ffi::OsStr::new(p))
        }

        fn keep(&self, original: Value, folder: &Path) -> Value {
            // When the picture's own file has gone (a generated or moved image, or one put back
            // from the cache before), Windows names its cached copy, `Themes\TranscodedWallpaper`.
            // Setting the still overwrites that file, so saving its path would put the still back.
            // Seen on the Acer. Keep a copy of it instead.
            let Some(p) = original.as_str().map(Path::new) else {
                return original;
            };
            if !windows_owns(p) {
                return original;
            }
            let Ok(bytes) = std::fs::read(p) else {
                return original;
            };
            let ext = if bytes.starts_with(b"\x89PNG") {
                "png"
            } else if bytes.starts_with(b"BM") {
                "bmp"
            } else {
                "jpg"
            };
            remove_copies(folder);
            let copy = folder.join(format!("{}{}.{ext}", super::ORIGINAL, super::millis()));
            match std::fs::write(&copy, &bytes) {
                Ok(()) => Value::String(copy.to_string_lossy().into_owned()),
                Err(_) => original,
            }
        }
    }

    /// Whether `path` is in the folder where Windows keeps its own copies of the picture.
    fn windows_owns(path: &Path) -> bool {
        let Some(appdata) = std::env::var_os("APPDATA") else {
            return false;
        };
        let themes = Path::new(&appdata).join(r"Microsoft\Windows\Themes");
        let lower = |p: &Path| p.to_string_lossy().to_lowercase();
        lower(path).starts_with(&(lower(&themes) + r"\"))
    }

    /// Older copies of the user's picture, which no saved picture names any more.
    fn remove_copies(folder: &Path) {
        let Ok(entries) = std::fs::read_dir(folder) else {
            return;
        };
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with(super::ORIGINAL) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::Desktop;
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use serde_json::Value;
    use std::ffi::{CStr, CString, c_char};
    use std::path::Path;

    // Makes sure AppKit is loaded, whatever program this is.
    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {}

    pub struct Mac;

    fn class(name: &CStr) -> Result<&'static AnyClass, String> {
        AnyClass::get(name).ok_or_else(|| format!("{name:?} is missing"))
    }

    fn ns_string(s: &str) -> Result<Retained<AnyObject>, String> {
        let c = CString::new(s).map_err(|e| e.to_string())?;
        let s: Option<Retained<AnyObject>> =
            unsafe { msg_send![class(c"NSString")?, stringWithUTF8String: c.as_ptr()] };
        s.ok_or_else(|| "no string".into())
    }

    fn rust_string(s: &AnyObject) -> String {
        let p: *const c_char = unsafe { msg_send![s, UTF8String] };
        if p.is_null() {
            return String::new();
        }
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }

    /// The shared workspace and every screen.
    fn screens() -> Result<(Retained<AnyObject>, Vec<Retained<AnyObject>>), String> {
        let ws: Option<Retained<AnyObject>> =
            unsafe { msg_send![class(c"NSWorkspace")?, sharedWorkspace] };
        let ws = ws.ok_or("no workspace")?;
        let list: Option<Retained<AnyObject>> = unsafe { msg_send![class(c"NSScreen")?, screens] };
        let list = list.ok_or("no screens")?;
        let n: usize = unsafe { msg_send![&*list, count] };
        let mut v = Vec::with_capacity(n);
        for i in 0..n {
            let s: Option<Retained<AnyObject>> = unsafe { msg_send![&*list, objectAtIndex: i] };
            v.extend(s);
        }
        Ok((ws, v))
    }

    fn set_all(paths: &[String]) -> Result<(), String> {
        let (ws, screens) = screens()?;
        let options: Option<Retained<AnyObject>> =
            unsafe { msg_send![class(c"NSDictionary")?, dictionary] };
        let options = options.ok_or("no dictionary")?;
        for (i, screen) in screens.iter().enumerate() {
            let Some(path) = paths.get(i).or(paths.first()) else {
                continue;
            };
            let url: Option<Retained<AnyObject>> =
                unsafe { msg_send![class(c"NSURL")?, fileURLWithPath: &*ns_string(path)?] };
            let url = url.ok_or("no URL")?;
            let ok: Bool = unsafe {
                msg_send![&*ws, setDesktopImageURL: &*url, forScreen: &**screen,
                    options: &*options, error: std::ptr::null_mut::<*mut AnyObject>()]
            };
            if !ok.as_bool() {
                return Err(format!("the desktop picture could not be set to {path}"));
            }
        }
        Ok(())
    }

    impl Desktop for Mac {
        fn current(&self) -> Result<Value, String> {
            let (ws, screens) = screens()?;
            let mut paths = vec![];
            for s in &screens {
                let url: Option<Retained<AnyObject>> =
                    unsafe { msg_send![&*ws, desktopImageURLForScreen: &**s] };
                let path = url.and_then(|u| {
                    let p: Option<Retained<AnyObject>> = unsafe { msg_send![&*u, path] };
                    p.map(|p| rust_string(&p))
                });
                paths.push(Value::String(path.unwrap_or_default()));
            }
            Ok(Value::Array(paths))
        }

        fn set_file(&self, path: &Path) -> Result<(), String> {
            set_all(&[path.to_string_lossy().into_owned()])
        }

        fn after_set(&self, path: &Path) -> Value {
            // `desktopImageURLForScreen` gives the old picture for a while after it is set (seen
            // on macOS 27), which made a later restore think the user had picked a new one.
            let n = screens().map_or(1, |(_, s)| s.len().max(1));
            let p = Value::String(path.to_string_lossy().into_owned());
            Value::Array(vec![p; n])
        }

        fn put_back(&self, original: &Value) -> Result<(), String> {
            let paths: Vec<String> = original
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            if paths.iter().all(String::is_empty) {
                return Ok(());
            }
            set_all(&paths)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A desktop that only remembers what it was set to.
    struct Fake(RefCell<Value>);

    impl Desktop for Fake {
        fn current(&self) -> Result<Value, String> {
            Ok(self.0.borrow().clone())
        }
        fn set_file(&self, path: &Path) -> Result<(), String> {
            *self.0.borrow_mut() = Value::String(path.to_string_lossy().into_owned());
            Ok(())
        }
        fn put_back(&self, original: &Value) -> Result<(), String> {
            *self.0.borrow_mut() = original.clone();
            Ok(())
        }
    }

    fn folder(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jetcarp-picture-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_still_goes_up_and_the_users_picture_comes_back() {
        let dir = folder("back");
        let desk = Fake(RefCell::new(Value::String("mine.jpg".into())));
        let first = show_on(&desk, &dir, b"one").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let second = show_on(&desk, &dir, b"two").unwrap();
        assert!(!first.exists(), "an older still is removed");
        assert_eq!(std::fs::read(&second).unwrap(), b"two");
        assert_eq!(restore_on(&desk, &dir).unwrap(), Restored::PutBack);
        assert_eq!(
            *desk.0.borrow(),
            "mine.jpg",
            "the original, not the first still"
        );
        assert!(!second.exists() && !dir.join(SAVED).exists());
        assert_eq!(restore_on(&desk, &dir).unwrap(), Restored::Nothing);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_files_each_system_shows_are_read_from_its_description() {
        // GNOME, as `gsettings get` prints the keys; the dark picture is the same file.
        let gnome = serde_json::json!({
            "picture-uri": "'file:///home/a%20b/.config/jetcarp/desktop-pictures/still-1.png'",
            "picture-uri-dark": "'file:///home/a%20b/.config/jetcarp/desktop-pictures/still-1.png'",
            "picture-options": "'zoom'",
        });
        assert_eq!(
            files_named(&gnome),
            [PathBuf::from(
                "/home/a b/.config/jetcarp/desktop-pictures/still-1.png"
            )]
        );
        // macOS: one path per screen; an empty one (no picture) is skipped.
        let mac = serde_json::json!(["/Users/a/Pictures/x.heic", "", "/Users/a/Pictures/x.heic"]);
        assert_eq!(
            files_named(&mac),
            [PathBuf::from("/Users/a/Pictures/x.heic")]
        );
        // Windows: one path, or an empty one for a plain colour. (On Linux a Windows path is
        // not absolute, so only the empty case is checked here.)
        assert!(files_named(&Value::String(String::new())).is_empty());
        assert!(files_named(&Value::Null).is_empty());
        assert_eq!(unescape("%2Fa%zz%4"), "/a%zz%4");
    }

    #[test]
    fn a_picture_the_user_chose_since_is_left_alone() {
        let dir = folder("alone");
        let desk = Fake(RefCell::new(Value::String("mine.jpg".into())));
        show_on(&desk, &dir, b"one").unwrap();
        *desk.0.borrow_mut() = Value::String("newer.jpg".into());
        assert_eq!(restore_on(&desk, &dir).unwrap(), Restored::LeftAlone);
        assert_eq!(*desk.0.borrow(), "newer.jpg");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
