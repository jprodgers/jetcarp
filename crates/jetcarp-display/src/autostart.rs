//! Starting the background host at login, and the commands that start JETCARP's parts.
//!
//! JETCARP is one program: `jetcarp` opens the app, `jetcarp host serve` runs the background host.
//! On Windows a small windowless launcher, `jetcarpw.exe`, sits beside `jetcarp.exe` so that
//! shortcuts, login and the `.scr` screen saver start it without a console window.
//!
//! Login uses each platform's own per-user mechanism: a `.desktop` file in `~/.config/autostart`
//! on Linux, a LaunchAgent on macOS (a login item needs a signed app bundle), and the `Run`
//! registry key on Windows. None needs administrator rights.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A program and its arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl ProgramCommand {
    /// Starts it so it outlives this program.
    pub fn spawn_detached(&self) -> Result<(), String> {
        let args: Vec<&str> = self.args.iter().map(String::as_str).collect();
        spawn_detached(&self.program, &args)
    }

    /// The command as one Windows command line (each part quoted when it needs it).
    pub fn command_line(&self) -> String {
        let mut parts = vec![format!("\"{}\"", self.program.display())];
        parts.extend(self.args.iter().map(|a| quote_arg(a)));
        parts.join(" ")
    }
}

/// One argument of a Windows command line, as programs split it again (`CommandLineToArgvW` and
/// the C runtime): quoted when it holds a space, a tab or a quote, or is empty; inside quotes a
/// quote takes a backslash, and so does each backslash before a quote or before the closing one.
fn quote_arg(s: &str) -> String {
    if !s.is_empty() && !s.contains([' ', '\t', '"']) {
        return s.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in s.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
}

/// The `jetcarp` executable: this program, or the `jetcarp` beside it when this is the Windows
/// launcher or a `.scr` copy of it.
pub fn jetcarp_exe() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let stem = exe.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let is_launcher = stem.eq_ignore_ascii_case("jetcarpw")
        || exe
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("scr"));
    if !is_launcher {
        return Ok(exe);
    }
    let main = exe.with_file_name(format!("jetcarp{}", std::env::consts::EXE_SUFFIX));
    if main.exists() {
        Ok(main)
    } else {
        Err(format!("{} was not found", main.display()))
    }
}

/// The windowless launcher `jetcarpw.exe` beside `jetcarp.exe` (Windows only; `None` elsewhere or
/// when it is not installed, as after a plain `cargo install`).
pub fn windows_launcher() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let p = jetcarp_exe().ok()?.with_file_name("jetcarpw.exe");
    p.exists().then_some(p)
}

/// `jetcarp <args>`, through the windowless launcher on Windows when it is installed.
pub fn self_command(args: &[&str]) -> Result<ProgramCommand, String> {
    let program = match windows_launcher() {
        Some(l) => l,
        None => jetcarp_exe()?,
    };
    Ok(ProgramCommand {
        program,
        args: args.iter().map(|a| a.to_string()).collect(),
    })
}

/// The command that starts the background host: `jetcarp host serve`.
pub fn host_command() -> Result<ProgramCommand, String> {
    self_command(&["host", "serve"])
}

/// The command the login entry runs: `jetcarp host serve --login`, which shows only the screens
/// placed on the desktop. (An entry written before `--login` existed is removed all the same: the
/// entry is found by its name or file, not by its command.)
pub fn login_command() -> Result<ProgramCommand, String> {
    self_command(&["host", "serve", "--login"])
}

/// The command that opens the app: `jetcarp` with no arguments.
pub fn app_command() -> Result<ProgramCommand, String> {
    self_command(&[])
}

/// Records how to start the host, for the things that start it from elsewhere: GNOME's menu and
/// the Windows screen saver. The file holds the program on its first line and one argument per
/// line after it.
pub fn record_host_program(cmd: &ProgramCommand) -> Result<(), String> {
    let dir = crate::host_settings::config_dir().ok_or("no configuration folder")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let file = dir.join("host-program");
    let mut text = cmd.program.display().to_string();
    for a in &cmd.args {
        text.push('\n');
        text.push_str(a);
    }
    text.push('\n');
    std::fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))
}

