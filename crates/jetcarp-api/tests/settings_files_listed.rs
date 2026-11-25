//! Every file and folder the code keeps at the top of JETCARP's settings folder is one
//! `jetcarp uninstall --settings` counts as JETCARP's (#349). `machine.json` and `limits.json`
//! were not on its list, so on any computer whose machine JETCARP had recorded, `--settings`
//! refused the folder as holding something else.
//!
//! The names are gathered from the code of every crate, not from a list kept by hand: every
//! path built in the settings folder with `.join(...)`, on `config_dir()` (the core's, or the
//! display crate's that calls it) or on a variable that holds the folder (named `settings`,
//! `config` or `settings_dir`, or bound from one of those in the same function), with the
//! folder's first component resolved from a string or a `const` of the code. A new file there
//! fails this test until `SETTINGS_FILES` or `SETTINGS_FOLDERS` (`src/ops/system.rs`) lists it.
//! Test code (`#[cfg(test)]` modules and items) is left out: its files are scratch ones.

use jetcarp_api::ops::system::settings_entry_listed;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Joins on the settings folder whose name is not a string or a `const`, each with why every
/// name it can take is listed: (the file, the argument as written, why).
const UNRESOLVED: &[(&str, &str, &str)] = &[
    (
        "jetcarp-display/src/saver_update.rs",
        "f",
        "the screen saver's stamp reads host.json, app.json, photosensitivity.json and paths.json, \
     named in the same expression",
    ),
    (
        "jetcarp-display/src/saver_update.rs",
        "d",
        "the screen saver's stamp reads the rotations and screensavers folders, named in the same \
     loop",
    ),
];

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    Punct(char),
    /// A number or a character literal.
    Other,
}

