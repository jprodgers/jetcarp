//! One pool of memory for all of JETCARP: the part *Reserved for JETCARP*. The app, the desktop
//! background, the command line, MCP servers, scripts and search jobs all draw on it.
//!
//! **How the processes see each other.** Each JETCARP program records how much memory it holds
//! (its resident memory, as the system counts it) in the runtime folder, as
//! `memory/<process id>.json`, when it starts and every [`EVERY`] after, and removes the record
//! when it ends. A record not refreshed for [`STALE_SECS`] belongs to a program that has gone,
//! and is not counted. So a new world, and the desktop background's grids, see what the other
//! programs leave of the pool ([`Pool::left`]).
//!
//! **What this does not do.** It checks a world when it is built, not as it grows; two worlds
//! started in two programs within the same moment can each see the same free memory; programs
//! other than JETCARP (and copies of JETCARP from before 0.4.3) are not counted, which is what
//! *Reserved for the System* is for. Code that never started recording (tests, the crates used as
//! a library) checks no pool, so its results and messages are the same everywhere.

use super::{Limits, bytes_text};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The records' folder inside the runtime folder.
pub const DIR: &str = "memory";
/// How often a program refreshes its record.
pub const EVERY: Duration = Duration::from_secs(2);
/// A record older than this belongs to a program that has gone.
pub const STALE_SECS: u64 = 10;

/// What one program holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Use {
    pub pid: u32,
    /// In words: `the app`, `the desktop background`, `jetcarp run`, `an MCP server`.
    pub program: String,
    /// Its resident memory, bytes.
    pub bytes: u64,
    /// When it was recorded, milliseconds since 1970.
    pub at_ms: u64,
    /// It holds still while JETCARP measures this computer ([`super::quiet::honour`]): the app,
    /// the desktop background, a program running a job.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pauses: bool,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The records' folder: `<runtime folder>/memory`.
pub fn folder() -> Option<PathBuf> {
    crate::paths::runtime_dir().map(|d| d.join(DIR))
}

/// Make `dir` (and the runtime folder above it), readable by this user only.
fn make_private(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
}

/// Write `u` as its record in `dir` (whole, then moved into place).
pub fn record_in(dir: &Path, u: &Use) -> std::io::Result<()> {
    make_private(dir)?;
    let f = dir.join(format!("{}.json", u.pid));
    let tmp = dir.join(format!("{}.json.tmp", u.pid));
    std::fs::write(&tmp, serde_json::to_vec(u).unwrap_or_default())?;
    std::fs::rename(&tmp, &f)
}

/// Remove `pid`'s record from `dir`.
pub fn remove_in(dir: &Path, pid: u32) {
    let _ = std::fs::remove_file(dir.join(format!("{pid}.json")));
}

/// The fresh records in `dir` at `now_ms`, but `skip`'s, largest first. Records gone stale for a
/// minute are removed.
pub fn read_in(dir: &Path, now_ms: u64, skip: Option<u32>) -> Vec<Use> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut out = vec![];
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Some(u) = std::fs::read(&p)
            .ok()
            .and_then(|b| serde_json::from_slice::<Use>(&b).ok())
        else {
            continue;
        };
        let age = now_ms.saturating_sub(u.at_ms);
        if age > 60_000 {
            let _ = std::fs::remove_file(&p);
            continue;
        }
        if age > STALE_SECS * 1000 || Some(u.pid) == skip {
            continue;
        }
        out.push(u);
    }
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.pid.cmp(&b.pid)));
    out
}

struct Me {
    program: String,
    rss: fn() -> Option<u64>,
}

static ME: OnceLock<Me> = OnceLock::new();
static STOPPED: AtomicBool = AtomicBool::new(false);
/// This program holds still while JETCARP measures ([`set_pauses`]).
static PAUSES: AtomicBool = AtomicBool::new(false);
/// Held while this program's record is written, and by [`stop`]: a refresh under way when the
/// program ends finishes before the record is removed, and none starts after, so no record is
/// left behind (#135).
static WRITING: Mutex<()> = Mutex::new(());

