//! JETCARP among the system's apps, with its icon (#105): what `jetcarp launcher install` makes,
//! the installers ask for, and `jetcarp uninstall` removes. Each is the user's own, so no
//! administrator rights are needed:
//!
//! - **Linux**: a menu entry, `jetcarp.desktop` in `$XDG_DATA_HOME/applications` (usually
//!   `~/.local/share/applications`), and the icon at each size in the `hicolor` icon theme there
//!   (`icons/hicolor/<N>x<N>/apps/jetcarp.png`). The entry's name is the app's windows' app id,
//!   `jetcarp`, which is how GNOME on Wayland finds their icon.
//! - **macOS**: `JETCARP.app` in `~/Applications`, made as `jetcarp host saver install` makes
//!   `JETCARP.saver` ([`crate::macsaver`]): an `Info.plist`, the icon (`jetcarp.icns`), and in
//!   `Contents/MacOS` a launcher, a shell script that starts the program; then signed ad hoc on
//!   this Mac. The bundle holds a launcher, not a copy of the program, so an upgrade needs no new
//!   bundle, and the program finds what it keeps beside itself (the starter pack, the screen
//!   saver's code). No Developer ID or notarisation is needed for a bundle made on the Mac itself.
//! - **Windows**: a Start menu entry, `JETCARP.lnk` in the user's Start menu folder
//!   (`%APPDATA%\Microsoft\Windows\Start Menu\Programs`), starting `jetcarpw.exe` (no console
//!   window), whose icon it shows.
//!
//! Each starts this program by a path an upgrade keeps ([`program`]).

use std::path::{Path, PathBuf};

/// The menu entry's file name: the windows' app id, `jetcarp`, with `.desktop`.
pub const DESKTOP_FILE: &str = "jetcarp.desktop";
/// The icon's name in the icon theme.
pub const ICON_NAME: &str = "jetcarp";
/// The macOS app's name, which Finder, Launchpad, Spotlight and the Dock show.
pub const BUNDLE: &str = "JETCARP.app";
/// The bundle's identifier, by which an app JETCARP made is told from another of the same name.
pub const BUNDLE_ID: &str = "com.jimmieprodgers.jetcarp";
/// The Start menu entry's file name.
pub const SHORTCUT: &str = "JETCARP.lnk";
/// The launcher's name inside the bundle (`Contents/MacOS/JETCARP`).
const EXECUTABLE: &str = "JETCARP";
/// The key in the menu entry naming the version of JETCARP that made it.
const VERSION_KEY: &str = "X-JETCARP-Version";
/// What the menu entry, the app and the shortcut say JETCARP is.
const COMMENT: &str = "Cellular automata on almost any tiling of the plane";

/// Where this system keeps it, for people: "the applications menu", "Applications", "the Start
/// menu".
pub fn place() -> &'static str {
    if cfg!(target_os = "macos") {
        "Applications"
    } else if cfg!(windows) {
        "the Start menu"
    } else {
        "the applications menu"
    }
}

/// What `jetcarp uninstall` lists for removing it.
pub fn undo_what() -> String {
    if cfg!(target_os = "macos") {
        format!("remove JETCARP.app from {}", folder_text())
    } else if cfg!(windows) {
        "remove JETCARP from the Start menu".into()
    } else {
        "remove JETCARP from the applications menu (its menu entry and icons)".into()
    }
}

fn folder_text() -> String {
    files()
        .ok()
        .and_then(|f| {
            f.first()
                .and_then(|p| p.parent())
                .map(|p| p.display().to_string())
        })
        .unwrap_or_else(|| "~/Applications".into())
}

