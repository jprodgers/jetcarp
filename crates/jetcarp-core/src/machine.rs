//! What JETCARP knows about the machine it runs on, kept as two kinds of knowledge:
//!
//! 1. **Capacity** ([`hardware`]): memory, the processor, each GPU's memory, read in
//!    milliseconds every time a JETCARP program starts. The memory-based limits come from this
//!    alone ([`limits`]), so a machine never measured still gets limits that fit it. A GPU whose
//!    memory only NVML or Vulkan gives is read through them once and kept, so a plain start does
//!    not open them ([`read_hardware`]); `jetcarp doctor` and measuring read it afresh.
//! 2. **Speed** ([`bench`], [`score`]): about a minute of JETCARP's own work, run at setup if
//!    chosen and by hand, never by itself. It gives the JETCARP Hardware Score, its parts, and
//!    (with Fast Fit) the defaults that depend on time.
//!
//! The process's limits ([`current`]) are the reference machine's ([`Limits::reference`], the
//! fixed limits before 0.4.3) until a program asks for this machine's ([`use_this_machine`]): the
//! `jetcarp` program does at start, while tests and code using the crates as a library keep the
//! reference, so their results and messages are the same everywhere. Limits never change a
//! result: they only allow or refuse.
//!
//! Files in the settings folder: `machine.json` ([`MachineFile`]: the hardware when last
//! measured, its fingerprint, the score and its parts, and each GPU as last read afresh) and
//! `limits.json` ([`limits::Own`]: only the person's own values, so measuring again never undoes
//! a choice).

pub mod bench;
pub mod cost;
pub mod gpu_load;
pub mod hardware;
pub mod limits;
pub mod nvml;
pub mod pool;
pub mod quiet;
pub mod score;
pub mod vulkan;

pub use hardware::{GpuMemory, Hardware};
pub use limits::{Entry, Id, Limits, OverLimit, Own, Source, Unit};
pub use score::Score;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// The settings file of what was last measured.
pub const MACHINE_FILE: &str = "machine.json";

/// `512 B`, `3 KB`, `1.5 GB`, `24 GB` (binary multiples, as the system's memory figures).
pub fn bytes_text(b: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i + 1 < UNITS.len() {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        return format!("{b} B");
    }
    let t = if v >= 100.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    };
    format!("{} {}", t.trim_end_matches(".0"), UNITS[i])
}

/// `2,000`, `64 million`, `4.3 billion`.
pub fn count_text(n: u64) -> String {
    let short = |v: f64, word: &str| {
        let t = if v >= 100.0 {
            format!("{v:.0}")
        } else {
            format!("{v:.1}")
        };
        format!("{} {word}", t.trim_end_matches(".0"))
    };
    if n >= 1_000_000_000 {
        short(n as f64 / 1e9, "billion")
    } else if n >= 1_000_000 {
        short(n as f64 / 1e6, "million")
    } else {
        let d = n.to_string();
        let mut out = String::new();
        for (i, c) in d.chars().enumerate() {
            if i > 0 && (d.len() - i).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
        }
        out
    }
}

/// Write `text` to `path` whole, then move it into place.
pub(crate) fn write_whole(path: &Path, text: &str) -> Result<(), String> {
    let dir = path.parent().ok_or("no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

/// `machine.json`: what was last measured on this machine.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MachineFile {
    #[serde(default = "one")]
    pub version: u32,
    /// The hardware when the benchmark last ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware: Option<Hardware>,
    /// Its fingerprint ([`Hardware::fingerprint`]).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub fingerprint: String,
    /// The last score, with its parts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<Score>,
    /// The limits as they were then (for the record: memory limits follow the hardware live).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<(String, f64)>,
    /// The machine fingerprint that a "measure again" suggestion was last made for, so it is
    /// made once.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub suggested_for: String,
    /// Each GPU as last read afresh, with its ids and PCI address: a plain start takes the memory
    /// of a GPU that only NVML or Vulkan gives from here, so as not to open them
    /// ([`read_hardware`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gpus: Vec<GpuMemory>,
    /// `hardware` lists every GPU, its memory read or why not (written since 0.4.3-rc.3); an
    /// older file lists only those whose memory was read.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all_gpus: bool,
}

/// How the hardware's GPU memory is read ([`read_hardware`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuReading {
    /// From what `machine.json` keeps for each GPU still there, opening NVML or Vulkan only for
    /// a GPU it has nothing for: a plain start.
    Saved,
    /// Afresh: `jetcarp doctor`, and measuring.
    Fresh,
}