/// The tokens of Rust source `src`, each with its line: comments dropped, strings kept as their
/// contents (escapes read for the few the code uses).
fn tokens(src: &str) -> Vec<(Tok, usize)> {
    let c: Vec<char> = src.chars().collect();
    let (mut i, mut line, mut out) = (0, 1, Vec::new());
    let at = |i: usize| c.get(i).copied().unwrap_or('\0');
    while i < c.len() {
        let ch = c[i];
        if ch == '\n' {
            line += 1;
            i += 1;
        } else if ch.is_whitespace() {
            i += 1;
        } else if ch == '/' && at(i + 1) == '/' {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if ch == '/' && at(i + 1) == '*' {
            let mut depth = 0;
            while i < c.len() {
                if c[i] == '/' && at(i + 1) == '*' {
                    depth += 1;
                    i += 2;
                } else if c[i] == '*' && at(i + 1) == '/' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if c[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
        } else if (ch == 'r'
            && (at(i + 1) == '"' || (at(i + 1) == '#' && "\"#".contains(at(i + 2)))))
            || (ch == 'b' && at(i + 1) == 'r' && "\"#".contains(at(i + 2)))
        {
            // A raw string: r"..", r#".."#, br"..".
            let start_line = line;
            i += if ch == 'b' { 2 } else { 1 };
            let mut hashes = 0;
            while at(i) == '#' {
                hashes += 1;
                i += 1;
            }
            i += 1;
            let mut s = String::new();
            loop {
                if i >= c.len() {
                    break;
                }
                if c[i] == '"' && (0..hashes).all(|k| at(i + 1 + k) == '#') {
                    i += 1 + hashes;
                    break;
                }
                if c[i] == '\n' {
                    line += 1;
                }
                s.push(c[i]);
                i += 1;
            }
            out.push((Tok::Str(s), start_line));
        } else if ch == '"' || (ch == 'b' && at(i + 1) == '"') {
            let start_line = line;
            i += if ch == 'b' { 2 } else { 1 };
            let mut s = String::new();
            while i < c.len() && c[i] != '"' {
                if c[i] == '\\' {
                    match at(i + 1) {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        '\n' => {
                            // A continued line: the newline and the next line's indent go.
                            line += 1;
                            i += 2;
                            while at(i).is_whitespace() {
                                if at(i) == '\n' {
                                    line += 1;
                                }
                                i += 1;
                            }
                            continue;
                        }
                        other => s.push(other),
                    }
                    i += 2;
                } else {
                    if c[i] == '\n' {
                        line += 1;
                    }
                    s.push(c[i]);
                    i += 1;
                }
            }
            i += 1;
            out.push((Tok::Str(s), start_line));
        } else if ch == '\'' {
            // A character literal ('a', '\n', '\'', '\u{..}'), or a lifetime ('a, 'static).
            if at(i + 1) == '\\' {
                i += 2;
                while i < c.len() && c[i] != '\'' {
                    i += 1;
                }
                i += 1;
                out.push((Tok::Other, line));
            } else if at(i + 2) == '\'' {
                i += 3;
                out.push((Tok::Other, line));
            } else {
                i += 1;
                while at(i).is_alphanumeric() || at(i) == '_' {
                    i += 1;
                }
            }
        } else if ch.is_alphabetic() || ch == '_' {
            let s0 = i;
            while at(i).is_alphanumeric() || at(i) == '_' {
                i += 1;
            }
            out.push((Tok::Ident(c[s0..i].iter().collect()), line));
        } else if ch.is_ascii_digit() {
            while at(i).is_alphanumeric() || at(i) == '_' {
                i += 1;
            }
            out.push((Tok::Other, line));
        } else {
            out.push((Tok::Punct(ch), line));
            i += 1;
        }
    }
    out
}

fn is_ident(t: Option<&(Tok, usize)>, name: &str) -> bool {
    matches!(t, Some((Tok::Ident(s), _)) if s == name)
}

fn is_punct(t: Option<&(Tok, usize)>, p: char) -> bool {
    matches!(t, Some((Tok::Punct(q), _)) if *q == p)
}

/// The index just past the bracket that closes the one at `open` (`(`, `[` or `{`).
fn closing(t: &[(Tok, usize)], open: usize) -> usize {
    let mut depth = 0i32;
    for (k, (tok, _)) in t.iter().enumerate().skip(open) {
        match tok {
            Tok::Punct('(' | '[' | '{') => depth += 1,
            Tok::Punct(')' | ']' | '}') => {
                depth -= 1;
                if depth == 0 {
                    return k + 1;
                }
            }
            _ => {}
        }
    }
    t.len()
}

/// The index of the bracket that opens the one that closes at `close`.
fn opening(t: &[(Tok, usize)], close: usize) -> usize {
    let mut depth = 0i32;
    for k in (0..=close).rev() {
        match t[k].0 {
            Tok::Punct(')' | ']' | '}') => depth += 1,
            Tok::Punct('(' | '[' | '{') => {
                depth -= 1;
                if depth == 0 {
                    return k;
                }
            }
            _ => {}
        }
    }
    0
}

/// The tokens without test code: each item under `#[cfg(test)]` or `#[cfg(all(test, ...))]` (a
/// module, a function, an `impl`, a field, a statement), and the names of test modules kept in
/// files of their own (`#[cfg(test)] mod shots;`).
fn without_tests(t: Vec<(Tok, usize)>) -> (Vec<(Tok, usize)>, Vec<String>) {
    let (mut out, mut test_mods, mut i) = (Vec::new(), Vec::new(), 0);
    // Where the attribute ends, when it is one of those.
    let cfg_test = |i: usize| -> Option<usize> {
        if !(is_punct(t.get(i), '#')
            && is_punct(t.get(i + 1), '[')
            && is_ident(t.get(i + 2), "cfg")
            && is_punct(t.get(i + 3), '('))
        {
            return None;
        }
        let test_only = is_ident(t.get(i + 4), "test") && is_punct(t.get(i + 5), ')')
            || is_ident(t.get(i + 4), "all")
                && is_punct(t.get(i + 5), '(')
                && is_ident(t.get(i + 6), "test");
        test_only.then(|| closing(&t, i + 1))
    };
    while i < t.len() {
        let Some(mut k) = cfg_test(i) else {
            out.push(t[i].clone());
            i += 1;
            continue;
        };
        // Further attributes on the item.
        while is_punct(t.get(k), '#') && is_punct(t.get(k + 1), '[') {
            k = closing(&t, k + 1);
        }
        let start = k;
        // The item ends at its block, or its `;`; a field, a variant or a match arm at its `,`,
        // or at the `}` of what it is in.
        while k < t.len() && !matches!(t[k].0, Tok::Punct('{' | ';' | ',' | '}')) {
            k = if is_punct(t.get(k), '(') || is_punct(t.get(k), '[') {
                closing(&t, k)
            } else {
                k + 1
            };
        }
        if is_punct(t.get(k), '}') {
            i = k;
        } else if is_punct(t.get(k), ',') {
            i = k + 1;
        } else if is_punct(t.get(k), ';') {
            // `mod name;`: its file is left out.
            let words: Vec<&str> = t[start..k]
                .iter()
                .filter_map(|(x, _)| match x {
                    Tok::Ident(s) => Some(s.as_str()),
                    _ => None,
                })
                .collect();
            if let [.., "mod", name] = words.as_slice() {
                test_mods.push(name.to_string());
            }
            i = k + 1;
        } else {
            i = closing(&t, k);
        }
    }
    (out, test_mods)
}

/// Every `.rs` file of the crates' code (`src/`, and files beside it such as
/// `launcher_find.rs`), without the files of test modules.
fn code_files(crates: &Path) -> Vec<PathBuf> {
    fn walk(d: &Path, out: &mut Vec<PathBuf>) {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(d)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|e| e == "rs") {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    let mut crate_dirs: Vec<PathBuf> = std::fs::read_dir(crates)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("Cargo.toml").is_file())
        .collect();
    crate_dirs.sort();
    for c in crate_dirs {
        walk(&c.join("src"), &mut files);
        for e in std::fs::read_dir(&c).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|e| e == "rs")
                && p.file_name() != Some("build.rs".as_ref())
            {
                files.push(p);
            }
        }
    }
    files
}

/// One file's code, its test code left out.
struct Code {
    /// The file, from the crates folder (`jetcarp-core/src/machine.rs`).
    name: String,
    toks: Vec<(Tok, usize)>,
}

/// Where a file's submodules live: `a/b.rs`'s in `a/b/`, `a/mod.rs`'s (and `lib.rs`'s,
/// `main.rs`'s) in `a/`.
fn module_dir(file: &Path) -> PathBuf {
    let stem = file.file_stem().unwrap().to_string_lossy();
    let parent = file.parent().unwrap();
    if matches!(stem.as_ref(), "mod" | "lib" | "main") {
        parent.to_path_buf()
    } else {
        parent.join(stem.as_ref())
    }
}

fn read_code(crates: &Path) -> Vec<Code> {
    let files = code_files(crates);
    let mut skip = BTreeSet::new();
    let mut parsed = Vec::new();
    for f in &files {
        let (toks, test_mods) = without_tests(tokens(&std::fs::read_to_string(f).unwrap()));
        for m in test_mods {
            let d = module_dir(f);
            skip.insert(d.join(format!("{m}.rs")));
            skip.insert(d.join(&m).join("mod.rs"));
            // And everything in that module's folder.
            skip.insert(d.join(&m));
        }
        parsed.push((f.clone(), toks));
    }
    parsed
        .into_iter()
        .filter(|(f, _)| !skip.iter().any(|s| f == s || f.starts_with(s)))
        .map(|(f, toks)| Code {
            name: f
                .strip_prefix(crates)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/"),
            toks,
        })
        .collect()
}

/// Every `const NAME: &str = "...";` of the code: by name, the values with the files they are in.
fn consts(code: &[Code]) -> BTreeMap<String, Vec<(String, String)>> {
    let mut out: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for c in code {
        let t = &c.toks;
        for k in 0..t.len() {
            if !is_ident(t.get(k), "const") {
                continue;
            }
            let Some((Tok::Ident(name), _)) = t.get(k + 1) else {
                continue;
            };
            if !is_punct(t.get(k + 2), ':') {
                continue;
            }
            let mut j = k + 3;
            let mut is_str = false;
            while j < t.len() && !is_punct(t.get(j), '=') && !is_punct(t.get(j), ';') {
                is_str |= is_ident(t.get(j), "str");
                j += 1;
            }
            if let (true, true, Some((Tok::Str(v), _))) =
                (is_str, is_punct(t.get(j), '='), t.get(j + 1))
                && is_punct(t.get(j + 2), ';')
            {
                out.entry(name.clone())
                    .or_default()
                    .push((c.name.clone(), v.clone()));
            }
        }
    }
    out
}

/// The variables, and the functions, that hold the settings folder by their name.
const FOLDER_NAMES: &[&str] = &["settings", "config", "settings_dir"];

/// Whether the identifier at `k` names the settings folder: `config_dir()` (not the system's,
/// `dirs::config_dir()`), a variable or call named as in [`FOLDER_NAMES`], a variable in
/// `vars` (bound to the folder in this function), or a call of a function in `fns` (this
/// file's functions that return the folder).
fn source_at(
    t: &[(Tok, usize)],
    k: usize,
    vars: &BTreeSet<String>,
    fns: &BTreeSet<String>,
) -> bool {
    let Some((Tok::Ident(s), _)) = t.get(k) else {
        return false;
    };
    let prev = k.checked_sub(1).and_then(|p| t.get(p));
    let field = is_punct(prev, '.');
    let defined = is_ident(prev, "fn");
    let called = is_punct(t.get(k + 1), '(');
    // `name:` is a parameter's or a field's name, `name::` a module's.
    let named = is_punct(t.get(k + 1), ':');
    if field || defined || named {
        return false;
    }
    if s == "config_dir" {
        return called && !(k >= 3 && is_ident(t.get(k - 3), "dirs") && is_punct(prev, ':'));
    }
    if fns.contains(s) {
        return called;
    }
    FOLDER_NAMES.contains(&s.as_str()) || vars.contains(s)
}

/// Where the expression chain that ends just before `end` starts: back over `a.b`, `a::b`,
/// `?`, calls and macros, and indexes.
fn chain_start(t: &[(Tok, usize)], end: usize) -> usize {
    #[derive(PartialEq)]
    enum Want {
        /// Before a `.`, `::` or `?`: what it applies to.
        Operand,
        /// Before an identifier: a `.` or `::` joining it to more of the chain.
        Link,
        /// Before `(..)` or `[..]`: what is called or indexed.
        Callee,
    }
    let (mut k, mut want) = (end, Want::Operand);
    while k > 0 {
        let p = k - 1;
        match (&t[p].0, &want) {
            (Tok::Punct('?'), Want::Operand | Want::Callee | Want::Link) => k = p,
            (Tok::Punct(')' | ']'), Want::Operand | Want::Callee) => {
                k = opening(t, p);
                want = Want::Callee;
            }
            (Tok::Ident(_), Want::Operand | Want::Callee) => {
                k = p;
                want = Want::Link;
            }
            (Tok::Punct('!'), Want::Callee) => {
                k = p;
                want = Want::Operand;
            }
            (Tok::Punct('.'), Want::Link | Want::Callee) => {
                k = p;
                want = Want::Operand;
            }
            (Tok::Punct(':'), Want::Link) if p > 0 && is_punct(t.get(p - 1), ':') => {
                k = p - 1;
                want = Want::Operand;
            }
            _ => break,
        }
    }
    k
}

/// The end of the head of the chain `t[from..to]`: its first segment, a path with its call
/// (`crate::host_settings::config_dir()`), before the first `.` or `?`.
fn head_end(t: &[(Tok, usize)], from: usize, to: usize) -> usize {
    let mut k = from;
    while k < to {
        match t[k].0 {
            Tok::Punct('.' | '?') => return k,
            Tok::Punct('(' | '[') => k = closing(t, k),
            _ => k += 1,
        }
    }
    to
}

/// Whether the chain `t[from..to]` is on the settings folder, with nothing between: its head
/// names the folder (or is the app's settings file, `settings_file`, whose `parent` it then
/// takes), and no `.join` or `.parent()` after that takes it elsewhere.
fn chain_on_folder(
    t: &[(Tok, usize)],
    from: usize,
    to: usize,
    vars: &BTreeSet<String>,
    fns: &BTreeSet<String>,
) -> bool {
    let head = head_end(t, from, to);
    let words = |a: usize, b: usize, w: &str| (a..b).filter(|&j| is_ident(t.get(j), w)).count();
    // The head's own path: what its call is given (`data_dir_for(config_dir())`) is not it.
    let mut source = None;
    let mut j = from;
    while j < head {
        if is_punct(t.get(j), '(') || is_punct(t.get(j), '[') {
            j = closing(t, j);
            continue;
        }
        if source_at(t, j, vars, fns) {
            source = Some(j);
        }
        j += 1;
    }
    if let Some(s) = source {
        return words(s, to, "join") == 0 && words(head, to, "parent") == 0;
    }
    // The app's settings file, in the settings folder: its parent is the folder.
    is_ident(t.get(from), "settings_file")
        && words(from, to, "parent") == 1
        && words(from, to, "join") == 0
}

/// The receiver of the `.` at `dot` is on the settings folder: as a chain, or as the parameter
/// of a closure given to a chain that is (`settings_file.and_then(|p| p.parent()).map(|d|
/// d.join("screensavers"))`).
fn receiver_on_folder(
    t: &[(Tok, usize)],
    dot: usize,
    vars: &BTreeSet<String>,
    fns: &BTreeSet<String>,
) -> bool {
    let s = chain_start(t, dot);
    if chain_on_folder(t, s, dot, vars, fns) {
        return true;
    }
    // `|d| d.join(..)`: the closure is the argument of `.map(` on the chain before it.
    let param = match t.get(s) {
        Some((Tok::Ident(p), _)) if s + 1 == dot && s >= 1 && is_punct(t.get(s - 1), '|') => p,
        _ => return false,
    };
    let Some(bar) = (0..s.saturating_sub(1))
        .rev()
        .take(4)
        .find(|&j| is_punct(t.get(j), '|'))
    else {
        return false;
    };
    let named = (bar + 1..s - 1).any(|j| is_ident(t.get(j), param));
    if !named || bar < 3 || !is_punct(t.get(bar - 1), '(') || !is_punct(t.get(bar - 3), '.') {
        return false;
    }
    let outer = chain_start(t, bar - 3);
    chain_on_folder(t, outer, bar - 3, vars, fns)
}

/// The end of the expression starting at `from` (a `let`'s value): the `;`, `else` or, for
/// `if let` and `while let`, `{` at its own depth.
fn expression_end(t: &[(Tok, usize)], from: usize, brace_ends: bool) -> usize {
    let mut k = from;
    while k < t.len() {
        match &t[k].0 {
            Tok::Punct('(' | '[') => {
                k = closing(t, k);
                continue;
            }
            Tok::Punct('{') if brace_ends => return k,
            Tok::Punct('{') => {
                k = closing(t, k);
                continue;
            }
            Tok::Punct(';' | '}' | ')' | ']' | ',') => return k,
            Tok::Punct('&') if is_punct(t.get(k + 1), '&') => return k,
            Tok::Ident(s) if s == "else" => return k,
            _ => {}
        }
        k += 1;
    }
    k
}

/// Where the value `t[from..to]` of a `let` has its chain: inside `Some(..)`, `Ok(..)`, past a
/// `&`.
fn value_chain(t: &[(Tok, usize)], from: usize, to: usize) -> (usize, usize) {
    let mut k = from;
    while is_punct(t.get(k), '&') || is_ident(t.get(k), "mut") {
        k += 1;
    }
    if (is_ident(t.get(k), "Some") || is_ident(t.get(k), "Ok")) && is_punct(t.get(k + 1), '(') {
        let close = closing(t, k + 1);
        if close == to {
            return value_chain(t, k + 2, close - 1);
        }
    }
    (k, to)
}

/// A path built in the settings folder: where, and the name's first component, or the argument
/// as written when it is neither a string nor a `const`.
#[derive(Debug)]
struct Join {
    file: String,
    line: usize,
    name: Result<String, String>,
}

/// The files a module path names (`jetcarp_store::library`, `crate::machine`), from the file
/// `file` it is written in.
fn module_files(path: &[&str], file: &str) -> Vec<String> {
    let (krate, mods) = match path.split_first() {
        Some((&"crate", rest)) => (file.split('/').next().unwrap_or("").to_string(), rest),
        Some((first, rest)) if first.starts_with("jetcarp") => (first.replace('_', "-"), rest),
        _ => return vec![],
    };
    if mods.is_empty() {
        return vec![format!("{krate}/src/lib.rs")];
    }
    let m = mods.join("/");
    vec![
        format!("{krate}/src/{m}.rs"),
        format!("{krate}/src/{m}/mod.rs"),
    ]
}

/// What `.join(...)`'s argument `arg` names, from strings and the code's `const`s (the module
/// its path names, else this file's, else the only one of the name): its first component;
/// `None` for a temporary file (`format!("{FILE}.tmp-{}")`).
fn resolve(
    arg: &[(Tok, usize)],
    file: &str,
    consts: &BTreeMap<String, Vec<(String, String)>>,
) -> Option<Result<String, String>> {
    let text = || {
        arg.iter()
            .map(|(t, _)| match t {
                Tok::Ident(s) => s.clone(),
                Tok::Str(s) => format!("{s:?}"),
                Tok::Punct(p) => p.to_string(),
                Tok::Other => "_".into(),
            })
            .collect::<String>()
    };
    let arg: Vec<&(Tok, usize)> = arg
        .iter()
        .skip_while(|(t, _)| *t == Tok::Punct('&'))
        .collect();
    let first = |v: &str| v.split(['/', '\\']).next().unwrap_or(v).to_string();
    match arg.as_slice() {
        [(Tok::Str(s), _)] => Some(Ok(first(s))),
        [(Tok::Ident(f), _), (Tok::Punct('!'), _), rest @ ..] if f == "format" => {
            match rest.get(1) {
                Some((Tok::Str(s), _)) if s.contains("tmp") => None,
                _ => Some(Err(text())),
            }
        }
        [.., (Tok::Ident(last), _)]
            if last
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                && arg
                    .iter()
                    .all(|(t, _)| matches!(t, Tok::Ident(_) | Tok::Punct(':'))) =>
        {
            let Some(found) = consts.get(last) else {
                return Some(Err(format!("{} (no such const)", text())));
            };
            let path: Vec<&str> = arg
                .iter()
                .filter_map(|(t, _)| match t {
                    Tok::Ident(s) => Some(s.as_str()),
                    _ => None,
                })
                .collect();
            let places = if path.len() > 1 {
                module_files(&path[..path.len() - 1], file)
            } else {
                vec![file.to_string()]
            };
            let pick = |of: &dyn Fn(&str) -> bool| -> BTreeSet<&String> {
                found
                    .iter()
                    .filter(|(f, _)| of(f))
                    .map(|(_, v)| v)
                    .collect()
            };
            let mut values = pick(&|f| places.iter().any(|p| p == f));
            if values.is_empty() {
                values = pick(&|_| true);
            }
            match values.len() {
                1 => Some(Ok(first(values.into_iter().next().unwrap()))),
                _ => Some(Err(format!("{} (a const of several values)", text()))),
            }
        }
        _ => Some(Err(text())),
    }
}

/// Every path the code builds in the settings folder.
fn joins_in_settings(code: &[Code]) -> Vec<Join> {
    let consts = consts(code);
    let mut out = Vec::new();
    let none = BTreeSet::new();
    for c in code {
        let t = &c.toks;
        // This file's functions that return the settings folder: a body that is a chain on it
        // (`fn dir() -> Result<PathBuf, String> { config_dir().ok_or(...) }`).
        let mut fns = BTreeSet::new();
        for k in 0..t.len() {
            if !is_ident(t.get(k), "fn") {
                continue;
            }
            let Some((Tok::Ident(name), _)) = t.get(k + 1) else {
                continue;
            };
            let mut b = k + 2;
            while b < t.len() && !is_punct(t.get(b), '{') && !is_punct(t.get(b), ';') {
                b = if is_punct(t.get(b), '(') {
                    closing(t, b)
                } else {
                    b + 1
                };
            }
            if !is_punct(t.get(b), '{') {
                continue;
            }
            let end = closing(t, b) - 1;
            let (s, e) = value_chain(t, b + 1, end);
            if chain_on_folder(t, s, e, &none, &none) && chain_start(t, e) <= s {
                fns.insert(name.clone());
            }
        }
        // Variables bound to the folder in the function being read.
        let mut vars: BTreeSet<String> = BTreeSet::new();
        for k in 0..t.len() {
            if is_ident(t.get(k), "fn") {
                vars.clear();
            }
            if is_ident(t.get(k), "let") {
                let brace_ends =
                    k > 0 && (is_ident(t.get(k - 1), "if") || is_ident(t.get(k - 1), "while"));
                let mut eq = k + 1;
                while eq < t.len() && !is_punct(t.get(eq), '=') && !is_punct(t.get(eq), ';') {
                    eq = if is_punct(t.get(eq), '(') {
                        closing(t, eq)
                    } else {
                        eq + 1
                    };
                }
                if is_punct(t.get(eq), '=') && !is_punct(t.get(eq + 1), '=') {
                    let end = expression_end(t, eq + 1, brace_ends);
                    let (s, e) = value_chain(t, eq + 1, end);
                    if chain_on_folder(t, s, e, &vars, &fns) {
                        for (tok, _) in &t[k + 1..eq] {
                            if let Tok::Ident(n) = tok
                                && !matches!(n.as_str(), "Some" | "Ok" | "mut" | "ref")
                                && n.chars().next().is_some_and(|ch| ch.is_lowercase())
                            {
                                vars.insert(n.clone());
                            }
                        }
                    }
                }
            }
            if is_punct(t.get(k), '.')
                && is_ident(t.get(k + 1), "join")
                && is_punct(t.get(k + 2), '(')
                && receiver_on_folder(t, k, &vars, &fns)
            {
                let close = closing(t, k + 2);
                if let Some(name) = resolve(&t[k + 3..close - 1], &c.name, &consts) {
                    out.push(Join {
                        file: c.name.clone(),
                        line: t[k].1,
                        name,
                    });
                }
            }
        }
    }
    out
}

#[test]
fn every_file_the_code_keeps_in_the_settings_folder_is_on_uninstall_s_list() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let code = read_code(&crates);
    assert!(
        code.len() > 100,
        "the crates' code was not found: {}",
        code.len()
    );
    let joins = joins_in_settings(&code);
    let mut problems = Vec::new();
    let mut unresolved_used = BTreeSet::new();
    let mut names = BTreeSet::new();
    for j in &joins {
        match &j.name {
            Ok(n) => {
                if !settings_entry_listed(n) {
                    problems.push(format!(
                        "{}:{}: `{n}` is kept in the settings folder, but `uninstall --settings` \
                         does not count it as JETCARP's: add it to SETTINGS_FILES or \
                         SETTINGS_FOLDERS (jetcarp-api's src/ops/system.rs), and to the book's \
                         folders page",
                        j.file, j.line
                    ));
                } else {
                    names.insert(n.clone());
                }
            }
            Err(arg) => match UNRESOLVED
                .iter()
                .position(|(f, a, _)| *f == j.file && a == arg)
            {
                Some(k) => {
                    unresolved_used.insert(k);
                }
                None => problems.push(format!(
                    "{}:{}: a path in the settings folder named by `{arg}`, which is not a \
                     string or a const: name it by one, or add it to UNRESOLVED here with why \
                     every name it can take is listed",
                    j.file, j.line
                )),
            },
        }
    }
    for (k, (f, a, _)) in UNRESOLVED.iter().enumerate() {
        if !unresolved_used.contains(&k) {
            problems.push(format!(
                "UNRESOLVED names `{a}` in {f}, which no longer joins it"
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    // The test finds what it should: the files of #349 among them, and the writers of each
    // crate that keeps something there.
    for want in [
        "machine.json",
        "limits.json",
        "app.json",
        "host.json",
        "setup.json",
        "paths.json",
        "photosensitivity.json",
        "gpu.json",
        "gpu-checks.json",
        "host-program",
        "desktop-picture.json",
        "desktop-pictures",
        "screen-saver-before.plist",
        "wallpaper-store-before-saver-remove.plist",
        "screen-saver-state.json",
        "screensavers",
        "rotations",
        "tune",
        "pack",
        "http-tokens",
    ] {
        assert!(
            names.contains(want),
            "{want} was not found in the code: {names:?}"
        );
    }
}

/// The gathering itself, on code written for it.
#[test]
fn the_gathering_follows_the_folder_through_variables_and_leaves_out_tests() {
    let src = r#"
        const MINE: &str = "mine.json";
        /// Not the settings folder's: the system's.
        fn os() -> Option<PathBuf> { dirs::config_dir().map(|d| d.join("elsewhere.json")) }
        fn dir() -> Result<PathBuf, String> { jetcarp_core::paths::config_dir().ok_or("none") }
        fn a(settings: &Path) {
            let f = settings.join(MINE);
            let tmp = settings.join(format!("{MINE}.tmp-{}", 1));
            let inner = settings.join("pack").join("key"); // "pack" only
            std::fs::write(f, "x // not a comment").unwrap();
        }
        fn b() {
            if let Some(d) = crate::host_settings::config_dir() && make(&d) {
                let _ = copy(&x, d.join("copied.plist"));
            }
            let p = dir()?.join("from-fn.json");
            let q = self.settings.folder.join("not-a-settings-path");
            let name = pick();
            let r = config.join(name);
        }
        #[cfg(test)]
        mod tests {
            fn t(settings: &Path) { settings.join("scratch.json"); }
        }
    "#;
    let code = vec![Code {
        name: "x/src/lib.rs".into(),
        toks: without_tests(tokens(src)).0,
    }];
    let got: Vec<Result<String, String>> = joins_in_settings(&code)
        .into_iter()
        .map(|j| j.name)
        .collect();
    assert_eq!(
        got,
        vec![
            Ok("mine.json".into()),
            Ok("pack".into()),
            Ok("copied.plist".into()),
            Ok("from-fn.json".into()),
            Err("name".into()),
        ]
    );
}
