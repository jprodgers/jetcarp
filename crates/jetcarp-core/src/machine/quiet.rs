//! Quiet while measuring (#301). The JETCARP Hardware Score times JETCARP's own work, so
//! anything else busy on the machine lowers it, JETCARP's own running worlds first of all.
//!
//! **Holding still.** While a program measures it says so in the runtime folder, as
//! `measuring/<process id>.json` ([`Hold`]), renewed every [`EVERY`] and removed when it has
//! measured, as the memory pool's records are ([`super::pool`]). Every JETCARP program that runs
//! worlds looks for one ([`held`]) and holds still while there is one, then carries on as it was:
//! the app (its world, screensaver mode included), the desktop background (its screens, GNOME's
//! lock screen and Windows' screen saver, which the host runs) and the job runner (every search
//! and job, in whatever process runs it, between two of its steps). A record not renewed for
//! [`STALE_SECS`] is from a program that has gone, so a measuring that crashed holds nothing for
//! long.
//!
//! **Who holds still.** A program that does says so in its memory-pool record ([`honour`]), so a
//! measuring can name the JETCARP programs that won't ([`others`]): `jetcarp run` stepping a
//! world, a script, a picture being rendered, another measuring. MCP servers are not named: they
//! work only while an AI's tool call runs, and the load check sees one that is busy. Neither is
//! macOS's screen saver, which runs in a sandbox that cannot read the runtime folder; it runs
//! only when the Mac is idle.
//!
//! **Load.** What else keeps the processor or the GPU busy when the measuring starts is read
//! from the system ([`Load::sample`]): other programs' share of the processor (the system's busy
//! time less this program's own), and on Linux the GPU's busy figure where its driver gives one
//! (`gpu_busy_percent`: AMD's). Over [`CPU_BUSY`] or [`GPU_BUSY`], or with a JETCARP program
//! that could not pause, the score is taken under load ([`Conditions`], kept beside it).

use super::pool;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The records' folder inside the runtime folder.
pub const DIR: &str = "measuring";
/// How often a measuring program renews its record.
pub const EVERY: Duration = Duration::from_secs(2);
/// A record older than this is from a program that has gone.
pub const STALE_SECS: u64 = 10;

/// Other programs' share of the processor above which the score is taken under load: 15%.
///
/// An idle desktop keeps a few percent busy (its compositor, a browser's open tabs, the
/// system's own services: 4% on the development desktop), so anything lower is noise. A game, a
/// video call, a build or another simulation keeps a quarter of the processor or more busy. The
/// parts on every core lose about the share others take, so at 15% they read about 15% low.
pub const CPU_BUSY: f64 = 0.15;

/// The GPU's busy figure above which the score is taken under load: 25%. Higher than the
/// processor's, since the GPU's figure counts the desktop's own drawing (the compositor, the
/// measuring's progress bar), and jumps with it; a game or a video being encoded keeps it far
/// above.
pub const GPU_BUSY: f64 = 0.25;

/// Programs that work only while they are asked (an AI's tool call): not named among those that
/// cannot pause.
pub const WORK_WHEN_ASKED: [&str; 1] = ["an MCP server"];

