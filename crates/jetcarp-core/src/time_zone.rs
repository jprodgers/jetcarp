//! The time zone JETCARP's local times are in: the output folders' names, records' times, a
//! machine score's date and the rotations' clock (#186).
//!
//! One rule on every system, jiff's own: a `TZ` that names a time zone decides it (an IANA name
//! such as `America/New_York`, a rule such as `EST5EDT,M3.2.0,M11.1.0`, or a TZif file); without
//! one, the system's own setting does. A `TZ` that is set but names no time zone JETCARP can read
//! (a misspelt name, an empty value) is never taken as UTC: it is ignored, the system's setting is
//! used, and the program says so once, on its standard error (its log), naming the value and the
//! zone used instead. `jetcarp doctor` shows the zone in use and where it came from ([`current`]).
//!
//! jiff reads the system's setting again after five minutes, so a time zone changed in the
//! system's settings reaches a running program within five minutes; daylight saving time, part of
//! the zone's rules, changes the clock at once.

use jiff::tz::TimeZone;
use std::ffi::{OsStr, OsString};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long the system's zone, read with an unreadable `TZ` set, is kept before it is read again:
/// as long as jiff keeps it otherwise.
const KEEP: Duration = Duration::from_secs(5 * 60);

/// Where the time zone in use came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The `TZ` environment variable.
    Tz,
    /// The system's own setting.
    System,
    /// Neither could be read: UTC, and a warning.
    Neither,
}

impl Source {
    /// As `jetcarp doctor --json` gives it.
    pub fn key(self) -> &'static str {
        match self {
            Source::Tz => "tz",
            Source::System => "system",
            Source::Neither => "none",
        }
    }
}

/// The time zone in use.
#[derive(Clone, Debug)]
pub struct Zone {
    pub tz: TimeZone,
    /// Its name: the IANA name, or the rule `TZ` gives, or, for the system's setting read without
    /// jiff (with an unreadable `TZ` set), what the system calls it.
    pub name: String,
    pub from: Source,
    /// A `TZ` that is set but was ignored, because it names no time zone JETCARP can read.
    pub tz_ignored: Option<String>,
}

impl Zone {
    /// The zone's offset from UTC now, as `+05:45` or `-05:00`.
    pub fn offset_now(&self) -> String {
        offset_text(self.tz.to_offset(jiff::Timestamp::now()).seconds())
    }

    /// The line `jetcarp doctor` gives it, after `time zone: `.
    pub fn describe(&self) -> String {
        let whence = match (self.from, &self.tz_ignored) {
            (Source::Tz, _) => "from TZ".to_string(),
            (Source::System, None) => "the system's".to_string(),
            (Source::System, Some(v)) => {
                format!(
                    "the system's; TZ={v:?} names no time zone JETCARP can read, so it is ignored"
                )
            }
            (Source::Neither, _) => "neither TZ nor the system's setting could be read".to_string(),
        };
        format!("{}, {} now ({whence})", self.name, self.offset_now())
    }

    /// The warning a program gives once when it can't use the `TZ` set, or reads no zone at all.
    pub fn warning(&self) -> Option<String> {
        match (self.from, &self.tz_ignored) {
            (Source::Neither, Some(v)) => Some(format!(
                "jetcarp: warning: TZ is set to {v:?}, which names no time zone JETCARP can read, \
                 and the system's time zone can't be read either: local times are in UTC"
            )),
            (Source::Neither, None) => Some(
                "jetcarp: warning: the system's time zone can't be read: local times are in UTC"
                    .to_string(),
            ),
            (_, Some(v)) => Some(format!(
                "jetcarp: warning: TZ is set to {v:?}, which names no time zone JETCARP can read; \
                 it is ignored, and local times are in {}, the system's time zone",
                self.name
            )),
            _ => None,
        }
    }
}

