# Licence

## JETCARP's own code, data and documentation

JETCARP is dual-licensed under the [Unlicense](https://github.com/jprodgers/jetcarp/blob/main/UNLICENSE)
and the [MIT licence](https://github.com/jprodgers/jetcarp/blob/main/LICENSE-MIT): you may use its own
code and data under either, at your option. The aim is public-domain research software. The Unlicense
dedicates the work to the public domain; the MIT licence is offered as a fallback for places that do
not recognise a public-domain dedication. The same dual licence covers this documentation: its
text, its figures and the scenes that make them. Short passages it quotes from other works, with their
sources cited, remain their authors'. The short explanation is in
[`COPYING`](https://github.com/jprodgers/jetcarp/blob/main/COPYING).

Contributions are accepted under the same dual licence (see [Contributing](contributing.md)).

## Data from JETCARP's searches

Packs, the format for sharing catalogues and search results, come in a later release. Packs made by
JETCARP's own searches will be released under the Creative Commons CC0 1.0 public-domain dedication
(`CC0-1.0`). People who make packs of their own can choose another open licence for them.

## Third-party work

A few files in JETCARP contain or are derived from other people's work. Each keeps its own licence,
all of which allow redistribution under JETCARP's terms, and their notices must stay with any copy or
derivative of those files. The full notices and licence texts are in
[`THIRD_PARTY_NOTICES.md`](https://github.com/jprodgers/jetcarp/blob/main/THIRD_PARTY_NOTICES.md); in
summary:

| What | File | Source | Licence |
| --- | --- | --- | --- |
| Hat tiling substitution | `crates/jetcarp-core/src/hat.rs` | Ported from Craig S. Kaplan's [hatviz](https://github.com/isohedral/hatviz) | BSD 3-Clause, © 2023 Craig S. Kaplan |
| Spectre substitution | `crates/jetcarp-core/src/spectre.rs` | Ported, by way of JETCARP's JavaScript prototype, from Craig S. Kaplan's Spectre visualizer | BSD 3-Clause, © 2023 Craig S. Kaplan, by his permission (below) |
| Isohedral tiling tables | `crates/jetcarp-core/src/isohedral/` | Converted from Craig S. Kaplan's [tactile-js](https://github.com/isohedral/tactile-js) | BSD 3-Clause, © 2018 Craig S. Kaplan |
| k-uniform tilings | `crates/jetcarp-core/data/tilings/kuniform.json`, from `tools/data/JSON_Galebach.json` | J. E. Soto Sánchez's Galebach collection, which encodes Brian Galebach's catalogue in the integer representation of Soto Sánchez, Weyrich, Medeiros e Sá and de Figueiredo (2021) | CC BY 4.0, with changes (below) |
| Evolutionary search: mutation step and rule notation | `crates/jetcarp-core/src/evolve.rs` (`mutate`), `crates/jetcarp-core/src/evolve/priority.rs` (`PriorityRule::notation`) | Adapted from James Edmond's [monotile-ca](https://github.com/jamesedmond/monotile-ca) | MIT, © 2026 James Edmond (monotile-ca is MIT OR Apache-2.0; JETCARP takes it under MIT) |
| The Lenia pattern *Orbium unicaudatus* | `crates/jetcarp-core/src/lenia.rs` (the `ORBIUM` constant only) | `Python/animals.json` in Bert Chan's [Lenia](https://github.com/Chakazul/Lenia) | MIT, © 2018 Bert Chan |

**The Spectre visualizer.** Kaplan's `spectre.js` was published without a licence. Asked whether
JETCARP could use it, he made it available to JETCARP under BSD 3-Clause, the same terms as hatviz,
by email to Jimmie Rodgers on 10 November 2026.

**The k-uniform data.** JETCARP converted the data: `tools/gen_kuniform.py` rebuilds each tiling's
exact geometry, translation lattice, symmetry group and vertex orbits from the source and writes them
in JETCARP's own format. The source file is kept unchanged in the repository. Work that uses these
tilings should credit the catalogue and its encoding (see [Citing JETCARP](citing.md#the-work-jetcarp-builds-on)).

### Methods written without copying code

These carry no licence obligation, but the work they come from is credited here and on the pages that
describe them:

- **The evolutionary glider search**, James Edmond's method, written from his paper (arXiv:2609.22579)
  and a reading of his repository, apart from the two functions listed above (see
  [Search methods](../research/search-methods.md#evolutionary-glider-search)).
- **The Spectre glider rule** (`SpectreGlider`), Edmond's published rule (Table 7 of his paper).
- **Von Neumann's 29-state automaton**, written from von Neumann's *Theory of Self-Reproducing
  Automata*, edited by A. W. Burks (1966), with no text, figures, rule files or patterns from other
  programs.
- **The WebP lossless encoder** (`crates/jetcarp-core/src/media/vp8l.rs`), written from the WebP
  Lossless Bitstream Specification (RFC 9649), with no libwebp code.

JETCARP contains no code, rule files or pattern files from Golly. The descriptions of named rules are
written in JETCARP's own words; the names, notations and credits come from LifeWiki and the other
sources listed in the [named rule catalogue](../automata/named-rules.md).

## Compiled releases

Compiled releases are built from Rust libraries under permissive licences (MIT, Apache-2.0, BSD,
Zlib, ISC, Unicode-3.0 and others); two are under MPL-2.0 and are used unmodified. Their notices ship
with each release in `THIRD_PARTY_LICENSES.html`, generated by
[cargo-about](https://github.com/EmbarkStudios/cargo-about). Releases also bundle the SQLite library,
which is in the public domain, and the user-interface fonts that come with egui: Ubuntu Light (Ubuntu
Font Licence 1.0), Hack (MIT, with the Bitstream Vera licence), Noto Emoji (SIL Open Font Licence 1.1)
and emoji-icon-font (MIT).
