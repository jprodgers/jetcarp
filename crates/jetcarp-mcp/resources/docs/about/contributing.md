# Contributing

Contributions are welcome: bug reports, corrections to the history and credits, rules and tilings,
code, and documentation. If you know of earlier work that this book has missed, please open an issue;
we will correct the page and credit it.

On GitHub, [new issues](https://github.com/jprodgers/jetcarp/issues/new/choose) offer forms for a bug
report, a feature request, a new rule or tiling, and a history or credit correction, and a pull request
starts from a short checklist. [CONTRIBUTING.md](https://github.com/jprodgers/jetcarp/blob/main/CONTRIBUTING.md)
sums this page up, and everyone taking part agrees to the
[Code of Conduct](https://github.com/jprodgers/jetcarp/blob/main/CODE_OF_CONDUCT.md). Security problems
are reported privately, with the Security tab's *Report a vulnerability* or by email, as
[SECURITY.md](https://github.com/jprodgers/jetcarp/blob/main/SECURITY.md) says.

**Pull requests:** small fixes can come straight as a pull request; for anything larger, open an issue
first so we can agree on the approach. **AI-assisted contributions are welcome**, as JETCARP itself is
written with an AI coding tool (see [How JETCARP was made](how-made.md)): say so in the pull request,
and make sure you understand, have run and can explain every change. Every contribution, whoever or
whatever wrote it, is reviewed in depth with AI tools and then tested heavily before it is merged: we
do not trust, but we verify. [How JETCARP is tested](testing.md) says how.

## Building and testing

JETCARP is a Rust workspace and needs Rust 1.95 or newer.

```sh
cargo build --release
cargo test --workspace --release
```

Building the whole test suite in the release profile takes a lot of disk: the profile keeps
debug information, and the tests' build came to more than 28 GB in the 0.4.2 audit. Without it,
the same tests build into about 5 GB, and the suite runs in well under an hour on four cores:

```sh
CARGO_PROFILE_RELEASE_DEBUG=0 cargo test --workspace --release
```

The app:

```sh
cargo run --release -p jetcarp
```

The command line:

```sh
cargo run --release -p jetcarp -- --help
```

On Linux the app needs the usual windowing libraries; on Debian and Ubuntu, continuous integration
installs `pkg-config libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev
libvulkan1`, plus `mesa-vulkan-drivers` so that the GPU tests can run on a software renderer (which
JETCARP uses only when asked for, with `JETCARP_GPU=software`). GPU tests are skipped, with a note, on a
machine with no usable graphics adapter, or with only one the GPU self-test turns down. With
`JETCARP_REQUIRE_GPU=1` they fail instead, naming the adapter and why, as on CI's runners: set it when you mean the GPU tests to run, so that a missing driver cannot pass for a pass.
On Windows, `JETCARP_ALLOW_WARP=1` lets them choose Windows' software renderer, WARP, which JETCARP
otherwise never uses (see [The GPU](../guides/gpu.md#which-adapter)): with `JETCARP_GPU=software`,
the GPU tests then run on it. The WARP that comes with Windows crashes compiling JETCARP's kernels;
a newer one from Microsoft's
[`Microsoft.Direct3D.WARP`](https://www.nuget.org/packages/Microsoft.Direct3D.WARP) package, its
`d3d10warp.dll` put beside the test program, runs them, as Windows CI does
([Testing](testing.md#continuous-integration)), all but the soup hunt's, which never runs on WARP.
A few slow surveys are marked `#[ignore]` and run only when asked (`cargo test … -- --ignored`).

On Windows the tests of the folder rules make directory junctions and symbolic links. Symbolic links
need Developer Mode (*Settings → System → For developers*) or an administrator's terminal; without
either, those cases are skipped, and say so (`cargo test … -- --nocapture` shows `skipped:`).
CI sets `JETCARP_TEST_LINKS=all`, which makes such a skip a failure. The installers' own tests are
`packaging/test-install-sh.sh` and `packaging/test-install-ps1.ps1`.

Tests should never touch your own settings, data or pictures. To be sure, point JETCARP's folders at
scratch folders while testing, as CI does:

```bash
export JETCARP_CONFIG_DIR=/scratch/jetcarp/config
export JETCARP_DATA_DIR=/scratch/jetcarp/data
export JETCARP_OUT=/scratch/jetcarp/out
export JETCARP_LIBRARY=/scratch/jetcarp/library
```

CI builds and tests the whole workspace on Linux (x86_64), Windows (x86_64) and macOS (Apple
silicon), and builds it for macOS on Intel and Linux on ARM.

The benchmarks run with one command, which builds them in release, runs each with the arguments
the weekly workflow uses, and writes their numbers to `bench-out/` (`bench.json`, `bench.csv` and a
table, `summary.md`); `compare` shows what changed between two such records, for a change meant to
be no slower:

```sh
python3 tools/bench.py run
python3 tools/bench.py compare before/bench.json bench-out/bench.json
```

JETCARP makes no network connections today, and networking comes only on purpose. CI checks the
dependency tree with [cargo-deny](https://github.com/EmbarkStudios/cargo-deny): it fails on a
network client or server crate that `deny.toml` does not allow, on a known vulnerability, and on a
crate from anywhere but crates.io. A change that adds networking allows its crate in `deny.toml`,
for the one JETCARP crate that uses it (the file's comments show how), and updates `SECURITY.md`.
CI checks each target JETCARP is released for on its own, as that target builds the tree. To run
the same check yourself (`cargo install --locked cargo-deny`):

```sh
for t in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-pc-windows-msvc \
         aarch64-apple-darwin x86_64-apple-darwin; do
  cargo deny --target "$t" check --warn unmaintained advisories bans sources
done
```

## The determinism pins

Three tests pin JETCARP's behaviour: `crates/jetcarp-core/tests/geometry_pins.rs` (the cell polygons
and neighbour lists of every tiling), `crates/jetcarp-core/tests/dynamics_pins.rs` (the state of every
built-in Quick Start entry after 48 generations) and `crates/jetcarp-core/tests/semantics_pins.rs`
(one fixed scene per rule family). See
[Reproducibility](../research/reproducibility.md#pinned-geometry-and-dynamics).

If your change makes a pin fail, either the change is a mistake, or it deliberately changes what a
tiling or rule does. Only in the second case, rewrite the pins. If a rule family's meaning changed,
first raise that family's version in `RULE_SEMANTICS` (`crates/jetcarp-core/src/semantics.rs`; see
[Rule-semantics versions](../reference/library-format.md#rule-semantics-versions)): the semantics
pins refuse to be rewritten for a family whose state changed while its version did not. Then

```bash
JETCARP_PIN_WRITE=1 cargo test --release -p jetcarp-core --test geometry_pins --test dynamics_pins --test semantics_pins
```

and say in the commit message which pins changed and why, so the change can be listed in the
[changelog](changelog.md): it changes what old scenes replay to. Never re-pin to make a test pass. A
rule family that turns out to give different bits on different platforms goes into the dynamics
test's `NOT_PORTABLE` list with the reason, instead of being re-pinned.

More generally, results must not depend on the thread count, the batch size or the platform. Anything
that feeds a cell's state uses JETCARP's own deterministic maths (`crates/jetcarp-core/src/detmath.rs`)
rather than the platform's `sin`, `exp` or `hypot`, sums in a fixed order, and draws random numbers
only from the scene's or the search's seeded generator.

## Proposing a rule or a tiling

Open an issue, or a pull request, with:

- **What it is**, and a scene that shows it.
- **Where it came from**: who devised it, and where and when it was first published or posted, with a
  link to the primary source. Secondary sources are welcome as well, marked as such.
- **Its licence**, if you want JETCARP to include any code, data, rule file or text from elsewhere.
  JETCARP can only include material whose licence allows redistribution under its own terms, with the
  notice added to `THIRD_PARTY_NOTICES.md`. Copyleft code (such as GPL code) and copyleft text (such
  as LifeWiki's) cannot be included; rules written down in a publication can be implemented from the
  publication. JETCARP uses no code, rule files or patterns from Golly.
- **How it runs on other tilings**: what it counts, measures or turns by, in terms of the neighbour
  graph (see [How JETCARP generalises automata](../automata/generalising.md)), or why it is tied to one
  grid.

New encyclopedia pages follow the template the others use: *what it is* (with a picture), *where it
came from* (people, dates, original publications), *how JETCARP runs it*, *what is new here* (only
with evidence from a prior-art search; otherwise "to our knowledge…" or nothing), *try it*, and
*references*.

## The documentation

The book is an [mdBook](https://rust-lang.github.io/mdBook/) in `docs/`:

```sh
mdbook build docs
```

(`mdbook serve docs` serves it as you edit.) Every internal link and `#anchor` must resolve:

```sh
python3 docs/tools/check-links.py
```

Every picture of a world is made by JETCARP from a scene in `docs/scenes/`, and its caption names
the scene; the pictures of the app are drawn offscreen by ignored tests, in
`crates/jetcarp-app/src/app/shots.rs` (the setup window's in `setup_ui.rs`).
`docs/tools/render-figures.sh` renders the stills and `docs/tools/render-animations.sh` the animations
(it needs Python with NumPy and Pillow; set `PYTHON=` if your default Python lacks them). Both use
`target/release/jetcarp` unless `JETCARP=` names another binary, and both take a filter to render only
some figures. Some pages are generated, and say so at the top: edit their sources and run
`docs/tools/gen-bibliography.py`, `gen-named-rules.py`, `gen-tiling-catalogue.py`, `gen-palettes.py`,
`gen-scripting.py`, `gen-cli.py`, `gen-mcp.py` or `gen-shortcuts.py` instead of editing the page (all but the bibliography's
and the scripting reference's read a freshly built `target/release/jetcarp`). Each takes `--check`,
which writes nothing and fails if the committed page is not what it would write; CI runs every one,
and `docs/tools/check-rule-notation.py`, which parses the rule notation page's table of examples.
`gen-scripting.py` also fails if the Rhai help and the functions the engine registers disagree. The MCP server's crate keeps a copy of the book for its
crates.io package, which `docs/tools/copy-mcp-resources.py` brings up to date; the release
workflow checks it.

The writing style is plain and direct, in British spelling ("colour", "neighbour"), and the book never
claims more than its evidence: novelty only as set out in
[What is new in JETCARP](../research/contributions.md), and credit to everyone whose work is used.

## Code style

- Each source file begins with a module comment (`//!`) that says what the file does and how, often
  with the design decisions behind it; public items have doc comments. Comments are in plain English,
  and say why as well as what.
- Every change to a world goes through a `Command`, so that it is logged and replays. New features
  for the front ends belong in the shared operations of `jetcarp-api`, so that the command line,
  scripts, the MCP server and the app all get them.
- Each layer depends only on the ones below it: the engine (`jetcarp-core`: tilings, rules,
  worlds, rendering, and what a pattern is), the store (`jetcarp-store`: the pattern library, Quick
  Start and media), the searches (`jetcarp-search`: the job runner and every search), the
  operations (`jetcarp-api`), then the front ends. The traits a GPU backend implements stay in the
  engine, so `jetcarp-gpu` needs only `jetcarp-core`.
- The crates that take in other people's data have no unsafe code, and say so with
  `#![forbid(unsafe_code)]`: `jetcarp-mcp`, `jetcarp-pack`, `jetcarp-script`, `jetcarp-store` and
  `jetcarp-gpu`. Elsewhere unsafe code is for calls into the operating system (the desktop
  background, the screen saver, power and login items) and a few hot loops; a new use needs a
  `// SAFETY:` comment saying why it is sound.
- Errors are messages a user can act on ("Margolus block rules need the square grid. Remove that CA
  before switching tilings."), not codes.
- Tests live beside the code (`#[cfg(test)]`) and in each crate's `tests/` folder. A new rule, tiling
  or search should come with tests, including one showing that its result does not depend on the
  thread count.
- The code follows rustfmt's default style: run `cargo fmt --all` before committing. CI checks it with
  `cargo fmt --all -- --check`.
- The code passes clippy with no warnings: CI fails on any clippy or compiler warning, on Linux
  and for Windows and macOS too.

  ```sh
  cargo clippy --workspace --all-targets --locked -- -D warnings
  ```

  The policy is in the root `Cargo.toml`, under `[workspace.lints.clippy]`, which every crate takes
  up (`[lints] workspace = true`): four lints are allowed throughout, each with its reason. Anywhere
  else, fix the warning, or, where clippy is wrong about one place, allow its lint there with
  `#[allow(clippy::…)]` and a comment saying why. A lint allowed in many places belongs in the
  policy, with its reason, rather than in each. CI runs clippy with the Rust releases are built
  with, named in `.github/rust-release`; a newer clippy may warn about more, so to see what CI sees,
  run that version (`rustup toolchain install 1.100.0`, then `cargo +1.100.0 clippy …`).
- Commit messages start with a short summary, often prefixed by the area (`cli:`, `jobs:`, `Docs:`),
  followed by a paragraph on what changed and why.

## Licence of contributions

JETCARP is dual-licensed under the Unlicense and the MIT licence (see [Licence](licence.md)). By
contributing, you agree that your contribution is offered under the same terms: dedicated to the
public domain under the Unlicense, or, where that is not recognised, under the MIT licence.
Third-party material keeps its own licence and needs its notice, as above.