/// The program a launcher starts: this one, by a path an upgrade keeps. Homebrew keeps each
/// version in a folder of its own (`<prefix>/Cellar/jetcarp/<version>/bin/jetcarp`) and links the
/// current one into `<prefix>/bin`, so a launcher starts the link. On Windows it is
/// `jetcarpw.exe`, beside `jetcarp.exe`, which opens no console window (`jetcarp.exe` when it is
/// missing, as after a plain `cargo install`).
pub fn program() -> Result<PathBuf, String> {
    let exe = match crate::autostart::windows_launcher() {
        Some(w) => w,
        None => crate::autostart::jetcarp_exe()?,
    };
    let real = std::fs::canonicalize(&exe)
        .map(jetcarp_core::paths::plain_path)
        .unwrap_or(exe);
    Ok(lasting_path(&real, |p| std::fs::canonicalize(p).ok()))
}

/// `real`, or the link Homebrew keeps to it in `<prefix>/bin` when `real` is a version's own copy
/// and the link (as `resolve` follows it) leads to it.
pub fn lasting_path(real: &Path, resolve: impl Fn(&Path) -> Option<PathBuf>) -> PathBuf {
    let parts: Vec<&std::ffi::OsStr> = real.iter().collect();
    let n = parts.len();
    // <prefix> / Cellar / jetcarp / <version> / bin / <program>
    if n >= 6
        && parts[n - 5] == "Cellar"
        && parts[n - 4] == "jetcarp"
        && parts[n - 2] == "bin"
        && let Some(prefix) = real.ancestors().nth(5)
    {
        let link = prefix.join("bin").join(parts[n - 1]);
        if resolve(&link).as_deref() == Some(real) {
            return link;
        }
    }
    real.to_path_buf()
}

/// Every file it is made of on this system, the first being the one that says it is there (the
/// menu entry, the app, the shortcut).
pub fn files() -> Result<Vec<PathBuf>, String> {
    if cfg!(target_os = "macos") {
        Ok(vec![
            dirs::home_dir()
                .ok_or("no home folder")?
                .join("Applications")
                .join(BUNDLE),
        ])
    } else if cfg!(windows) {
        Ok(vec![start_menu()?.join(SHORTCUT)])
    } else {
        let data = dirs::data_dir().ok_or("no data folder (XDG_DATA_HOME or HOME)")?;
        let mut v = vec![data.join("applications").join(DESKTOP_FILE)];
        v.extend(icon_paths(&data.join("icons/hicolor")));
        Ok(v)
    }
}

/// The user's Start menu programs folder: `%APPDATA%\Microsoft\Windows\Start Menu\Programs`.
fn start_menu() -> Result<PathBuf, String> {
    let appdata = std::env::var_os("APPDATA")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::data_dir)
        .ok_or("no APPDATA folder")?;
    Ok(appdata.join(r"Microsoft\Windows\Start Menu\Programs"))
}

/// Whether it is there, made by JETCARP (a menu entry or an app of the same name that JETCARP did
/// not make is not counted, nor ever replaced or removed).
pub fn installed() -> bool {
    let Ok(files) = files() else {
        return false;
    };
    let first = &files[0];
    if cfg!(target_os = "macos") {
        bundle_is_ours(first)
    } else if cfg!(windows) {
        first.exists()
    } else {
        entry_is_ours(first)
    }
}

/// The icon theme's files, one for each of [`crate::icon::HICOLOR_SIZES`]:
/// `<hicolor>/<N>x<N>/apps/jetcarp.png`. (The sizes alone, so that only the program that writes
/// the icons carries them.)
fn icon_paths(hicolor: &Path) -> Vec<PathBuf> {
    crate::icon::HICOLOR_SIZES
        .iter()
        .map(|n| {
            hicolor
                .join(format!("{n}x{n}"))
                .join("apps")
                .join(format!("{ICON_NAME}.png"))
        })
        .collect()
}

/// A string value of a desktop entry, escaped as the spec asks (`\\`, `\n`, `\t`, `\r`).
fn desktop_string(s: &str) -> String {
    s.replace('\\', r"\\")
        .replace('\n', r"\n")
        .replace('\t', r"\t")
        .replace('\r', r"\r")
}