#[derive(Serialize, Deserialize)]
struct Record {
    pid: u32,
    /// In words: `jetcarp score`, `the app`.
    program: String,
    /// When it was renewed, milliseconds since 1970.
    at_ms: u64,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The records' folder: `<runtime folder>/measuring`.
pub fn folder() -> Option<PathBuf> {
    crate::paths::runtime_dir().map(|d| d.join(DIR))
}

/// This process's holds (changed only under [`WRITING`]).
static HOLDS: AtomicUsize = AtomicUsize::new(0);
/// Held while this process's record is written or removed, so a renewal under way when the last
/// hold ends finishes before the record is removed, and none starts after.
static WRITING: Mutex<()> = Mutex::new(());

/// Write this process's record as `program`, while it holds. Only a program that records its
/// memory in the pool writes one ([`pool::recording`]): tests and code using the crates as a
/// library hold still only within their own process, and never write to the runtime folder.
fn renew(program: &str) {
    if !pool::recording() {
        return;
    }
    let Some(dir) = folder() else { return };
    let _w = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    if HOLDS.load(Ordering::SeqCst) == 0 {
        return;
    }
    let _ = write_in(&dir, std::process::id(), program, now_ms());
}

/// Write a record for `pid` in `dir` (whole, then moved into place), readable by this user only.
/// Another program ending its measuring removes the folder when it leaves it empty, so a folder
/// gone between making it and writing into it is made again, once.
pub fn write_in(dir: &Path, pid: u32, program: &str, at_ms: u64) -> std::io::Result<()> {
    match write_once(dir, pid, program, at_ms) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => write_once(dir, pid, program, at_ms),
        r => r,
    }
}

fn write_once(dir: &Path, pid: u32, program: &str, at_ms: u64) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(dir)?;
    let r = Record {
        pid,
        program: program.into(),
        at_ms,
    };
    let f = dir.join(format!("{pid}.json"));
    let tmp = dir.join(format!("{pid}.json.tmp"));
    std::fs::write(&tmp, serde_json::to_vec(&r).unwrap_or_default())?;
    std::fs::rename(&tmp, &f)
}

/// While it lives, this program is measuring: the rest of JETCARP holds still. Dropping it
/// ends that (the last of this process's holds removes the record).
pub struct Hold {
    stop: Arc<AtomicBool>,
}

impl Hold {
    /// Say that `program` measures, now and every [`EVERY`] until dropped.
    pub fn take(program: &str) -> Hold {
        {
            let _w = WRITING.lock().unwrap_or_else(|e| e.into_inner());
            HOLDS.fetch_add(1, Ordering::SeqCst);
        }
        renew(program);
        let stop = Arc::new(AtomicBool::new(false));
        let (s, program) = (stop.clone(), program.to_string());
        let _ = std::thread::Builder::new()
            .name("jetcarp-measuring".into())
            .spawn(move || {
                let mut last = Instant::now();
                while !s.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(100));
                    if last.elapsed() >= EVERY && !s.load(Ordering::SeqCst) {
                        renew(&program);
                        last = Instant::now();
                    }
                }
            });
        Hold { stop }
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _w = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        // Counted under the lock, as taking one is (and after `end_now`, none is left).
        let before = HOLDS.load(Ordering::SeqCst);
        HOLDS.store(before.saturating_sub(1), Ordering::SeqCst);
        if before == 1
            && let Some(dir) = folder()
        {
            let _ = std::fs::remove_file(dir.join(format!("{}.json", std::process::id())));
            // Its folder too, once no other program measures (only an empty one goes).
            let _ = std::fs::remove_dir(&dir);
        }
        // Whoever looks next reads the folder again.
        if let Ok(mut c) = CACHE.lock() {
            *c = None;
        }
    }
}

/// End this process's measuring at once, whatever holds it (Ctrl-C, as the program quits):
/// its record goes, so the rest of JETCARP carries on now, not once the record goes stale.
pub fn end_now() {
    let _w = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    HOLDS.store(0, Ordering::SeqCst);
    if let Some(dir) = folder() {
        let _ = std::fs::remove_file(dir.join(format!("{}.json", std::process::id())));
        // Its folder too, once no other program measures (only an empty one goes).
        let _ = std::fs::remove_dir(&dir);
    }
}

/// Whether a program other than `own` measures, by the records in `dir` at `now_ms`.
pub fn held_in(dir: &Path, now_ms: u64, own: u32) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return false;
    };
    rd.flatten().any(|e| {
        let p = e.path();
        p.extension().and_then(|x| x.to_str()) == Some("json")
            && std::fs::read(&p)
                .ok()
                .and_then(|b| serde_json::from_slice::<Record>(&b).ok())
                .is_some_and(|r| {
                    r.pid != own && now_ms.saturating_sub(r.at_ms) <= STALE_SECS * 1000
                })
    })
}