/// `+05:45`, `-05:00`; with seconds when the offset has them (historical zones).
fn offset_text(secs: i32) -> String {
    let sign = if secs < 0 { '-' } else { '+' };
    let s = secs.unsigned_abs();
    let (h, m, rest) = (s / 3600, s / 60 % 60, s % 60);
    if rest == 0 {
        format!("{sign}{h:02}:{m:02}")
    } else {
        format!("{sign}{h:02}:{m:02}:{rest:02}")
    }
}

/// The time zone local times are in now.
pub fn local() -> TimeZone {
    current().tz
}

/// The time zone in use now, where it came from, and a `TZ` ignored. Warns once per process for
/// each `TZ` value it ignores ([`Zone::warning`]).
pub fn current() -> Zone {
    let tz = std::env::var_os("TZ");
    if let Some(v) = &tz
        && let Some(z) = kept(v)
    {
        return z;
    }
    let zone = decide(tz.as_deref(), || TimeZone::try_system().ok(), system_alone);
    if let Some(v) = &tz
        && zone.tz_ignored.is_some()
    {
        keep(v, &zone);
    }
    if let Some(w) = zone.warning() {
        warn_once(tz, &w);
    }
    zone
}

/// The zone for a `TZ` of `tz` (None: not set): `with_jiff` reads it as jiff does (with `TZ` set,
/// from `TZ`; else the system's setting), and `system` reads the system's setting alone, for when
/// `TZ` can't be used.
fn decide(
    tz: Option<&OsStr>,
    with_jiff: impl FnOnce() -> Option<TimeZone>,
    system: impl FnOnce() -> Option<(TimeZone, String)>,
) -> Zone {
    let Some(value) = tz else {
        return match with_jiff() {
            Some(zone) => named(zone, Source::System, None, ""),
            None => neither(None),
        };
    };
    let shown = value.to_string_lossy().into_owned();
    // An empty TZ means UTC to some programs; to JETCARP it names no zone.
    if !value.is_empty()
        && let Some(zone) = with_jiff()
    {
        return named(zone, Source::Tz, None, &shown);
    }
    match system() {
        Some((zone, name)) => named(zone, Source::System, Some(shown), &name),
        None => neither(Some(shown)),
    }
}

/// A zone with its name: its IANA name, else `fallback`, else its offset now.
fn named(tz: TimeZone, from: Source, tz_ignored: Option<String>, fallback: &str) -> Zone {
    let name = tz
        .iana_name()
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .or_else(|| (!fallback.is_empty()).then(|| fallback.to_string()))
        .unwrap_or_else(|| {
            let now = tz.to_offset(jiff::Timestamp::now()).seconds();
            format!("UTC{}", offset_text(now))
        });
    Zone {
        tz,
        name,
        from,
        tz_ignored,
    }
}

fn neither(tz_ignored: Option<String>) -> Zone {
    Zone {
        tz: TimeZone::UTC,
        name: "UTC".into(),
        from: Source::Neither,
        tz_ignored,
    }
}

/// The zone read for an unreadable `TZ`, kept for [`KEEP`] so it isn't read again each frame.
static KEPT: Mutex<Option<(OsString, Instant, Zone)>> = Mutex::new(None);

fn kept(tz: &OsStr) -> Option<Zone> {
    let k = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    k.as_ref()
        .filter(|(v, at, _)| v == tz && at.elapsed() < KEEP)
        .map(|(_, _, z)| z.clone())
}

fn keep(tz: &OsStr, zone: &Zone) {
    let mut k = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    *k = Some((tz.to_owned(), Instant::now(), zone.clone()));
}

/// The `TZ` values warned of in this process.
static WARNED: Mutex<Vec<Option<OsString>>> = Mutex::new(Vec::new());

fn warn_once(tz: Option<OsString>, warning: &str) {
    let mut w = WARNED.lock().unwrap_or_else(|e| e.into_inner());
    if !w.contains(&tz) {
        w.push(tz);
        eprintln!("{warning}");
    }
}

/// The system's own time zone, read without `TZ`: `/etc/localtime`, as jiff reads it (a link into
/// a `zoneinfo` folder names the zone; else the file itself is read).
#[cfg(unix)]
fn system_alone() -> Option<(TimeZone, String)> {
    read_localtime(std::path::Path::new("/etc/localtime"))
}

