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
//! when the program started (SIGHUP under `nohup`) stays ignored. Linux and macOS: Windows has no
//! such signals, and [`on_signal`] does nothing there.

/// From now on, the first of SIGTERM, SIGHUP and SIGINT to come runs `stop`, on a thread of its
/// own, with the signal's name (`"SIGTERM"`); `stop` asks the program to stop as it stops
/// itself. A second signal ends the process at once. Once per process: a later call does
/// nothing and returns `false`, as on Windows, and where the socket pair or the thread cannot be
/// made (the signals then end the program at once, as before).
pub fn on_signal(stop: impl FnOnce(&'static str) + Send + 'static) -> bool {
    imp::on_signal(Box::new(stop))
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
        let fd = WAKE.load(Ordering::SeqCst);
        if fd >= 0 {
            // Signal numbers are small (under 65), so one byte holds one.
            let byte = [sig as u8];
            // SAFETY: `write` is async-signal-safe; `byte` outlives the call. A failure leaves the
            // program running, as an ignored signal would, and a second signal still ends it.
            unsafe { libc::write(fd, byte.as_ptr().cast(), 1) };
        }
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

#[cfg(not(unix))]
mod imp {
    pub(super) fn on_signal(_stop: Box<dyn FnOnce(&'static str) + Send>) -> bool {
        false
    }
}

#[cfg(all(test, unix))]
mod tests {
    //! Each test starts an `#[ignore]`d twin of its own in a process of its own (this test
    //! program, run again for that one test), and signals only that process.
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, ExitStatus, Stdio};
    use std::sync::mpsc::Receiver;
    use std::time::{Duration, Instant};

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
    #[test]
    #[ignore = "started by the_first_signal_stops_the_program_and_a_second_ends_it_at_once"]
    fn child_stops_when_signalled() {
        if std::env::var_os(CHILD).is_none() {
            return;
        }
        child_until_stopped();
    }

    /// As [`child_stops_when_signalled`], started ignoring SIGHUP.
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
}
