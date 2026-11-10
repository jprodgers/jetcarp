//! A small code editor for Rhai scripts: syntax colours (an egui `LayoutJob`), line numbers, and
//! the line of the current error marked. The tokenizer is plain Rust so it can be tested.

use eframe::egui::{self, Color32, FontId, TextFormat, text::LayoutJob};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    Text,
    Comment,
    Str,
    Number,
    Keyword,
    /// `true`, `false`, `()`.
    Literal,
    /// An identifier followed by `(`.
    Call,
    /// A map key: an identifier followed by `:` inside `#{ }`.
    Key,
    Punct,
}

const KEYWORDS: &[&str] = &[
    "let",
    "const",
    "fn",
    "if",
    "else",
    "for",
    "in",
    "while",
    "loop",
    "do",
    "until",
    "return",
    "break",
    "continue",
    "switch",
    "throw",
    "try",
    "catch",
    "import",
    "export",
    "as",
    "private",
    "this",
    "is_def_var",
    "is_def_fn",
    "Fn",
    "call",
    "curry",
    "type_of",
    "print",
    "debug",
];

/// Split source into coloured spans (byte ranges). Unterminated strings and comments run to the
/// end, as an editor shows them while typing.
pub fn tokens(src: &str) -> Vec<(std::ops::Range<usize>, Tok)> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let push = |out: &mut Vec<(std::ops::Range<usize>, Tok)>, r: std::ops::Range<usize>, t| {
        if r.is_empty() {
            return;
        }
        match out.last_mut() {
            Some((last, lt)) if *lt == t && last.end == r.start => last.end = r.end,
            _ => out.push((r, t)),
        }
    };
    while i < b.len() {
        let c = b[i];
        let start = i;
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            push(&mut out, start..i, Tok::Comment);
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            let mut depth = 0;
            while i < b.len() {
                if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    i += 2;
                } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            push(&mut out, start..i, Tok::Comment);
        } else if c == b'"' || c == b'`' || c == b'\'' {
            i += 1;
            while i < b.len() && b[i] != c {
                if b[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(b.len());
            push(&mut out, start..i, Tok::Str);
        } else if c.is_ascii_digit() {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'.') {
                // `0..10` is a range, not a number with two dots.
                if b[i] == b'.' && b.get(i + 1) == Some(&b'.') {
                    break;
                }
                i += 1;
            }
            push(&mut out, start..i, Tok::Number);
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let word = &src[start..i];
            let mut k = i;
            while k < b.len() && (b[k] == b' ' || b[k] == b'\t') {
                k += 1;
            }
            let next = b.get(k).copied();
            let t = if word == "true" || word == "false" {
                Tok::Literal
            } else if next == Some(b':') && b.get(k + 1) != Some(&b':') {
                Tok::Key
            } else if KEYWORDS.contains(&word) && next != Some(b'(') || word == "fn" {
                Tok::Keyword
            } else if next == Some(b'(') {
                Tok::Call
            } else {
                Tok::Text
            };
            push(&mut out, start..i, t);
        } else if c.is_ascii_punctuation() {
            i += 1;
            push(&mut out, start..i, Tok::Punct);
        } else {
            // Whitespace and anything else (UTF-8 continuation bytes stay in one span).
            i += 1;
            while i < b.len() && !b[i].is_ascii_graphic() && b[i] != b'\n' && !b[i].is_ascii() {
                i += 1;
            }
            push(&mut out, start..i, Tok::Text);
        }
    }
    out
}

fn color(t: Tok) -> Color32 {
    match t {
        Tok::Text => Color32::from_rgb(0xd8, 0xdc, 0xe4),
        Tok::Comment => Color32::from_rgb(0x7f, 0x95, 0x7f),
        Tok::Str => Color32::from_rgb(0xf0, 0xb0, 0x70),
        Tok::Number => Color32::from_rgb(0xb5, 0xe0, 0x8c),
        Tok::Keyword => Color32::from_rgb(0xc5, 0x92, 0xff),
        Tok::Literal => Color32::from_rgb(0x7f, 0xc8, 0xff),
        Tok::Call => Color32::from_rgb(0xff, 0xd8, 0x6b),
        Tok::Key => Color32::from_rgb(0x8c, 0xd4, 0xe8),
        Tok::Punct => Color32::from_rgb(0xa0, 0xa8, 0xb4),
    }
}

