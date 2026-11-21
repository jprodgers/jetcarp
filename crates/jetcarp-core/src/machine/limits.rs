//! The machine's limits, as one table. Each limit is the person's own value if they set one
//! (`limits.json` in the settings folder), else the machine's suggestion, and never above its
//! hard ceiling in the code.
//!
//! **Memory** comes first: two linked values, *Reserved for the System* and *Reserved for
//! JETCARP*, which add up to the machine's memory (setting one sets the other). A smaller machine
//! leaves half its memory to the system; a bigger one reserves at most [`SYSTEM_CAP`] for the
//! system and gives JETCARP the rest. Every other memory limit is a share of JETCARP's part,
//! scaled from the value the code had fixed before 0.4.3 on a 16 GB machine ([`REFERENCE`]), so
//! such a machine keeps the limits it had, a bigger one gets more and a smaller one less.
//!
//! **Limits only allow or refuse.** A world too big for the machine is refused with the reason,
//! never shrunk or altered, so anyone with a big enough machine gets the same result.

use super::hardware::Hardware;
use super::{bytes_text, count_text};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const GIB: u64 = 1 << 30;
const MIB: u64 = 1 << 20;

/// The machine the fixed limits before 0.4.3 were set for: 16 GB, so JETCARP's part is 8 GB.
pub const REFERENCE: u64 = 16 * GIB;
/// The most a suggestion reserves for the system.
pub const SYSTEM_CAP: u64 = 16 * GIB;
/// The least either side of the memory split may have.
pub const MEMORY_FLOOR: u64 = 512 * MIB;

/// The largest lattice grid the code can number: cells are numbered with 32-bit integers in
/// places (commands, seeds, ants, layers), so every index must fit in one (0.4.3 checked each
/// such place, `machine::limits` tests the extremes).
pub const CELL_CEILING: u64 = u32::MAX as u64;
/// The largest world the GPU simulator takes: it numbers cells and buffer words with 32-bit
/// integers and needs room for twice the cells in one buffer.
pub const GPU_CELL_CEILING: u64 = (u32::MAX / 2) as u64;
/// HashLife's node store numbers its nodes with 32-bit integers, and its table grows to twice
/// the nodes.
pub const NODE_CEILING: u64 = 1 << 31;
/// An AI's worlds may have this many cells at the small end of its share: the MCP server's
/// limit before 0.4.3.
pub const AI_SMALL_CELLS: u64 = 4_000_000;
/// Memory an AI's world may take at the small end of its share: enough for any world the MCP
/// server allowed before 0.4.3 (4 million cells of any kind, a level-7 patch), so there the cell
/// limit decides, as it did.
pub const AI_SMALL_BYTES: u64 = 512 * MIB;
/// Bytes per cell of a Life world, for limits counted in cells ([`super::cost::field_bytes`]).
pub const LIFE_CELL_BYTES: u64 = 8;
/// Bytes per cell of a one-field world on the GPU (two state buffers, the snapshot, staging).
pub const GPU_CELL_BYTES: u64 = 16;

/// The settings file of the person's own values.
pub const LIMITS_FILE: &str = "limits.json";

/// One limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Id {
    ReservedSystem,
    ReservedJetcarp,
    World,
    GpuWorld,
    HostCells,
    HostMemory,
    HashlifeNodes,
    HuntCache,
    MediaPixels,
    WallpaperCells,
    AnimationFrames,
    AiShare,
}

/// What a limit counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Bytes,
    Cells,
    Nodes,
    Pixels,
    Frames,
    /// A fraction from 0 (small) to 1 (all of it).
    Share,
}

impl Id {
    pub const ALL: [Id; 12] = [
        Id::ReservedSystem,
        Id::ReservedJetcarp,
        Id::World,
        Id::GpuWorld,
        Id::HostCells,
        Id::HostMemory,
        Id::HashlifeNodes,
        Id::HuntCache,
        Id::MediaPixels,
        Id::WallpaperCells,
        Id::AnimationFrames,
        Id::AiShare,
    ];