/// Record this program's memory from now on, as `program`, read with `rss`: once now, then every
/// [`EVERY`] on a thread of its own. The `jetcarp` program does this at start.
pub fn start(program: &str, rss: fn() -> Option<u64>) {
    if ME
        .set(Me {
            program: program.into(),
            rss,
        })
        .is_err()
    {
        return;
    }
    record_now();
    let _ = std::thread::Builder::new()
        .name("jetcarp-memory".into())
        .spawn(|| {
            while !STOPPED.load(Ordering::Relaxed) {
                std::thread::sleep(EVERY);
                if !STOPPED.load(Ordering::Relaxed) {
                    record_now();
                }
            }
        });
}

/// Whether this program records its memory ([`start`] ran): the `jetcarp` and `jetcarp-mcp`
/// programs do, tests and code using the crates as a library don't.
pub fn recording() -> bool {
    ME.get().is_some()
}

/// This program in words, as its record names it (none: it does not record).
pub fn program() -> Option<String> {
    ME.get().map(|m| m.program.clone())
}

/// Say in this program's record whether it holds still while JETCARP measures this computer
/// ([`super::quiet`]); recorded at once.
pub fn set_pauses(on: bool) {
    if PAUSES.swap(on, Ordering::SeqCst) != on {
        record_now();
    }
}

/// Refresh this program's record now (after it built or dropped something large).
pub fn record_now() {
    let (Some(me), Some(dir)) = (ME.get(), folder()) else {
        return;
    };
    let _writing = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    if STOPPED.load(Ordering::Relaxed) {
        return;
    }
    if let Some(bytes) = (me.rss)() {
        let _ = record_in(
            &dir,
            &Use {
                pid: std::process::id(),
                program: me.program.clone(),
                bytes,
                at_ms: now_ms(),
                pauses: PAUSES.load(Ordering::Relaxed),
            },
        );
    }
}

/// Stop recording and remove this program's record (at its end: the `jetcarp` program's `main`
/// as it returns, [`exit`], and `jetcarp_display::exit::end_process`, how the app and the host
/// end).
pub fn stop() {
    let _writing = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    STOPPED.store(true, Ordering::Relaxed);
    if let Some(dir) = folder() {
        remove_in(&dir, std::process::id());
    }
}

/// How the program ends once its record is removed, when it is not `std::process::exit`
/// ([`end_with`]).
static END: OnceLock<fn(i32) -> !> = OnceLock::new();

/// End the program with `code`, its record removed first ([`stop`]): every early exit of a
/// program that records goes through here, so none leaves a record behind. It ends as
/// [`end_with`] says, else through `std::process::exit`.
pub fn exit(code: i32) -> ! {
    stop();
    match END.get() {
        Some(end) => end(code),
        None => std::process::exit(code),
    }
}

/// From now on, [`exit`] ends the program through `end`: the `jetcarp` program's ends through
/// `jetcarp_display::exit::end_process` once it has used the GPU (#315), so a second Ctrl-C, a
/// closed pipe or a script's stop doesn't end it while a graphics driver's threads run. Once per
/// process; a later call does nothing.
pub fn end_with(end: fn(i32) -> !) {
    let _ = END.set(end);
}

/// The pool as it stands: its size, what the other programs hold, and what this one does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pool {
    /// Reserved for JETCARP, bytes.
    pub total: u64,
    /// The other JETCARP programs, largest first.
    pub others: Vec<Use>,
    /// This program, bytes.
    pub own: u64,
    /// This program in words.
    pub program: String,
}

impl Pool {
    /// The pool now, if this program records its memory (none in tests and library code).
    pub fn now(l: &Limits) -> Option<Pool> {
        let me = ME.get()?;
        let dir = folder()?;
        Some(Pool {
            total: l.jetcarp_memory(),
            others: read_in(&dir, now_ms(), Some(std::process::id())),
            own: (me.rss)().unwrap_or(0),
            program: me.program.clone(),
        })
    }