/// The Linux menu entry for `program`, following the Desktop Entry spec (1.5), which
/// `desktop-file-validate` checks. `TryExec` hides it if the program has gone.
pub fn desktop_entry(program: &Path) -> String {
    let path = program.display().to_string();
    let exec = crate::autostart::desktop_quote(&path);
    let version = env!("CARGO_PKG_VERSION");
    format!(
        "# Made by JETCARP {version} (`jetcarp launcher install`); `jetcarp uninstall` removes it.\n\
         [Desktop Entry]\n\
         Type=Application\n\
         Name=JETCARP\n\
         GenericName=Cellular automata\n\
         Comment={COMMENT}\n\
         Exec={exec}\n\
         TryExec={}\n\
         Icon={ICON_NAME}\n\
         Terminal=false\n\
         Categories=Science;Math;\n\
         Keywords=cellular automata;Game of Life;tilings;aperiodic;Penrose;\n\
         StartupNotify=true\n\
         StartupWMClass=jetcarp\n\
         {VERSION_KEY}={version}\n",
        desktop_string(&path)
    )
}

/// `s` as one word of a POSIX shell: in single quotes, with a quote inside closed, escaped and
/// opened again.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The bundle's launcher: a shell script that starts `program`, else a JETCARP found where the
/// installers and Homebrew put it (an app started from Finder has only the system's `PATH`), else
/// says it was not found. It passes no arguments: the app is `jetcarp` with none.
pub fn launcher_script(program: &Path) -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        r#"#!/bin/sh
# Opens JETCARP. Made by JETCARP {version} (`jetcarp launcher install`), which starts the program
# named first below; `jetcarp uninstall` removes this app.
for program in {} /opt/homebrew/bin/jetcarp /usr/local/bin/jetcarp "$HOME/.local/bin/jetcarp"; do
    if [ -x "$program" ]; then
        exec "$program"
    fi
done
/usr/bin/osascript -e 'display alert "JETCARP was not found" message "Install JETCARP again, then run `jetcarp launcher install` to make this app anew."' >/dev/null 2>&1
exit 1
"#,
        sh_quote(&program.display().to_string())
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The app bundle's `Info.plist`: its name, identifier, launcher, icon and version.
pub fn info_plist() -> String {
    let version = xml_escape(env!("CARGO_PKG_VERSION"));
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleDisplayName</key>
	<string>JETCARP</string>
	<key>CFBundleExecutable</key>
	<string>{EXECUTABLE}</string>
	<key>CFBundleGetInfoString</key>
	<string>{COMMENT}</string>
	<key>CFBundleIconFile</key>
	<string>{ICON_NAME}</string>
	<key>CFBundleIdentifier</key>
	<string>{BUNDLE_ID}</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>JETCARP</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>{version}</string>
	<key>CFBundleVersion</key>
	<string>{version}</string>
	<key>LSApplicationCategoryType</key>
	<string>public.app-category.education</string>
	<key>LSMinimumSystemVersion</key>
	<string>11.0</string>
	<key>NSHighResolutionCapable</key>
	<true/>
</dict>
</plist>
"#
    )
}

/// Whether the bundle at `dest` is one JETCARP made (its `Info.plist` names [`BUNDLE_ID`]).
pub fn bundle_is_ours(dest: &Path) -> bool {
    std::fs::read_to_string(dest.join("Contents/Info.plist"))
        .is_ok_and(|t| t.contains(&format!("<string>{BUNDLE_ID}</string>")))
}

/// Writes the app bundle at `dest`, starting `program`, with `icns` as its icon. A bundle there
/// already is replaced if JETCARP made it, and refused otherwise.
pub fn write_bundle(dest: &Path, program: &Path, icns: &[u8]) -> Result<(), String> {
    let err = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    if std::fs::symlink_metadata(dest).is_ok() {
        if !bundle_is_ours(dest) {
            return Err(format!(
                "{} is there already and JETCARP did not make it; move it away first",
                dest.display()
            ));
        }
        std::fs::remove_dir_all(dest).map_err(|e| err(dest, e))?;
    }
    let contents = dest.join("Contents");
    let (macos, resources) = (contents.join("MacOS"), contents.join("Resources"));
    for d in [&macos, &resources] {
        std::fs::create_dir_all(d).map_err(|e| err(d, e))?;
    }
    let plist = contents.join("Info.plist");
    std::fs::write(&plist, info_plist()).map_err(|e| err(&plist, e))?;
    let pkginfo = contents.join("PkgInfo");
    std::fs::write(&pkginfo, "APPL????").map_err(|e| err(&pkginfo, e))?;
    let exe = macos.join(EXECUTABLE);
    std::fs::write(&exe, launcher_script(program)).map_err(|e| err(&exe, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| err(&exe, e))?;
    }
    let icon = resources.join(format!("{ICON_NAME}.icns"));
    std::fs::write(&icon, icns).map_err(|e| err(&icon, e))?;
    Ok(())
}