    /// Its name on the command line and in `limits.json`.
    pub fn name(self) -> &'static str {
        match self {
            Id::ReservedSystem => "reserved-system",
            Id::ReservedJetcarp => "reserved-jetcarp",
            Id::World => "world",
            Id::GpuWorld => "gpu-world",
            Id::HostCells => "host-cells",
            Id::HostMemory => "host-memory",
            Id::HashlifeNodes => "hashlife-nodes",
            Id::HuntCache => "hunt-cache",
            Id::MediaPixels => "media-pixels",
            Id::WallpaperCells => "wallpaper-cells",
            Id::AnimationFrames => "animation-frames",
            Id::AiShare => "ai-share",
        }
    }

    pub fn parse(s: &str) -> Option<Id> {
        let s = s.trim().to_ascii_lowercase().replace('_', "-");
        Id::ALL.into_iter().find(|i| i.name() == s)
    }

    /// Its name in the window.
    pub fn label(self) -> &'static str {
        match self {
            Id::ReservedSystem => "Reserved for the System",
            Id::ReservedJetcarp => "Reserved for JETCARP",
            Id::World => "Largest world",
            Id::GpuWorld => "Largest world on the GPU",
            Id::HostCells => "Desktop background: cells",
            Id::HostMemory => "Desktop background: memory",
            Id::HashlifeNodes => "HashLife: nodes",
            Id::HuntCache => "Hunt caches",
            Id::MediaPixels => "Media: pixels in one animation",
            Id::WallpaperCells => "Wallpaper export: fresh grid",
            Id::AnimationFrames => "Animation export: frames",
            Id::AiShare => "An AI's share",
        }
    }

    /// What it limits, in a sentence.
    pub fn what(self) -> &'static str {
        match self {
            Id::ReservedSystem => "Memory left to the system and other programs.",
            Id::ReservedJetcarp => {
                "Memory JETCARP may use; every other memory limit is a share of it."
            }
            Id::World => {
                "Memory one world may take (its cells, and a patch's graph); a larger one is refused, \
                 or in the app built only when asked."
            }
            Id::GpuWorld => {
                "Cells a world may have to be stepped on the GPU; a larger one steps on the processor, \
                 with the same result."
            }
            Id::HostCells => "Cells one world of the desktop background may have.",
            Id::HostMemory => {
                "Memory the desktop background may use before it gives up its largest world."
            }
            Id::HashlifeNodes => {
                "Nodes HashLife keeps before it collects unused ones (about 45 bytes each)."
            }
            Id::HuntCache => "Memory a soup hunt's object caches may hold together.",
            Id::MediaPixels => {
                "Pixels (width x height x frames) one catalog animation may hold in memory."
            }
            Id::WallpaperCells => "Cells of a fresh grid the app's wallpaper export may build.",
            Id::AnimationFrames => "Frames an animation exported from the app may have.",
            Id::AiShare => {
                "How much of JETCARP's limits an AI's worlds (MCP and its scripts) may use: from small \
                 (4 million cells, as before) to all of it."
            }
        }
    }

    pub fn unit(self) -> Unit {
        match self {
            Id::ReservedSystem
            | Id::ReservedJetcarp
            | Id::World
            | Id::HostMemory
            | Id::HuntCache => Unit::Bytes,
            Id::GpuWorld | Id::HostCells | Id::WallpaperCells => Unit::Cells,
            Id::HashlifeNodes => Unit::Nodes,
            Id::MediaPixels => Unit::Pixels,
            Id::AnimationFrames => Unit::Frames,
            Id::AiShare => Unit::Share,
        }
    }

    /// The value fixed in the code before 0.4.3 (on every machine), for the limits that had one.
    pub fn before(self) -> Option<f64> {
        Some(match self {
            Id::HostCells => 64_000_000.0,
            Id::HostMemory => (6144 * MIB) as f64,
            Id::HashlifeNodes => 48_000_000.0,
            Id::HuntCache => (1536 * MIB) as f64,
            Id::MediaPixels => 300_000_000.0,
            Id::WallpaperCells => 16_000_000.0,
            Id::AnimationFrames => 2000.0,
            _ => return None,
        })
    }
}

/// A value in a limit's unit, for people: `24 GB`, `64 million cells`, `25%`.
pub fn value_text(unit: Unit, v: f64) -> String {
    match unit {
        Unit::Bytes => bytes_text(v as u64),
        Unit::Cells => format!("{} cells", count_text(v as u64)),
        Unit::Nodes => format!("{} nodes", count_text(v as u64)),
        Unit::Pixels => format!("{} pixels", count_text(v as u64)),
        Unit::Frames => format!("{} frames", count_text(v as u64)),
        Unit::Share if v <= 0.0 => "small".into(),
        Unit::Share if v >= 1.0 => "all of it".into(),
        Unit::Share => {
            let p = v * 100.0;
            if p >= 10.0 {
                format!("{p:.0}%")
            } else {
                format!("{}%", trim_zeros(&format!("{p:.2}")))
            }
        }
    }
}

fn trim_zeros(s: &str) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s.to_string()
    }
}

