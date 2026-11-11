//! Local paths never leave the machine: a pack names a file by its name only.
//!
//! [`strip`] rewrites every absolute path in a text (`/home/x/scenes/a.json`, `~/a.json`,
//! `C:\Users\x\a.json`, `\\server\share\a.json`, `file:///...`) as its last component, and
//! [`strip_json`] does it to every string in a JSON value. [`find_absolute`] is the check the
//! tests run over everything a publish writes.

use serde_json::Value;

/// Does a path-like token start at byte `i` of `s` (after a separator or at the start)?
fn starts_path(s: &[u8], i: usize) -> Option<usize> {
    let rest = &s[i..];
    let boundary = i == 0
        || matches!(
            s[i - 1],
            b' ' | b'\t' | b'\n' | b'"' | b'\'' | b'(' | b'=' | b',' | b':' | b'[' | b'<' | b'`'
        );
    if !boundary {
        return None;
    }
    if rest.starts_with(b"file://") {
        return Some(7);
    }
    if rest.starts_with(b"~/") || rest.starts_with(b"\\\\") {
        return Some(0);
    }
    if rest.len() >= 3
        && rest[0].is_ascii_alphabetic()
        && rest[1] == b':'
        && (rest[2] == b'\\' || rest[2] == b'/')
    {
        return Some(0);
    }
    // `/x...`: a Unix absolute path with at least one more component (`/` alone, `c/4` and
    // `B3/S23` are not paths).
    if rest.len() >= 2 && rest[0] == b'/' && rest[1].is_ascii_alphanumeric() {
        let tail = &rest[1..];
        let end = tail
            .iter()
            .position(|b| matches!(b, b' ' | b'\t' | b'\n' | b'"' | b'\'' | b')' | b','))
            .unwrap_or(tail.len());
        if tail[..end].contains(&b'/') {
            return Some(0);
        }
    }
    None
}

/// The text with every absolute path replaced by its last component.
pub fn strip(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let mut copied = 0;
    while i < b.len() {
        if let Some(skip) = starts_path(b, i) {
            let start = i + skip;
            let end = b[start..]
                .iter()
                .position(|c| matches!(c, b' ' | b'\t' | b'\n' | b'"' | b'\'' | b')' | b','))
                .map(|p| start + p)
                .unwrap_or(b.len());
            let path = &s[start..end];
            let name = path
                .rsplit(['/', '\\'])
                .find(|x| !x.is_empty())
                .unwrap_or("");
            out.push_str(&s[copied..i]);
            out.push_str(name);
            i = end;
            copied = end;
            continue;
        }
        i += 1;
    }
    out.push_str(&s[copied..]);
    out
}

/// Every string in a value (keys too) stripped of absolute paths.
pub fn strip_json(v: Value) -> Value {
    match v {
        Value::String(s) => Value::String(strip(&s)),
        Value::Array(a) => Value::Array(a.into_iter().map(strip_json).collect()),
        Value::Object(m) => Value::Object(
            m.into_iter()
                .map(|(k, v)| (strip(&k), strip_json(v)))
                .collect(),
        ),
        x => x,
    }
}

/// The first absolute path in some bytes (text or binary), if any: what the no-local-paths
/// test looks for in every file a publish writes.
pub fn find_absolute(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let b = text.as_bytes();
    for i in 0..b.len() {
        if let Some(skip) = starts_path(b, i) {
            let start = i + skip;
            let end = (start + 80).min(b.len());
            let tok = String::from_utf8_lossy(&b[start..end]).to_string();
            // Common prefixes of real local paths only, so random bytes in compressed data
            // do not count.
            for p in [
                "/home/", "/Users/", "/tmp/", "/root/", "/var/", "/mnt/", "/media/", "~/",
            ] {
                if tok.starts_with(p) {
                    return Some(tok);
                }
            }
            if tok.len() > 3 && tok.as_bytes()[1] == b':' && tok[2..].starts_with('\\') {
                return Some(tok);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_become_names() {
        assert_eq!(
            strip("import /home/j/cat/catalog.sqlite"),
            "import catalog.sqlite"
        );
        assert_eq!(
            strip("scene ~/scenes/glider.json ok"),
            "scene glider.json ok"
        );
        assert_eq!(strip(r"C:\Users\j\a.json"), "a.json");
        assert_eq!(strip("file:///tmp/x/y.png"), "y.png");
        assert_eq!(strip(r"\\server\share\z.json"), "z.json");
        // Not paths: rules, speeds, relative names.
        for keep in [
            "B3/S23",
            "c/4 diagonal",
            "collide a + b",
            "runs/part-000001.parquet",
            "search.sqlite",
            "/",
            "ratings:ratings1",
        ] {
            assert_eq!(strip(keep), keep);
        }
        assert_eq!(
            strip_json(serde_json::json!({"import": "/data/x/search.sqlite", "n": 3})),
            serde_json::json!({"import": "search.sqlite", "n": 3})
        );
        assert!(find_absolute(b"source /home/jimmie/x").is_some());
        assert!(find_absolute(b"B3/S23 c/4").is_none());
    }
}