/// The folder's answer, read at most every half second.
static CACHE: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

/// Whether JETCARP is measuring this computer, in this process or another: hold still while it
/// is. Cheap enough for every frame (the folder is read at most twice a second).
pub fn held() -> bool {
    if HOLDS.load(Ordering::SeqCst) > 0 {
        return true;
    }
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, v)) = *c
        && at.elapsed() < Duration::from_millis(500)
    {
        return v;
    }
    let v = folder().is_some_and(|d| held_in(&d, now_ms(), std::process::id()));
    *c = Some((Instant::now(), v));
    v
}

/// This program holds still while JETCARP measures: its memory-pool record says so, so a
/// measuring does not name it among the programs that cannot pause. The app and the desktop
/// background say so at start, the job runner as it runs a job.
pub fn honour() {
    pool::set_pauses(true);
}

/// The other JETCARP programs running now, by their memory-pool records: those that hold still
/// while JETCARP measures, and those that don't ([`WORK_WHEN_ASKED`] left out), each name once.
pub fn others() -> (Vec<String>, Vec<String>) {
    let Some(dir) = pool::folder() else {
        return (vec![], vec![]);
    };
    others_in(&pool::read_in(&dir, now_ms(), Some(std::process::id())))
}

/// [`others`] of these records.
pub fn others_in(records: &[pool::Use]) -> (Vec<String>, Vec<String>) {
    let (mut pausing, mut not) = (vec![], vec![]);
    for u in records {
        let v = if u.pauses {
            &mut pausing
        } else if WORK_WHEN_ASKED.contains(&u.program.as_str()) {
            continue;
        } else {
            &mut not
        };
        if !v.contains(&u.program) {
            v.push(u.program.clone());
        }
    }
    (pausing, not)
}

/// The load on the processor and the GPU from other programs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Load {
    /// Other programs' share of the processor, 0 to 1 (none: not known on this system).
    pub cpu: Option<f64>,
    /// The GPU's busy figure, 0 to 1 (none: this system doesn't say).
    pub gpu: Option<f64>,
}

/// A way to measure the load over so many seconds.
type Sampler = fn(f64) -> Load;

/// What [`Load::sample`] uses instead of the system's figures (tests).
static SAMPLER: RwLock<Option<Sampler>> = RwLock::new(None);

/// Measure the load with `f` instead of the system's figures, in this process from now on: the
/// tests, whose machine is busy with other tests.
pub fn sample_with(f: Sampler) {
    if let Ok(mut s) = SAMPLER.write() {
        *s = Some(f);
    }
}

impl Load {
    /// The load over the next `secs` seconds.
    pub fn sample(secs: f64) -> Load {
        if let Some(f) = SAMPLER.read().ok().and_then(|s| *s) {
            return f(secs);
        }
        let cpu0 = imp::cpu_times();
        let own0 = imp::own_time();
        let mut gpu = vec![];
        let n = 5;
        for _ in 0..n {
            std::thread::sleep(Duration::from_secs_f64(secs.max(0.05) / n as f64));
            if let Some(g) = imp::gpu_busy() {
                gpu.push(g);
            }
        }
        let cpu = match (cpu0, imp::cpu_times(), own0, imp::own_time()) {
            (Some((b0, t0)), Some((b1, t1)), Some(o0), Some(o1)) if t1 > t0 => {
                let others = (b1.saturating_sub(b0) as f64 - (o1 - o0).max(0.0)).max(0.0);
                Some((others / (t1 - t0) as f64).clamp(0.0, 1.0))
            }
            _ => None,
        };
        Load {
            cpu,
            gpu: (!gpu.is_empty()).then(|| gpu.iter().sum::<f64>() / gpu.len() as f64),
        }
    }