thread_local! {
    static LAST_READING: std::cell::Cell<Option<GpuReading>> = const { std::cell::Cell::new(None) };
}

/// Whether a fresh reading is kept in `machine.json` ([`keep_readings`]); off until a program asks.
static KEEP_READINGS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Keep what NVML or Vulkan said of each GPU in the settings folder's `machine.json`, so a later
/// start need not open them. The `jetcarp` program turns this on when it starts, as it does for
/// the GPU self-test's results (`jetcarp_gpu::select::keep_checks`). Code that uses this crate as
/// a library (the test suites among it) only reads what is kept, so it never writes to the user's
/// own settings folder.
pub fn keep_readings(on: bool) {
    KEEP_READINGS.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// How this thread last read the hardware ([`read_hardware`]), for the tests.
pub fn last_reading() -> Option<GpuReading> {
    LAST_READING.with(|c| c.get())
}

/// This machine's hardware, read now, its GPUs' memory as `how` says. A plain start
/// ([`GpuReading::Saved`]) takes a GPU's memory from what `settings`' `machine.json` keeps for a
/// GPU of the same ids and PCI address, so NVML and Vulkan are opened (and a laptop's sleeping
/// GPU woken) only for a GPU it has nothing for: a new or changed one, or the first start. In the
/// `jetcarp` program ([`keep_readings`]), a reading that opened either is kept there for the next
/// start.
pub fn read_hardware(settings: Option<&Path>, how: GpuReading) -> Hardware {
    LAST_READING.with(|c| c.set(Some(how)));
    let saved = match how {
        GpuReading::Saved => MachineFile::load(settings)
            .map(|f| f.gpus)
            .unwrap_or_default(),
        GpuReading::Fresh => vec![],
    };
    let (hw, opened) = Hardware::read_with(&saved);
    if opened
        && KEEP_READINGS.load(std::sync::atomic::Ordering::Relaxed)
        && let Some(dir) = settings
    {
        keep_gpus(dir, &hw.gpus);
    }
    hw
}

/// Keep `gpus` in `machine.json` for the next start: the file read again just before, so what
/// else it holds is kept; nothing written if it holds them already. A file that can't be written
/// only means the next start reads them again.
fn keep_gpus(dir: &Path, gpus: &[GpuMemory]) {
    let mut f = MachineFile::load(Some(dir)).unwrap_or(MachineFile {
        version: 1,
        ..Default::default()
    });
    if f.gpus == gpus {
        return;
    }
    f.gpus = gpus.to_vec();
    let _ = f.save(dir);
}

fn one() -> u32 {
    1
}

impl MachineFile {
    pub fn file(settings: &Path) -> PathBuf {
        settings.join(MACHINE_FILE)
    }

    /// The saved file; none when missing or unreadable (it is only a record: measuring again
    /// writes a new one).
    pub fn load(settings: Option<&Path>) -> Option<MachineFile> {
        let t = std::fs::read_to_string(MachineFile::file(settings?)).ok()?;
        serde_json::from_str(&t).ok()
    }

    pub fn save(&self, settings: &Path) -> Result<PathBuf, String> {
        let f = MachineFile::file(settings);
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        write_whole(&f, &(text + "\n"))?;
        Ok(f)
    }

    /// Why the saved score should be measured again on `now`'s machine, if it should: the
    /// machine changed, or the benchmark did. None when it is current, or there is none.
    pub fn stale(&self, now: &Hardware) -> Option<String> {
        let s = self.score.as_ref()?;
        if s.benchmark != score::BENCHMARK {
            return Some(format!(
                "the score is from an older benchmark (score v{}, this JETCARP's is v{})",
                s.benchmark,
                score::BENCHMARK
            ));
        }
        if !self.fingerprint.is_empty() && self.fingerprint != now.fingerprint() {
            let what = self
                .hardware
                .as_ref()
                .map(|before| changes(before, now, self.all_gpus))
                .filter(|c| !c.is_empty())
                .map_or("its processor, memory, GPUs or system".into(), |c| {
                    c.join("; ")
                });
            return Some(format!(
                "this computer has changed since it was measured ({what})"
            ));
        }
        None
    }
}

/// What changed from `before` (the hardware when measured) to `now`, among what the fingerprint
/// counts, in words. `all_gpus`: `before` lists every GPU, read or not, so a GPU not in it is
/// new; an older record lists only those whose memory was read, so a GPU NVML, dmem.capacity or
/// Vulkan now reads may have been there all along.
fn changes(before: &Hardware, now: &Hardware, all_gpus: bool) -> Vec<String> {
    let gib = |b: u64| (b + (1 << 29)) >> 30;
    let mut v = vec![];
    if (&before.os, &before.arch) != (&now.os, &now.arch) {
        v.push(format!("its system is now {} {}", now.os, now.arch));
    }
    if before.cpu.trim() != now.cpu.trim() {
        v.push(format!("its processor is now {}", now.cpu.trim()));
    } else if before.threads != now.threads {
        v.push(format!("its processor now runs {} threads", now.threads));
    }
    if gib(before.memory_total) != gib(now.memory_total) {
        v.push(format!(
            "its memory changed from {} to {}",
            bytes_text(before.memory_total),
            bytes_text(now.memory_total)
        ));
    }
    let id = |g: &GpuMemory| format!("{:04x}:{:04x}", g.vendor, g.device);
    let mut paired = vec![false; before.gpus.len()];
    for g in &now.gpus {
        let b = before
            .gpus
            .iter()
            .enumerate()
            .find(|(i, b)| !paired[*i] && (b.vendor, b.device) == (g.vendor, g.device));
        match b {
            Some((i, b)) => {
                paired[i] = true;
                match (b.read(), g.read()) {
                    (true, true) if b.dedicated.map(gib) != g.dedicated.map(gib) => {
                        v.push(format!(
                            "its GPU {}'s memory changed from {} to {}",
                            id(g),
                            b.memory_words(),
                            g.memory_words()
                        ))
                    }
                    (false, true) => v.push(format!(
                        "its GPU {}'s memory is now read, {} from {}",
                        id(g),
                        g.memory_words(),
                        g.source_words()
                    )),
                    (true, false) => {
                        v.push(format!("its GPU {}'s memory is no longer read", id(g)))
                    }
                    _ => {}
                }
            }
            None if g.read() => {
                let newly_read = matches!(g.source.as_str(), "NVML" | "dmem.capacity" | "Vulkan");
                if newly_read && !all_gpus {
                    v.push(format!(
                        "its GPU {}'s memory is now read, {} from {}",
                        id(g),
                        g.memory_words(),
                        g.source_words()
                    ));
                } else {
                    v.push(format!(
                        "it has a new GPU, {}, {} from {}",
                        id(g),
                        g.memory_words(),
                        g.source_words()
                    ));
                }
            }
            None => {}
        }
    }
    for (b, _) in before
        .gpus
        .iter()
        .zip(&paired)
        .filter(|(b, p)| !**p && b.read())
    {
        v.push(format!("it no longer has GPU {}", id(b)));
    }
    v
}

/// The suggestion to measure again, once per machine change: the text for the status line, or
/// none (nothing changed, never measured, or already suggested for this machine). Records that
/// it was made.
pub fn suggest_measuring(settings: Option<&Path>, now: &Hardware) -> Option<String> {
    let dir = settings?;
    let mut f = MachineFile::load(Some(dir))?;
    let why = f.stale(now)?;
    let key = format!("{}/{}", now.fingerprint(), score::BENCHMARK);
    if f.suggested_for == key {
        return None;
    }
    f.suggested_for = key;
    let _ = f.save(dir);
    Some(format!(
        "{why}: measure it again (Tools → This computer…, or `jetcarp score`) for limits and a \
         score that fit"
    ))
}

static CURRENT: RwLock<Option<Arc<Limits>>> = RwLock::new(None);
static HARDWARE: RwLock<Option<Arc<Hardware>>> = RwLock::new(None);

/// The process's limits: this machine's once [`use_this_machine`] ran, else the reference's.
pub fn current() -> Arc<Limits> {
    if let Some(l) = CURRENT.read().ok().and_then(|g| g.clone()) {
        return l;
    }
    static REF: std::sync::OnceLock<Arc<Limits>> = std::sync::OnceLock::new();
    REF.get_or_init(|| Arc::new(Limits::reference())).clone()
}

/// The hardware [`use_this_machine`] read, if it ran.
pub fn hardware() -> Option<Arc<Hardware>> {
    HARDWARE.read().ok().and_then(|g| g.clone())
}

/// Use these limits in this process from now on.
pub fn install(l: Limits) -> Arc<Limits> {
    let l = Arc::new(l);
    if let Ok(mut g) = CURRENT.write() {
        *g = Some(l.clone());
    }
    l
}

/// This machine's limits: its hardware, read now (each GPU's memory as saved, where it was:
/// [`read_hardware`]), and the person's own values from the settings folder; used in this process
/// from now on. The note says a `limits.json` that could
/// not be read.
pub fn use_this_machine() -> (Arc<Limits>, Option<String>) {
    let settings = crate::paths::config_dir();
    let hw = read_hardware(settings.as_deref(), GpuReading::Saved);
    let (own, note) = Own::load(settings.as_deref());
    let l = Limits::compute(&hw, &own);
    if let Ok(mut g) = HARDWARE.write() {
        *g = Some(Arc::new(hw));
    }
    (install(l), note)
}

/// Read the person's own values again (after the window or `jetcarp limits` changed them), on
/// the hardware already read; this machine's limits from then on.
pub fn reload() -> Arc<Limits> {
    let settings = crate::paths::config_dir();
    let hw = hardware()
        .map(|h| (*h).clone())
        .unwrap_or_else(|| read_hardware(settings.as_deref(), GpuReading::Saved));
    let (own, _) = Own::load(settings.as_deref());
    if let Ok(mut g) = HARDWARE.write() {
        *g = Some(Arc::new(hw.clone()));
    }
    install(Limits::compute(&hw, &own))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_counts_for_people() {
        assert_eq!(bytes_text(0), "0 B");
        assert_eq!(bytes_text(512), "512 B");
        assert_eq!(bytes_text(3 << 10), "3 KB");
        assert_eq!(bytes_text(6144 << 20), "6 GB");
        assert_eq!(bytes_text(3 << 29), "1.5 GB");
        assert_eq!(bytes_text(24 << 30), "24 GB");
        assert_eq!(bytes_text(112 << 30), "112 GB");
        assert_eq!(count_text(2000), "2,000");
        assert_eq!(count_text(64_000_000), "64 million");
        assert_eq!(count_text(4_294_967_295), "4.3 billion");
        assert_eq!(count_text(150_000_000), "150 million");
    }

    #[test]
    fn the_reference_until_asked() {
        // Tests never ask for this machine's limits, so they get the reference's.
        assert_eq!(*current(), Limits::reference());
    }

    /// What a fresh reading found of the GPUs is kept in `machine.json` for the next start,
    /// beside the score already there, and the file is not written again when it holds them.
    #[test]
    fn the_gpus_read_afresh_are_kept_beside_the_score() {
        let d = crate::scratch::ScratchDir::new("machine-keep-gpus");
        std::fs::create_dir_all(&d).unwrap();
        let gpus = vec![GpuMemory {
            vendor: 0x10de,
            device: 0x2684,
            dedicated: Some(24 << 30),
            source: "NVML".into(),
            address: "0000:01:00.0".into(),
            ..Default::default()
        }];
        // No file yet: one is made, holding only them.
        keep_gpus(&d, &gpus);
        let f = MachineFile::load(Some(&d)).unwrap();
        assert_eq!((f.version, &f.gpus, f.score.is_none()), (1, &gpus, true));
        // Beside a score: the score stays.
        MachineFile {
            score: Some(Score {
                benchmark: score::BENCHMARK,
                score: 7.0,
                ..Default::default()
            }),
            ..f
        }
        .save(&d)
        .unwrap();
        let mut other = gpus.clone();
        other[0].dedicated = Some(23 << 30);
        keep_gpus(&d, &other);
        let f = MachineFile::load(Some(&d)).unwrap();
        assert_eq!(
            (f.gpus.clone(), f.score.map(|s| s.score)),
            (other.clone(), Some(7.0))
        );
        // The same again: not written.
        let file = MachineFile::file(&d);
        let before = std::fs::metadata(&file).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        keep_gpus(&d, &other);
        assert_eq!(
            std::fs::metadata(&file).unwrap().modified().unwrap(),
            before
        );
    }

    /// The note that the computer changed says what changed: a GPU whose memory is now read
    /// (with where from), a new GPU, the memory, the processor.
    #[test]
    fn the_change_is_said() {
        let nvidia = |dedicated: Option<u64>, source: &str| GpuMemory {
            vendor: 0x10de,
            device: 0x2684,
            dedicated,
            source: source.into(),
            not_read: dedicated.is_none().then(|| gpu_load::NO_NVML.to_string()),
            address: "0000:01:00.0".into(),
            ..Default::default()
        };
        let before = Hardware {
            os: "linux".into(),
            cpu: "A".into(),
            threads: 8,
            memory_total: 32 << 30,
            ..Default::default()
        };
        let measured = |hw: &Hardware, all_gpus| MachineFile {
            hardware: Some(hw.clone()),
            fingerprint: hw.fingerprint(),
            score: Some(Score {
                benchmark: score::BENCHMARK,
                ..Default::default()
            }),
            all_gpus,
            ..Default::default()
        };
        let now = Hardware {
            gpus: vec![nvidia(Some(24 << 30), "NVML")],
            ..before.clone()
        };
        // Measured before NVIDIA's GPUs were read (they were left out): now read.
        assert_eq!(
            measured(&before, false).stale(&now).as_deref(),
            Some(
                "this computer has changed since it was measured (its GPU 10de:2684's memory is \
                 now read, 24 GB from NVIDIA's NVML)"
            )
        );
        // Measured with it listed but not read: now read.
        let unread = Hardware {
            gpus: vec![nvidia(None, "")],
            ..before.clone()
        };
        assert_eq!(
            measured(&unread, true).stale(&now).as_deref(),
            Some(
                "this computer has changed since it was measured (its GPU 10de:2684's memory is \
                 now read, 24 GB from NVIDIA's NVML)"
            )
        );
        // Measured since every GPU is listed, without it: a new GPU.
        assert_eq!(
            measured(&before, true).stale(&now).as_deref(),
            Some(
                "this computer has changed since it was measured (it has a new GPU, 10de:2684, \
                 24 GB from NVIDIA's NVML)"
            )
        );
        // Its memory, a GPU gone, and the processor.
        let other = Hardware {
            cpu: "B".into(),
            memory_total: 64 << 30,
            ..before.clone()
        };
        assert_eq!(
            measured(&now, true).stale(&other).as_deref(),
            Some(
                "this computer has changed since it was measured (its processor is now B; its \
                 memory changed from 32 GB to 64 GB; it no longer has GPU 10de:2684)"
            )
        );
    }

    #[test]
    fn measuring_again_is_suggested_once_per_change() {
        let d = crate::scratch::ScratchDir::new("machine-suggest");
        std::fs::create_dir_all(&d).unwrap();
        let hw = Hardware {
            os: "linux".into(),
            cpu: "A".into(),
            threads: 8,
            memory_total: 32 << 30,
            ..Default::default()
        };
        // Never measured: nothing to suggest.
        assert_eq!(suggest_measuring(Some(&d), &hw), None);
        let f = MachineFile {
            hardware: Some(hw.clone()),
            fingerprint: hw.fingerprint(),
            score: Some(Score {
                benchmark: score::BENCHMARK,
                ..Default::default()
            }),
            ..Default::default()
        };
        f.save(&d).unwrap();
        // The same machine: nothing.
        assert_eq!(suggest_measuring(Some(&d), &hw), None);
        // A new memory split: once.
        let changed = Hardware {
            memory_total: 64 << 30,
            ..hw.clone()
        };
        let s = suggest_measuring(Some(&d), &changed).unwrap();
        assert!(s.starts_with("this computer has changed"), "{s}");
        assert_eq!(suggest_measuring(Some(&d), &changed), None);
        // An older benchmark's score.
        let old = MachineFile {
            score: Some(Score {
                benchmark: 0,
                ..Default::default()
            }),
            ..f
        };
        old.save(&d).unwrap();
        let s = suggest_measuring(Some(&d), &hw).unwrap();
        assert!(s.contains("older benchmark (score v0"), "{s}");
    }
}
