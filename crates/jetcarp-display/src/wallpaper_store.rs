//! macOS's wallpaper store, where macOS 14 and later keep the desktop picture (`Desktop`) and the
//! screen saver (`Idle`) chosen for every display and Space:
//! `~/Library/Application Support/com.apple.wallpaper/Store/Index.plist`.
//!
//! Deleting `JETCARP.saver` leaves the store choosing it: System Settings then shows a blank tile
//! named "custom" on every display and Space (seen on the Mac in rc1). So installing records the
//! `Idle` choices it is about to replace ([`record`]), and removing puts them back where they still
//! name JETCARP's saver ([`restore`]), or, with nothing recorded, shows that display's desktop
//! picture as the screen saver, the choice the store held before rc1 on that Mac.
//!
//! The store is a binary property list; macOS's `plutil` turns it into XML and back (see
//! [`crate::macsaver`]). This module reads and writes that XML, and works on the tree, so all of
//! it is tested on every system. Values it does not look into (data, dates, numbers) are kept as
//! their text, so a store read and written again is unchanged.

/// A property list value, as XML property lists hold them.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// Keys in their order.
    Dict(Vec<(String, Value)>),
    Array(Vec<Value>),
    String(String),
    /// Base64 text, without its whitespace.
    Data(String),
    Date(String),
    Integer(String),
    Real(String),
    Bool(bool),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Dict(d) => d.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        match self {
            Value::Dict(d) => d.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// The value at a path of dictionary keys.
    fn at(&self, path: &[String]) -> Option<&Value> {
        path.iter().try_fold(self, |v, k| v.get(k))
    }

    fn at_mut(&mut self, path: &[String]) -> Option<&mut Value> {
        let mut v = self;
        for k in path {
            v = v.get_mut(k)?;
        }
        Some(v)
    }
}

// ---------------------------------------------------------------------------------------------
// XML

/// Reads an XML property list, as `plutil -convert xml1` writes one.
pub fn parse(xml: &str) -> Result<Value, String> {
    let mut p = Parser { s: xml, i: 0 };
    p.skip_misc();
    let (name, empty) = p.open_tag()?;
    if name != "plist" || empty {
        return Err("not a property list".into());
    }
    let v = p.value()?;
    p.skip_misc();
    p.close_tag("plist")?;
    Ok(v)
}

struct Parser<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        &self.s[self.i..]
    }

    /// Skips whitespace, the XML declaration, the doctype and comments.
    fn skip_misc(&mut self) {
        loop {
            let r = self.rest();
            let t = r.trim_start();
            self.i += r.len() - t.len();
            let end = if t.starts_with("<?") {
                t.find("?>").map(|e| e + 2)
            } else if t.starts_with("<!--") {
                t.find("-->").map(|e| e + 3)
            } else if t.starts_with("<!") {
                t.find('>').map(|e| e + 1)
            } else {
                None
            };
            match end {
                Some(e) => self.i += e,
                None => return,
            }
        }
    }

    /// `<name ...>` or `<name .../>`: the name, and whether it closed itself.
    fn open_tag(&mut self) -> Result<(String, bool), String> {
        self.skip_misc();
        let r = self.rest();
        if !r.starts_with('<') || r.starts_with("</") {
            return Err(format!("expected a tag at byte {}", self.i));
        }
        let end = r.find('>').ok_or("a tag is not closed")?;
        let inner = &r[1..end];
        let empty = inner.ends_with('/');
        let inner = inner.trim_end_matches('/');
        let name = inner.split_whitespace().next().unwrap_or("").to_string();
        self.i += end + 1;
        Ok((name, empty))
    }

    fn close_tag(&mut self, name: &str) -> Result<(), String> {
        self.skip_misc();
        let want = format!("</{name}>");
        if self.rest().starts_with(&want) {
            self.i += want.len();
            Ok(())
        } else {
            Err(format!("expected {want} at byte {}", self.i))
        }
    }

    fn at_close(&mut self, name: &str) -> bool {
        self.skip_misc();
        self.rest().starts_with(&format!("</{name}>"))
    }

    /// Text up to `</name>`, entities decoded, and the closing tag.
    fn text(&mut self, name: &str) -> Result<String, String> {
        let want = format!("</{name}>");
        let r = self.rest();
        let end = r.find(&want).ok_or_else(|| format!("{want} is missing"))?;
        let t = unescape(&r[..end])?;
        self.i += end + want.len();
        Ok(t)
    }

    fn value(&mut self) -> Result<Value, String> {
        let (name, empty) = self.open_tag()?;
        let text = |p: &mut Self| -> Result<String, String> {
            if empty {
                Ok(String::new())
            } else {
                p.text(&name)
            }
        };
        Ok(match name.as_str() {
            "dict" => {
                let mut d = Vec::new();
                if !empty {
                    while !self.at_close("dict") {
                        let (k, kempty) = self.open_tag()?;
                        if k != "key" {
                            return Err(format!("expected a key in a dict, got <{k}>"));
                        }
                        let key = if kempty {
                            String::new()
                        } else {
                            self.text("key")?
                        };
                        d.push((key, self.value()?));
                    }
                    self.close_tag("dict")?;
                }
                Value::Dict(d)
            }
            "array" => {
                let mut a = Vec::new();
                if !empty {
                    while !self.at_close("array") {
                        a.push(self.value()?);
                    }
                    self.close_tag("array")?;
                }
                Value::Array(a)
            }
            "string" => Value::String(text(self)?),
            "data" => Value::Data(text(self)?.split_whitespace().collect()),
            "date" => Value::Date(text(self)?.trim().to_string()),
            "integer" => Value::Integer(text(self)?.trim().to_string()),
            "real" => Value::Real(text(self)?.trim().to_string()),
            "true" if empty => Value::Bool(true),
            "false" if empty => Value::Bool(false),
            other => return Err(format!("unknown property list element <{other}>")),
        })
    }
}

