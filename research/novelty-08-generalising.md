# Novelty audit 08: generalising CA families from square grids to arbitrary tilings

Audit date: 2026-11-10.

## What JETCARP does

Every rule family runs on a neighbour graph with cyclic neighbour order (needed by turmites), built
from any tiling: lattices, 1,248 k-uniform tilings, 81 isohedral types, Penrose, hat and spectre. The
families are Life-like and Generations, turmites and Langton's ant (`Ant RL`, `Turmite …`, with
L2/R2 for two-slot turns), WireWorld, cyclic CA, Greenberg–Hastings, rock-paper-scissors, Hodgepodge,
forest fire, Larger than Life (graph-distance radius), sandpiles, JACA and layered rules.

## Verdict: Prior art found for the general idea and for most families on some non-square or aperiodic tiling. The breadth (every family × every tiling class in one tool, with tests) is the defensible contribution

## Prior work (verified)

**General-purpose tools that already generalise beyond squares**

1. **Ready** (GollyGang: Hutton, Munafo, Trevorrow, Rokicki, Wills), https://github.com/GollyGang/ready .
   It explores "continuous and discrete cellular automata, including reaction-diffusion systems and
   PDEs", and "supports 1D, 2D and 3D data, as well as polygonal and polyhedral meshes". It was the
   reference simulator Edmond used to check Goucher's Penrose glider table.
   *Arbitrary meshes, including Penrose patches, with user-defined rules: this is the main prior art
   for "any CA on any polygonal tiling".*
2. **Golly** RuleTable format (https://golly.sourceforge.io/Help/formats.html ): neighbourhoods
   "vonNeumann, Moore, hexagonal, oneDimensional", with "Other neighborhoods … supported through
   emulation" and triangular icons for 4-state rules "emulating a triangular neighborhood". This covers
   hexagons and emulated triangles, not general tilings.
3. **LifeViewer** (Rowett) supports square, hexagonal and triangular grids (from the LifeWiki summary;
   see novelty-02).
4. **HatLife** (Andrew Trevorrow, ConwayLife.com forum thread t=6015, May 2023; see novelty-06). Rule
   formats on the hat (edge or edge+vertex neighbours): BSN Life-like/Generations-style ("Nn" states),
   Larger-than-Life-style ranges (`RrBbmin-bmaxSsmin-smaxNn`), WireWorld, Worms, and later EdmondA/B/C.
   *So Life-like, Generations-style, WireWorld and Larger than Life had all run on the hat by 2023.* We
   did not confirm whether HatLife's range is graph distance.
5. **Edmond's monotile-ca** (2026; see novelty-06): a "pure no_std graph CA (Generations and
   priority-table rules, uniform or class-stratified)" on hat, spectre and Penrose.
6. **Murdoch Gabbay's "Aperiodic Life"** (release 3.7.x, October 2026), per Edmond's compendium. It is
   an aperiodic-tiling CA app with an evolutionary search. Not independently verified.

**Family by family**

| Family | Earliest non-square / aperiodic precedent found | Source (verified unless noted) |
| --- | --- | --- |
| Life-like | Triangles (Bays 1994), hexagons and Cairo (Bays 2005), Penrose (Hill, Stepney and Wan 2005; Owens and Stepney 2010), Archimedean (Guttesen 2026), hat (HatLife 2023) | novelty-05 |
| Generations | Hat and spectre (Edmond 2026, HatLife 2023) | repo README; forum |
| Langton's ant / turmites | Hexagonal ants with turns N, R1, R2, U, L2, L1 (Wikipedia "Langton's ant", no source cited for the hexagonal part); multi-colour ants (Gale, Propp, Sutherland and Troubetzkoy, "Further Travels with My Ant", Mathematical Intelligencer 17(3):48–56, 1995, arXiv:math/9501233); Langton's ant on Penrose tilings (github.com/ptiles/ant, MIT licence, undated) | Wikipedia; arXiv; GitHub |
| WireWorld | Hat (HatLife 2023) | forum |
| Larger than Life | Hat, LtL-style ranges (HatLife 2023) | forum |
| Cyclic CA | On networks: Cai, Y. and Ghrist, R., "Cyclic Network Automata and Cohomological Waves", arXiv:1210.5232 (2012). On Penrose/aperiodic: none found | arXiv |
| Greenberg–Hastings | Square lattice classics (Fisch, Gravner and Griffeath); none found on aperiodic tilings | search |
| Rock-paper-scissors | Search found mostly square lattices; hexagonal and aperiodic versions not confirmed | search |
| Hodgepodge | None found off the square grid | — |
| Forest fire | Not searched in depth; triangular and hexagonal Drossel–Schwabl variants probably exist (unverified) | — |
| Sandpile | Penrose (Fersula et al. 2020), hexagonal and 4.8.8 (Johnson 2019) | novelty-07 |

