//! Starting the background host at login, and finding JETCARP's other programs.
//!
//! Each platform's own per-user mechanism: a `.desktop` file in `~/.config/autostart` on Linux, a
//! LaunchAgent on macOS (a login item needs a signed app bundle), and the `Run` registry key on
//! Windows. None needs administrator rights.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// JETCARP's program `name` beside this one.
pub fn sibling(name: &str) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let path = exe
        .parent()
        .ok_or("this program has no folder")?
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    if path.exists() {
        Ok(path)
    } else {
        Err(format!(
            "{} was not found. Build it with `cargo build --release -p {name}`.",
            path.display()
        ))
    }
}

/// Records where the host program is, for the things that start it or its siblings from
/// elsewhere: GNOME's menu, and the screen saver's copy of it.
pub fn record_host_program(exe: &Path) -> Result<(), String> {
    let dir = crate::host_settings::config_dir().ok_or("no configuration folder")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let file = dir.join("host-program");
    std::fs::write(&file, exe.display().to_string()).map_err(|e| format!("{}: {e}", file.display()))
}

/// The host program last recorded.
pub fn host_program() -> Option<PathBuf> {
    let file = crate::host_settings::config_dir()?.join("host-program");
    let text = std::fs::read_to_string(file).ok()?;
    Some(PathBuf::from(text.trim()))
}

#[cfg(windows)]
fn no_console(c: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    c.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn no_console(_: &mut Command) {}

/// Starts `path` so it outlives this program.
pub fn spawn_detached(path: &Path, args: &[&str]) -> Result<(), String> {
    let mut c = Command::new(path);
    c.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    no_console(&mut c);
    c.spawn()
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// The contents of a Linux autostart entry for `exe`.
pub fn desktop_entry(exe: &Path) -> String {
    // The Desktop Entry spec: inside quotes these four take a backslash, and % is doubled.
    let mut path = String::new();
    for c in exe.display().to_string().chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                path.push('\\');
                path.push(c);
            }
            '%' => path.push_str("%%"),
            _ => path.push(c),
        }
    }
    format!(
        "[Desktop Entry]\nType=Application\nName=JETCARP background host\nComment=Live cellular automata on the desktop\nExec=\"{path}\"\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The contents of a macOS LaunchAgent for `exe`.
pub fn launch_agent(exe: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LAUNCH_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#,
        xml_escape(&exe.display().to_string())
    )
}

const LAUNCH_LABEL: &str = "com.jimmieprodgers.jetcarp.host";

/// The file that makes the host start at login (Windows uses the registry).
#[cfg(not(windows))]
fn entry_file() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        Some(
            dirs::home_dir()?
                .join("Library/LaunchAgents")
                .join(format!("{LAUNCH_LABEL}.plist")),
        )
    } else {
        Some(dirs::config_dir()?.join("autostart/jetcarp-host.desktop"))
    }
}

#[cfg(windows)]
mod registry {
    use super::{Command, no_console};
    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "JETCARP";

    fn reg(args: &[&str]) -> std::io::Result<std::process::Output> {
        let mut c = Command::new("reg");
        c.args(args);
        no_console(&mut c);
        c.output()
    }

    pub fn enabled() -> bool {
        reg(&["query", KEY, "/v", NAME]).is_ok_and(|o| o.status.success())
    }

    pub fn set(on: bool, exe: &std::path::Path) -> Result<(), String> {
        let quoted = format!("\"{}\"", exe.display());
        let out = if on {
            reg(&["add", KEY, "/v", NAME, "/t", "REG_SZ", "/d", &quoted, "/f"])
        } else if enabled() {
            reg(&["delete", KEY, "/v", NAME, "/f"])
        } else {
            return Ok(());
        }
        .map_err(|e| format!("reg: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
}

/// Whether the host starts at login.
pub fn enabled() -> bool {
    #[cfg(windows)]
    {
        registry::enabled()
    }
    #[cfg(not(windows))]
    {
        entry_file().is_some_and(|p| p.exists())
    }
}

/// Makes `exe` start at login, or stops that. A host already running is left alone.
pub fn set(on: bool, exe: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        registry::set(on, exe)
    }
    #[cfg(not(windows))]
    {
        let path = entry_file().ok_or("no home folder on this system")?;
        if !on {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    Err(format!("{}: {e}", path.display()))
                }
                _ => Ok(()),
            };
        }
        let dir = path.parent().expect("a folder");
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let text = if cfg!(target_os = "macos") {
            launch_agent(exe)
        } else {
            desktop_entry(exe)
        };
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_quote_their_paths() {
        let d = desktop_entry(Path::new("/opt/my \"jetcarp\"/jetcarp-host"));
        assert!(d.contains(r#"Exec="/opt/my \"jetcarp\"/jetcarp-host""#), "{d}");
        assert!(d.starts_with("[Desktop Entry]\n"));
        let p = launch_agent(Path::new("/Users/a&b/jetcarp-host"));
        assert!(
            p.contains("<string>/Users/a&amp;b/jetcarp-host</string>"),
            "{p}"
        );
        assert!(p.contains("<key>RunAtLoad</key>"));
    }
}
