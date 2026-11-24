//! A program ended by a signal as by its own stop (#135, #278): SIGTERM (what `kill` sends,
//! launchd at logout on the Mac, a Linux session as it ends, and a system as it shuts down),
//! SIGHUP (the terminal it was started from closing) and SIGINT (Ctrl-C there). The app closes as
//! its window's close button closes it, the background host stops as `jetcarp host stop` stops
//! it, and `jetcarp mcp --http` as Ctrl-C stopped it: each puts its own things in order (settings
//! saved, jobs stopped at a checkpoint, its endpoint file and its record in the memory pool
//! removed) and then ends. Before, each of these signals ended the process at once and left them
//! behind.
//!
//! A signal handler may do next to nothing safely, so this one only writes the signal's number to
//! a socket pair; a thread of its own waits on the other end and runs what the program gave
//! [`on_signal`]. A second signal ends the process at once, with the status a shell gives a
//! program a signal ended (128 + its number), for a program that does not stop. A signal ignored
//! when the program started (SIGHUP under `nohup`) stays ignored.
//!
//! Windows has no such signals, but the same moments (#319): Ctrl-C and Ctrl-Break, the console
//! window closing, signing out and shutting down. [`on_signal`] takes them there too, through
//! the console's handler ([`console_event`], `SetConsoleCtrlHandler`) and a hidden window of its
//! own that hears the session end (`WM_ENDSESSION`). Windows ends the process as soon as the
//! handler of a close, a sign-out or a shutdown returns, so that handler waits for the program to
//! stop and end itself, as long as Windows waits for a program that does not answer. A console
//! program that loads `user32.dll` (`jetcarp.exe` does, for its windows) is not told of a
//! sign-out or a shutdown through its console at all (Microsoft's `SetConsoleCtrlHandler`): the
//! hidden window is how it hears them.

use std::time::Duration;

