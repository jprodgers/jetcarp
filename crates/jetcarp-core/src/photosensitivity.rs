//! The photosensitive seizure warning (#96): what it says, and the answer given to it.
//!
//! JETCARP's animations can flash rapidly, so nothing animates on screen until someone has read
//! the warning and answered it. The app shows it before anything else, setup included; `jetcarp
//! setup` asks it in a terminal, or takes `--seizure-warning`; and the desktop background, the
//! screen saver, starting at login and the `host_control` tool refuse to start, with the warning,
//! until it has been accepted. Every front end reads the answer and the text from here.
//!
//! The answer is kept in [`FILE`] in the settings folder ([`crate::paths::config_dir`]), as
//! `{"version": 1, "answer": "no_disorder", "date": "...", "jetcarp": "...", "via": "app"}`. Both
//! answers count as accepted: someone who has, or may have, a seizure disorder is advised not to
//! use JETCARP, and may go on at their own risk. With no answer saved the warning shows again;
//! once saved it is not asked again.
//!
//! **The strobe guard** ([`crate::strobe`], #97) is on for everyone. Its two switches are saved
//! with the answer: `guard_off` for the app, and `desktop_guard_off` for the desktop background
//! and the screen saver, each with the date and the version that turned it off. Only someone who
//! answered *no disorder* can turn either off ([`set_guard_in`]), through the app's deliberate
//! confirmation ([`GUARD_OFF_TITLE`] and the rest); for someone [`at_risk`] the guard is on
//! whatever the file says. A new answer starts with both switches on. Nothing but the app's
//! Settings → Safety turns a switch off: the command line and the MCP server only read them.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The file in the settings folder that holds the answer.
pub const FILE: &str = "photosensitivity.json";

/// The format of [`FILE`].
pub const FORMAT: u32 = 1;

/// The warning's heading.
pub const TITLE: &str = "Photosensitive seizure warning";

/// The sentence that matters most, shown in bold.
pub const KEY: &str = "JETCARP's animations can flash and flicker rapidly, and can trigger \
                       seizures in people with photosensitive epilepsy.";

/// The rest of the warning, a paragraph each.
pub const DETAILS: [&str; 3] = [
    "Some rules change most of the screen every generation. Hodgepodge, for example, can flicker \
     across the whole field at each step. Run fast, full screen or as the desktop background, such \
     a rule can flicker many times a second.",
    "Flashing like this can also trigger a seizure, occasionally, in someone with no history of \
     seizures.",
    "If you feel dizzy, or notice altered vision, eye or muscle twitching, disorientation or any \
     involuntary movement, stop at once, look away from the screen, and see a doctor.",
];

/// The question the two answers reply to.
pub const QUESTION: &str = "Do you have epilepsy or a seizure disorder?";

/// The answer that goes on, as a button or a choice.
pub const NO_DISORDER: &str = "I don't have epilepsy or a seizure disorder";

/// The other answer, which leads to [`RISK_TITLE`].
pub const AT_RISK: &str = "I have, or may have, a seizure disorder";

/// The heading of the advice for someone at risk.
pub const RISK_TITLE: &str = "Please don't use JETCARP";

/// Its key sentence, in bold.
pub const RISK_KEY: &str = "If you have, or may have, epilepsy or a seizure disorder, don't use \
                            JETCARP.";

/// The rest of the advice.
pub const RISK_DETAILS: [&str; 2] = [
    "JETCARP can flash rapidly, and some of its rules flicker the whole screen many times a \
     second. Its strobe guard softens, slows or pauses scenes that flash more than three times a \
     second, but it can only lower the risk, not remove it.",
    "If you continue, JETCARP saves your answer, and the strobe guard stays on for you: it can't \
     be turned off. Even so, the safe choice is to quit.",
];

/// The confirmation that turns the strobe guard off: its heading.
pub const GUARD_OFF_TITLE: &str = "Turn off the strobe guard?";

