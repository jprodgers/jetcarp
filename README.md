# JETCARP

**Jimmie's Euclidean Tilings and Cellular Automata Research Program**

JETCARP runs cellular automata on almost any tiling of the plane: squares and hexagons, all eleven
Archimedean tilings, the 1,248 k-uniform tilings up to k = 6, 81 isohedral tile types, tiles you
draw yourself, and the aperiodic Penrose, hat and spectre tilings. Several automata can run at
once, on shared or separate cell fields. It has a graphical app, a command line, a scripting
language, and a Model Context Protocol (MCP) server, plus tools that search for spaceships,
oscillators and other patterns and keep verified catalogues of what they find.

> **Status:** pre-release. Version 0.4.0 is being prepared. Installation instructions and full
> documentation will be published with the release.

## Building from source

JETCARP is written in Rust (1.95 or newer).

```bash
cargo build --release
cargo test --workspace --release
```

## Licence

JETCARP is dual-licensed under the [Unlicense](UNLICENSE) and the [MIT licence](LICENSE-MIT); use
either. A few files derived from other people's work keep their own licences, listed in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## Citing JETCARP

If JETCARP helps your research, please cite it. GitHub's *Cite this repository* button reads
[CITATION.cff](CITATION.cff). Each release will also have a DOI on Zenodo.

## How JETCARP was made

Jimmie Rodgers designed JETCARP, chose its research directions, and tested it on Linux, Windows
and macOS. Its code was written with [Claude Code](https://claude.com/claude-code), Anthropic's AI
coding tool. Every simulation result JETCARP reports is checked by its own test suite or by exact
verification (for example, exact recurrence for spaceships), but please treat it as you would any
research software: check the results that matter to you.

## Acknowledgements

Emily Teich rated hundreds of animated clips from JETCARP's exploratory search. Her ratings
trained the score that decides which runs the search shows people, which brought her eye for
what is beautiful into the search itself.
