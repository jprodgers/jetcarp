//! The JETCARP book's pages and the JSON Schemas, embedded in the server (`build.rs`): the
//! resources `jetcarp://docs/{page}` and `jetcarp://schema/{name}.json`, and `help {"page": ...}`.
//!
//! The pages are copies of `docs/src` kept in `crates/jetcarp-mcp/resources/docs` (so the crate
//! packages on its own), the schemas copies of `docs/src/reference/*.schema.json`;
//! `docs/tools/copy-mcp-resources.py` refreshes both, and a test fails when they differ.

use std::sync::OnceLock;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/resources.rs"));
}

/// Every embedded book file: its path under `docs/src` (with `.md`) and its text.
pub fn files() -> &'static [(&'static str, &'static str)] {
    embedded::DOCS
}

/// Every embedded schema: its file name under `docs/src/reference` without `.schema` (`scene.json`)
/// and its text.
pub fn schemas() -> &'static [(&'static str, &'static str)] {
    embedded::SCHEMAS
}

/// A schema by name (`scene.json`, `pack.json`).
pub fn schema(name: &str) -> Option<&'static str> {
    schemas().iter().find(|(n, _)| *n == name).map(|(_, t)| *t)
}

/// One page of the book.
#[derive(Clone, Debug)]
pub struct Page {
    /// Its path without `.md` (`guides/searching`).
    pub path: &'static str,
    /// Its first heading.
    pub title: String,
    pub text: &'static str,
}

/// The book's pages in the order of its table of contents (`SUMMARY.md`), then any others.
pub fn pages() -> &'static [Page] {
    static P: OnceLock<Vec<Page>> = OnceLock::new();
    P.get_or_init(|| {
        let mut order: Vec<&str> = vec![];
        if let Some((_, summary)) = files().iter().find(|(p, _)| *p == "SUMMARY.md") {
            let mut rest = *summary;
            while let Some(i) = rest.find("](") {
                rest = &rest[i + 2..];
                if let Some(j) = rest.find(')') {
                    let link = &rest[..j];
                    if let Some(p) = link.strip_suffix(".md") {
                        order.push(p);
                    }
                    rest = &rest[j..];
                }
            }
        }
        let mut v: Vec<Page> = files()
            .iter()
            .filter(|(p, _)| *p != "SUMMARY.md")
            .map(|(p, text)| Page {
                path: p.strip_suffix(".md").unwrap_or(p),
                title: text
                    .lines()
                    .find_map(|l| l.strip_prefix("# "))
                    .unwrap_or(p)
                    .trim()
                    .to_string(),
                text,
            })
            .collect();
        v.sort_by_key(|p| {
            (
                order
                    .iter()
                    .position(|o| *o == p.path)
                    .unwrap_or(usize::MAX),
                p.path,
            )
        });
        v
    })
}

/// A page's text by its path (`guides/searching`), as the book's file has it.
pub fn page(path: &str) -> Option<&'static str> {
    pages().iter().find(|p| p.path == path).map(|p| p.text)
}

/// A page as the server hands it out (`resources/read`, `help {"page": ...}`): [`page`] with the
/// HTML entities [`served`] turns back into characters.
pub fn page_served(path: &str) -> Option<String> {
    page(path).map(|t| served(t).into_owned())
}

/// Markdown as a client reads it, as text: the HTML entities a page writes so that the rendered
/// book shows `<` and `>` (the MCP reference's `&lt;id>` and `L&lt;level>`, #92) turned back into
/// the characters, outside code, where Markdown shows an entity as it is written (#126).
pub fn served(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains('&') {
        return std::borrow::Cow::Borrowed(text);
    }
    let decode = |s: &str| {
        s.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&amp;", "&")
    };
    let mut out = String::with_capacity(text.len());
    let mut fenced = false;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            out.push_str(line);
        } else if fenced || !line.contains('&') {
            out.push_str(line);
        } else {
            // Between backticks is a code span: the odd-numbered pieces.
            for (i, piece) in line.split('`').enumerate() {
                if i > 0 {
                    out.push('`');
                }
                if i % 2 == 0 {
                    out.push_str(&decode(piece));
                } else {
                    out.push_str(piece);
                }
            }
        }
    }
    std::borrow::Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::served;

    #[test]
    fn entities_become_characters_outside_code() {
        let md = "a &lt;b> and L&lt;level>, &amp;\n`&lt;c>` stays\n```\n&lt;d>\n```\nno entity\n";
        assert_eq!(
            served(md),
            "a <b> and L<level>, &\n`&lt;c>` stays\n```\n&lt;d>\n```\nno entity\n"
        );
        assert!(matches!(served("plain"), std::borrow::Cow::Borrowed(_)));
    }
}
