//! Progress lines redrawn in place on standard error, fitted to the terminal's width (#104).
//!
//! A line redrawn with `\r` that is wider than the terminal wraps, and `\r` then goes back only to
//! the start of its last row, so each redraw leaves the rows above behind. A line is cut to one
//! column short of the width: its least important parts (` | newest: ...`) go first. Written to a
//! file or a pipe, lines stay whole.
//!
//! Before a redraw the line is erased with `\r` and the erase-line code `ESC [2K`. Windows' old
//! console window (conhost) shows escape codes as text while they are off, as cmd and PowerShell
//! leave them, so [`init`] turns them on for standard error at start-up; where that fails, the line
//! is erased with spaces instead (#127).

use std::borrow::Cow;

/// At start-up. On Windows: turn on escape codes in standard error's console, if it is one (the
/// old console window has them off; Windows Terminal has them on already). Elsewhere nothing.
pub(crate) fn init() {
    #[cfg(windows)]
    let _ = ESCAPES.set(enable_escapes());
}

/// Whether the terminal standard error shows acts on escape codes, rather than printing them.
fn escapes_shown() -> bool {
    #[cfg(windows)]
    return ESCAPES.get().copied().unwrap_or(true);
    #[cfg(not(windows))]
    true
}

/// What erases the progress line, before it is redrawn or other output takes its place: `\r` and
/// `ESC [2K`, or, where escape codes would print as text, `\r`, spaces to one column short of the
/// width, and `\r` again (#127).
pub(crate) fn clear_line() -> Cow<'static, str> {
    clear_line_for(escapes_shown(), stderr_columns())
}

fn clear_line_for(escapes: bool, columns: Option<usize>) -> Cow<'static, str> {
    if escapes {
        Cow::Borrowed("\r\x1b[2K")
    } else {
        let w = columns.unwrap_or(80).saturating_sub(1);
        Cow::Owned(format!("\r{}\r", " ".repeat(w)))
    }
}

#[cfg(windows)]
static ESCAPES: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Turn on `ENABLE_VIRTUAL_TERMINAL_PROCESSING` for standard error's console: true when escape
/// codes then work, or when standard error is no console at all (a file or a pipe, where no line
/// is redrawn); false for a console that refuses (Windows before 10).
#[cfg(windows)]
fn enable_escapes() -> bool {
    use windows_sys::Win32::System::Console::{
        CONSOLE_MODE, ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle,
        STD_ERROR_HANDLE, SetConsoleMode,
    };
    // SAFETY: the handle is the process's own standard error; GetConsoleMode fills in the mode
    // given and fails (returns 0) for a handle that is not a console; SetConsoleMode only sets it.
    unsafe {
        let h = GetStdHandle(STD_ERROR_HANDLE);
        let mut mode: CONSOLE_MODE = 0;
        if GetConsoleMode(h, &mut mode) == 0 {
            return true;
        }
        mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING != 0
            || SetConsoleMode(h, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
    }
}

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
    use super::{clear_line_for, fit};

    #[test]
    fn the_line_is_erased_with_the_code_or_with_spaces() {
        assert_eq!(clear_line_for(true, Some(60)), "\r\x1b[2K");
        assert_eq!(clear_line_for(true, None), "\r\x1b[2K");
        // Escape codes off (Windows' old console window, #127): spaces, no escape character.
        let c = clear_line_for(false, Some(60));
        assert_eq!(c, format!("\r{}\r", " ".repeat(59)));
        assert!(!c.contains('\x1b'));
        assert_eq!(clear_line_for(false, None).len(), 81);
    }

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