/// A value typed by a person, in a limit's unit: bytes as `24GB`, `512 MB`, `1.5 GiB` (GB means
/// 2^30 bytes here, as the system's memory figures), or a plain number of bytes; counts as
/// `64000000`, `64M`, `1.5 billion`; a share as `0.25`, `25%`, `small` or `all`.
pub fn parse_value(unit: Unit, text: &str) -> Result<f64, String> {
    let t = text.trim().to_ascii_lowercase().replace([',', '_'], "");
    let split = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(t.len());
    let (num, suffix) = t.split_at(split);
    let suffix = suffix.trim();
    if unit == Unit::Share {
        return match (num, suffix) {
            ("", "small") | ("", "none") => Ok(0.0),
            ("", "all") => Ok(1.0),
            (n, "%") => n
                .parse::<f64>()
                .map(|p| p / 100.0)
                .map_err(|_| format!("'{text}' is not a share")),
            (n, "") => n
                .parse::<f64>()
                .map_err(|_| format!("'{text}' is not a share")),
            _ => Err(format!(
                "'{text}' is not a share: give 0 to 1, a percentage, small or all"
            )),
        };
    }
    let n: f64 = num
        .parse()
        .map_err(|_| format!("'{text}' is not a number"))?;
    let mult = if unit == Unit::Bytes {
        match suffix {
            "" | "b" => 1.0,
            "k" | "kb" | "kib" => 1024.0,
            "m" | "mb" | "mib" => MIB as f64,
            "g" | "gb" | "gib" => GIB as f64,
            "t" | "tb" | "tib" => (GIB as f64) * 1024.0,
            _ => return Err(format!("'{text}': give bytes as 512MB, 24GB or 1.5TB")),
        }
    } else {
        match suffix {
            "" => 1.0,
            "k" | "thousand" => 1e3,
            "m" | "million" => 1e6,
            "b" | "billion" => 1e9,
            _ => {
                return Err(format!(
                    "'{text}': give a count as 64000000, 64M or 1.5 billion"
                ));
            }
        }
    };
    let v = (n * mult).round();
    if !v.is_finite() || v < 0.0 {
        return Err(format!("'{text}' is not a size"));
    }
    Ok(v)
}

/// The person's own values (`limits.json`): only what they set, so measuring the machine again
/// never undoes a choice. *Reserved for JETCARP* is kept as the system's part (the other side of
/// the same split), so a machine whose memory changes keeps what the system was given.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Own {
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default)]
    pub own: BTreeMap<Id, f64>,
}

fn one() -> u32 {
    1
}

impl Own {
    /// `<settings>/limits.json`.
    pub fn file(settings: &Path) -> PathBuf {
        settings.join(LIMITS_FILE)
    }

    /// The saved values; none when there is no file. A file that cannot be read is reported and
    /// counts as none (the suggestions apply).
    pub fn load(settings: Option<&Path>) -> (Own, Option<String>) {
        let Some(dir) = settings else {
            return (Own::default(), None);
        };
        let f = Own::file(dir);
        match std::fs::read_to_string(&f) {
            Ok(t) => match serde_json::from_str::<Own>(&t) {
                Ok(o) => (o, None),
                Err(e) => (
                    Own::default(),
                    Some(format!(
                        "{}: {e}; the machine's suggestions apply",
                        f.display()
                    )),
                ),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Own::default(), None),
            Err(e) => (
                Own::default(),
                Some(format!(
                    "{}: {e}; the machine's suggestions apply",
                    f.display()
                )),
            ),
        }
    }

    /// Writes them (whole, then moved into place); with none left, removes the file.
    pub fn save(&self, settings: &Path) -> Result<PathBuf, String> {
        let f = Own::file(settings);
        if self.own.is_empty() {
            return match std::fs::remove_file(&f) {
                Ok(()) => Ok(f),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(f),
                Err(e) => Err(format!("removing {}: {e}", f.display())),
            };
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        super::write_whole(&f, &(text + "\n"))?;
        Ok(f)
    }
}

/// Where a limit's value came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The machine's suggestion.
    Suggested,
    /// The person's own value.
    Own,
    /// Their own value, or the suggestion, was above the ceiling.
    Ceiling,
}

/// One limit as it applies on this machine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: Id,
    pub name: String,
    pub label: String,
    pub unit: Unit,
    /// What applies.
    pub value: f64,
    pub source: Source,
    /// The machine's suggestion, and how it was worked out.
    pub suggestion: f64,
    pub why: String,
    /// The person's own value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub own: Option<f64>,
    /// The least and most it may be.
    pub floor: f64,
    pub ceiling: f64,
    pub ceiling_why: String,
}

impl Entry {
    /// The value for people.
    pub fn text(&self) -> String {
        value_text(self.unit, self.value)
    }
    /// Where the value came from, for a refusal: `half of the 48 GB reserved for JETCARP, from
    /// its hardware`, or `your own value`.
    pub fn origin(&self) -> String {
        match self.source {
            Source::Own => "your own value".into(),
            Source::Suggested => format!("{}, from its hardware", self.why),
            Source::Ceiling => format!("the ceiling: {}", self.ceiling_why),
        }
    }
}

/// The limits on this machine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    /// The machine's memory these were worked out from, in bytes.
    pub memory_total: u64,
    /// The memory could not be read: these are the limits of a 16 GB machine.
    pub memory_estimated: bool,
    pub entries: Vec<Entry>,
}