    /// What keeps the machine busy beyond [`CPU_BUSY`] and [`GPU_BUSY`], in words (empty: it
    /// is quiet, as far as the system says).
    pub fn busy(&self) -> Vec<String> {
        let mut v = vec![];
        if let Some(c) = self.cpu.filter(|&c| c >= CPU_BUSY) {
            v.push(format!(
                "the processor is {} busy with other programs",
                percent(c)
            ));
        }
        if let Some(g) = self.gpu.filter(|&g| g >= GPU_BUSY) {
            v.push(format!("the GPU is {} busy", percent(g)));
        }
        v
    }
}

/// `42%`.
pub fn percent(x: f64) -> String {
    format!("{:.0}%", (x * 100.0).clamp(0.0, 100.0))
}

/// What else was going on when a score was measured, kept beside it in `machine.json` and the
/// report (#301), so a low score taken under load can be told apart.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Conditions {
    /// Other programs' share of the processor as the measuring started, 0 to 1 (none: not known
    /// on this system).
    #[serde(default)]
    pub cpu: Option<f64>,
    /// The GPU's busy figure then, 0 to 1 (none: this system doesn't say).
    #[serde(default)]
    pub gpu: Option<f64>,
    /// The JETCARP programs that held still while it measured: `the desktop background`,
    /// `a search job`.
    #[serde(default)]
    pub paused: Vec<String>,
    /// The JETCARP programs that could not.
    #[serde(default)]
    pub not_paused: Vec<String>,
    /// Taken under load: the processor or the GPU was busy beyond [`CPU_BUSY`] or [`GPU_BUSY`],
    /// or a JETCARP program could not pause. Fast Fit's first guesses don't use such a score.
    #[serde(default)]
    pub under_load: bool,
}

impl Conditions {
    pub fn of(load: Load, paused: Vec<String>, not_paused: Vec<String>) -> Conditions {
        Conditions {
            under_load: !load.busy().is_empty() || !not_paused.is_empty(),
            cpu: load.cpu,
            gpu: load.gpu,
            paused,
            not_paused,
        }
    }

    /// In a line: `taken under load: the processor was 42% busy with other programs; jetcarp run
    /// could not pause`, or `taken with the computer quiet (other programs 3% of the processor)`.
    pub fn text(&self) -> String {
        let load = Load {
            cpu: self.cpu,
            gpu: self.gpu,
        };
        if self.under_load {
            let mut why: Vec<String> = load
                .busy()
                .into_iter()
                .map(|s| s.replace(" is ", " was "))
                .collect();
            if !self.not_paused.is_empty() {
                why.push(format!("{} could not pause", and_list(&self.not_paused)));
            }
            return format!("taken under load: {}", why.join("; "));
        }
        let mut figures = vec![];
        if let Some(c) = self.cpu {
            figures.push(format!("other programs {} of the processor", percent(c)));
        }
        if let Some(g) = self.gpu {
            figures.push(format!("the GPU {} busy", percent(g)));
        }
        if figures.is_empty() {
            "taken with no JETCARP program running alongside (this system does not say how busy \
             it was)"
                .into()
        } else {
            format!("taken with the computer quiet ({})", figures.join(", "))
        }
    }
}

