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
//! use JETCARP, and may go on at their own risk. The answer, its date and the version that
//! recorded it are kept so that the planned strobe limiter (#97) can switch itself on for them
//! ([`at_risk`]). With no answer saved the warning shows again; once saved it is not asked again.

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
    "Some rules change most of the screen every generation. A rule with B0, for example, turns \
     the whole field over at each step. Run fast, full screen or as the desktop background, such \
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
     second. It cannot yet detect or limit flashing, so it cannot protect you from it.",
    "An automatic limiter is planned. If you continue, JETCARP saves your answer, so that the \
     limiter can switch itself on for you when it comes. Until then, the safe choice is to quit.",
];

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

/// The answer as saved, with when and by what.
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
    };
    std::fs::create_dir_all(settings)
        .map_err(|e| format!("creating {}: {e}", settings.display()))?;
    let file = file_in(settings);
    let body = serde_json::to_string_pretty(&rec).map_err(|e| e.to_string())?;
    let tmp = settings.join(format!("{FILE}.tmp-{}", std::process::id()));
    std::fs::write(&tmp, body + "\n").map_err(|e| format!("writing {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &file).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("writing {}: {e}", file.display())
    })?;
    Ok(rec)
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

/// Whether the answer saved says they have, or may have, a seizure disorder: the strobe limiter
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
        assert!(e.contains(KEY) && e.contains("B0"), "{e}");
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

    /// At risk: saved as such (for the strobe limiter), and accepted too, since they chose to go
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