/// A world over the limit: what it needs, the limit and where the limit came from.
#[derive(Clone, Debug, PartialEq)]
pub struct OverLimit {
    pub needs: u64,
    pub limit: u64,
    pub origin: String,
    /// It is the person's own value (so raising it is up to them, not the machine).
    pub own: bool,
}

impl std::fmt::Display for OverLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "this world needs {}; this machine's limit is {} ({}: Tools → This computer…, or \
             `jetcarp limits`)",
            bytes_text(self.needs),
            bytes_text(self.limit),
            self.origin
        )
    }
}

impl Limits {
    /// The limits of a 16 GB machine with no own values: the fixed limits before 0.4.3. What code
    /// gets that never asked for this machine's (tests, and programs using the crates as a
    /// library), so their results and messages are the same everywhere.
    pub fn reference() -> Limits {
        Limits::compute(
            &Hardware {
                memory_total: REFERENCE,
                ..Default::default()
            },
            &Own::default(),
        )
    }

    /// This machine's limits: its hardware and the person's own values.
    pub fn compute(hw: &Hardware, own: &Own) -> Limits {
        let estimated = hw.memory_total == 0;
        let total = if estimated {
            REFERENCE
        } else {
            hw.memory_total
        };
        let mut entries = vec![];
        let (lo, hi) = (
            MEMORY_FLOOR.min(total / 2),
            total.saturating_sub(MEMORY_FLOOR.min(total / 2)),
        );
        // The split: the system's part, then JETCARP's (the rest).
        let sys_suggest = (total / 2).min(SYSTEM_CAP);
        let sys_why = if total / 2 <= SYSTEM_CAP {
            format!("half of its {}", bytes_text(total))
        } else {
            format!(
                "{} of its {}, the most a suggestion leaves the system",
                bytes_text(SYSTEM_CAP),
                bytes_text(total)
            )
        };
        let sys_own = own.own.get(&Id::ReservedSystem).copied();
        let sys = pick(sys_suggest as f64, sys_own, lo as f64, hi as f64);
        let system = sys.0 as u64;
        let jetcarp = total - system;
        let ceiling_why = format!("its {} of memory", bytes_text(total));
        entries.push(Entry {
            value: sys.0,
            source: sys.1,
            suggestion: sys_suggest as f64,
            why: sys_why,
            own: sys_own,
            floor: lo as f64,
            ceiling: hi as f64,
            ceiling_why: ceiling_why.clone(),
            ..entry(Id::ReservedSystem)
        });
        entries.push(Entry {
            value: jetcarp as f64,
            source: sys.1,
            suggestion: (total - sys_suggest) as f64,
            why: format!("the rest of its {}", bytes_text(total)),
            own: sys_own.map(|s| total as f64 - s),
            floor: lo as f64,
            ceiling: hi as f64,
            ceiling_why,
            ..entry(Id::ReservedJetcarp)
        });
        let m = jetcarp as f64;
        let share = m / (REFERENCE / 2) as f64;
        let of_jetcarp = format!("of the {} reserved for JETCARP", bytes_text(jetcarp));
        let scaled = |id: Id| -> (f64, String) {
            let b = id.before().unwrap_or(0.0);
            (
                (b * share).round().max(1.0),
                format!(
                    "{} on a 16 GB machine, scaled to the {} reserved for JETCARP",
                    value_text(id.unit(), b),
                    bytes_text(jetcarp)
                ),
            )
        };
        let mut add = |id: Id, suggestion: f64, why: String, ceiling: f64, ceiling_why: String| {
            let o = own.own.get(&id).copied();
            let (value, source) = pick(suggestion, o, 0.0, ceiling);
            entries.push(Entry {
                value,
                source,
                suggestion,
                why,
                own: o,
                floor: 0.0,
                ceiling,
                ceiling_why,
                ..entry(id)
            });
        };
        let all = format!("all {of_jetcarp}");
        add(
            Id::World,
            (m / 2.0).floor(),
            format!("half {of_jetcarp}"),
            m,
            all.clone(),
        );
        let (gpu, gpu_why) = gpu_suggestion(hw, m);
        add(
            Id::GpuWorld,
            gpu.min(GPU_CELL_CEILING as f64),
            gpu_why,
            GPU_CELL_CEILING as f64,
            "what the GPU simulator can number".into(),
        );
        let (v, why) = scaled(Id::HostMemory);
        add(Id::HostMemory, v, why, m, all.clone());
        let (v, why) = scaled(Id::HostCells);
        add(
            Id::HostCells,
            v.min(CELL_CEILING as f64),
            why,
            CELL_CEILING as f64,
            "what the code can number".into(),
        );
        let (v, why) = scaled(Id::HashlifeNodes);
        add(
            Id::HashlifeNodes,
            v.min(NODE_CEILING as f64),
            why,
            NODE_CEILING as f64,
            "what HashLife can number".into(),
        );
        let (v, why) = scaled(Id::HuntCache);
        add(Id::HuntCache, v, why, m, all.clone());
        let (v, why) = scaled(Id::MediaPixels);
        add(
            Id::MediaPixels,
            v,
            why,
            (m / 4.0).floor(),
            format!("all {of_jetcarp}, at 4 bytes a pixel"),
        );
        let (v, why) = scaled(Id::WallpaperCells);
        add(
            Id::WallpaperCells,
            v.min(CELL_CEILING as f64),
            why,
            CELL_CEILING as f64,
            "what the code can number".into(),
        );
        let (v, why) = scaled(Id::AnimationFrames);
        add(
            Id::AnimationFrames,
            v,
            why,
            1_000_000.0,
            "a million frames".into(),
        );
        add(
            Id::AiShare,
            0.0,
            "small, until you give an AI more".into(),
            1.0,
            "all of JETCARP's limits".into(),
        );
        Limits {
            memory_total: total,
            memory_estimated: estimated,
            entries,
        }
    }