    /// What the other programs hold together.
    pub fn others_bytes(&self) -> u64 {
        self.others.iter().map(|u| u.bytes).sum()
    }

    /// What is left for something new in this program, counting `credit` (what it will give
    /// back: the world a new one replaces) as free.
    pub fn left(&self, credit: u64) -> u64 {
        self.total
            .saturating_sub(self.others_bytes())
            .saturating_sub(self.own.saturating_sub(credit))
    }

    /// What the other programs leave this one, all of it: the desktop background's limit.
    pub fn left_for_this_program(&self) -> u64 {
        self.total.saturating_sub(self.others_bytes())
    }

    /// Who holds what, in words: `the desktop background 30 GB, the app 2 GB`.
    pub fn users_text(&self) -> String {
        let mut v: Vec<String> = self
            .others
            .iter()
            .map(|u| format!("{} {}", u.program, bytes_text(u.bytes)))
            .collect();
        v.push(format!("{} {}", self.program, bytes_text(self.own)));
        v.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(pid: u32, program: &str, gib: u64, at_ms: u64) -> Use {
        Use {
            pid,
            program: program.into(),
            bytes: gib << 30,
            at_ms,
            pauses: false,
        }
    }

    #[test]
    fn records_are_read_back_and_stale_ones_left_out() {
        let d = crate::scratch::ScratchDir::new("machine-pool");
        let now = 1_000_000_000;
        record_in(&d, &u(11, "the app", 2, now - 1000)).unwrap();
        record_in(&d, &u(12, "the desktop background", 30, now - 3000)).unwrap();
        // Gone quiet for 11 s: its program has gone.
        record_in(&d, &u(13, "jetcarp run", 5, now - 11_000)).unwrap();
        // Gone for over a minute: removed.
        record_in(&d, &u(14, "jetcarp run", 5, now - 61_000)).unwrap();
        let r = read_in(&d, now, None);
        assert_eq!(
            r.iter().map(|x| x.pid).collect::<Vec<_>>(),
            [12, 11],
            "largest first, stale ones left out"
        );
        assert!(!d.join("14.json").exists());
        assert!(d.join("13.json").exists());
        // This program's own record is not one of the others.
        assert_eq!(read_in(&d, now, Some(12)).len(), 1);
        remove_in(&d, 11);
        assert_eq!(read_in(&d, now, None).len(), 1);
        assert_eq!(read_in(&d.join("missing"), now, None), vec![]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let m = std::fs::metadata(&*d).unwrap().permissions().mode() & 0o777;
            assert_eq!(m, 0o700);
        }
    }

    #[test]
    fn what_is_left() {
        let p = Pool {
            total: 48 << 30,
            others: vec![
                u(12, "the desktop background", 30, 0),
                u(11, "the app", 2, 0),
            ],
            own: 4 << 30,
            program: "jetcarp run".into(),
        };
        assert_eq!(p.others_bytes(), 32 << 30);
        assert_eq!(p.left(0), 12 << 30);
        // A world that replaces 3 GB of this program's frees them first.
        assert_eq!(p.left(3 << 30), 15 << 30);
        assert_eq!(p.left_for_this_program(), 16 << 30);
        assert_eq!(
            p.users_text(),
            "the desktop background 30 GB, the app 2 GB, jetcarp run 4 GB"
        );
        // Others over the pool: nothing left, never below nothing.
        let full = Pool {
            others: vec![u(12, "the desktop background", 60, 0)],
            ..p
        };
        assert_eq!((full.left(0), full.left_for_this_program()), (0, 0));
    }

    #[test]
    fn no_pool_until_a_program_records() {
        // Tests never start recording, so they check no pool.
        assert!(Pool::now(&Limits::reference()).is_none());
    }
}
