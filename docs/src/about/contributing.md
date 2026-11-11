# Contributing

Contributions are welcome: bug reports, corrections to the history and credits, rules and tilings,
code, and documentation. If you know of earlier work that this book has missed, please open an issue;
we will correct the page and credit it.

## Building and testing

JETCARP is a Rust workspace and needs Rust 1.95 or newer.

```bash
cargo build --release
cargo test --workspace --release
cargo run --release -p jetcarp            # the app
cargo run --release -p jetcarp -- --help  # the command line
```

On Linux the app needs the usual windowing libraries; on Debian and Ubuntu, continuous integration
installs `pkg-config libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev
libvulkan1`, plus `mesa-vulkan-drivers` so that the GPU tests can run on a software renderer. GPU tests
are skipped, with a note, on a machine with no usable graphics adapter. A few slow surveys are marked
`#[ignore]` and run only when asked (`cargo test … -- --ignored`).

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

## The determinism pins

Two tests pin JETCARP's behaviour: `crates/jetcarp-core/tests/geometry_pins.rs` (the cell polygons and
neighbour lists of every tiling) and `crates/jetcarp-core/tests/dynamics_pins.rs` (the state of every
built-in Quick Start entry after 48 generations). See
[Reproducibility](../research/reproducibility.md#pinned-geometry-and-dynamics).

If your change makes a pin fail, either the change is a mistake, or it deliberately changes what a
tiling or rule does. Only in the second case, rewrite the pins with

```bash
JETCARP_PIN_WRITE=1 cargo test --release -p jetcarp-core --test geometry_pins --test dynamics_pins
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

```bash
mdbook build docs                  # or: mdbook serve docs
python3 docs/tools/check-links.py  # every internal link and #anchor must resolve
```

Every picture is made by JETCARP from a scene in `docs/scenes/`, and its caption names the scene.
`docs/tools/render-figures.sh` renders the stills and `docs/tools/render-animations.sh` the animations
(it needs Python with NumPy and Pillow; set `PYTHON=` if your default Python lacks them). Both use
`target/release/jetcarp` unless `JETCARP=` names another binary, and both take a filter to render only
some figures. Some pages are generated, and say so at the top: edit their sources and run
`docs/tools/gen-bibliography.py`, `gen-named-rules.py`, `gen-tiling-catalogue.py` or
`gen-scripting.py` instead of editing the page. `gen-scripting.py` also fails if the Rhai help and
the functions the engine registers disagree.

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
- Errors are messages a user can act on ("Margolus block rules need the square grid. Remove that CA
  before switching tilings."), not codes.
- Tests live beside the code (`#[cfg(test)]`) and in each crate's `tests/` folder. A new rule, tiling
  or search should come with tests, including one showing that its result does not depend on the
  thread count.
- The code follows rustfmt's default style: run `cargo fmt --all` before committing. CI checks it with
  `cargo fmt --all -- --check`.
- Commit messages start with a short summary, often prefixed by the area (`cli:`, `jobs:`, `Docs:`),
  followed by a paragraph on what changed and why.

## Licence of contributions

JETCARP is dual-licensed under the Unlicense and the MIT licence (see [Licence](licence.md)). By
contributing, you agree that your contribution is offered under the same terms: dedicated to the
public domain under the Unlicense, or, where that is not recognised, under the MIT licence.
Third-party material keeps its own licence and needs its notice, as above.
