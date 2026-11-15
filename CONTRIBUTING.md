# Contributing to JETCARP

Thank you for helping. JETCARP welcomes bug reports, corrections to its history and credits, new
rules and tilings, code and documentation. It is research software maintained by one person, so
please be patient with replies.

This page is the short version. The full guide, with how to build and test, the determinism pins,
how to propose a rule or a tiling, the documentation and the code style, is the book's
[Contributing](https://jprodgers.github.io/jetcarp/about/contributing.html) page
([source](docs/src/about/contributing.md)).

Everyone taking part agrees to the [Code of Conduct](CODE_OF_CONDUCT.md).

## Reporting a bug

1. Search the [open issues](https://github.com/jprodgers/jetcarp/issues) first, in case it is known.
2. Open a **Bug report**. It asks for:
   - what you did, what you expected and what happened;
   - `jetcarp --version` and how you installed JETCARP;
   - your system, and the output of `jetcarp doctor` (it names your graphics adapters and JETCARP's
     folders; it holds no personal data beyond your folder paths, which you may shorten);
   - if you can, the scene (*File → Save scene…* in the app, or `jetcarp run … --save FILE`) or the commands that show it.

A confusing moment counts too: if the app or the command line left you unsure what to do, that is a
bug worth reporting.

**Security problems** go by email, not in a public issue: see [SECURITY.md](SECURITY.md).

## Suggesting a feature, a rule or a tiling, or a correction

- **Feature request:** say what you are trying to do, not only the feature you want.
- **Rule or tiling:** say what it is, who devised it and where it was first published (with a link
  to the primary source), its licence if JETCARP would include any code or data from elsewhere, and
  how it would run on other tilings.
- **History or credits correction:** if the book misses earlier work, or credits something wrongly,
  say which page and give your sources. JETCARP corrects the page and credits the work.

## Pull requests

- **Small fixes** (a typo, a broken link, an obvious one-line bug): open a pull request directly.
- **Anything larger:** open an issue first, so we can agree on the approach before you spend time on it.
- Keep a pull request to one change, and say why as well as what.
- Before you open it, please run:

  ```bash
  cargo fmt --all
  cargo test --workspace --release
  python3 docs/tools/check-links.py          # if you changed the book
  python3 docs/tools/gen-cli.py --check      # if you changed the command line's help
  ```

  Point JETCARP's folders at scratch folders while testing (`JETCARP_CONFIG_DIR`, `JETCARP_DATA_DIR`,
  `JETCARP_OUT`, `JETCARP_LIBRARY`), so tests never touch your own settings or library.
- New behaviour comes with tests; a new rule, tiling or search with one showing that its result does
  not depend on the thread count.
- If your change makes a determinism pin fail, read
  [The determinism pins](https://jprodgers.github.io/jetcarp/about/contributing.html#the-determinism-pins)
  before re-pinning: never re-pin just to make a test pass.
- Add a line to the [changelog](docs/src/about/changelog.md), under the unreleased version, for
  anything a user would notice.
- **AI-assisted contributions are welcome**, as JETCARP itself is written with an AI coding tool.
  Say so in the pull request, and make sure you understand, have run and can explain every change.

## Licence of contributions

JETCARP is dual-licensed under the [Unlicense](UNLICENSE) and the [MIT licence](LICENSE-MIT). By
contributing, you agree that your contribution is offered under the same terms: dedicated to the
public domain under the Unlicense, or, where that is not recognised, under the MIT licence.
Material from elsewhere keeps its own licence and needs its notice in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md); copyleft code and text can't be included.