#[cfg(unix)]
fn read_localtime(path: &std::path::Path) -> Option<(TimeZone, String)> {
    if let Ok(target) = std::fs::read_link(path)
        && let Some(t) = target.to_str()
        && let Some(at) = t.rfind("zoneinfo/")
    {
        let name = &t[at + "zoneinfo/".len()..];
        if let Ok(tz) = jiff::tz::db().get(name) {
            return Some((tz, name.to_string()));
        }
    }
    let data = std::fs::read(path).ok()?;
    let tz = TimeZone::tzif("", &data).ok()?;
    Some((tz, String::new()))
}

/// The system's own time zone, read without `TZ`: Windows' setting, its rules for this year made a
/// POSIX rule ([`windows_rule`]), named as Windows names it (`Eastern Standard Time`).
#[cfg(windows)]
fn system_alone() -> Option<(TimeZone, String)> {
    use windows_sys::Win32::System::Time::{
        DYNAMIC_TIME_ZONE_INFORMATION, GetDynamicTimeZoneInformation, TIME_ZONE_ID_INVALID,
    };
    let mut info = DYNAMIC_TIME_ZONE_INFORMATION::default();
    // SAFETY: the call fills in the struct it is given, which lives until it returns.
    if unsafe { GetDynamicTimeZoneInformation(&mut info) } == TIME_ZONE_ID_INVALID {
        return None;
    }
    let date = |d: &windows_sys::Win32::Foundation::SYSTEMTIME| WindowsDate {
        year: d.wYear,
        month: d.wMonth,
        day_of_week: d.wDayOfWeek,
        day: d.wDay,
        hour: d.wHour,
        minute: d.wMinute,
        second: d.wSecond,
    };
    let rule = windows_rule(&WindowsZone {
        bias: info.Bias,
        standard_bias: info.StandardBias,
        daylight_bias: info.DaylightBias,
        standard: date(&info.StandardDate),
        daylight: date(&info.DaylightDate),
        no_daylight_saving: info.DynamicDaylightTimeDisabled,
    });
    let tz = TimeZone::posix(&rule).ok()?;
    let key = &info.TimeZoneKeyName;
    let end = key.iter().position(|&c| c == 0).unwrap_or(key.len());
    Some((tz, String::from_utf16_lossy(&key[..end])))
}

#[cfg(not(any(unix, windows)))]
fn system_alone() -> Option<(TimeZone, String)> {
    None
}

/// A transition date as Windows gives it (`SYSTEMTIME`): with no year, the `day`th (5: last)
/// `day_of_week` (0: Sunday) of `month`; with a year, that day of the month.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
#[derive(Clone, Copy, Debug, Default)]
struct WindowsDate {
    year: u16,
    month: u16,
    day_of_week: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
}

/// Windows' time zone settings (`DYNAMIC_TIME_ZONE_INFORMATION`): offsets in minutes west of UTC.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
#[derive(Clone, Copy, Debug, Default)]
struct WindowsZone {
    bias: i32,
    standard_bias: i32,
    daylight_bias: i32,
    /// When daylight saving time ends.
    standard: WindowsDate,
    /// When it begins.
    daylight: WindowsDate,
    /// *Adjust for daylight saving time automatically* is off.
    no_daylight_saving: bool,
}

