//! The command line Windows gives a `.scr` file.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScrMode {
    /// `/s`: full screen until there is input.
    Run,
    /// `/c`, or nothing at all: the settings.
    Configure,
    /// `/p <window>`: draw inside the Screen Saver dialog's little preview.
    Preview(isize),
}

/// Reads the arguments, in the forms Windows uses: `/s`, `-S`, `/p 1234`, `/p:1234`, `/c:1234`.
pub fn parse(args: &[String]) -> ScrMode {
    let Some(first) = args.first() else {
        return ScrMode::Configure;
    };
    let flag = first.trim_start_matches(['/', '-']).to_ascii_lowercase();
    let (name, attached) = match flag.split_once(':') {
        Some((n, v)) => (n, Some(v)),
        None => (flag.as_str(), None),
    };
    match name {
        "s" => ScrMode::Run,
        "p" | "l" => attached
            .or(args.get(1).map(String::as_str))
            .and_then(|v| v.trim().parse().ok())
            .map_or(ScrMode::Configure, ScrMode::Preview),
        _ => ScrMode::Configure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &[&str]) -> ScrMode {
        parse(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn every_form_windows_uses() {
        assert_eq!(p(&[]), ScrMode::Configure);
        assert_eq!(p(&["/s"]), ScrMode::Run);
        assert_eq!(p(&["/S"]), ScrMode::Run);
        assert_eq!(p(&["-s"]), ScrMode::Run);
        assert_eq!(p(&["/c"]), ScrMode::Configure);
        assert_eq!(p(&["/c:1234"]), ScrMode::Configure);
        assert_eq!(p(&["/p", "1234"]), ScrMode::Preview(1234));
        assert_eq!(p(&["/P:987"]), ScrMode::Preview(987));
        assert_eq!(p(&["/p"]), ScrMode::Configure);
        assert_eq!(p(&["/p", "nonsense"]), ScrMode::Configure);
    }
}