fn unescape(s: &str) -> Result<String, String> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(a) = rest.find('&') {
        out.push_str(&rest[..a]);
        let after = &rest[a + 1..];
        let e = after.find(';').ok_or("an entity is not closed")?;
        let name = &after[..e];
        let c = match name {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let n = if let Some(h) = name.strip_prefix("#x") {
                    u32::from_str_radix(h, 16).ok()
                } else if let Some(d) = name.strip_prefix('#') {
                    d.parse().ok()
                } else {
                    None
                };
                n.and_then(char::from_u32)
                    .ok_or_else(|| format!("unknown entity &{name};"))?
            }
        };
        out.push(c);
        rest = &after[e + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Writes an XML property list, laid out as `plutil` lays one out.
pub fn write(v: &Value) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n",
    );
    write_value(&mut s, v, 0);
    s.push_str("</plist>\n");
    s
}

fn write_value(s: &mut String, v: &Value, depth: usize) {
    let pad = "\t".repeat(depth);
    let leaf = |s: &mut String, tag: &str, text: &str| {
        s.push_str(&format!("{pad}<{tag}>{text}</{tag}>\n"));
    };
    match v {
        Value::Dict(d) if d.is_empty() => s.push_str(&format!("{pad}<dict/>\n")),
        Value::Dict(d) => {
            s.push_str(&format!("{pad}<dict>\n"));
            for (k, x) in d {
                s.push_str(&format!("{pad}\t<key>{}</key>\n", escape(k)));
                write_value(s, x, depth + 1);
            }
            s.push_str(&format!("{pad}</dict>\n"));
        }
        Value::Array(a) if a.is_empty() => s.push_str(&format!("{pad}<array/>\n")),
        Value::Array(a) => {
            s.push_str(&format!("{pad}<array>\n"));
            for x in a {
                write_value(s, x, depth + 1);
            }
            s.push_str(&format!("{pad}</array>\n"));
        }
        Value::String(t) => leaf(s, "string", &escape(t)),
        Value::Data(t) => leaf(s, "data", t),
        Value::Date(t) => leaf(s, "date", t),
        Value::Integer(t) => leaf(s, "integer", t),
        Value::Real(t) => leaf(s, "real", t),
        Value::Bool(true) => s.push_str(&format!("{pad}<true/>\n")),
        Value::Bool(false) => s.push_str(&format!("{pad}<false/>\n")),
    }
}

/// Base64 to bytes (whitespace and padding ignored; `None` for anything else).
fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------------------------
// The screen saver choices

/// The provider of a screen saver choice in the store.
pub const SAVER_PROVIDER: &str = "com.apple.wallpaper.choice.screen-saver";

/// Whether a choice's configuration names the bundle `bundle` (`JETCARP.saver`): the bytes of its
/// file URL, which a binary property list keeps as ASCII, or as UTF-16 when the path is not ASCII.
fn names_bundle(config: &[u8], bundle: &str) -> bool {
    let ascii = bundle.as_bytes();
    let utf16: Vec<u8> = bundle.encode_utf16().flat_map(u16::to_be_bytes).collect();
    let has = |needle: &[u8]| config.windows(needle.len()).any(|w| w == needle);
    has(ascii) || has(&utf16)
}