## How JETCARP differs

- One neighbour-graph abstraction with cyclic neighbour order, so turmites (which need "turn left by
  k") run on every tiling, including tiles of different degrees and the hat and spectre.
- Every family runs on every tiling class, with GPU/CPU equality tests, ant engines matched cell for
  cell across seven tilings, and so on.
- Families for which we found no aperiodic-tiling precedent: cyclic CA, Greenberg–Hastings,
  rock-paper-scissors, Hodgepodge and forest fire on Penrose, hat and spectre, and turmites on the hat
  and spectre. These are likely firsts in public software, but each is a direct application of a known
  rule to a known graph, so they are weak novelty claims.

## Suggested credit wording

> Running CAs on non-square and aperiodic tilings has a long history: Bays's triangular and hexagonal
> Life, Life on Penrose tilings (Hill, Stepney and Wan; Owens and Stepney; Goucher), Ready's arbitrary
> meshes, Trevorrow's HatLife (Life-like, Larger than Life and WireWorld on the hat), and Edmond's
> Generations and priority-table CAs on the hat and spectre. JETCARP brings these families together
> with Langton's ant and turmites, cyclic, Greenberg–Hastings, rock-paper-scissors, Hodgepodge and
> forest-fire rules on one neighbour-graph engine that runs every family on every supported tiling,
> including the hat and spectre.

Do not claim "first CA on aperiodic tilings" or "first to generalise X to tilings" without a check for
that family.

## Search log (2026-11-10)

- WebFetch github.com/GollyGang/ready; golly.sourceforge.io/Help/formats.html; en.wikipedia.org/wiki/Langton%27s_ant;
  github.com/ptiles/ant; arxiv.org/abs/1210.5232; cp4space.hatsya.com 2023-07-23 (not relevant).
- WebSearch: `Langton's ant Penrose tiling OR "aperiodic tiling" OR "hat tiling" turmite simulation`
- WebSearch: `"Further travels with my ant" Gale Propp Sutherland Troubetzkoy hexagonal triangular lattice generalized ants`
- WebSearch: `Greenberg-Hastings OR "cyclic cellular automaton" OR "excitable medium" Penrose tiling OR quasicrystal lattice spiral waves simulation`
- WebSearch: `Aperiodic Life app Gabbay Penrose hat spectre cellular automata rules WireWorld Generations release`
- WebSearch: `rock-paper-scissors cyclic competition cellular automaton hexagonal OR triangular lattice OR "random graph" spirals`
- Also relied on the HatLife thread (novelty-06) and the Edmond repository README.

## Gaps / unverified leads

- Ready's sample files likely include Penrose-tiling meshes with GH/cyclic-type rules. They were not
  inspected, and doing so could turn several "none found" rows into prior art.
- MCell, Golly's "Rule Table Repository", and Mathematica Demonstrations of hexagonal cyclic CA and
  hexagonal WireWorld were not searched.
- The Langton's ant hexagonal notation in Wikipedia has no primary source. Turmites on hexagonal grids
  are attested in Ed Pegg's MathWorld "Turmite" entry and Tim Hutton's Golly turmite scripts (both
  unverified here).
- Forest fire on hexagonal or triangular lattices, and Hodgepodge off the square grid, were not searched
  in depth.