/// `a`, `a and b`, `a, b and c`.
pub fn and_list(v: &[String]) -> String {
    match v {
        [] => String::new(),
        [a] => a.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(target_os = "linux")]
mod imp {
    /// (busy, total) in clock ticks over every core, from `/proc/stat`'s first line.
    pub fn cpu_times() -> Option<(u64, u64)> {
        let stat = std::fs::read_to_string("/proc/stat").ok()?;
        let line = stat.lines().next()?.strip_prefix("cpu ")?;
        let v: Vec<u64> = line
            .split_whitespace()
            .take(8)
            .filter_map(|n| n.parse().ok())
            .collect();
        if v.len() < 5 {
            return None;
        }
        let total: u64 = v.iter().sum();
        // idle and iowait
        Some((total - v[3] - v[4], total))
    }

    /// This process's processor time, in the same clock ticks: `/proc/self/stat`'s utime and
    /// stime.
    pub fn own_time() -> Option<f64> {
        let s = std::fs::read_to_string("/proc/self/stat").ok()?;
        // The fields after the command's name (in parentheses, which may hold spaces).
        let rest = s.rsplit_once(')')?.1;
        let f: Vec<&str> = rest.split_whitespace().collect();
        // utime and stime are fields 14 and 15 of the whole line, 12 and 13 after the name.
        let u: f64 = f.get(11)?.parse().ok()?;
        let t: f64 = f.get(12)?.parse().ok()?;
        Some(u + t)
    }

    /// The busiest GPU's busy figure, 0 to 1, where the driver gives one (`gpu_busy_percent`).
    pub fn gpu_busy() -> Option<f64> {
        std::fs::read_dir("/sys/class/drm")
            .ok()?
            .flatten()
            .filter(|e| {
                let n = e.file_name();
                let n = n.to_string_lossy();
                n.starts_with("card") && !n.contains('-')
            })
            .filter_map(|e| {
                std::fs::read_to_string(e.path().join("device/gpu_busy_percent"))
                    .ok()?
                    .trim()
                    .parse::<f64>()
                    .ok()
            })
            .map(|p| (p / 100.0).clamp(0.0, 1.0))
            .reduce(f64::max)
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetProcessTimes, GetSystemTimes,
    };

    fn ticks(f: &FILETIME) -> u64 {
        ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
    }

    /// (busy, total) in 100 ns units over every core.
    pub fn cpu_times() -> Option<(u64, u64)> {
        let mut idle: FILETIME = unsafe { std::mem::zeroed() };
        let mut kernel = idle;
        let mut user = idle;
        // SAFETY: the call fills in the three times given.
        if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
            return None;
        }
        // Kernel time includes idle time.
        let total = ticks(&kernel) + ticks(&user);
        Some((total.saturating_sub(ticks(&idle)), total))
    }

    /// This process's processor time, in 100 ns units.
    pub fn own_time() -> Option<f64> {
        let z: FILETIME = unsafe { std::mem::zeroed() };
        let (mut created, mut exited, mut kernel, mut user) = (z, z, z, z);
        // SAFETY: the handle is this process's own pseudo-handle; the call fills in the times.
        let ok = unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        };
        (ok != 0).then(|| (ticks(&kernel) + ticks(&user)) as f64)
    }

    pub fn gpu_busy() -> Option<f64> {
        None
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::sync::OnceLock;

    unsafe extern "C" {
        fn mach_host_self() -> u32;
        fn host_statistics(host: u32, flavor: i32, info: *mut i32, count: *mut u32) -> i32;
    }

    const HOST_CPU_LOAD_INFO: i32 = 3;
    /// The ticks `host_statistics` counts in, a second.
    const TICKS_PER_SECOND: f64 = 100.0;

    /// (busy, total) in ticks over every core.
    pub fn cpu_times() -> Option<(u64, u64)> {
        // Each call to mach_host_self takes a port right, so it is taken once.
        static HOST: OnceLock<u32> = OnceLock::new();
        let host = *HOST.get_or_init(|| unsafe { mach_host_self() });
        // user, system, idle, nice
        let mut ticks = [0u32; 4];
        let mut count = 4u32;
        // SAFETY: the call fills in the four counts given.
        let r = unsafe {
            host_statistics(
                host,
                HOST_CPU_LOAD_INFO,
                ticks.as_mut_ptr().cast(),
                &mut count,
            )
        };
        if r != 0 {
            return None;
        }
        let total: u64 = ticks.iter().map(|&t| t as u64).sum();
        Some((total - ticks[2] as u64, total))
    }

    /// This process's processor time, in the same ticks (`getrusage`).
    pub fn own_time() -> Option<f64> {
        let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: the call fills in the usage given.
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) } != 0 {
            return None;
        }
        let s = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
        Some((s(ru.ru_utime) + s(ru.ru_stime)) * TICKS_PER_SECOND)
    }

    pub fn gpu_busy() -> Option<f64> {
        None
    }
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
mod imp {
    pub fn cpu_times() -> Option<(u64, u64)> {
        None
    }
    pub fn own_time() -> Option<f64> {
        None
    }
    pub fn gpu_busy() -> Option<f64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(pid: u32, program: &str, pauses: bool) -> pool::Use {
        pool::Use {
            pid,
            program: program.into(),
            bytes: 1 << 20,
            at_ms: 0,
            pauses,
        }
    }