/// What the guard does, and what turning it off means.
pub const GUARD_OFF_BODY: &str = "The guard slows or softens scenes that flash more than three \
                                  times a second, or show strong flickering stripes. Without \
                                  it, some scenes will flash rapidly at full speed.";

/// The box that must be ticked (it starts unticked).
pub const GUARD_OFF_CHECK: &str = "I don't have epilepsy or a seizure disorder, and I understand \
                                   that rapid flashing can occasionally trigger a first seizure \
                                   in someone with no history of seizures.";

/// The confirmation's sentence for the desktop background and the screen saver.
pub const GUARD_OFF_ROOM: &str = "The desktop background and the screen saver can be seen by \
                                  anyone in the room, who has not answered this warning. One of \
                                  them may have epilepsy, or not know that they do.";

/// The default button: nothing changes.
pub const GUARD_KEEP: &str = "Keep the guard on";

/// The other button, which works only with the box ticked.
pub const GUARD_TURN_OFF: &str = "Turn it off";

/// Why someone at risk has no switch.
pub const GUARD_ALWAYS: &str = "You answered that you have, or may have, a seizure disorder, so \
                                the strobe guard is always on.";

/// How a switch is turned off, for refusals elsewhere (the command line, the tools).
pub const GUARD_HOW: &str = "The strobe guard can be turned off only in the JETCARP app, in \
                             Settings → Safety, by someone who answered the photosensitive \
                             seizure warning that they don't have epilepsy or a seizure \
                             disorder.";

/// The advice's main button.
pub const QUIT: &str = "Quit JETCARP";

/// Its smaller button.
pub const CONTINUE: &str = "Continue at my own risk";

/// Why the background host holds every screen while the warning has not been accepted, as its
/// status says it (`held_why`).
pub const HOST_HELD: &str = "the photosensitive seizure warning has not been accepted (open the \
                             JETCARP app, or run `jetcarp setup`, to read and answer it)";

/// How to answer the warning, for a refusal.
const HOW: &str = "To read it and answer, open the JETCARP app, or run `jetcarp setup` in a \
                   terminal. Without a terminal, `jetcarp setup --yes --seizure-warning \
                   no-disorder` records that you don't have epilepsy or a seizure disorder.";

/// The answer given to the warning.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    /// They confirmed that they don't have epilepsy or a seizure disorder.
    NoDisorder,
    /// They have, or may have, a seizure disorder, were advised not to use JETCARP, and went on
    /// at their own risk.
    AtRisk,
}

impl Answer {
    /// `no_disorder` or `at_risk`, as [`FILE`] and the host's status write it.
    pub fn name(self) -> &'static str {
        match self {
            Answer::NoDisorder => "no_disorder",
            Answer::AtRisk => "at_risk",
        }
    }

    /// The answer in words, for status lines.
    pub fn words(self) -> &'static str {
        match self {
            Answer::NoDisorder => "no epilepsy or seizure disorder",
            Answer::AtRisk => "has, or may have, a seizure disorder; continued at own risk",
        }
    }
}

/// The answer as saved, with when and by what, and the strobe guard's switches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// [`FORMAT`].
    pub version: u32,
    pub answer: Answer,
    /// When it was given (UTC, `2026-11-16T12:00:00Z`).
    pub date: String,
    /// The JETCARP that recorded it.
    pub jetcarp: String,
    /// Where it was given: `app` or `setup`.
    pub via: String,
    /// The app's strobe guard was turned off, when and by which JETCARP (absent: on).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard_off: Option<Switched>,
    /// The desktop background's and screen saver's strobe guard was turned off (absent: on).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop_guard_off: Option<Switched>,
}

/// When a strobe guard switch was turned off, and by which JETCARP.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Switched {
    /// UTC, `2026-11-16T12:00:00Z`.
    pub date: String,
    pub jetcarp: String,
}

/// Where a strobe guard switch applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardPlace {
    /// The app: its view, full screen, outputs, previews and exported animations.
    App,
    /// The desktop background and the screen saver, which others may see.
    Desktop,
}