/// The host command last recorded.
pub fn host_program() -> Option<ProgramCommand> {
    let file = crate::host_settings::config_dir()?.join("host-program");
    parse_host_program(&std::fs::read_to_string(file).ok()?)
}

fn parse_host_program(text: &str) -> Option<ProgramCommand> {
    let mut lines = text.lines().map(str::trim_end).filter(|l| !l.is_empty());
    let program = PathBuf::from(lines.next()?);
    Some(ProgramCommand {
        program,
        args: lines.map(str::to_string).collect(),
    })
}

#[cfg(windows)]
fn no_console(c: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    c.creation_flags(CREATE_NO_WINDOW);
}

/// Starts `path` so it outlives this program, with nothing of this program's: standard input,
/// output and error are the null device, and no other handle or descriptor goes with it. So
/// `jetcarp host start | tee log`, `$(...)` and a subprocess capturing the output end when the
/// command does, not when the host it started stops.
#[cfg(not(windows))]
pub fn spawn_detached(path: &Path, args: &[&str]) -> Result<(), String> {
    // Every descriptor Rust opens is closed on exec, and these three are replaced.
    Command::new(path)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Windows: as elsewhere. The standard library's `Command` lets the child inherit every handle
/// this process may pass on, including a pipe it was given as its own output (Git Bash's `| tee`
/// waited for the host), so the process is made here, inheriting only the null device given as
/// its standard handles (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`).
#[cfg(windows)]
pub fn spawn_detached(path: &Path, args: &[&str]) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Threading::{
        CREATE_NO_WINDOW, CreateProcessW, DeleteProcThreadAttributeList,
        EXTENDED_STARTUPINFO_PRESENT, InitializeProcThreadAttributeList,
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_INFORMATION, STARTF_USESTDHANDLES,
        STARTUPINFOEXW, UpdateProcThreadAttribute,
    };
    let err = |what: &str| {
        format!(
            "{}: {what}: {}",
            path.display(),
            std::io::Error::last_os_error()
        )
    };
    let wide = |s: &std::ffi::OsStr| -> Vec<u16> { s.encode_wide().chain(Some(0)).collect() };
    let cmd = ProgramCommand {
        program: path.to_path_buf(),
        args: args.iter().map(|a| a.to_string()).collect(),
    };
    let app = wide(path.as_os_str());
    let mut line = wide(std::ffi::OsStr::new(&cmd.command_line()));
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    };
    let nul_name = wide(std::ffi::OsStr::new("NUL"));
    let nul: HANDLE = unsafe {
        CreateFileW(
            nul_name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &sa,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if nul == INVALID_HANDLE_VALUE {
        return Err(err("the null device"));
    }
    // The attribute list, in memory aligned for the pointers it holds.
    let mut size = 0usize;
    unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut size) };
    let mut buf = vec![0usize; size.div_ceil(std::mem::size_of::<usize>()).max(1)];
    let list: windows_sys::Win32::System::Threading::LPPROC_THREAD_ATTRIBUTE_LIST =
        buf.as_mut_ptr().cast();
    let result = (|| {
        if unsafe { InitializeProcThreadAttributeList(list, 1, 0, &mut size) } == 0 {
            return Err(err("attribute list"));
        }
        let handles = [nul];
        let set = unsafe {
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr().cast(),
                std::mem::size_of_val(&handles),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        let started = set != 0 && {
            let mut si = STARTUPINFOEXW::default();
            si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
            si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            si.StartupInfo.hStdInput = nul;
            si.StartupInfo.hStdOutput = nul;
            si.StartupInfo.hStdError = nul;
            si.lpAttributeList = list;
            let mut pi = PROCESS_INFORMATION::default();
            let ok = unsafe {
                CreateProcessW(
                    app.as_ptr(),
                    line.as_mut_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    1,
                    EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW,
                    std::ptr::null(),
                    std::ptr::null(),
                    &si.StartupInfo,
                    &mut pi,
                )
            };
            if ok != 0 {
                unsafe {
                    CloseHandle(pi.hThread);
                    CloseHandle(pi.hProcess);
                }
            }
            ok != 0
        };
        let e = (!started).then(|| err("could not start it"));
        unsafe { DeleteProcThreadAttributeList(list) };
        e.map_or(Ok(()), Err)
    })();
    unsafe { CloseHandle(nul) };
    result
}