/// Whether an `Idle` value chooses the screen saver `bundle`.
pub fn idle_chooses(idle: &Value, bundle: &str) -> bool {
    let Some(Value::Array(choices)) = idle.get("Content").and_then(|c| c.get("Choices")) else {
        return false;
    };
    choices.iter().any(|c| {
        c.get("Provider").and_then(Value::as_str) == Some(SAVER_PROVIDER)
            && match c.get("Configuration") {
                Some(Value::Data(d)) => base64(d).is_some_and(|b| names_bundle(&b, bundle)),
                Some(Value::String(t)) => names_bundle(t.as_bytes(), bundle),
                _ => false,
            }
    })
}

/// The path of keys to every dictionary in the store that has an `Idle` choice: the store-wide
/// ones (`AllSpacesAndDisplays`, `SystemDefault`), each Space's `Default`, and each display's on
/// each Space.
pub fn idle_places(store: &Value) -> Vec<Vec<String>> {
    fn walk(v: &Value, path: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
        let Value::Dict(d) = v else { return };
        if matches!(v.get("Idle"), Some(Value::Dict(_))) {
            out.push(path.clone());
        }
        for (k, x) in d {
            if k == "Idle" || k == "Desktop" {
                continue;
            }
            path.push(k.clone());
            walk(x, path, out);
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(store, &mut Vec::new(), &mut out);
    out
}

/// What installing records: each place's `Idle` choice that is not `bundle`'s, by its path,
/// keeping what an earlier record (`old`) holds for the places that already choose `bundle`
/// (installing again over a chosen JETCARP). A list of `{Path: [keys], Idle: value}`.
pub fn record(store: &Value, old: Option<&Value>, bundle: &str) -> Value {
    let mut out: Vec<Value> = Vec::new();
    let path_value = |p: &[String]| Value::Array(p.iter().cloned().map(Value::String).collect());
    for p in idle_places(store) {
        let Some(idle) = store.at(&p).and_then(|e| e.get("Idle")) else {
            continue;
        };
        let keep = if idle_chooses(idle, bundle) {
            old.and_then(|o| recorded(o, &p)).cloned()
        } else {
            Some(idle.clone())
        };
        if let Some(idle) = keep {
            out.push(Value::Dict(vec![
                ("Path".into(), path_value(&p)),
                ("Idle".into(), idle),
            ]));
        }
    }
    Value::Array(out)
}

/// The `Idle` value a record holds for a place.
fn recorded<'a>(record: &'a Value, path: &[String]) -> Option<&'a Value> {
    let Value::Array(list) = record else {
        return None;
    };
    list.iter().find_map(|e| {
        let Some(Value::Array(p)) = e.get("Path") else {
            return None;
        };
        let same = p.len() == path.len() && p.iter().zip(path).all(|(a, b)| a.as_str() == Some(b));
        same.then(|| e.get("Idle")).flatten()
    })
}

/// What [`restore`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Restored {
    /// Places put back as the record had them.
    pub recorded: usize,
    /// Places with nothing recorded, now showing their desktop picture as the screen saver.
    pub desktop: usize,
    /// Places still choosing `bundle`, with no desktop picture to show instead.
    pub left: usize,
}

impl Restored {
    pub fn changed(&self) -> bool {
        self.recorded + self.desktop > 0
    }
}