    pub fn get(&self, id: Id) -> &Entry {
        self.entries
            .iter()
            .find(|e| e.id == id)
            .expect("every limit is in the table")
    }

    fn u(&self, id: Id) -> u64 {
        self.get(id).value as u64
    }

    /// Memory reserved for JETCARP, bytes.
    pub fn jetcarp_memory(&self) -> u64 {
        self.u(Id::ReservedJetcarp)
    }
    /// Memory one world may take, bytes.
    pub fn world_bytes(&self) -> u64 {
        self.u(Id::World)
    }
    /// Cells a world may have on the GPU.
    pub fn gpu_world_cells(&self) -> u64 {
        self.u(Id::GpuWorld)
    }
    pub fn host_cells(&self) -> u64 {
        self.u(Id::HostCells)
    }
    pub fn host_memory_mb(&self) -> u64 {
        self.u(Id::HostMemory) / MIB
    }
    pub fn hashlife_nodes(&self) -> usize {
        self.u(Id::HashlifeNodes) as usize
    }
    pub fn hunt_cache_bytes(&self) -> usize {
        self.u(Id::HuntCache) as usize
    }
    pub fn media_pixels(&self) -> u64 {
        self.u(Id::MediaPixels)
    }
    pub fn wallpaper_cells(&self) -> u64 {
        self.u(Id::WallpaperCells)
    }
    pub fn animation_frames(&self) -> u32 {
        self.u(Id::AnimationFrames).min(u32::MAX as u64) as u32
    }
    pub fn ai_share(&self) -> f64 {
        self.get(Id::AiShare).value
    }

    /// Memory an AI's world may take: its share of [`world_bytes`](Self::world_bytes), at least
    /// [`AI_SMALL_BYTES`], never more than the machine's own limit.
    pub fn ai_world_bytes(&self) -> u64 {
        let all = self.world_bytes();
        ((all as f64 * self.ai_share()) as u64)
            .max(AI_SMALL_BYTES)
            .min(all)
    }

    /// Cells an AI's world may have: [`AI_SMALL_CELLS`] at the small end, else a Life world in
    /// [`ai_world_bytes`](Self::ai_world_bytes); never more than the machine's own limit holds,
    /// nor the code's ceiling. The MCP tools' `max_cells`.
    pub fn ai_cells(&self) -> u64 {
        let c = if self.ai_share() <= 0.0 {
            AI_SMALL_CELLS
        } else {
            (self.ai_world_bytes() / LIFE_CELL_BYTES).max(AI_SMALL_CELLS)
        };
        c.min(self.world_bytes() / LIFE_CELL_BYTES)
            .clamp(1, CELL_CEILING)
    }

    /// Refuse a world that needs more memory than one may take here.
    pub fn check_world(&self, needs: u64) -> Result<(), OverLimit> {
        let e = self.get(Id::World);
        self.check_against(needs, e.value as u64, e)
    }

    /// Refuse a world over `limit` bytes (an AI's share), saying where the limit came from.
    pub fn check_ai_world(&self, needs: u64) -> Result<(), OverLimit> {
        let limit = self.ai_world_bytes();
        if needs <= limit {
            return Ok(());
        }
        let share = self.get(Id::AiShare);
        Err(OverLimit {
            needs,
            limit,
            origin: format!(
                "an AI's share, {}, of the machine's {}",
                share.text(),
                self.get(Id::World).text()
            ),
            own: share.source == Source::Own,
        })
    }

    fn check_against(&self, needs: u64, limit: u64, e: &Entry) -> Result<(), OverLimit> {
        if needs <= limit {
            return Ok(());
        }
        Err(OverLimit {
            needs,
            limit,
            origin: e.origin(),
            own: e.source == Source::Own,
        })
    }

