//! What JETCARP knows about the machine it runs on, kept as two kinds of knowledge:
//!
//! 1. **Capacity** ([`hardware`]): memory, the processor, each GPU's memory, read in
//!    milliseconds every time a JETCARP program starts. The memory-based limits come from this
//!    alone ([`limits`]), so a machine never measured still gets limits that fit it.
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
//! measured, its fingerprint, the score and its parts) and `limits.json` ([`limits::Own`]: only
//! the person's own values, so measuring again never undoes a choice).

pub mod bench;
pub mod cost;
pub mod hardware;
pub mod limits;
pub mod pool;
pub mod score;

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
            return Some(
                "this computer has changed since it was measured (its processor, memory, GPUs or \
                 system)"
                    .into(),
            );
        }
        None
    }
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

/// This machine's limits: its hardware, read now, and the person's own values from the
/// settings folder; used in this process from now on. The note says a `limits.json` that could
/// not be read.
pub fn use_this_machine() -> (Arc<Limits>, Option<String>) {
    let hw = Hardware::read();
    let settings = crate::paths::config_dir();
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
    let hw = hardware()
        .map(|h| (*h).clone())
        .unwrap_or_else(Hardware::read);
    let settings = crate::paths::config_dir();
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