impl GuardPlace {
    pub fn name(self) -> &'static str {
        match self {
            GuardPlace::App => "app",
            GuardPlace::Desktop => "desktop",
        }
    }
    /// For sentences: "the app", "the desktop background and the screen saver".
    pub fn words(self) -> &'static str {
        match self {
            GuardPlace::App => "the app",
            GuardPlace::Desktop => "the desktop background and the screen saver",
        }
    }
}

impl Record {
    /// Whether the strobe guard is on for `place`: unless the answer is *no disorder* and that
    /// switch was turned off.
    pub fn guard_on(&self, place: GuardPlace) -> bool {
        let off = match place {
            GuardPlace::App => &self.guard_off,
            GuardPlace::Desktop => &self.desktop_guard_off,
        };
        self.answer == Answer::AtRisk || off.is_none()
    }
}

/// [`FILE`] in this settings folder.
pub fn file_in(settings: &Path) -> PathBuf {
    settings.join(FILE)
}

/// The answer saved in this settings folder, if there is one. A file that is missing or cannot
/// be read counts as no answer, so the warning shows again.
pub fn read_in(settings: &Path) -> Option<Record> {
    let text = std::fs::read_to_string(file_in(settings)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Save `answer` in this settings folder (made if need be; written whole, then renamed into
/// place), given `via` (`app`, `setup`).
pub fn record_in(settings: &Path, answer: Answer, via: &str) -> Result<Record, String> {
    let rec = Record {
        version: FORMAT,
        answer,
        date: crate::pattern::now_utc(),
        jetcarp: crate::VERSION.to_string(),
        via: via.to_string(),
        guard_off: None,
        desktop_guard_off: None,
    };
    write_in(settings, &rec)?;
    Ok(rec)
}

/// Writes `rec` whole into this settings folder, then renames it into place.
fn write_in(settings: &Path, rec: &Record) -> Result<(), String> {
    std::fs::create_dir_all(settings)
        .map_err(|e| format!("creating {}: {e}", settings.display()))?;
    let file = file_in(settings);
    let body = serde_json::to_string_pretty(rec).map_err(|e| e.to_string())?;
    let tmp = settings.join(format!("{FILE}.tmp-{}", std::process::id()));
    std::fs::write(&tmp, body + "\n").map_err(|e| format!("writing {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &file).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("writing {}: {e}", file.display())
    })
}

/// Whether the strobe guard for `place` is on, by the answer saved in this settings folder: on
/// with no answer, for someone at risk, and unless that switch was turned off.
pub fn guard_on_in(settings: &Path, place: GuardPlace) -> bool {
    read_in(settings).is_none_or(|r| r.guard_on(place))
}

/// The same in the settings folder.
pub fn guard_on(place: GuardPlace) -> bool {
    crate::paths::config_dir().is_none_or(|d| guard_on_in(&d, place))
}

/// Turns the strobe guard for `place` on or off in this settings folder, with the date and this
/// version. Turning it off is refused unless the answer saved is *no disorder*. This is for the
/// app's confirmation alone: the command line and the tools never call it to turn a guard off.
pub fn set_guard_in(settings: &Path, place: GuardPlace, on: bool) -> Result<Record, String> {
    let mut rec = read_in(settings).ok_or(
        "the photosensitive seizure warning has not been answered, so the strobe guard stays on",
    )?;
    if !on && rec.answer != Answer::NoDisorder {
        return Err(GUARD_ALWAYS.into());
    }
    let value = (!on).then(|| Switched {
        date: crate::pattern::now_utc(),
        jetcarp: crate::VERSION.to_string(),
    });
    match place {
        GuardPlace::App => rec.guard_off = value,
        GuardPlace::Desktop => rec.desktop_guard_off = value,
    }
    write_in(settings, &rec)?;
    Ok(rec)
}

/// The same in the settings folder.
pub fn set_guard(place: GuardPlace, on: bool) -> Result<Record, String> {
    let dir = crate::paths::config_dir()
        .ok_or("this system has no settings folder (set JETCARP_CONFIG_DIR)")?;
    set_guard_in(&dir, place, on)
}

/// The answer saved in the settings folder, if there is one.
pub fn read() -> Option<Record> {
    read_in(&crate::paths::config_dir()?)
}

/// Save `answer` in the settings folder.
pub fn record(answer: Answer, via: &str) -> Result<Record, String> {
    let dir = crate::paths::config_dir().ok_or(
        "this system has no settings folder to save the answer in (set JETCARP_CONFIG_DIR)",
    )?;
    record_in(&dir, answer, via)
}

/// Whether the warning has been accepted: either answer is saved.
pub fn accepted() -> bool {
    read().is_some()
}

/// Whether the answer saved says they have, or may have, a seizure disorder: the strobe guard
/// (#97) is always on for them.
pub fn at_risk() -> bool {
    read().is_some_and(|r| r.answer == Answer::AtRisk)
}

/// The warning as plain text: its heading in capitals, then each paragraph.
pub fn text() -> String {
    let mut s = format!("{}\n\n{KEY}", TITLE.to_uppercase());
    for p in DETAILS {
        s.push_str("\n\n");
        s.push_str(p);
    }
    s
}

/// The advice for someone at risk, as plain text.
pub fn risk_text() -> String {
    let mut s = format!("{}\n\n{RISK_KEY}", RISK_TITLE.to_uppercase());
    for p in RISK_DETAILS {
        s.push_str("\n\n");
        s.push_str(p);
    }
    s
}

/// The refusal of something that would animate on screen before the warning was accepted:
/// `action` says what (`start the background host`), and the warning and how to answer it
/// follow.
pub fn refusal(action: &str) -> String {
    format!(
        "JETCARP won't {action} until its photosensitive seizure warning has been accepted.\n\n{}\n\n{HOW}",
        text()
    )
}

/// Ok once the warning has been accepted, else [`refusal`] of `action`.
pub fn require(action: &str) -> Result<(), String> {
    if accepted() {
        Ok(())
    } else {
        Err(refusal(action))
    }
}

/// [`require`] with this settings folder.
pub fn require_in(settings: &Path, action: &str) -> Result<(), String> {
    if read_in(settings).is_some() {
        Ok(())
    } else {
        Err(refusal(action))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "jetcarp-photosensitivity-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// No answer: not accepted, and a refusal carries the warning and how to answer it.
    #[test]
    fn with_no_answer_it_is_not_accepted() {
        let d = scratch("none");
        assert_eq!(read_in(&d), None);
        let e = require_in(&d, "start the background host").unwrap_err();
        assert!(
            e.starts_with("JETCARP won't start the background host until"),
            "{e}"
        );
        assert!(e.contains(KEY) && e.contains("Hodgepodge"), "{e}");
        assert!(e.contains("--seizure-warning no-disorder"), "{e}");
        // A file that cannot be read is no answer either.
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(file_in(&d), "{not json").unwrap();
        assert_eq!(read_in(&d), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// No disorder: saved with the date and version, and accepted.
    #[test]
    fn no_disorder_is_saved_and_accepted() {
        let d = scratch("no-disorder").join("settings");
        let r = record_in(&d, Answer::NoDisorder, "setup").unwrap();
        assert_eq!(r.version, FORMAT);
        assert_eq!(r.jetcarp, env!("CARGO_PKG_VERSION"));
        assert!(r.date.ends_with('Z') && r.date.len() == 20, "{}", r.date);
        assert_eq!(read_in(&d).as_ref(), Some(&r));
        assert!(require_in(&d, "x").is_ok());
        let text = std::fs::read_to_string(file_in(&d)).unwrap();
        assert!(text.contains("\"answer\": \"no_disorder\""), "{text}");
        assert!(text.contains("\"via\": \"setup\""), "{text}");
        let _ = std::fs::remove_dir_all(d.parent().unwrap());
    }

    /// At risk: saved as such (for the strobe guard), and accepted too, since they chose to go
    /// on; a later answer replaces it.
    #[test]
    fn at_risk_is_saved_and_accepted() {
        let d = scratch("at-risk");
        record_in(&d, Answer::AtRisk, "app").unwrap();
        let r = read_in(&d).unwrap();
        assert_eq!(r.answer, Answer::AtRisk);
        assert_eq!(r.answer.name(), "at_risk");
        assert!(require_in(&d, "x").is_ok());
        assert!(
            std::fs::read_to_string(file_in(&d))
                .unwrap()
                .contains("\"answer\": \"at_risk\"")
        );
        record_in(&d, Answer::NoDisorder, "app").unwrap();
        assert_eq!(read_in(&d).unwrap().answer, Answer::NoDisorder);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The strobe guard's switches: on by default, saved with the answer when turned off (with the
    /// date and version), never off for someone at risk, and back on with a new answer.
    #[test]
    fn the_guard_is_on_unless_someone_with_no_disorder_turns_it_off() {
        let d = scratch("guard");
        // No answer: on, and no switch.
        assert!(guard_on_in(&d, GuardPlace::App) && guard_on_in(&d, GuardPlace::Desktop));
        assert!(set_guard_in(&d, GuardPlace::App, false).is_err());
        // No disorder: each switch on its own.
        record_in(&d, Answer::NoDisorder, "app").unwrap();
        assert!(guard_on_in(&d, GuardPlace::App));
        let r = set_guard_in(&d, GuardPlace::App, false).unwrap();
        let off = r.guard_off.as_ref().unwrap();
        assert!(off.date.ends_with('Z') && off.jetcarp == env!("CARGO_PKG_VERSION"));
        assert!(!guard_on_in(&d, GuardPlace::App));
        assert!(
            guard_on_in(&d, GuardPlace::Desktop),
            "the desktop's is separate"
        );
        let text = std::fs::read_to_string(file_in(&d)).unwrap();
        assert!(
            text.contains("\"guard_off\"") && !text.contains("desktop_guard_off"),
            "{text}"
        );
        set_guard_in(&d, GuardPlace::Desktop, false).unwrap();
        assert!(!guard_on_in(&d, GuardPlace::Desktop));
        // Back on.
        set_guard_in(&d, GuardPlace::App, true).unwrap();
        assert!(guard_on_in(&d, GuardPlace::App));
        assert_eq!(read_in(&d).unwrap().answer, Answer::NoDisorder);
        // A new answer starts with both on.
        record_in(&d, Answer::NoDisorder, "setup").unwrap();
        assert!(guard_on_in(&d, GuardPlace::Desktop));
        // At risk: no switch, and on whatever the file says.
        record_in(&d, Answer::AtRisk, "app").unwrap();
        let e = set_guard_in(&d, GuardPlace::App, false).unwrap_err();
        assert_eq!(e, GUARD_ALWAYS);
        let mut forged = read_in(&d).unwrap();
        forged.guard_off = Some(Switched {
            date: "x".into(),
            jetcarp: "x".into(),
        });
        write_in(&d, &forged).unwrap();
        assert!(
            guard_on_in(&d, GuardPlace::App),
            "at risk: on whatever the file says"
        );
        // A file from before the switches reads as both on.
        std::fs::write(
            file_in(&d),
            r#"{"version": 1, "answer": "no_disorder", "date": "d", "jetcarp": "0.4.2", "via": "app"}"#,
        )
        .unwrap();
        assert!(guard_on_in(&d, GuardPlace::App) && guard_on_in(&d, GuardPlace::Desktop));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The text: the heading in capitals and every paragraph, and the advice's.
    #[test]
    fn the_text_has_every_paragraph() {
        let t = text();
        assert!(t.starts_with("PHOTOSENSITIVE SEIZURE WARNING\n\n"));
        for p in std::iter::once(KEY).chain(DETAILS) {
            assert!(t.contains(p), "{p}");
            assert!(!p.contains('!') && !p.contains("  "), "{p}");
        }
        let r = risk_text();
        for p in std::iter::once(RISK_KEY).chain(RISK_DETAILS) {
            assert!(r.contains(p), "{p}");
            assert!(!p.contains('!') && !p.contains("  "), "{p}");
        }
    }
}