/// One `Exec` argument of a desktop entry, quoted as the Desktop Entry spec asks: inside quotes
/// these four take a backslash, and `%` is doubled.
fn desktop_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '%' => out.push_str("%%"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The contents of a Linux autostart entry for `cmd`.
pub fn desktop_entry(cmd: &ProgramCommand) -> String {
    let mut exec = desktop_quote(&cmd.program.display().to_string());
    for a in &cmd.args {
        exec.push(' ');
        exec.push_str(&desktop_quote(a));
    }
    format!(
        "[Desktop Entry]\nType=Application\nName=JETCARP background host\nComment=Live cellular automata on the desktop\nExec={exec}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The contents of a macOS LaunchAgent for `cmd`.
pub fn launch_agent(cmd: &ProgramCommand) -> String {
    let mut args = format!(
        "        <string>{}</string>\n",
        xml_escape(&cmd.program.display().to_string())
    );
    for a in &cmd.args {
        args.push_str(&format!("        <string>{}</string>\n", xml_escape(a)));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LAUNCH_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
{args}    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#
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

    pub fn set(on: bool, cmd: &super::ProgramCommand) -> Result<(), String> {
        let quoted = cmd.command_line();
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

/// Makes `cmd` start at login, or stops that. A host already running is left alone.
pub fn set(on: bool, cmd: &ProgramCommand) -> Result<(), String> {
    #[cfg(windows)]
    {
        registry::set(on, cmd)
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
            launch_agent(cmd)
        } else {
            desktop_entry(cmd)
        };
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(program: &str) -> ProgramCommand {
        ProgramCommand {
            program: PathBuf::from(program),
            args: vec!["host".into(), "serve".into()],
        }
    }

    #[test]
    fn entries_quote_their_paths() {
        let d = desktop_entry(&cmd("/opt/my \"jetcarp\"/jetcarp"));
        assert!(
            d.contains(r#"Exec="/opt/my \"jetcarp\"/jetcarp" "host" "serve""#),
            "{d}"
        );
        assert!(d.starts_with("[Desktop Entry]\n"));
        let p = launch_agent(&cmd("/Users/a&b/jetcarp"));
        assert!(p.contains("<string>/Users/a&amp;b/jetcarp</string>"), "{p}");
        assert!(
            p.contains("<string>host</string>\n        <string>serve</string>"),
            "{p}"
        );
        assert!(p.contains("<key>RunAtLoad</key>"));
    }

    #[test]
    fn windows_command_lines_quote_what_needs_it() {
        let mut c = cmd(r"C:\Program Files\JETCARP\jetcarpw.exe");
        assert_eq!(
            c.command_line(),
            r#""C:\Program Files\JETCARP\jetcarpw.exe" host serve"#
        );
        c.args.push("a b".into());
        assert!(c.command_line().ends_with(r#" "a b""#));
        // As `CommandLineToArgvW` reads them back.
        assert_eq!(quote_arg(""), r#""""#);
        assert_eq!(quote_arg(r"C:\dir\x"), r"C:\dir\x");
        assert_eq!(quote_arg(r"a\b c\"), r#""a\b c\\""#);
        assert_eq!(quote_arg(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote_arg(r#"a\"b"#), r#""a\\\"b""#);
    }

    #[test]
    fn host_program_files_round_trip() {
        let c = cmd("/usr/bin/jetcarp");
        let text = "/usr/bin/jetcarp\nhost\nserve\n";
        assert_eq!(parse_host_program(text), Some(c));
        // A file from before the arguments were recorded: the program alone.
        let old = parse_host_program("/usr/bin/jetcarp-host\n").unwrap();
        assert!(old.args.is_empty());
    }
}
