//! Starting the background host at login, and the commands that start JETCARP's parts.
//!
//! JETCARP is one program: `jetcarp` opens the app, `jetcarp host serve` runs the background host.
//! On Windows a small windowless launcher, `jetcarpw.exe`, sits beside `jetcarp.exe` so that
//! shortcuts, login and the `.scr` screen saver start it without a console window.
//!
//! Login uses each platform's own per-user mechanism: a `.desktop` file in `~/.config/autostart`
//! on Linux, a LaunchAgent on macOS (a login item needs a signed app bundle), and the `Run`
//! registry key on Windows. None needs administrator rights. On macOS the LaunchAgent is also
//! registered with launchd, and again after an upgrade ([`refresh`]): launchd refuses at login a
//! program whose signature changed since it was registered (#99).

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

/// The arguments of the login entry's command.
const LOGIN_ARGS: [&str; 3] = ["host", "serve", "--login"];

/// The command the login entry runs: `jetcarp host serve --login`, which shows only the screens
/// placed on the desktop. (An entry written before `--login` existed is removed all the same: the
/// entry is found by its name or file, not by its command.)
pub fn login_command() -> Result<ProgramCommand, String> {
    self_command(&LOGIN_ARGS)
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

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// How a LaunchAgent names the version of JETCARP that registered it with launchd: in a comment,
/// since launchd.plist(5) doesn't say what launchd does with a key it doesn't know, while the
/// property list format skips comments.
const REGISTERED_BY: &str = "<!-- Registered with launchd by JETCARP ";

/// The contents of a macOS LaunchAgent for `cmd`. `registered_by` is the version of JETCARP that
/// registers it with launchd; with none, the next [`refresh`] registers it.
pub fn launch_agent(cmd: &ProgramCommand, registered_by: Option<&str>) -> String {
    let mut args = format!(
        "        <string>{}</string>\n",
        xml_escape(&cmd.program.display().to_string())
    );
    for a in &cmd.args {
        args.push_str(&format!("        <string>{}</string>\n", xml_escape(a)));
    }
    let mark = registered_by
        .map(|v| format!("    {REGISTERED_BY}{v} -->\n"))
        .unwrap_or_default();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
{mark}    <key>Label</key>
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

/// The version of JETCARP that registered the LaunchAgent `text` with launchd, if one did.
pub fn agent_registered_by(text: &str) -> Option<&str> {
    let rest = &text[text.find(REGISTERED_BY)? + REGISTERED_BY.len()..];
    Some(rest[..rest.find("-->")?].trim()).filter(|v| !v.is_empty())
}

/// The command the LaunchAgent `text` runs: its `ProgramArguments`.
pub fn agent_command(text: &str) -> Option<ProgramCommand> {
    let rest = &text[text.find("<key>ProgramArguments</key>")?..];
    let rest = &rest[rest.find("<array>")? + "<array>".len()..];
    let mut array = &rest[..rest.find("</array>")?];
    let mut parts = vec![];
    while let Some(i) = array.find("<string>") {
        let s = &array[i + "<string>".len()..];
        let end = s.find("</string>")?;
        parts.push(xml_unescape(&s[..end]));
        array = &s[end + "</string>".len()..];
    }
    let mut parts = parts.into_iter();
    Some(ProgramCommand {
        program: PathBuf::from(parts.next()?),
        args: parts.collect(),
    })
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

/// Makes `cmd` start at login, or stops that. A host already running is left alone. Starting
/// at login is refused until the photosensitive seizure warning has been accepted (#96). On macOS
/// this only writes the LaunchAgent, which the next [`refresh`] registers with launchd; [`enable`]
/// registers it at once.
pub fn set(on: bool, cmd: &ProgramCommand) -> Result<(), String> {
    if on {
        jetcarp_core::photosensitivity::require("start the background host at login")?;
    }
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
            launch_agent(cmd, None)
        } else {
            desktop_entry(cmd)
        };
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// What [`enable`] did with launchd.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launchd {
    /// Nothing: only macOS registers its login item with launchd.
    NotUsed,
    /// Registered: launchd has started the job, a new host, unless one is running already (that
    /// one holds the host's single-instance lock, so the job's ends at once). A host launchd had
    /// started from the item before was stopped first.
    Registered,
    /// Not registered, and why: the file written is not the one launchd reads at login.
    Skipped(String),
}

/// Makes `cmd` start at login, as [`set`] does, and on macOS registers it with launchd now, so the
/// item works without waiting for a login and launchd takes this program as it is now (#99).
/// Refused, as `set` is, until the photosensitive seizure warning has been accepted.
pub fn enable(cmd: &ProgramCommand) -> Result<Launchd, String> {
    #[cfg(target_os = "macos")]
    {
        jetcarp_core::photosensitivity::require("start the background host at login")?;
        let domain = match launchctl::domain() {
            Ok(d) => d,
            Err(why) => return set(true, cmd).map(|()| Launchd::Skipped(why)),
        };
        let path = entry_file().ok_or("no home folder on this system")?;
        launchctl::write_and_register(&path, cmd, env!("CARGO_PKG_VERSION"), |p| {
            launchctl::register(&domain, p)
        })?;
        Ok(Launchd::Registered)
    }
    #[cfg(not(target_os = "macos"))]
    {
        set(true, cmd).map(|()| Launchd::NotUsed)
    }
}

/// After an upgrade on macOS, brings the login item up to this program, so that the host still
/// starts at login (#99). launchd keeps the code requirement of the program a LaunchAgent runs,
/// taken when it was registered or last started, and at login refuses to start a program that has
/// changed since: Homebrew's builds are signed ad hoc, so each version has a signature of its own.
/// When the host starts at login and its LaunchAgent was registered by another version of JETCARP,
/// or by none, this writes it again for this program, with the same arguments, and registers it
/// with launchd again ([`enable`]'s way), which stops a host launchd started from the old one and
/// starts this program. Nothing to do when the host doesn't start at login or this version
/// registered it, nor on other systems. Returns what it did, in words, when it did something.
pub fn refresh() -> Result<Option<String>, String> {
    #[cfg(target_os = "macos")]
    {
        let Some(path) = entry_file().filter(|p| p.exists()) else {
            return Ok(None);
        };
        // A LaunchAgent outside this account's home folder isn't the one launchd reads at login.
        let Ok(domain) = launchctl::domain() else {
            return Ok(None);
        };
        launchctl::refresh_at(&path, &jetcarp_exe()?, env!("CARGO_PKG_VERSION"), |p| {
            launchctl::register(&domain, p)
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(None)
    }
}

/// Registering the LaunchAgent with launchd: macOS only, but the steps are tested everywhere, with a
/// stand-in for launchctl and a scratch file.
#[cfg(any(target_os = "macos", test))]
mod launchctl {
    use super::{
        LAUNCH_LABEL, LOGIN_ARGS, ProgramCommand, agent_command, agent_registered_by, launch_agent,
    };
    use std::path::Path;
    use std::time::Duration;

    /// What launchctl answered: Ok, or its exit status and what it said.
    pub type Answer = Result<(), (i32, String)>;

    /// Writes the LaunchAgent `path` for `cmd`, marked as registered by `version`, and registers it
    /// through `register`. If that fails the mark comes off again, so the next refresh tries again.
    pub fn write_and_register(
        path: &Path,
        cmd: &ProgramCommand,
        version: &str,
        register: impl FnOnce(&Path) -> Result<(), String>,
    ) -> Result<(), String> {
        let write = |text: String| -> Result<(), String> {
            let dir = path.parent().expect("a folder");
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
        };
        write(launch_agent(cmd, Some(version)))?;
        register(path).inspect_err(|_| {
            let _ = write(launch_agent(cmd, None));
        })
    }

    /// [`super::refresh`] for the LaunchAgent `path`, `exe` being this program and `version` its
    /// version, registering through `register`.
    pub fn refresh_at(
        path: &Path,
        exe: &Path,
        version: &str,
        register: impl FnOnce(&Path) -> Result<(), String>,
    ) -> Result<Option<String>, String> {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Ok(None);
        };
        let was = agent_registered_by(&text);
        if was == Some(version) {
            return Ok(None);
        }
        // The same arguments, unless they aren't a host's (an entry for another program).
        let args = agent_command(&text)
            .map(|c| c.args)
            .filter(|a| a.first().is_some_and(|w| w == "host"))
            .unwrap_or_else(|| LOGIN_ARGS.map(String::from).to_vec());
        let cmd = ProgramCommand {
            program: exe.to_path_buf(),
            args,
        };
        write_and_register(path, &cmd, version, register)?;
        Ok(Some(match was {
            Some(v) => format!(
                "registered the login item with launchd again, for JETCARP {version} (it was {v}'s)"
            ),
            None => format!("registered the login item with launchd, for JETCARP {version}"),
        }))
    }

    /// Registers the LaunchAgent `plist` with launchd again, in the GUI domain `domain`
    /// (`gui/<uid>`), through `launchctl`, waiting with `sleep`:
    /// 1. `bootout` unloads the job as launchd has it, stopping a host launchd started from it (a
    ///    job that isn't loaded is fine; any other refusal stops here, leaving the host alone);
    /// 2. `print`, until launchd has let the job go;
    /// 3. `bootstrap` loads the file again (tried again while launchd is still letting the old job
    ///    go), and launchd starts the job, as `RunAtLoad` asks;
    /// 4. `kickstart` starts it in case that didn't (a job that runs is left alone). Starting the
    ///    program from launchd is what makes it take the program's code requirement again.
    ///
    /// The job runs `jetcarp host serve --login`, which ends at once when a host is running
    /// already, since that one holds the host's single-instance lock: never two hosts.
    pub fn register_with(
        domain: &str,
        plist: &Path,
        launchctl: &mut dyn FnMut(&[&str]) -> Answer,
        sleep: &mut dyn FnMut(Duration),
    ) -> Result<(), String> {
        let job = format!("{domain}/{LAUNCH_LABEL}");
        let plist = plist.display().to_string();
        let failed = |args: &[&str], (code, said): (i32, String)| {
            format!("`launchctl {}` failed ({code}): {said}", args.join(" "))
        };
        let bootout = ["bootout", job.as_str()];
        match launchctl(&bootout) {
            Ok(()) => {}
            // Not loaded (3: no such process; 113: could not find the service), or going (36).
            Err((3 | 36 | 113, _)) => {}
            Err((_, said))
                if said.contains("No such process") || said.contains("Could not find") => {}
            Err(e) => return Err(failed(&bootout, e)),
        }
        for _ in 0..100 {
            if launchctl(&["print", &job]).is_err() {
                break;
            }
            sleep(Duration::from_millis(100));
        }
        let bootstrap = ["bootstrap", domain, plist.as_str()];
        let mut tries = 0;
        loop {
            match launchctl(&bootstrap) {
                Ok(()) => break,
                // 5 (input/output error) or 37 (operation already in progress): launchd may still
                // be letting the old job go.
                Err((5 | 37, _)) if tries < 10 => {
                    tries += 1;
                    sleep(Duration::from_millis(500));
                }
                Err(e) => {
                    return Err(failed(&bootstrap, e)
                        + " (if jetcarp is switched off under Allow in the Background, in System \
                           Settings > General > Login Items & Extensions, switch it on)");
                }
            }
        }
        let kickstart = ["kickstart", job.as_str()];
        launchctl(&kickstart).map_err(|e| failed(&kickstart, e))
    }

    /// This user's GUI domain, `gui/<uid>`, when the LaunchAgents folder is this account's own.
    /// With `HOME` pointing elsewhere (a test's scratch folder, say) the file written is not the
    /// one launchd reads at login, and the job of that name in the domain isn't its to change.
    #[cfg(target_os = "macos")]
    pub fn domain() -> Result<String, String> {
        let home = dirs::home_dir().ok_or("not registered with launchd: no home folder")?;
        let own = account_home()
            .ok_or("not registered with launchd: this account's home folder is unknown")?;
        let same = home == own
            || matches!((home.canonicalize(), own.canonicalize()), (Ok(a), Ok(b)) if a == b);
        if !same {
            return Err(format!(
                "not registered with launchd: HOME ({}) is not this account's home folder ({}), \
                 whose LaunchAgents launchd reads at login",
                home.display(),
                own.display()
            ));
        }
        // SAFETY: getuid has no preconditions and cannot fail.
        Ok(format!("gui/{}", unsafe { libc::getuid() }))
    }

    /// This account's home folder as the system records it, whatever `HOME` says.
    #[cfg(target_os = "macos")]
    fn account_home() -> Option<std::path::PathBuf> {
        use std::os::unix::ffi::OsStrExt;
        let mut buf = vec![0 as libc::c_char; 16 * 1024];
        // SAFETY: an all-zero passwd is a valid value to be filled in; the strings it is given
        // point into `buf`, which outlives their use here.
        let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
        let mut found: *mut libc::passwd = std::ptr::null_mut();
        let r = unsafe {
            libc::getpwuid_r(
                libc::getuid(),
                &mut pw,
                buf.as_mut_ptr(),
                buf.len(),
                &mut found,
            )
        };
        if r != 0 || found.is_null() || pw.pw_dir.is_null() {
            return None;
        }
        let dir = unsafe { std::ffi::CStr::from_ptr(pw.pw_dir) };
        Some(std::ffi::OsStr::from_bytes(dir.to_bytes()).into())
    }

    /// [`register_with`] this system's launchctl.
    #[cfg(target_os = "macos")]
    pub fn register(domain: &str, plist: &Path) -> Result<(), String> {
        register_with(domain, plist, &mut run, &mut std::thread::sleep)
    }

    #[cfg(target_os = "macos")]
    fn run(args: &[&str]) -> Answer {
        let out = std::process::Command::new("/bin/launchctl")
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| (-1, format!("/bin/launchctl: {e}")))?;
        if out.status.success() {
            return Ok(());
        }
        let mut said = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if said.is_empty() {
            said = String::from_utf8_lossy(&out.stdout).trim().to_string();
        }
        Err((out.status.code().unwrap_or(-1), said))
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
        let p = launch_agent(&cmd("/Users/a&b/jetcarp"), None);
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

    /// A scratch LaunchAgents folder for one test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jetcarp-autostart-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{LAUNCH_LABEL}.plist"))
    }

    fn login(program: &str) -> ProgramCommand {
        ProgramCommand {
            program: PathBuf::from(program),
            args: LOGIN_ARGS.map(String::from).to_vec(),
        }
    }

    /// A LaunchAgent names the version that registered it, in a comment launchd skips, and its
    /// command reads back as written (#99).
    #[test]
    fn launch_agents_record_their_version_and_read_back() {
        let c = ProgramCommand {
            program: PathBuf::from("/Users/a&b <c>/jetcarp"),
            args: vec!["host".into(), "serve".into(), "--login".into()],
        };
        let text = launch_agent(&c, Some("0.4.2"));
        assert!(
            text.contains("<dict>\n    <!-- Registered with launchd by JETCARP 0.4.2 -->\n"),
            "{text}"
        );
        assert_eq!(agent_registered_by(&text), Some("0.4.2"));
        assert_eq!(agent_command(&text), Some(c.clone()));
        let unregistered = launch_agent(&c, None);
        assert_eq!(agent_registered_by(&unregistered), None);
        assert!(!unregistered.contains("<!--"), "{unregistered}");
        assert_eq!(agent_command(&unregistered), Some(c));
        assert_eq!(agent_command("<plist><dict></dict></plist>"), None);
    }

    /// `refresh` leaves alone a login item that is off or this version's, and brings one another
    /// version or none registered up to this program, with its arguments, registering it once.
    #[test]
    fn refresh_registers_an_item_from_another_version_once() {
        use std::cell::RefCell;
        let path = scratch("refresh");
        let exe = Path::new("/opt/homebrew/bin/jetcarp");
        let calls = RefCell::new(vec![]);
        let register = |p: &Path| {
            calls.borrow_mut().push(p.to_path_buf());
            Ok(())
        };
        // Off: no file.
        assert_eq!(
            launchctl::refresh_at(&path, exe, "0.4.2", register),
            Ok(None)
        );
        assert!(calls.borrow().is_empty() && !path.exists());
        // This version's: left as it is.
        let current = launch_agent(&login("/opt/homebrew/bin/jetcarp"), Some("0.4.2"));
        std::fs::write(&path, &current).unwrap();
        assert_eq!(
            launchctl::refresh_at(&path, exe, "0.4.2", register),
            Ok(None)
        );
        assert!(calls.borrow().is_empty());
        // Another version's, at another path and from before `--login`: this program, the same
        // arguments, this version, registered.
        let old = ProgramCommand {
            program: PathBuf::from("/usr/local/bin/jetcarp"),
            args: vec!["host".into(), "serve".into()],
        };
        std::fs::write(&path, launch_agent(&old, Some("0.4.1"))).unwrap();
        let note = launchctl::refresh_at(&path, exe, "0.4.2", register)
            .unwrap()
            .unwrap();
        assert!(note.contains("0.4.2") && note.contains("0.4.1's"), "{note}");
        assert_eq!(*calls.borrow(), std::slice::from_ref(&path));
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(agent_registered_by(&text), Some("0.4.2"));
        let now = agent_command(&text).unwrap();
        assert_eq!(now.program, exe);
        assert_eq!(now.args, ["host", "serve"]);
        // Now current: nothing more.
        assert_eq!(
            launchctl::refresh_at(&path, exe, "0.4.2", register),
            Ok(None)
        );
        assert_eq!(calls.borrow().len(), 1);
        // Written by `set` (no version): registered.
        std::fs::write(
            &path,
            launch_agent(&login("/opt/homebrew/bin/jetcarp"), None),
        )
        .unwrap();
        assert!(
            launchctl::refresh_at(&path, exe, "0.4.2", register)
                .unwrap()
                .is_some()
        );
        assert_eq!(calls.borrow().len(), 2);
        assert_eq!(
            agent_command(&std::fs::read_to_string(&path).unwrap()),
            Some(login("/opt/homebrew/bin/jetcarp"))
        );
        // An entry for another program gets the login entry's arguments.
        let mut odd = cmd("/usr/local/bin/jetcarp-host");
        odd.args.clear();
        std::fs::write(&path, launch_agent(&odd, Some("0.3.0"))).unwrap();
        launchctl::refresh_at(&path, exe, "0.4.2", register).unwrap();
        assert_eq!(
            agent_command(&std::fs::read_to_string(&path).unwrap()),
            Some(login("/opt/homebrew/bin/jetcarp"))
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// A registration that fails says so and leaves the item unmarked, so the next refresh tries
    /// again.
    #[test]
    fn a_failed_registration_is_tried_again() {
        let path = scratch("refresh-fails");
        let exe = Path::new("/opt/homebrew/bin/jetcarp");
        std::fs::write(
            &path,
            launch_agent(&login("/opt/homebrew/bin/jetcarp"), Some("0.4.1")),
        )
        .unwrap();
        let e = launchctl::refresh_at(&path, exe, "0.4.2", |_| Err("no launchd".into()));
        assert_eq!(e, Err("no launchd".into()));
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(agent_registered_by(&text), None);
        assert_eq!(
            agent_command(&text),
            Some(login("/opt/homebrew/bin/jetcarp"))
        );
        let mut tried = false;
        let again = launchctl::refresh_at(&path, exe, "0.4.2", |_| {
            tried = true;
            Ok(())
        });
        assert!(tried && again.unwrap().is_some());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// launchd is asked to let the old job go, waited for, given the file again (tried again while
    /// it is still busy) and asked to start the job; a refusal to let go stops there, leaving a
    /// running host alone. No real launchctl runs.
    #[test]
    fn registering_boots_out_waits_and_bootstraps() {
        use std::time::Duration;
        let plist =
            Path::new("/Users/me/Library/LaunchAgents/com.jimmieprodgers.jetcarp.host.plist");
        let job = "gui/501/com.jimmieprodgers.jetcarp.host";
        let run = |answers: &mut dyn FnMut(&[&str]) -> launchctl::Answer| {
            let mut calls: Vec<String> = vec![];
            let mut slept = Duration::ZERO;
            let r = launchctl::register_with(
                "gui/501",
                plist,
                &mut |args| {
                    calls.push(args.join(" "));
                    answers(args)
                },
                &mut |d| slept += d,
            );
            (r, calls, slept)
        };
        // Loaded and running: unloaded, gone after two looks, busy once, then started.
        let (mut prints, mut busy) = (0, true);
        let (r, calls, slept) = run(&mut |args| match args[0] {
            "print" => {
                prints += 1;
                if prints <= 2 {
                    Ok(())
                } else {
                    Err((113, "Could not find service".into()))
                }
            }
            "bootstrap" if busy => {
                busy = false;
                Err((5, "Bootstrap failed: 5: Input/output error".into()))
            }
            _ => Ok(()),
        });
        assert_eq!(r, Ok(()));
        let bootstrap = format!("bootstrap gui/501 {}", plist.display());
        assert_eq!(
            calls,
            [
                format!("bootout {job}"),
                format!("print {job}"),
                format!("print {job}"),
                format!("print {job}"),
                bootstrap.clone(),
                bootstrap.clone(),
                format!("kickstart {job}"),
            ]
        );
        assert_eq!(slept, Duration::from_millis(700));
        // Not loaded: fine.
        let (r, calls, _) = run(&mut |args| match args[0] {
            "bootout" => Err((
                113,
                "Boot-out failed: 113: Could not find specified service".into(),
            )),
            "print" => Err((113, String::new())),
            _ => Ok(()),
        });
        assert_eq!(r, Ok(()));
        assert_eq!(calls.len(), 4, "{calls:?}");
        // Refused: nothing more is asked.
        let (r, calls, _) = run(&mut |args| match args[0] {
            "bootout" => Err((1, "Boot-out failed: 1: Operation not permitted".into())),
            _ => Ok(()),
        });
        let e = r.unwrap_err();
        assert!(
            e.contains(&format!("`launchctl bootout {job}` failed (1)")),
            "{e}"
        );
        assert_eq!(calls.len(), 1);
        // A bootstrap that fails for good says what failed, and starts nothing.
        let (r, calls, _) = run(&mut |args| match args[0] {
            "print" => Err((113, String::new())),
            "bootstrap" => Err((
                125,
                "Bootstrap failed: 125: Domain does not support specified action".into(),
            )),
            _ => Ok(()),
        });
        let e = r.unwrap_err();
        assert!(
            e.contains(&format!("`launchctl {bootstrap}` failed (125)")),
            "{e}"
        );
        assert!(!calls.iter().any(|c| c.starts_with("kickstart")));
    }
}