    #[test]
    fn holds_are_read_back_and_stale_ones_left_out() {
        let d = crate::scratch::ScratchDir::new("machine-quiet");
        let now = 1_000_000_000;
        assert!(!held_in(&d, now, 7));
        write_in(&d, 11, "jetcarp score", now - 3000).unwrap();
        assert!(held_in(&d, now, 7));
        // This process's own record is not another program measuring.
        assert!(!held_in(&d, now, 11));
        // Gone quiet for 11 s: its program has gone.
        write_in(&d, 11, "jetcarp score", now - 11_000).unwrap();
        assert!(!held_in(&d, now, 7));
        assert!(!held_in(&d.join("missing"), now, 7));
    }

    #[test]
    fn a_hold_in_this_process_lasts_while_it_lives() {
        assert!(!held() || HOLDS.load(Ordering::SeqCst) > 0);
        let h = Hold::take("a test");
        let g = Hold::take("a test");
        assert!(held());
        drop(h);
        assert!(held(), "one hold is left");
        drop(g);
        assert_eq!(HOLDS.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn the_programs_that_cannot_pause_are_named_once() {
        let (p, n) = others_in(&[
            u(1, "the desktop background", true),
            u(2, "a search job", true),
            u(3, "a search job", true),
            u(4, "jetcarp run", false),
            u(5, "an MCP server", false),
            u(6, "jetcarp run", false),
        ]);
        assert_eq!(p, ["the desktop background", "a search job"]);
        assert_eq!(n, ["jetcarp run"]);
    }

    #[test]
    fn load_over_the_thresholds_is_taken_under_load() {
        let quiet = Load {
            cpu: Some(0.03),
            gpu: Some(0.10),
        };
        assert!(quiet.busy().is_empty());
        let c = Conditions::of(quiet, vec!["the desktop background".into()], vec![]);
        assert!(!c.under_load);
        assert_eq!(
            c.text(),
            "taken with the computer quiet (other programs 3% of the processor, the GPU 10% busy)"
        );
        let busy = Load {
            cpu: Some(0.42),
            gpu: Some(0.80),
        };
        assert_eq!(
            busy.busy(),
            [
                "the processor is 42% busy with other programs",
                "the GPU is 80% busy"
            ]
        );
        let c = Conditions::of(busy, vec![], vec!["jetcarp run".into()]);
        assert!(c.under_load);
        assert_eq!(
            c.text(),
            "taken under load: the processor was 42% busy with other programs; the GPU was 80% \
             busy; jetcarp run could not pause"
        );
        // Just under the processor's threshold, a JETCARP program that cannot pause is enough.
        let c = Conditions::of(
            Load {
                cpu: Some(CPU_BUSY - 0.01),
                gpu: None,
            },
            vec![],
            vec!["jetcarp script".into(), "jetcarp media".into()],
        );
        assert!(c.under_load);
        assert!(
            c.text()
                .ends_with("jetcarp script and jetcarp media could not pause"),
            "{}",
            c.text()
        );
        // Unknown figures: not under load by themselves.
        assert!(!Conditions::of(Load::default(), vec![], vec![]).under_load);
    }

    #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
    #[test]
    fn the_processor_load_is_read() {
        let l = Load::sample(0.2);
        let c = l.cpu.expect("the processor's load on this system");
        assert!((0.0..=1.0).contains(&c), "{c}");
    }
}