    /// `own` with `id` set to `value` (typed in the limit's unit), after checking it against
    /// this machine's floor and ceiling. *Reserved for JETCARP* is kept as the system's part.
    pub fn set(&self, own: &Own, id: Id, value: f64) -> Result<Own, String> {
        let e = self.get(id);
        if !value.is_finite() || value < e.floor || value > e.ceiling {
            return Err(format!(
                "{} must be {} to {} here ({}), not {}",
                e.label,
                value_text(e.unit, e.floor),
                value_text(e.unit, e.ceiling),
                e.ceiling_why,
                value_text(e.unit, value)
            ));
        }
        let mut o = own.clone();
        match id {
            Id::ReservedJetcarp => {
                o.own
                    .insert(Id::ReservedSystem, self.memory_total as f64 - value);
            }
            _ => {
                o.own.insert(id, value);
            }
        }
        Ok(o)
    }

    /// `own` without `id` (both memory values go together), or without any (`None`).
    pub fn reset(own: &Own, id: Option<Id>) -> Own {
        let mut o = own.clone();
        match id {
            None => o.own.clear(),
            Some(Id::ReservedJetcarp) | Some(Id::ReservedSystem) => {
                o.own.remove(&Id::ReservedSystem);
                o.own.remove(&Id::ReservedJetcarp);
            }
            Some(i) => {
                o.own.remove(&i);
            }
        }
        o
    }
}

/// The value that applies: the own value, else the suggestion, within floor and ceiling.
fn pick(suggestion: f64, own: Option<f64>, floor: f64, ceiling: f64) -> (f64, Source) {
    let (v, s) = match own {
        Some(o) => (o, Source::Own),
        None => (suggestion, Source::Suggested),
    };
    if v > ceiling {
        (ceiling, Source::Ceiling)
    } else if v < floor {
        (floor, Source::Ceiling)
    } else {
        (v, s)
    }
}

fn entry(id: Id) -> Entry {
    Entry {
        id,
        name: id.name().into(),
        label: id.label().into(),
        unit: id.unit(),
        value: 0.0,
        source: Source::Suggested,
        suggestion: 0.0,
        why: String::new(),
        own: None,
        floor: 0.0,
        ceiling: 0.0,
        ceiling_why: String::new(),
    }
}