/// Puts back the `Idle` choice of every place that chooses `bundle`: the record's for that place,
/// else the record's store-wide one; with nothing recorded, the place's own desktop picture (or
/// the store-wide default's), which is a choice the store holds for a screen saver too.
pub fn restore(store: &mut Value, record: Option<&Value>, bundle: &str) -> Restored {
    let mut r = Restored::default();
    let wide = |name: &str| vec![name.to_string()];
    let fallback_desktop = [wide("SystemDefault"), wide("AllSpacesAndDisplays")]
        .iter()
        .find_map(|p| store.at(p).and_then(|e| e.get("Desktop")).cloned());
    for p in idle_places(store) {
        let ours = store
            .at(&p)
            .and_then(|e| e.get("Idle"))
            .is_some_and(|i| idle_chooses(i, bundle));
        if !ours {
            continue;
        }
        let from_record = record.and_then(|rec| {
            [
                p.clone(),
                wide("AllSpacesAndDisplays"),
                wide("SystemDefault"),
            ]
            .iter()
            .find_map(|q| recorded(rec, q).filter(|i| !idle_chooses(i, bundle)))
            .cloned()
        });
        let entry = store.at(&p);
        let (new, counter) = match from_record {
            Some(i) => (Some(i), &mut r.recorded),
            None => (
                entry
                    .and_then(|e| e.get("Desktop"))
                    .cloned()
                    .or_else(|| fallback_desktop.clone()),
                &mut r.desktop,
            ),
        };
        match (new, store.at_mut(&p).and_then(|e| e.get_mut("Idle"))) {
            (Some(n), Some(slot)) => {
                *slot = n;
                *counter += 1;
            }
            _ => r.left += 1,
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    fn to_b64(b: &[u8]) -> String {
        let mut s = String::new();
        for c in b.chunks(3) {
            let n = c.iter().fold(0u32, |a, &x| (a << 8) | x as u32) << (8 * (3 - c.len()));
            for k in 0..4 {
                if k <= c.len() {
                    s.push(B64[(n >> (18 - 6 * k) & 63) as usize] as char);
                } else {
                    s.push('=');
                }
            }
        }
        s
    }

    /// A choice as the store keeps one: its configuration a small property list of its own.
    fn choice(provider: &str, url: &str) -> String {
        let config = to_b64(format!("bplist00\u{1}{url}\u{2}").as_bytes());
        format!(
            "<dict><key>Configuration</key><data>\n\t{config}\n\t</data><key>Files</key><array/><key>Provider</key><string>{provider}</string></dict>"
        )
    }

    fn content(provider: &str, url: &str, placement: &str) -> String {
        format!(
            "<dict><key>Content</key><dict><key>Choices</key><array>{}</array><key>EncodedOptionValues</key><string>{placement}</string><key>Shuffle</key><string>$null</string></dict><key>LastSet</key><date>2026-09-27T05:33:01Z</date><key>LastUse</key><date>2026-11-12T06:18:00Z</date></dict>",
            choice(provider, url)
        )
    }

    const IMAGE: &str = "com.apple.wallpaper.choice.image";
    const PICTURE: &str = "file:///Users/ada/Pictures/Screenshot%20A&amp;B.png";
    const OURS: &str = "file:///Users/ada/Library/Screen%20Savers/JETCARP.saver";
    const FLURRY: &str = "file:///System/Library/Screen%20Savers/Flurry.saver";

    /// The store as the Mac in rc1 had it, in the shape `plutil -convert xml1` gives: the
    /// screen saver choice (Idle) on every place, the desktop picture on most.
    fn store(idle: &str) -> String {
        let place = |idle: &str| {
            format!(
                "<dict><key>Desktop</key>{}<key>Idle</key>{idle}<key>Type</key><string>individual</string></dict>",
                content(IMAGE, PICTURE, "Crop")
            )
        };
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>AllSpacesAndDisplays</key>
	<dict><key>Idle</key>{idle}<key>Type</key><string>idle</string></dict>
	<key>Displays</key><dict/>
	<key>Spaces</key>
	<dict>
		<key></key>
		<dict><key>Default</key>{p}<key>Displays</key><dict><key>37D8832A-2D66</key>{p}</dict></dict>
		<key>12324632-DEDA</key>
		<dict><key>Default</key>{p}<key>Displays</key><dict><key>37D8832A-2D66</key>{p}<key>6F1FB3C0</key>{p}</dict></dict>
	</dict>
	<key>SystemDefault</key>{p}
	<key>Version</key><integer>1</integer>
	<key>Flag</key><true/>
</dict>
</plist>
"#,
            p = place(idle)
        )
    }

    fn idle(provider: &str, url: &str) -> String {
        content(provider, url, "SizeToFit")
    }

    #[test]
    fn a_store_reads_and_writes_back_unchanged() {
        let v = parse(&store(&idle(IMAGE, PICTURE))).unwrap();
        assert_eq!(parse(&write(&v)).unwrap(), v);
        assert_eq!(idle_places(&v).len(), 7);
        let first = v
            .at(&["Spaces".into(), "".into(), "Default".into()])
            .unwrap();
        let url = first.get("Desktop").unwrap().get("Content").unwrap();
        assert!(matches!(url.get("Shuffle"), Some(Value::String(s)) if s == "$null"));
        // Entities and empty elements.
        let t = parse("<plist><dict><key>a&lt;b</key><string>x &amp; &#233;&#x41;</string><key/><string/></dict></plist>").unwrap();
        assert_eq!(t.get("a<b"), Some(&Value::String("x & éA".into())));
        assert_eq!(t.get(""), Some(&Value::String(String::new())));
        assert!(write(&t).contains("<key>a&lt;b</key>"));
        assert!(parse("<plist><dict><string>x</string></dict></plist>").is_err());
    }

    #[test]
    fn the_saver_is_found_in_a_choice_in_ascii_or_utf16() {
        let v = parse(&store(&idle(SAVER_PROVIDER, OURS))).unwrap();
        let i = v.get("AllSpacesAndDisplays").unwrap().get("Idle").unwrap();
        assert!(idle_chooses(i, "JETCARP.saver"));
        assert!(!idle_chooses(i, "Flurry.saver"));
        // A picture named JETCARP.saver is not the screen saver.
        let pic = parse(&store(&idle(IMAGE, OURS))).unwrap();
        let i = pic
            .get("AllSpacesAndDisplays")
            .unwrap()
            .get("Idle")
            .unwrap();
        assert!(!idle_chooses(i, "JETCARP.saver"));
        let utf16: Vec<u8> = "/Users/zoë/Library/Screen Savers/JETCARP.saver"
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect();
        assert!(names_bundle(&utf16, "JETCARP.saver"));
        assert_eq!(
            base64(&to_b64(b"any bytes \x00\xff")).unwrap(),
            b"any bytes \x00\xff"
        );
    }

    #[test]
    fn removing_puts_back_what_installing_recorded() {
        // Installed while the picture was the screen saver; then JETCARP was chosen.
        let before = parse(&store(&idle(IMAGE, PICTURE))).unwrap();
        let rec = record(&before, None, "JETCARP.saver");
        let Value::Array(list) = &rec else { panic!() };
        assert_eq!(list.len(), 7);
        let mut chosen = parse(&store(&idle(SAVER_PROVIDER, OURS))).unwrap();
        // Installing again over the chosen JETCARP keeps the first record.
        let again = record(&chosen, Some(&rec), "JETCARP.saver");
        assert_eq!(again, rec);
        let r = restore(&mut chosen, Some(&rec), "JETCARP.saver");
        assert_eq!(
            r,
            Restored {
                recorded: 7,
                desktop: 0,
                left: 0
            }
        );
        assert_eq!(chosen, before);
        // Nothing left to do a second time.
        assert!(!restore(&mut chosen, Some(&rec), "JETCARP.saver").changed());
    }

    #[test]
    fn another_saver_chosen_since_is_left_alone() {
        let before = parse(&store(&idle(IMAGE, PICTURE))).unwrap();
        let rec = record(&before, None, "JETCARP.saver");
        let mut flurry = parse(&store(&idle(SAVER_PROVIDER, FLURRY))).unwrap();
        let same = flurry.clone();
        assert!(!restore(&mut flurry, Some(&rec), "JETCARP.saver").changed());
        assert_eq!(flurry, same);
    }

    #[test]
    fn with_no_record_the_desktop_picture_becomes_the_screen_saver() {
        let mut chosen = parse(&store(&idle(SAVER_PROVIDER, OURS))).unwrap();
        let r = restore(&mut chosen, None, "JETCARP.saver");
        // The six places with a desktop picture show it; the store-wide one, which has none,
        // takes the system default's.
        assert_eq!(
            r,
            Restored {
                recorded: 0,
                desktop: 7,
                left: 0
            }
        );
        for p in idle_places(&chosen) {
            let e = chosen.at(&p).unwrap();
            assert!(!idle_chooses(e.get("Idle").unwrap(), "JETCARP.saver"));
        }
        let sd = chosen.get("SystemDefault").unwrap();
        assert_eq!(sd.get("Idle"), sd.get("Desktop"));
        assert_eq!(
            chosen.get("AllSpacesAndDisplays").unwrap().get("Idle"),
            sd.get("Desktop")
        );
    }

    #[test]
    fn a_space_made_after_installing_takes_the_store_wide_record() {
        let before = parse(&store(&idle(SAVER_PROVIDER, FLURRY))).unwrap();
        let mut rec = record(&before, None, "JETCARP.saver");
        // Keep only the store-wide entry, as if the Spaces came later.
        if let Value::Array(list) = &mut rec {
            list.retain(|e| {
                matches!(e.get("Path"), Some(Value::Array(p)) if p.len() == 1
                    && p[0].as_str() == Some("AllSpacesAndDisplays"))
            });
        }
        let mut chosen = parse(&store(&idle(SAVER_PROVIDER, OURS))).unwrap();
        assert_eq!(
            restore(&mut chosen, Some(&rec), "JETCARP.saver"),
            Restored {
                recorded: 7,
                desktop: 0,
                left: 0
            }
        );
        assert_eq!(chosen, before);
    }
}