/// The coloured layout. `error_line` (1-based) gets a red background.
pub fn highlight(src: &str, font: FontId, error_line: Option<usize>) -> LayoutJob {
    let mut job = LayoutJob::default();
    let err_bg = Color32::from_rgba_unmultiplied(0xc0, 0x30, 0x30, 70);
    let mut line = 1usize;
    let add = |job: &mut LayoutJob, text: &str, t: Tok, line: usize| {
        let mut f = TextFormat::simple(font.clone(), color(t));
        if error_line == Some(line) {
            f.background = err_bg;
        }
        job.append(text, 0.0, f);
    };
    for (r, t) in tokens(src) {
        // Split at newlines so the error background stays on its line.
        let text = &src[r];
        let mut rest = text;
        while let Some(k) = rest.find('\n') {
            add(&mut job, &rest[..=k], t, line);
            line += 1;
            rest = &rest[k + 1..];
        }
        if !rest.is_empty() {
            add(&mut job, rest, t, line);
        }
    }
    job.wrap.max_width = f32::INFINITY;
    job
}

/// The editor: line numbers and the coloured text, in one scroll area. Returns the text edit's
/// response.
pub fn editor(
    ui: &mut egui::Ui,
    text: &mut String,
    error_line: Option<usize>,
    height: f32,
) -> egui::Response {
    let font = FontId::monospace(13.5);
    let lines = text.split('\n').count().max(1);
    let row_h = ui.fonts_mut(|f| f.row_height(&font));
    let mut out = None;
    egui::ScrollArea::both()
        .id_salt("rhai-editor")
        .max_height(height)
        .min_scrolled_height(height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                // Gutter.
                let digits = lines.to_string().len().max(2);
                let mut job = LayoutJob::default();
                for n in 1..=lines {
                    let c = if error_line == Some(n) {
                        Color32::from_rgb(0xff, 0x70, 0x70)
                    } else {
                        Color32::from_gray(110)
                    };
                    let s = if n < lines {
                        format!("{n:>digits$}\n")
                    } else {
                        format!("{n:>digits$}")
                    };
                    job.append(&s, 0.0, TextFormat::simple(font.clone(), c));
                }
                let galley = ui.fonts_mut(|f| f.layout_job(job));
                let size = egui::vec2(galley.size().x + 6.0, galley.size().y.max(row_h));
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                // Match the text edit's inner margin so rows line up.
                let margin = ui.spacing().button_padding.y.max(2.0);
                ui.painter().galley(
                    rect.min + egui::vec2(0.0, margin + 2.0),
                    galley,
                    Color32::WHITE,
                );
                let f2 = font.clone();
                let mut layouter = move |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _w: f32| {
                    let job = highlight(buf.as_str(), f2.clone(), error_line);
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                let r = ui.add(
                    egui::TextEdit::multiline(text)
                        .id_salt("rhai-editor-text")
                        .font(font.clone())
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(((height / row_h) as usize).max(4))
                        .lock_focus(true)
                        .layouter(&mut layouter),
                );
                out = Some(r);
            });
        });
    out.expect("editor drawn")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(&str, Tok)> {
        tokens(src)
            .into_iter()
            .map(|(r, t)| (&src[r], t))
            .filter(|(s, _)| !s.trim().is_empty())
            .collect()
    }

    #[test]
    fn tokens_cover_the_source() {
        let src = "let w = world(#{ tiling: \"hex\", until: \"done\" }); // go\nfor n in 0..10 { w.step(n); }\n/* a\n b */ `x ${n}` 3.5 true é";
        let t = tokens(src);
        let mut at = 0;
        for (r, _) in &t {
            assert_eq!(r.start, at);
            at = r.end;
        }
        assert_eq!(at, src.len());
        let k = kinds(src);
        assert!(k.contains(&("let", Tok::Keyword)));
        assert!(k.contains(&("world", Tok::Call)));
        assert!(k.contains(&("tiling", Tok::Key)));
        assert!(k.contains(&("until", Tok::Key)));
        assert!(k.contains(&("\"hex\"", Tok::Str)));
        assert!(k.contains(&("// go", Tok::Comment)));
        assert!(k.contains(&("0", Tok::Number)));
        assert!(k.contains(&("10", Tok::Number)));
        assert!(k.contains(&("/* a\n b */", Tok::Comment)));
        assert!(k.contains(&("`x ${n}`", Tok::Str)));
        assert!(k.contains(&("3.5", Tok::Number)));
        assert!(k.contains(&("true", Tok::Literal)));
        assert!(k.contains(&("step", Tok::Call)));
    }

    #[test]
    fn unterminated_strings_run_to_the_end() {
        let src = "let s = \"abc";
        let t = tokens(src);
        assert_eq!(t.last().unwrap().1, Tok::Str);
        assert_eq!(t.last().unwrap().0.end, src.len());
    }

    #[test]
    fn error_line_is_marked() {
        let job = highlight("a\nb c\nd", FontId::monospace(12.0), Some(2));
        let marked: String = job
            .sections
            .iter()
            .filter(|s| s.format.background != Color32::TRANSPARENT)
            .map(|s| &job.text[s.byte_range.start.0..s.byte_range.end.0])
            .collect();
        assert_eq!(marked, "b c\n");
    }
}