/// From now on, the first of SIGTERM, SIGHUP and SIGINT to come runs `stop`, on a thread of its
/// own, with the signal's name (`"SIGTERM"`); `stop` asks the program to stop as it stops
/// itself. A second signal ends the process at once. On Windows, Ctrl-C and Ctrl-Break come as
/// SIGINT, the console closing as SIGHUP (as a closed terminal does on Linux), and a sign-out, a
/// shutdown or `taskkill` without `/F` as SIGTERM (as a session's end and `kill` do there); see
/// [`console_event`]. Once per process: a later call does nothing and returns `false`, as where
/// the thread cannot be made (the signals then end the program at once, as before).
pub fn on_signal(stop: impl FnOnce(&'static str) + Send + 'static) -> bool {
    imp::on_signal(Box::new(stop))
}

/// Windows' console control events, as it gives them to a console program's handler (Microsoft's
/// `HandlerRoutine`, its `dwCtrlType`): Ctrl-C.
pub const CTRL_C_EVENT: u32 = 0;
/// Ctrl-Break.
pub const CTRL_BREAK_EVENT: u32 = 1;
/// The console closing: its window's close button, or *End task* in the Task Manager.
pub const CTRL_CLOSE_EVENT: u32 = 2;
/// A sign-out.
pub const CTRL_LOGOFF_EVENT: u32 = 5;
/// A shutdown.
pub const CTRL_SHUTDOWN_EVENT: u32 = 6;

/// How long Windows waits for a program that does not answer before it ends it, unless changed
/// (`SPI_GETHUNGAPPTIMEOUT` for a closed console, `SPI_GETWAITTOKILLTIMEOUT` for a sign-out or
/// a shutdown; Microsoft's `HandlerRoutine`): 5 seconds each.
pub const WINDOWS_WAIT: Duration = Duration::from_secs(5);

/// What the program does with Windows' console control event `ctrl`. [`on_signal`] makes this
/// the console's handler on Windows, which runs it on a thread it makes in the process for each
/// event; it can be called as Windows calls it on every system, as the tests do.
///
/// - Ctrl-C and Ctrl-Break ([`CTRL_C_EVENT`], [`CTRL_BREAK_EVENT`]): the first stops the program
///   as SIGINT does (`"SIGINT"`); one more while it stops ends it at once, with 130, its record in
///   the memory pool removed. Returns at once, and the program goes on to its stop.
/// - The console closing ([`CTRL_CLOSE_EVENT`]): stops it as SIGHUP does (`"SIGHUP"`). A sign-out
///   or a shutdown ([`CTRL_LOGOFF_EVENT`], [`CTRL_SHUTDOWN_EVENT`]), which Windows gives only a
///   console program that has not loaded `user32.dll`: as SIGTERM (`"SIGTERM"`). Windows ends
///   the process as soon as this returns, so it does not return at once: it waits for the program
///   to stop and end itself, as long as Windows itself would wait ([`end_wait`]). Only a program
///   still stopping then is ended by Windows, as before.
///
/// Returns whether it took the event (`false` for any other, which is left to the next handler).
pub fn console_event(ctrl: u32) -> bool {
    match ctrl {
        CTRL_C_EVENT | CTRL_BREAK_EVENT => {
            stop_or_end("SIGINT");
            true
        }
        CTRL_CLOSE_EVENT => {
            stop_and_wait("SIGHUP", end_wait(ctrl));
            true
        }
        CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT => {
            stop_and_wait("SIGTERM", end_wait(ctrl));
            true
        }
        _ => false,
    }
}

/// How long the handler of `ctrl` waits for the program to end itself: as long as Windows waits
/// for a program that does not answer, as this computer has it (`SystemParametersInfo`), 1 to 60
/// seconds; [`WINDOWS_WAIT`] where it cannot be read, and on Linux and macOS.
pub fn end_wait(ctrl: u32) -> Duration {
    imp::end_wait(ctrl)
        .filter(|d| (Duration::from_secs(1)..=Duration::from_secs(60)).contains(d))
        .unwrap_or(WINDOWS_WAIT)
}

/// The first event that stops the program stops it (the stop given to [`on_signal`] runs with
/// `name`); one more while it stops ends it at once, with 128 + the signal's number, its record
/// in the memory pool removed.
fn stop_or_end(name: &'static str) {
    if !imp::stop_first(name) {
        let code = match name {
            "SIGHUP" => 128 + 1,
            "SIGTERM" => 128 + 15,
            _ => 128 + 2,
        };
        crate::machine::pool::exit(code);
    }
}

/// For an event that ends the process once its handler returns: the program's stop, if it is not
/// stopping already, then a wait of `wait`, in which the program normally stops and ends itself.
fn stop_and_wait(name: &'static str, wait: Duration) {
    imp::stop_first(name);
    std::thread::sleep(wait);
}

#[cfg(unix)]
mod imp {
    use std::io::Read;
    use std::os::fd::IntoRawFd;
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

    /// The signals that stop a program, with their names.
    const SIGNALS: [(libc::c_int, &str); 3] = [
        (libc::SIGTERM, "SIGTERM"),
        (libc::SIGHUP, "SIGHUP"),
        (libc::SIGINT, "SIGINT"),
    ];

    /// The end of the socket pair the handler writes to (-1 until there is one).
    static WAKE: AtomicI32 = AtomicI32::new(-1);
    /// How many of the signals have come.
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    /// Whether [`on_signal`] has been called.
    static CALLED: AtomicBool = AtomicBool::new(false);

    /// The handler: the first signal wakes the thread that stops the program; a second ends the
    /// process at once, with 128 + its number. Only async-signal-safe calls (`write` and
    /// `_exit`) and lock-free atomics.
    extern "C" fn on_signal_handler(sig: libc::c_int) {
        if COUNT.fetch_add(1, Ordering::SeqCst) > 0 {
            // SAFETY: `_exit` is async-signal-safe and does not return.
            unsafe { libc::_exit(128 + sig) };
        }
        wake(sig);
    }

    /// Wakes the thread that stops the program, with `sig`. Async-signal-safe.
    fn wake(sig: libc::c_int) {
        let fd = WAKE.load(Ordering::SeqCst);
        if fd >= 0 {
            // Signal numbers are small (under 65), so one byte holds one.
            let byte = [sig as u8];
            // SAFETY: `write` is async-signal-safe; `byte` outlives the call. A failure leaves the
            // program running, as an ignored signal would, and a second signal still ends it.
            unsafe { libc::write(fd, byte.as_ptr().cast(), 1) };
        }
    }

    /// [`super::console_event`]'s way in, as the first signal comes: `true` when it is the first
    /// (and the program's stop runs with `name`), `false` when the program is stopping already.
    pub(super) fn stop_first(name: &'static str) -> bool {
        if COUNT.fetch_add(1, Ordering::SeqCst) > 0 {
            return false;
        }
        if let Some((sig, _)) = SIGNALS.iter().find(|(_, n)| *n == name) {
            wake(*sig);
        }
        true
    }

    /// Windows' own waits: none here.
    pub(super) fn end_wait(_ctrl: u32) -> Option<std::time::Duration> {
        None
    }

    pub(super) fn on_signal(stop: Box<dyn FnOnce(&'static str) + Send>) -> bool {
        if CALLED.swap(true, Ordering::SeqCst) {
            return false;
        }
        let Ok((mut woken, wake)) = UnixStream::pair() else {
            return false;
        };
        let watcher = std::thread::Builder::new()
            .name("jetcarp-quit-signal".into())
            .spawn(move || {
                let mut byte = [0u8];
                if woken.read_exact(&mut byte).is_ok() {
                    let name = SIGNALS
                        .iter()
                        .find(|(s, _)| *s == libc::c_int::from(byte[0]))
                        .map_or("a signal", |(_, n)| n);
                    stop(name);
                }
            });
        if watcher.is_err() {
            return false;
        }
        WAKE.store(wake.into_raw_fd(), Ordering::SeqCst);
        let handler: extern "C" fn(libc::c_int) = on_signal_handler;
        for (sig, _) in SIGNALS {
            // SAFETY: plain structs filled in before use; the handler is async-signal-safe.
            unsafe {
                let mut was: libc::sigaction = std::mem::zeroed();
                if libc::sigaction(sig, std::ptr::null(), &mut was) == 0
                    && was.sa_sigaction == libc::SIG_IGN
                {
                    continue;
                }
                let mut sa: libc::sigaction = std::mem::zeroed();
                sa.sa_sigaction = handler as libc::sighandler_t;
                sa.sa_flags = libc::SA_RESTART;
                libc::sigemptyset(&mut sa.sa_mask);
                libc::sigaction(sig, &sa, std::ptr::null_mut());
            }
        }
        true
    }
}

#[cfg(windows)]
mod imp {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc::Sender;
    use std::time::Duration;

    /// The thread that stops the program, once there is one.
    static WAKE: Mutex<Option<Sender<&'static str>>> = Mutex::new(None);
    /// How many of the events that stop a program have come.
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    /// Whether [`on_signal`] has been called.
    static CALLED: AtomicBool = AtomicBool::new(false);

    /// The console's handler, which Windows runs on a thread of its own for each event.
    unsafe extern "system" fn handler(ctrl: u32) -> windows_sys::core::BOOL {
        windows_sys::core::BOOL::from(super::console_event(ctrl))
    }

    pub(super) fn stop_first(name: &'static str) -> bool {
        if COUNT.fetch_add(1, Ordering::SeqCst) > 0 {
            return false;
        }
        if let Some(tx) = &*WAKE.lock().unwrap_or_else(|e| e.into_inner()) {
            let _ = tx.send(name);
        }
        true
    }

    pub(super) fn end_wait(ctrl: u32) -> Option<Duration> {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SPI_GETHUNGAPPTIMEOUT, SPI_GETWAITTOKILLTIMEOUT, SystemParametersInfoW,
        };
        let what = if ctrl == super::CTRL_CLOSE_EVENT {
            SPI_GETHUNGAPPTIMEOUT
        } else {
            SPI_GETWAITTOKILLTIMEOUT
        };
        let mut ms: u32 = 0;
        // SAFETY: `ms` is the integer both of these write, and outlives the call.
        let ok = unsafe { SystemParametersInfoW(what, 0, (&raw mut ms).cast(), 0) } != 0;
        ok.then(|| Duration::from_millis(u64::from(ms)))
    }

    pub(super) fn on_signal(stop: Box<dyn FnOnce(&'static str) + Send>) -> bool {
        use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
        if CALLED.swap(true, Ordering::SeqCst) {
            return false;
        }
        let (tx, rx) = std::sync::mpsc::channel::<&'static str>();
        let watcher = std::thread::Builder::new()
            .name("jetcarp-quit-signal".into())
            .spawn(move || {
                if let Ok(name) = rx.recv() {
                    stop(name);
                }
            });
        if watcher.is_err() {
            return false;
        }
        *WAKE.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
        // SAFETY: a plain call; the handler is a function that lasts as long as the process.
        let console = unsafe { SetConsoleCtrlHandler(Some(handler), 1) } != 0;
        let session = super::session::watch();
        console || session
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNT: AtomicUsize = AtomicUsize::new(0);

    pub(super) fn stop_first(_name: &'static str) -> bool {
        COUNT.fetch_add(1, Ordering::SeqCst) == 0
    }

    pub(super) fn end_wait(_ctrl: u32) -> Option<std::time::Duration> {
        None
    }

    pub(super) fn on_signal(_stop: Box<dyn FnOnce(&'static str) + Send>) -> bool {
        false
    }
}

/// The class of the hidden window that hears the session end on Windows ([`on_signal`]); a test
/// finds a program's window by it, to send it what Windows sends at a sign-out.
pub const SESSION_WINDOW_CLASS: &str = "JETCARP session end";

/// Windows: a top-level window of the program's own, never shown, on a thread of its own, for
/// the messages Windows sends every program's windows as the session ends. A console program
/// that has loaded `user32.dll` hears a sign-out or a shutdown only this way (Microsoft's
/// `SetConsoleCtrlHandler`); the app's and the host's own windows are winit's, which leaves these
/// messages to Windows' defaults. A message-only window would hear none of them.
///
/// - `WM_QUERYENDSESSION`: yes, the session may end; the program stops when it does.
/// - `WM_ENDSESSION`, the session ending: the program stops as SIGTERM stops it, and the window
///   answers only once it has ended itself, or after as long as Windows waits for a program that
///   does not answer ([`end_wait`]): Windows may end the process once every window has answered.
/// - `WM_CLOSE`, which `taskkill` without `/F` sends a program's windows: the program stops as
///   SIGTERM (what `kill` sends) stops it, and the window stays; a second ends it at once, as a
///   second signal does.
#[cfg(windows)]
mod session {
    use std::sync::atomic::{AtomicPtr, Ordering};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, RegisterClassW,
        TranslateMessage, WM_CLOSE, WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSW, WS_OVERLAPPED,
    };

    /// The window, once it is made.
    static WINDOW: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

    /// Makes the window on a thread of its own, which then answers its messages for the rest of
    /// the process. `false` when the thread cannot be made.
    pub(super) fn watch() -> bool {
        std::thread::Builder::new()
            .name("jetcarp-session-end".into())
            .spawn(run)
            .is_ok()
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    fn run() {
        let class = wide(super::SESSION_WINDOW_CLASS);
        let title = wide("JETCARP");
        // SAFETY: plain calls; the strings outlive them, and the class's procedure lasts as long
        // as the process.
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let wc = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(procedure),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: class.as_ptr(),
            };
            if RegisterClassW(&wc) == 0 {
                return;
            }
            // No parent, so a top-level window; no WS_VISIBLE, so never shown.
            let window = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
            if window.is_null() {
                return;
            }
            WINDOW.store(window, Ordering::SeqCst);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    unsafe extern "system" fn procedure(
        window: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_QUERYENDSESSION => 1,
            WM_ENDSESSION => {
                if wparam != 0 {
                    super::stop_and_wait("SIGTERM", super::end_wait(super::CTRL_LOGOFF_EVENT));
                }
                0
            }
            WM_CLOSE => {
                super::stop_or_end("SIGTERM");
                0
            }
            // SAFETY: the arguments Windows gave this procedure.
            _ => unsafe { DefWindowProcW(window, msg, wparam, lparam) },
        }
    }

    /// The window, once it is made.
    pub(super) fn window() -> Option<HWND> {
        let w = WINDOW.load(Ordering::SeqCst);
        (!w.is_null()).then_some(w)
    }
}

/// Windows: this program's hidden session window ([`SESSION_WINDOW_CLASS`]), once [`on_signal`]
/// has made it, as a handle's number (for tests, which send it what Windows sends).
#[cfg(windows)]
pub fn session_window() -> Option<isize> {
    session::window().map(|w| w as isize)
}

#[cfg(test)]
mod tests {
    //! Each test starts an `#[ignore]`d twin of its own in a process of its own (this test
    //! program, run again for that one test), and signals only that process. The tests of
    //! Windows' events run on every system: the twin is handed the event as Windows hands it
    //! (on a thread of its own, through [`super::console_event`] or the session window), and when
    //! the handler returns, ends at once with [`ENDED_BY_WINDOWS`], as Windows then ends it.
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, ExitStatus, Stdio};
    use std::sync::mpsc::Receiver;
    use std::time::{Duration, Instant};

    /// What a twin ends with when the handler it was handed an event through returns: Windows
    /// ends the process then.
    const ENDED_BY_WINDOWS: i32 = 58;

    /// Set in a twin's process by its test.
    const CHILD: &str = "JETCARP_QUIT_SIGNAL_CORE_CHILD";

    /// The twin `name`, started, and the lines it prints.
    fn twin(name: &str) -> (Child, Receiver<String>) {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                name,
                "--exact",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start the twin");
        let out = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for l in BufReader::new(out).lines().map_while(Result::ok) {
                let _ = tx.send(l);
            }
        });
        (child, rx)
    }

    /// Waits up to a minute for the twin to print a line holding `text`.
    #[cfg(unix)]
    fn said(rx: &Receiver<String>, text: &str) -> bool {
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(60) {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(l) if l.contains(text) => return true,
                Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return false,
            }
        }
        false
    }

    #[cfg(unix)]
    fn kill(child: &Child, sig: libc::c_int) {
        // SAFETY: a plain call, to the process this test started.
        let r = unsafe { libc::kill(child.id() as libc::pid_t, sig) };
        assert_eq!(r, 0, "kill {sig} {}", child.id());
    }

    /// How the twin ended, waiting up to `limit`; killed if it has not.
    fn ended(child: &mut Child, limit: Duration) -> Option<ExitStatus> {
        let t0 = Instant::now();
        while t0.elapsed() < limit {
            if let Some(s) = child.try_wait().unwrap() {
                return Some(s);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = child.kill();
        let _ = child.wait();
        None
    }

    /// The first signal runs the program's own stop, which here says which signal it was and
    /// ends the twin with 0; a second, while that stop has not finished, ends it at once with
    /// 128 + its number.
    #[cfg(unix)]
    #[test]
    fn the_first_signal_stops_the_program_and_a_second_ends_it_at_once() {
        let (mut child, rx) = twin("quit_signal::tests::child_stops_when_signalled");
        assert!(said(&rx, "ready"), "the twin did not start");
        kill(&child, libc::SIGTERM);
        assert!(said(&rx, "stopping on SIGTERM"), "the stop did not run");
        assert_eq!(
            ended(&mut child, Duration::from_secs(30)).and_then(|s| s.code()),
            Some(0)
        );

        // Its stop never finishes, so the second signal ends it.
        let (mut child, rx) = twin("quit_signal::tests::child_stops_when_signalled");
        assert!(said(&rx, "ready"), "the twin did not start");
        kill(&child, libc::SIGINT);
        assert!(said(&rx, "stopping on SIGINT"), "the stop did not run");
        kill(&child, libc::SIGTERM);
        assert_eq!(
            ended(&mut child, Duration::from_secs(10)).and_then(|s| s.code()),
            Some(128 + libc::SIGTERM)
        );
    }

    /// A signal the program was started ignoring (SIGHUP under `nohup`) stays ignored; the others
    /// still stop it.
    #[cfg(unix)]
    #[test]
    fn a_signal_ignored_at_start_stays_ignored() {
        let (mut child, rx) = twin("quit_signal::tests::child_ignores_sighup");
        assert!(said(&rx, "ready"), "the twin did not start");
        kill(&child, libc::SIGHUP);
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            child.try_wait().unwrap().is_none(),
            "SIGHUP ended a program that ignored it"
        );
        kill(&child, libc::SIGTERM);
        assert!(said(&rx, "stopping on SIGTERM"), "the stop did not run");
        assert_eq!(
            ended(&mut child, Duration::from_secs(30)).and_then(|s| s.code()),
            Some(0)
        );
    }

    /// Stops when signalled: says so, then ends with 0 after a second, unless a second signal
    /// ends it first (the first test's second twin sends it in that second).
    #[cfg(unix)]
    #[test]
    #[ignore = "started by the_first_signal_stops_the_program_and_a_second_ends_it_at_once"]
    fn child_stops_when_signalled() {
        if std::env::var_os(CHILD).is_none() {
            return;
        }
        child_until_stopped();
    }

    /// As [`child_stops_when_signalled`], started ignoring SIGHUP.
    #[cfg(unix)]
    #[test]
    #[ignore = "started by a_signal_ignored_at_start_stays_ignored"]
    fn child_ignores_sighup() {
        if std::env::var_os(CHILD).is_none() {
            return;
        }
        // SAFETY: a plain call, before any handler is set.
        unsafe { libc::signal(libc::SIGHUP, libc::SIG_IGN) };
        child_until_stopped();
    }

    #[cfg(unix)]
    fn child_until_stopped() {
        use std::io::Write;
        let installed = super::on_signal(|sig| {
            println!("stopping on {sig}");
            let _ = std::io::stdout().flush();
            // A stop that takes its time, as a program's own does.
            std::thread::sleep(Duration::from_secs(1));
            std::process::exit(0);
        });
        assert!(installed);
        assert!(!super::on_signal(|_| {}), "once per process");
        println!("ready");
        let _ = std::io::stdout().flush();
        std::thread::sleep(Duration::from_secs(120));
        println!("not signalled in two minutes");
        std::process::exit(3);
    }

    /// The lines of a twin until it ends, and how it ended (waiting up to `limit`).
    fn lines_until_ended(
        child: &mut Child,
        rx: &Receiver<String>,
        limit: Duration,
    ) -> (Option<ExitStatus>, Vec<String>) {
        let status = ended(child, limit);
        // What it printed last, once its end of the pipe has closed.
        std::thread::sleep(Duration::from_millis(200));
        (status, rx.try_iter().collect())
    }

    /// Windows' console closing (#319): the program's own stop runs, as SIGHUP's does, and the
    /// handler does not return while the program stops, so the program ends itself, with its own
    /// status. Windows ends the process as soon as the handler returns: a handler that returned
    /// at once, as the `ctrlc` crate's does, would end it in the middle of its stop.
    #[test]
    fn a_closed_console_stops_the_program_and_its_handler_waits_for_it() {
        let (mut child, rx) = twin("quit_signal::tests::child_handed_a_closed_console");
        let (status, lines) = lines_until_ended(&mut child, &rx, Duration::from_secs(60));
        let said = lines.join("\n");
        assert!(
            lines.iter().any(|l| l.ends_with("stopping on SIGHUP")),
            "the stop did not run:\n{said}"
        );
        assert!(
            lines.iter().any(|l| l.ends_with("stopped")),
            "the program did not finish its stop:\n{said}"
        );
        assert!(
            !said.contains("the handler returned"),
            "the handler returned before the program ended:\n{said}"
        );
        assert_eq!(status.and_then(|s| s.code()), Some(0), "{said}");
    }

    /// A program still stopping once Windows' own wait has passed: the handler returns then, and
    /// Windows ends it, as before.
    #[test]
    fn the_handler_of_a_closed_console_returns_after_windows_wait() {
        let (mut child, rx) = twin("quit_signal::tests::child_that_does_not_stop");
        let (status, lines) = lines_until_ended(&mut child, &rx, Duration::from_secs(90));
        let said = lines.join("\n");
        assert!(
            lines.iter().any(|l| l.ends_with("stopping on SIGHUP")),
            "the stop did not run:\n{said}"
        );
        let number = |prefix: &str| {
            lines
                .iter()
                .find_map(|l| l.split_once(prefix)?.1.parse::<u64>().ok())
                .unwrap_or_else(|| panic!("no {prefix:?} line:\n{said}"))
        };
        let (wait, after) = (
            number("wait ms: "),
            number("the handler returned after ms: "),
        );
        assert!(
            after + 50 >= wait && after < wait + 10_000,
            "the handler returned after {after} ms, Windows waits {wait} ms:\n{said}"
        );
        assert_eq!(
            status.and_then(|s| s.code()),
            Some(ENDED_BY_WINDOWS),
            "{said}"
        );
    }

    /// Ctrl-C in the console: the first stops the program and its handler returns at once (the
    /// console stays); a second while it stops ends it at once with 130.
    #[test]
    fn ctrl_c_stops_the_program_and_a_second_ends_it_at_once() {
        let (mut child, rx) = twin("quit_signal::tests::child_handed_ctrl_c_twice");
        let (status, lines) = lines_until_ended(&mut child, &rx, Duration::from_secs(60));
        let said = lines.join("\n");
        assert!(
            lines.iter().any(|l| l.ends_with("stopping on SIGINT")),
            "the stop did not run:\n{said}"
        );
        let after = lines
            .iter()
            .find_map(|l| {
                l.split_once("the first returned after ms: ")?
                    .1
                    .parse::<u64>()
                    .ok()
            })
            .unwrap_or_else(|| panic!("the first Ctrl-C's handler did not return:\n{said}"));
        assert!(after < 2_000, "{after} ms:\n{said}");
        assert_eq!(status.and_then(|s| s.code()), Some(130), "{said}");
    }

    /// A stop that says which event it was, takes its time as a program's own does, says it has
    /// finished and ends the program with 0; or one that never finishes (`ends: false`).
    fn stop_that_ends(ends: bool) -> impl FnOnce(&'static str) + Send + 'static {
        use std::io::Write;
        move |sig| {
            println!("stopping on {sig}");
            let _ = std::io::stdout().flush();
            if !ends {
                std::thread::sleep(Duration::from_secs(600));
            }
            std::thread::sleep(Duration::from_secs(1));
            println!("stopped");
            let _ = std::io::stdout().flush();
            std::process::exit(0);
        }
    }

    /// Hands the twin `ctrl` as Windows does, on a thread of its own, and ends the twin at once
    /// when the handler returns, as Windows then does.
    fn hand_over(ctrl: u32) {
        use std::io::Write;
        std::thread::spawn(move || {
            let t0 = Instant::now();
            super::console_event(ctrl);
            println!(
                "the handler returned after ms: {}",
                t0.elapsed().as_millis()
            );
            let _ = std::io::stdout().flush();
            std::process::exit(ENDED_BY_WINDOWS);
        });
    }

    /// The twin's own program: it waits to be stopped.
    fn wait_to_be_stopped() -> ! {
        std::thread::sleep(Duration::from_secs(120));
        println!("not stopped in two minutes");
        std::process::exit(3);
    }

    #[test]
    #[ignore = "started by a_closed_console_stops_the_program_and_its_handler_waits_for_it"]
    fn child_handed_a_closed_console() {
        if std::env::var_os(CHILD).is_none() {
            return;
        }
        assert!(super::on_signal(stop_that_ends(true)));
        hand_over(super::CTRL_CLOSE_EVENT);
        wait_to_be_stopped();
    }

    #[test]
    #[ignore = "started by the_handler_of_a_closed_console_returns_after_windows_wait"]
    fn child_that_does_not_stop() {
        if std::env::var_os(CHILD).is_none() {
            return;
        }
        assert!(super::on_signal(stop_that_ends(false)));
        println!(
            "wait ms: {}",
            super::end_wait(super::CTRL_CLOSE_EVENT).as_millis()
        );
        hand_over(super::CTRL_CLOSE_EVENT);
        wait_to_be_stopped();
    }

    #[test]
    #[ignore = "started by ctrl_c_stops_the_program_and_a_second_ends_it_at_once"]
    fn child_handed_ctrl_c_twice() {
        use std::io::Write;
        if std::env::var_os(CHILD).is_none() {
            return;
        }
        assert!(super::on_signal(stop_that_ends(false)));
        std::thread::spawn(|| {
            let t0 = Instant::now();
            assert!(super::console_event(super::CTRL_C_EVENT));
            println!("the first returned after ms: {}", t0.elapsed().as_millis());
            let _ = std::io::stdout().flush();
            std::thread::sleep(Duration::from_millis(300));
            super::console_event(super::CTRL_C_EVENT);
            println!("the second returned");
            let _ = std::io::stdout().flush();
        });
        wait_to_be_stopped();
    }

    /// Windows: at a sign-out or a shutdown, the hidden session window answers
    /// `WM_QUERYENDSESSION` with yes, and `WM_ENDSESSION` stops the program as SIGTERM does,
    /// answering only once the program has ended itself; Windows may end the process as soon as
    /// it answers.
    #[cfg(windows)]
    #[test]
    fn the_session_ending_stops_the_program_and_its_window_waits_for_it() {
        let (mut child, rx) = twin("quit_signal::tests::child_told_the_session_ends");
        let (status, lines) = lines_until_ended(&mut child, &rx, Duration::from_secs(60));
        let said = lines.join("\n");
        assert!(lines.iter().any(|l| l.ends_with("may it end: 1")), "{said}");
        assert!(
            lines.iter().any(|l| l.ends_with("stopping on SIGTERM")),
            "the stop did not run:\n{said}"
        );
        assert!(lines.iter().any(|l| l.ends_with("stopped")), "{said}");
        assert!(
            !said.contains("the session's end was answered"),
            "the window answered before the program ended:\n{said}"
        );
        assert_eq!(status.and_then(|s| s.code()), Some(0), "{said}");
    }

    /// Windows: `taskkill` without `/F` (which sends a program's windows `WM_CLOSE`) stops it as
    /// SIGTERM does, and the window stays; a second ends it at once, with 143.
    #[cfg(windows)]
    #[test]
    fn taskkill_stops_the_program_and_a_second_ends_it_at_once() {
        let (mut child, rx) = twin("quit_signal::tests::child_closed_twice");
        let (status, lines) = lines_until_ended(&mut child, &rx, Duration::from_secs(60));
        let said = lines.join("\n");
        assert!(
            lines.iter().any(|l| l.ends_with("stopping on SIGTERM")),
            "{said}"
        );
        assert!(
            lines
                .iter()
                .any(|l| l.ends_with("the first close was answered")),
            "{said}"
        );
        assert_eq!(status.and_then(|s| s.code()), Some(128 + 15), "{said}");
    }

    /// The session window, once it is made, as Windows would find it.
    #[cfg(windows)]
    fn session_window() -> windows_sys::Win32::Foundation::HWND {
        let t0 = Instant::now();
        loop {
            if let Some(w) = super::session_window() {
                return w as windows_sys::Win32::Foundation::HWND;
            }
            assert!(t0.elapsed() < Duration::from_secs(30), "no session window");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "started by the_session_ending_stops_the_program_and_its_window_waits_for_it"]
    fn child_told_the_session_ends() {
        use std::io::Write;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            ENDSESSION_LOGOFF, SendMessageW, WM_ENDSESSION, WM_QUERYENDSESSION,
        };
        if std::env::var_os(CHILD).is_none() {
            return;
        }
        assert!(super::on_signal(stop_that_ends(true)));
        std::thread::spawn(|| {
            let w = session_window();
            let logoff = ENDSESSION_LOGOFF as isize;
            // SAFETY: plain calls, to a window of this process.
            let may = unsafe { SendMessageW(w, WM_QUERYENDSESSION, 0, logoff) };
            println!("may it end: {may}");
            let _ = std::io::stdout().flush();
            unsafe { SendMessageW(w, WM_ENDSESSION, 1, logoff) };
            println!("the session's end was answered");
            let _ = std::io::stdout().flush();
            std::process::exit(ENDED_BY_WINDOWS);
        });
        wait_to_be_stopped();
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "started by taskkill_stops_the_program_and_a_second_ends_it_at_once"]
    fn child_closed_twice() {
        use std::io::Write;
        use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_CLOSE};
        if std::env::var_os(CHILD).is_none() {
            return;
        }
        assert!(super::on_signal(stop_that_ends(false)));
        std::thread::spawn(|| {
            let w = session_window();
            // SAFETY: plain calls, to a window of this process.
            unsafe { SendMessageW(w, WM_CLOSE, 0, 0) };
            println!("the first close was answered");
            let _ = std::io::stdout().flush();
            std::thread::sleep(Duration::from_millis(300));
            unsafe { SendMessageW(w, WM_CLOSE, 0, 0) };
            println!("the second close was answered");
            let _ = std::io::stdout().flush();
        });
        wait_to_be_stopped();
    }
}