/// Whether the menu entry at `path` is one JETCARP wrote (it has the [`VERSION_KEY`] key).
pub fn entry_is_ours(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|t| t.lines().any(|l| l.starts_with(VERSION_KEY)))
}

/// Writes the Linux menu entry for `program` and the icons, under the data folder `data`
/// (`~/.local/share`). Returns the files written. An entry there already is replaced if JETCARP
/// wrote it, and refused otherwise.
pub fn write_menu_entry(data: &Path, program: &Path) -> Result<Vec<PathBuf>, String> {
    let err = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    let mut written = vec![];
    let entry = data.join("applications").join(DESKTOP_FILE);
    if entry.exists() && !entry_is_ours(&entry) {
        return Err(format!(
            "{} is there already and JETCARP did not make it; move it away first",
            entry.display()
        ));
    }
    let icons = data.join("icons/hicolor");
    for (path, (_, png)) in icon_paths(&icons).into_iter().zip(crate::icon::HICOLOR) {
        let dir = path.parent().unwrap_or(&icons);
        std::fs::create_dir_all(dir).map_err(|e| err(dir, e))?;
        std::fs::write(&path, png).map_err(|e| err(&path, e))?;
        written.push(path);
    }
    // The icon theme's folder, marked as changed, so that desktops that keep a list of its icons
    // look at it again.
    if let Ok(f) = std::fs::File::open(&icons) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
    // The entry last, so that it never names an icon that is not there yet.
    let dir = entry.parent().unwrap_or(data);
    std::fs::create_dir_all(dir).map_err(|e| err(dir, e))?;
    std::fs::write(&entry, desktop_entry(program)).map_err(|e| err(&entry, e))?;
    written.insert(0, entry);
    Ok(written)
}

/// What [`install`] did.
pub struct Installed {
    /// The menu entry, the app or the shortcut.
    pub path: PathBuf,
    /// The program it starts.
    pub program: PathBuf,
    /// What it did, in a sentence for people.
    pub said: String,
}

