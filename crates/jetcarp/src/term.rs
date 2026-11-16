//! Progress lines redrawn in place on standard error, fitted to the terminal's width (#104).
//!
//! A line redrawn with `\r` that is wider than the terminal wraps, and `\r` then goes back only to
//! the start of its last row, so each redraw leaves the rows above behind. A line is cut to one
//! column short of the width: its least important parts (` | newest: ...`) go first. Written to a
//! file or a pipe, lines stay whole.

use std::borrow::Cow;

/// `line` fitted to `width` columns: its ` | ` parts dropped from the end while it is too wide
/// (the least important, such as a hunt's ` | newest: ...`), then the rest cut, ending in `…`.
/// A character counts as one column.
pub(crate) fn fit(line: &str, width: usize) -> Cow<'_, str> {
    let cols = |s: &str| s.chars().count();
    let mut s = line;
    while cols(s) > width {
        match s.rfind(" | ") {
            Some(i) => s = &s[..i],
            None => break,
        }
    }
    if cols(s) <= width {
        return Cow::Borrowed(s);
    }
    if width == 0 {
        return Cow::Borrowed("");
    }
    let mut cut: String = s.chars().take(width - 1).collect();
    cut.push('…');
    Cow::Owned(cut)
}

/// `line` fitted to the terminal standard error shows, one column short of its width so the
/// cursor never wraps; whole when the width is unknown (not a terminal).
pub(crate) fn fit_to_stderr(line: &str) -> Cow<'_, str> {
    match stderr_columns() {
        Some(w) => fit(line, w.saturating_sub(1)),
        None => Cow::Borrowed(line),
    }
}

/// The width in columns of the terminal standard error shows: asked of the terminal (each time,
/// so a resized window is followed), else `COLUMNS`. `None` when it is not a terminal.
pub(crate) fn stderr_columns() -> Option<usize> {
    asked_columns().or_else(|| {
        std::env::var("COLUMNS")
            .ok()?
            .trim()
            .parse()
            .ok()
            .filter(|&w| w > 0)
    })
}

#[cfg(unix)]
fn asked_columns() -> Option<usize> {
    // SAFETY: TIOCGWINSZ fills in the winsize it is given, and only reads the descriptor.
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::ioctl(libc::STDERR_FILENO, libc::TIOCGWINSZ, &mut ws) };
    (r == 0 && ws.ws_col > 0).then_some(ws.ws_col as usize)
}

#[cfg(windows)]
fn asked_columns() -> Option<usize> {
    use windows_sys::Win32::System::Console::{
        CONSOLE_SCREEN_BUFFER_INFO, GetConsoleScreenBufferInfo, GetStdHandle, STD_ERROR_HANDLE,
    };
    // SAFETY: the handle is the process's own standard error; the call fills in the info given,
    // and fails (returns 0) for a handle that is not a console.
    unsafe {
        let h = GetStdHandle(STD_ERROR_HANDLE);
        let mut info: CONSOLE_SCREEN_BUFFER_INFO = std::mem::zeroed();
        if GetConsoleScreenBufferInfo(h, &mut info) == 0 {
            return None;
        }
        let w = i32::from(info.srWindow.Right) - i32::from(info.srWindow.Left) + 1;
        (w > 0).then_some(w as usize)
    }
}

#[cfg(not(any(unix, windows)))]
fn asked_columns() -> Option<usize> {
    None
}

#[cfg(test)]
mod tests {
    use super::fit;

    const HUNT: &str = "hunt 1: 192/5,000 soups, 41 soups/s, 380 objects (ships 2, osc 120, \
                        still 258), 37 distinct, 3 new | newest: p3 oscillator (B3/S23)";

    #[test]
    fn a_line_that_fits_stays_whole() {
        assert_eq!(fit(HUNT, HUNT.len()), HUNT);
        assert_eq!(fit(HUNT, 500), HUNT);
        assert_eq!(fit("", 0), "");
    }

    #[test]
    fn the_newest_part_goes_first_then_the_rest_is_cut() {
        let head = HUNT.split(" | ").next().unwrap();
        // One column short of the whole line: the `newest:` part goes, not the counts.
        assert_eq!(fit(HUNT, HUNT.len() - 1), head);
        assert_eq!(fit(HUNT, head.len()), head);
        // Narrower than the counts: cut, ending in an ellipsis, exactly as wide as allowed.
        let cut = fit(HUNT, 40);
        assert_eq!(cut.chars().count(), 40);
        assert!(
            cut.ends_with('…') && head.starts_with(cut.trim_end_matches('…')),
            "{cut}"
        );
        assert_eq!(fit(HUNT, 1), "…");
        assert_eq!(fit(HUNT, 0), "");
        // Several parts: dropped from the end, one at a time, as far as needed.
        assert_eq!(fit("a | b | c", 5), "a | b");
        assert_eq!(fit("a | b | c", 4), "a");
    }

    #[test]
    fn columns_are_characters_not_bytes() {
        // "×" and "…" are two and three bytes, one column each.
        let line = "3×3 grid … done";
        assert_eq!(fit(line, line.chars().count()), line);
        assert_eq!(fit(line, 4), "3×3…");
    }
}