/// The GPU's suggestion: half of the largest GPU memory the system reports, at
/// [`GPU_CELL_BYTES`] a cell; with none reported (or one sharing the system's memory), JETCARP's
/// largest world, said as an estimate. The GPU's own buffer limits come on top, where it opens.
fn gpu_suggestion(hw: &Hardware, jetcarp: f64) -> (f64, String) {
    let dedicated = hw
        .gpus
        .iter()
        .filter(|g| !g.unified)
        .filter_map(|g| g.dedicated)
        .max();
    match dedicated {
        Some(v) if v > 0 => (
            ((v / 2) / GPU_CELL_BYTES) as f64,
            format!(
                "half of the GPU's {} at {GPU_CELL_BYTES} bytes a cell, within its buffer limits",
                bytes_text(v)
            ),
        ),
        _ => (
            ((jetcarp / 2.0) as u64 / GPU_CELL_BYTES) as f64,
            format!(
                "an estimate: half of the {} reserved for JETCARP at {GPU_CELL_BYTES} bytes a cell, \
                 within the GPU's buffer limits",
                bytes_text(jetcarp as u64)
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(gib: u64) -> Hardware {
        Hardware {
            memory_total: gib * GIB,
            ..Default::default()
        }
    }

    #[test]
    fn the_split_follows_the_machine() {
        // Half to the system on smaller machines; at most 16 GB on bigger ones.
        for (total, jetcarp) in [(8, 4), (16, 8), (32, 16), (64, 48), (96, 80), (128, 112)] {
            let l = Limits::compute(&machine(total), &Own::default());
            assert_eq!(
                (l.jetcarp_memory(), l.u(Id::ReservedSystem)),
                (jetcarp * GIB, (total - jetcarp) * GIB),
                "{total} GB"
            );
        }
    }

    #[test]
    fn the_reference_machine_keeps_the_old_limits() {
        let l = Limits::reference();
        assert_eq!(l.host_cells(), 64_000_000);
        assert_eq!(l.host_memory_mb(), 6144);
        assert_eq!(l.hashlife_nodes(), 48_000_000);
        assert_eq!(l.hunt_cache_bytes(), 1536 << 20);
        assert_eq!(l.media_pixels(), 300_000_000);
        assert_eq!(l.wallpaper_cells(), 16_000_000);
        assert_eq!(l.animation_frames(), 2000);
        assert_eq!(l.ai_cells(), AI_SMALL_CELLS);
        assert_eq!(l.world_bytes(), 4 * GIB);
        assert!(l.entries.iter().all(|e| e.source == Source::Suggested));
    }

    #[test]
    fn memory_limits_scale_with_jetcarps_part() {
        let big = Limits::compute(&machine(64), &Own::default());
        // 48 GB for JETCARP is six times the reference's 8.
        assert_eq!(big.host_cells(), 6 * 64_000_000);
        assert_eq!(big.hashlife_nodes(), 6 * 48_000_000);
        assert_eq!(big.world_bytes(), 24 * GIB);
        let small = Limits::compute(&machine(4), &Own::default());
        assert_eq!(small.host_cells(), 16_000_000);
        assert_eq!(small.animation_frames(), 500);
    }

    #[test]
    fn own_values_win_within_the_ceiling() {
        let hw = machine(64);
        let l = Limits::compute(&hw, &Own::default());
        let own = l
            .set(&Own::default(), Id::World, (10 * GIB) as f64)
            .unwrap();
        let l2 = Limits::compute(&hw, &own);
        assert_eq!(l2.world_bytes(), 10 * GIB);
        assert_eq!(l2.get(Id::World).source, Source::Own);
        // Above the ceiling (all of JETCARP's 48 GB): refused when set...
        let e = l
            .set(&Own::default(), Id::World, (60 * GIB) as f64)
            .unwrap_err();
        assert!(e.contains("Largest world must be 0 B to 48 GB"), "{e}");
        // ...and held to the ceiling when the machine shrinks under a saved value.
        let small = Limits::compute(&machine(16), &own);
        assert_eq!(small.world_bytes(), 8 * GIB);
        assert_eq!(small.get(Id::World).source, Source::Ceiling);
        // Back to the suggestion.
        let back = Limits::compute(&hw, &Limits::reset(&own, Some(Id::World)));
        assert_eq!(back.world_bytes(), 24 * GIB);
    }

    #[test]
    fn the_two_memory_values_are_linked() {
        let hw = machine(96);
        let l = Limits::compute(&hw, &Own::default());
        // Reserve 32 GB for the system: JETCARP gets 64.
        let own = l
            .set(&Own::default(), Id::ReservedSystem, (32 * GIB) as f64)
            .unwrap();
        let l2 = Limits::compute(&hw, &own);
        assert_eq!(l2.jetcarp_memory(), 64 * GIB);
        // Giving JETCARP 90 GB keeps 6 for the system, saved as the system's part.
        let own = l.set(&own, Id::ReservedJetcarp, (90 * GIB) as f64).unwrap();
        assert_eq!(own.own.get(&Id::ReservedSystem), Some(&((6 * GIB) as f64)));
        assert!(!own.own.contains_key(&Id::ReservedJetcarp));
        let l3 = Limits::compute(&hw, &own);
        assert_eq!(
            (l3.jetcarp_memory(), l3.u(Id::ReservedSystem)),
            (90 * GIB, 6 * GIB)
        );
        // Every memory limit follows JETCARP's part.
        assert_eq!(l3.world_bytes(), 45 * GIB);
        // Resetting either resets both.
        let back = Limits::compute(&hw, &Limits::reset(&own, Some(Id::ReservedJetcarp)));
        assert_eq!(back.jetcarp_memory(), 80 * GIB);
        // Nothing for the system, or everything: refused.
        assert!(l.set(&Own::default(), Id::ReservedSystem, 0.0).is_err());
        assert!(
            l.set(&Own::default(), Id::ReservedJetcarp, (96 * GIB) as f64)
                .is_err()
        );
    }

    #[test]
    fn a_refusal_names_the_limit_and_where_it_came_from() {
        let l = Limits::compute(&machine(64), &Own::default());
        assert!(l.check_world(24 * GIB).is_ok());
        let e = l.check_world(31 * GIB).unwrap_err();
        assert_eq!(
            e.to_string(),
            "this world needs 31 GB; this machine's limit is 24 GB (half of the 48 GB reserved \
             for JETCARP, from its hardware: Tools → This computer…, or `jetcarp limits`)"
        );
        assert!(!e.own);
        let own = l.set(&Own::default(), Id::World, (2 * GIB) as f64).unwrap();
        let e = Limits::compute(&machine(64), &own)
            .check_world(3 * GIB)
            .unwrap_err();
        assert!(e.to_string().contains("(your own value: Tools"), "{e}");
        assert!(e.own);
    }

    #[test]
    fn an_ais_share() {
        let hw = machine(64);
        let l = Limits::compute(&hw, &Own::default());
        // Small by default: the 4 million cells of before, on any machine, and memory enough for
        // any world they allowed.
        assert_eq!(l.ai_cells(), AI_SMALL_CELLS);
        assert_eq!(
            Limits::compute(&machine(128), &Own::default()).ai_cells(),
            AI_SMALL_CELLS
        );
        assert!(l.check_ai_world(AI_SMALL_BYTES).is_ok());
        let e = l.check_ai_world(AI_SMALL_BYTES + 1).unwrap_err();
        assert!(
            e.to_string()
                .contains("an AI's share, small, of the machine's 24 GB"),
            "{e}"
        );
        // All of it: the machine's own limit.
        let all = Limits::compute(&hw, &l.set(&Own::default(), Id::AiShare, 1.0).unwrap());
        assert_eq!(all.ai_world_bytes(), all.world_bytes());
        assert_eq!(all.ai_cells(), 24 * GIB / 8);
        // A quarter.
        let q = Limits::compute(&hw, &l.set(&Own::default(), Id::AiShare, 0.25).unwrap());
        assert_eq!(q.ai_world_bytes(), 6 * GIB);
        assert_eq!(q.ai_cells(), 6 * GIB / 8);
        // A tiny share still allows what small does.
        let tiny = Limits::compute(&hw, &l.set(&Own::default(), Id::AiShare, 0.001).unwrap());
        assert_eq!(tiny.ai_world_bytes(), AI_SMALL_BYTES);
        assert_eq!(tiny.ai_cells(), AI_SMALL_BYTES / 8);
        assert!(l.set(&Own::default(), Id::AiShare, 1.5).is_err());
    }

    #[test]
    fn gpu_suggestions() {
        let mut hw = machine(96);
        let l = Limits::compute(&hw, &Own::default());
        // No GPU memory reported: an estimate from JETCARP's part.
        assert!(l.get(Id::GpuWorld).why.starts_with("an estimate"));
        hw.gpus.push(super::super::hardware::GpuMemory {
            vendor: 0x1002,
            device: 1,
            dedicated: Some(32 * GIB),
            shared: None,
            unified: false,
            source: "sysfs".into(),
            name: String::new(),
        });
        let l = Limits::compute(&hw, &Own::default());
        assert_eq!(l.gpu_world_cells(), 16 * GIB / GPU_CELL_BYTES);
        assert!(
            l.get(Id::GpuWorld)
                .why
                .starts_with("half of the GPU's 32 GB")
        );
    }

    #[test]
    fn unreadable_memory_gives_the_reference() {
        let l = Limits::compute(&Hardware::default(), &Own::default());
        assert!(l.memory_estimated);
        assert_eq!(l.entries, Limits::reference().entries);
    }

    #[test]
    fn values_typed_by_people() {
        assert_eq!(parse_value(Unit::Bytes, "24GB"), Ok((24 * GIB) as f64));
        assert_eq!(
            parse_value(Unit::Bytes, "1.5 GiB"),
            Ok((3 * GIB / 2) as f64)
        );
        assert_eq!(parse_value(Unit::Bytes, "512 MB"), Ok((512 * MIB) as f64));
        assert_eq!(parse_value(Unit::Bytes, "4096"), Ok(4096.0));
        assert_eq!(parse_value(Unit::Cells, "64M"), Ok(64e6));
        assert_eq!(parse_value(Unit::Cells, "1.5 billion"), Ok(1.5e9));
        assert_eq!(parse_value(Unit::Cells, "64,000,000"), Ok(64e6));
        assert_eq!(parse_value(Unit::Share, "25%"), Ok(0.25));
        assert_eq!(parse_value(Unit::Share, "all"), Ok(1.0));
        assert_eq!(parse_value(Unit::Share, "small"), Ok(0.0));
        assert!(parse_value(Unit::Bytes, "lots").is_err());
        assert!(parse_value(Unit::Bytes, "12 parsecs").is_err());
        assert!(parse_value(Unit::Share, "-1").is_err());
        assert_eq!(value_text(Unit::Share, 0.25), "25%");
        assert_eq!(value_text(Unit::Share, 0.005), "0.5%");
        assert_eq!(value_text(Unit::Cells, 64e6), "64 million cells");
    }

    #[test]
    fn own_values_round_trip_through_the_file() {
        let d = crate::scratch::ScratchDir::new("machine-limits-file");
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(Own::load(Some(&d)), (Own::default(), None));
        let l = Limits::compute(&machine(32), &Own::default());
        let own = l.set(&Own::default(), Id::HashlifeNodes, 1e8).unwrap();
        let f = own.save(&d).unwrap();
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("\"hashlife-nodes\": 100000000"), "{text}");
        assert_eq!(Own::load(Some(&d)).0, own);
        // Nothing of their own left: no file.
        Own::default().save(&d).unwrap();
        assert!(!f.exists());
        // A broken file is reported, and the suggestions apply.
        std::fs::write(&f, "{").unwrap();
        let (o, note) = Own::load(Some(&d));
        assert_eq!(o, Own::default());
        assert!(note.unwrap().contains("the machine's suggestions apply"));
    }
}