/// Windows' settings as a POSIX time zone rule, which jiff reads: `STD5:00DST4:00,M3.2.0/2:00:00,
/// M11.1.0/2:00:00` for Eastern time. Both write offsets west of UTC, and transition times in the
/// local time before the change.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn windows_rule(z: &WindowsZone) -> String {
    let offset = |minutes: i32| {
        let sign = if minutes < 0 { "-" } else { "" };
        let m = minutes.unsigned_abs();
        format!("{sign}{}:{:02}", m / 60, m % 60)
    };
    let standard = format!("STD{}", offset(z.bias + z.standard_bias));
    if z.no_daylight_saving || z.standard.month == 0 || z.daylight.month == 0 {
        return standard;
    }
    let when = |d: &WindowsDate| {
        let day = if d.year == 0 {
            format!("M{}.{}.{}", d.month, d.day.clamp(1, 5), d.day_of_week)
        } else {
            // A day of the month, as the day of a year with no 29 February.
            const BEFORE: [u16; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
            let m = usize::from(d.month.clamp(1, 12)) - 1;
            format!("J{}", BEFORE[m] + d.day.max(1))
        };
        format!("{day}/{}:{:02}:{:02}", d.hour, d.minute, d.second)
    };
    format!(
        "{standard}DST{},{},{}",
        offset(z.bias + z.daylight_bias),
        when(&z.daylight),
        when(&z.standard)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(name: &str) -> TimeZone {
        TimeZone::get(name).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    /// Moments through 2026 and 2027, a little over a month apart.
    fn moments() -> impl Iterator<Item = jiff::Timestamp> {
        (0..24i64).map(|k| jiff::Timestamp::from_second(1_767_268_800 + k * 2_700_000).unwrap())
    }

    /// The rule for each `TZ`: one that names a zone decides it; with none, the system's setting
    /// as jiff reads it; one that names none (or is empty) is ignored for the system's setting
    /// read alone, never taken as UTC; and with nothing readable, UTC, said.
    #[test]
    fn a_tz_decides_and_one_that_names_no_zone_is_ignored_never_taken_as_utc() {
        let system = || Some((zone("America/St_Johns"), "America/St_Johns".to_string()));
        let z = decide(
            Some(OsStr::new("Asia/Kathmandu")),
            || Some(zone("Asia/Kathmandu")),
            || panic!("the system's zone isn't read when TZ names one"),
        );
        assert_eq!((z.name.as_str(), z.from), ("Asia/Kathmandu", Source::Tz));
        assert_eq!(z.tz_ignored, None);
        assert_eq!(z.warning(), None);

        let z = decide(
            None,
            || Some(zone("America/St_Johns")),
            || panic!("jiff reads it"),
        );
        assert_eq!(
            (z.name.as_str(), z.from),
            ("America/St_Johns", Source::System)
        );
        assert_eq!(z.warning(), None);

        for bad in ["Not/A_Zone", ""] {
            let z = decide(Some(OsStr::new(bad)), || None, system);
            assert_eq!(z.name, "America/St_Johns", "{bad:?}");
            assert_eq!(z.from, Source::System, "{bad:?}");
            assert_eq!(z.tz_ignored.as_deref(), Some(bad));
            assert!(z.tz != TimeZone::UTC, "{bad:?}");
            let w = z.warning().unwrap();
            assert!(
                w.contains(&format!("{bad:?}")) && w.contains("America/St_Johns"),
                "{w}"
            );
            assert!(
                z.describe().contains("the system's; TZ="),
                "{}",
                z.describe()
            );
        }
        // An empty TZ isn't handed to jiff, which takes it as UTC.
        let z = decide(Some(OsStr::new("")), || Some(TimeZone::UTC), system);
        assert_eq!(z.name, "America/St_Johns");

        let z = decide(Some(OsStr::new("Not/A_Zone")), || None, || None);
        assert_eq!((z.from, z.tz.clone()), (Source::Neither, TimeZone::UTC));
        assert!(z.warning().unwrap().contains("UTC"));
        let z = decide(None, || None, || None);
        assert_eq!(z.from, Source::Neither);
        assert!(
            z.warning()
                .unwrap()
                .contains("the system's time zone can't be read")
        );
    }

    #[test]
    fn offsets_are_written_as_doctor_shows_them() {
        assert_eq!(offset_text(0), "+00:00");
        assert_eq!(offset_text(5 * 3600 + 45 * 60), "+05:45");
        assert_eq!(offset_text(-(3 * 3600 + 30 * 60)), "-03:30");
        assert_eq!(offset_text(-(4 * 60 + 56) * 60 - 2), "-04:56:02");
        let z = named(zone("Asia/Kathmandu"), Source::Tz, None, "");
        assert_eq!(z.describe(), "Asia/Kathmandu, +05:45 now (from TZ)");
    }

    /// `/etc/localtime` as a link into a `zoneinfo` folder gives the zone by name; as a file, the
    /// zone itself, unnamed.
    #[cfg(unix)]
    #[test]
    fn the_system_s_zone_is_read_from_localtime_as_a_link_or_a_file() {
        let dir = crate::scratch::ScratchDir::new("time-zone-localtime");
        std::fs::create_dir_all(dir.join("zoneinfo/Asia")).unwrap();
        let Ok(data) = std::fs::read("/usr/share/zoneinfo/Asia/Kathmandu") else {
            eprintln!("skipped: no system time zone database to copy a zone from");
            return;
        };
        std::fs::write(dir.join("zoneinfo/Asia/Kathmandu"), &data).unwrap();
        let link = dir.join("localtime-link");
        std::os::unix::fs::symlink(dir.join("zoneinfo/Asia/Kathmandu"), &link).unwrap();
        let (tz, name) = read_localtime(&link).unwrap();
        assert_eq!(name, "Asia/Kathmandu");
        assert_eq!(tz.iana_name(), Some("Asia/Kathmandu"));
        let file = dir.join("localtime-file");
        std::fs::write(&file, &data).unwrap();
        let (tz, name) = read_localtime(&file).unwrap();
        assert_eq!(name, "");
        for t in moments() {
            assert_eq!(tz.to_offset(t), zone("Asia/Kathmandu").to_offset(t));
        }
        let z = named(tz, Source::System, None, &name);
        assert_eq!(z.name, "UTC+05:45");
        assert!(read_localtime(&dir.join("nowhere")).is_none());
    }

    /// Windows' settings for a zone with daylight saving time in either hemisphere, one with none,
    /// and one with it switched off, made POSIX rules, give the same offsets as the IANA zones
    /// through 2026 and 2027.
    #[test]
    fn windows_settings_make_rules_with_the_zones_offsets() {
        let sunday = |month, day, hour| WindowsDate {
            month,
            day,
            hour,
            ..Default::default()
        };
        let eastern = WindowsZone {
            bias: 300,
            standard_bias: 0,
            daylight_bias: -60,
            standard: sunday(11, 1, 2),
            daylight: sunday(3, 2, 2),
            no_daylight_saving: false,
        };
        assert_eq!(
            windows_rule(&eastern),
            "STD5:00DST4:00,M3.2.0/2:00:00,M11.1.0/2:00:00"
        );
        // Sydney: daylight saving time from the first Sunday of October to the first of April.
        let sydney = WindowsZone {
            bias: -600,
            standard_bias: 0,
            daylight_bias: -60,
            standard: sunday(4, 1, 3),
            daylight: sunday(10, 1, 2),
            no_daylight_saving: false,
        };
        let india = WindowsZone {
            bias: -330,
            ..Default::default()
        };
        for (z, iana) in [
            (eastern, "America/New_York"),
            (sydney, "Australia/Sydney"),
            (india, "Asia/Kolkata"),
        ] {
            let rule = windows_rule(&z);
            let tz = TimeZone::posix(&rule).unwrap_or_else(|e| panic!("{rule}: {e}"));
            for t in moments() {
                assert_eq!(tz.to_offset(t), zone(iana).to_offset(t), "{rule} at {t}");
            }
        }
        let off = WindowsZone {
            no_daylight_saving: true,
            ..eastern
        };
        assert_eq!(windows_rule(&off), "STD5:00");
        let absolute = WindowsZone {
            standard: WindowsDate {
                year: 2026,
                month: 11,
                day: 1,
                hour: 2,
                ..Default::default()
            },
            daylight: WindowsDate {
                year: 2026,
                month: 3,
                day: 8,
                hour: 2,
                ..Default::default()
            },
            ..eastern
        };
        assert_eq!(
            windows_rule(&absolute),
            "STD5:00DST4:00,J67/2:00:00,J305/2:00:00"
        );
    }
}