/// Makes it on this system for this program, replacing one made before.
pub fn install() -> Result<Installed, String> {
    let program = program()?;
    let files = files()?;
    let path = files[0].clone();
    #[cfg(target_os = "macos")]
    let said = {
        write_bundle(&path, &program, crate::icon::ICNS)?;
        crate::macsaver::sign(&path)?;
        format!(
            "made {}, signed on this Mac: open JETCARP from Launchpad, Spotlight or that folder, \
             and keep it in the Dock from there",
            path.display()
        )
    };
    #[cfg(windows)]
    let said = {
        let dir = path.parent().ok_or("no Start menu folder")?;
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        shortcut::make(&path, &program)?;
        format!("added JETCARP to the Start menu ({})", path.display())
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let said = {
        let data = dirs::data_dir().ok_or("no data folder (XDG_DATA_HOME or HOME)")?;
        write_menu_entry(&data, &program)?;
        format!(
            "added JETCARP to the applications menu ({}), with its icon in {}",
            path.display(),
            data.join("icons/hicolor").display()
        )
    };
    Ok(Installed {
        path,
        program,
        said,
    })
}

/// Removes it, if it is there and JETCARP made it. Returns what was removed (none: nothing was
/// there).
pub fn remove() -> Result<Vec<PathBuf>, String> {
    let files = files()?;
    let mut removed = vec![];
    if cfg!(target_os = "macos") {
        let dest = &files[0];
        if dest.exists() {
            if !bundle_is_ours(dest) {
                return Err(format!(
                    "{} was not made by JETCARP, so it is left as it is",
                    dest.display()
                ));
            }
            std::fs::remove_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
            removed.push(dest.clone());
        }
        return Ok(removed);
    }
    if !cfg!(windows) && files[0].exists() && !entry_is_ours(&files[0]) {
        return Err(format!(
            "{} was not made by JETCARP, so it is left as it is, with the icons named after it",
            files[0].display()
        ));
    }
    for f in &files {
        match std::fs::remove_file(f) {
            Ok(()) => removed.push(f.clone()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", f.display())),
        }
    }
    // The icon theme's size folders this left empty (`<N>x<N>/apps`, then `<N>x<N>`).
    for f in files.iter().skip(1) {
        for dir in f.ancestors().skip(1).take(2) {
            let _ = std::fs::remove_dir(dir);
        }
    }
    Ok(removed)
}

/// One line for people.
pub fn describe() -> String {
    match files() {
        Err(e) => format!("JETCARP cannot be added to {} here: {e}.", place()),
        Ok(f) if installed() => format!("JETCARP is in {}: {}.", place(), f[0].display()),
        Ok(f) if f[0].exists() => format!(
            "JETCARP is not in {}: {} is there, but JETCARP did not make it.",
            place(),
            f[0].display()
        ),
        Ok(_) => format!(
            "JETCARP is not in {}; `jetcarp launcher install` adds it.",
            place()
        ),
    }
}

/// The Start menu shortcut, through the shell's own `IShellLinkW` (COM), which the `windows`
/// crate already in the tree provides.
#[cfg(windows)]
mod shortcut {
    use std::path::Path;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize, IPersistFile,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::{HSTRING, Interface};

    /// Writes the shortcut `lnk` to `target`, which shows the target's own icon and starts in its
    /// folder.
    pub fn make(lnk: &Path, target: &Path) -> Result<(), String> {
        // S_FALSE (COM already on for this thread) is balanced too.
        let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
        let r = (|| -> windows::core::Result<()> {
            let link: IShellLinkW =
                unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }?;
            unsafe {
                link.SetPath(&HSTRING::from(target))?;
                link.SetDescription(&HSTRING::from(super::COMMENT))?;
                link.SetIconLocation(&HSTRING::from(target), 0)?;
                if let Some(dir) = target.parent() {
                    link.SetWorkingDirectory(&HSTRING::from(dir))?;
                }
            }
            let file: IPersistFile = link.cast()?;
            unsafe { file.Save(&HSTRING::from(lnk), true) }
        })();
        if com {
            unsafe { CoUninitialize() };
        }
        r.map_err(|e| format!("{}: {e}", lnk.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn homebrew_s_link_is_started_where_it_leads_to_this_version() {
        let real = Path::new("/opt/homebrew/Cellar/jetcarp/0.4.3/bin/jetcarp");
        let link = Path::new("/opt/homebrew/bin/jetcarp");
        let resolve = |p: &Path| (p == link).then(|| real.to_path_buf());
        assert_eq!(lasting_path(real, resolve), link);
        // A link elsewhere, or none: the program itself.
        assert_eq!(lasting_path(real, |_| None), real);
        let other = Path::new("/opt/homebrew/Cellar/jetcarp/0.4.2/bin/jetcarp");
        assert_eq!(
            lasting_path(real, |p: &Path| (p == link).then(|| other.to_path_buf())),
            real
        );
        // Not Homebrew's.
        let installer = Path::new("/home/ada/.local/lib/jetcarp/jetcarp");
        assert_eq!(lasting_path(installer, |_| None), installer);
        let linuxbrew = Path::new("/home/linuxbrew/.linuxbrew/Cellar/jetcarp/0.4.3/bin/jetcarp");
        let lb_link = Path::new("/home/linuxbrew/.linuxbrew/bin/jetcarp");
        assert_eq!(
            lasting_path(linuxbrew, |p: &Path| (p == lb_link)
                .then(|| linuxbrew.to_path_buf())),
            lb_link
        );
    }

    #[test]
    fn the_menu_entry_quotes_the_program_and_names_the_icon() {
        let e = desktop_entry(Path::new("/home/ada/my apps/jet\"carp"));
        assert!(
            e.contains("\nExec=\"/home/ada/my apps/jet\\\"carp\"\n"),
            "{e}"
        );
        assert!(e.contains("\nTryExec=/home/ada/my apps/jet\"carp\n"), "{e}");
        assert!(e.contains("\nIcon=jetcarp\n"));
        assert!(e.contains("\nStartupWMClass=jetcarp\n"));
        assert!(e.contains(&format!(
            "\nX-JETCARP-Version={}\n",
            env!("CARGO_PKG_VERSION")
        )));
        assert!(e.lines().nth(1) == Some("[Desktop Entry]"));
    }

    #[test]
    fn the_launcher_script_quotes_the_program_and_starts_it_alone() {
        let s = launcher_script(Path::new("/Users/o'neil/.local/lib/jetcarp/jetcarp"));
        assert!(s.starts_with("#!/bin/sh\n"));
        assert!(
            s.contains("for program in '/Users/o'\\''neil/.local/lib/jetcarp/jetcarp' /opt"),
            "{s}"
        );
        assert!(s.contains("exec \"$program\"\n"), "no arguments: {s}");
    }

    /// The bundle as `jetcarp launcher install` writes it on a Mac (signing aside), here in a
    /// scratch folder on any system.
    #[test]
    fn the_bundle_has_its_launcher_icon_and_plist_and_replaces_only_its_own() {
        let dir = std::env::temp_dir().join(format!("jetcarp-launcher-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let dest = dir.join(BUNDLE);
        let program = Path::new("/opt/homebrew/bin/jetcarp");
        write_bundle(&dest, program, crate::icon::ICNS).unwrap();
        // Again, as an install over an earlier one does.
        write_bundle(&dest, program, crate::icon::ICNS).unwrap();
        let plist = std::fs::read_to_string(dest.join("Contents/Info.plist")).unwrap();
        for want in [
            "<string>APPL</string>",
            "<string>JETCARP</string>",
            "<string>com.jimmieprodgers.jetcarp</string>",
            "<key>CFBundleIconFile</key>\n\t<string>jetcarp</string>",
        ] {
            assert!(plist.contains(want), "{want}: {plist}");
        }
        assert!(bundle_is_ours(&dest));
        let script = std::fs::read_to_string(dest.join("Contents/MacOS/JETCARP")).unwrap();
        assert!(script.contains("'/opt/homebrew/bin/jetcarp'"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dest.join("Contents/MacOS/JETCARP"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o755);
        }
        assert_eq!(
            std::fs::read(dest.join("Contents/Resources/jetcarp.icns")).unwrap(),
            crate::icon::ICNS
        );
        // Someone else's JETCARP.app is left alone.
        std::fs::write(dest.join("Contents/Info.plist"), "<plist/>").unwrap();
        assert!(write_bundle(&dest, program, crate::icon::ICNS).is_err());
        assert_eq!(
            std::fs::read_to_string(dest.join("Contents/Info.plist")).unwrap(),
            "<plist/>"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_menu_entry_comes_with_every_icon_size() {
        let dir = std::env::temp_dir().join(format!("jetcarp-menu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let written = write_menu_entry(&dir, Path::new("/usr/bin/jetcarp")).unwrap();
        assert_eq!(written[0], dir.join("applications/jetcarp.desktop"));
        assert_eq!(written.len(), 1 + 8);
        for (n, png) in crate::icon::HICOLOR {
            let p = dir.join(format!("icons/hicolor/{n}x{n}/apps/jetcarp.png"));
            assert_eq!(std::fs::read(&p).unwrap(), png, "{}", p.display());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
