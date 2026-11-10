# Search methods

JETCARP has nine ways of searching: eight run as *jobs* through `jetcarp search <strategy>`, and the
exploratory search has its own command, `jetcarp explore`. This page describes each one: what it
does, how it classifies or scores what it sees, how its results are checked, and how to start it. For
a hands-on introduction see [Searching for patterns](../guides/searching.md); for what the searches
have found see [Findings](findings.md) and [What is new in JETCARP](contributions.md).

| Strategy | Looks for | Command |
| --- | --- | --- |
| [Soup hunt](#soup-hunt) | Still lifes, oscillators and spaceships in small random soups on an unbounded plane | `jetcarp search hunt` |
| [Bounded soup search](#bounded-soup-search) | The same, on a finite grid of any tiling, aperiodic patches included | `jetcarp search bounded` |
| [Rule census](#rule-census) | How whole rules behave: died, frozen, oscillating, active or exploded | `jetcarp search census` |
| [Ants and turmites](#ants-and-turmites) | Highways, orbits and symmetric growth | `jetcarp search ants` |
| [Movers on a sliding window](#movers-on-a-sliding-window) | How far a glider on an aperiodic tiling really travels | `jetcarp search mover` |
| [Large sandpiles](#large-sandpiles) | The final state of N grains dropped on one spot | `jetcarp search pile` |
| [Collisions](#collisions) | What two patterns make when they meet | `jetcarp search collide` |
| [Evolutionary glider search](#evolutionary-glider-search) | Gliders, by evolving rules (James Edmond's method) | `jetcarp search evolve` |
| [Explore](#explore) | Interesting runs of any family on any tiling, by quality-diversity search | `jetcarp explore run` |

## Jobs

Every `jetcarp search` strategy runs as a job: a row in the [pattern catalogue](../guides/pattern-library.md)
holding the job's specification, its status, its progress and a checkpoint. A job runs in steps of a
few seconds, and after each step it writes the step's finds, its progress and its checkpoint in one
transaction. It can therefore be stopped and carried on at any time, in the same program or another,
and it ends exactly as it would have without the interruption (see
[Reproducibility](reproducibility.md#interrupted-searches-end-where-uninterrupted-ones-would)).
Results never depend on the number of threads.

The options shared by every strategy: `--minutes N` and `--limit N` stop the job, still resumable,
after a time or a number of items; `--resume JOB` carries one on; `--label` names it; `--detach` runs
it in a process of its own; `--threads` sets the worker threads; `--library` picks the catalogue. The
jobs themselves are managed with `jetcarp jobs list`, `status`, `stop`, `resume`, `report` (a Markdown
summary of what a job searched and found) and `remove`. Ctrl-C stops a job at its next checkpoint.

### How finds are verified

A search's patterns go into the catalogue only after an exact check. The pattern is pasted alone on an
empty grid with dead edges, large enough to hold its whole path for the periods checked plus a margin,
and run: every generation its live cells must equal the stored phase exactly (position, tile and
state), shifted by the displacement for a spaceship. By default 16 periods are checked for short
periods and fewer for long ones, at least 4 and, where the period allows, at least 64 generations. A
live cell that reaches the frame's edge fails the check, so a pass means the pattern behaves the same
on the infinite plane (searched rules never have birth on 0, so empty space stays empty). Patterns on
aperiodic tilings run on the patch they were found on, at their own site, and must stay away from the
patch's open boundary.

Each pattern also gets a rule-free identity: a hash of the tiling, the neighbourhood, the class,
period and displacement, and its whole evolution over one period in a canonical orientation (the
smallest over every phase and every symmetry of the tiling). The same object found in another phase,
orientation or rule is therefore recognised as one pattern. For Life-like and Generations rules the
catalogue also stores the pattern's *rule range*: every transition the pattern asks of the rule over
one period fixes some birth and survival counts and forbids others, and every rule between the
smallest and the largest consistent rule runs the pattern through exactly the same phases. That is how
the findings can say that a ship "runs in the 1,024 rules from `B3/S235` up".

`jetcarp library verify` checks stored patterns again with the running build, for instance after a
change of geometry.

## Soup hunt

**What it does.** Runs small random soups, by default 16 by 16 unit cells at density 0.5, on an
unbounded plane, for any list of Life-like or Generations rules without birth on 0, on any periodic
tiling or set of tilings (all Archimedean, k-uniform with a given k, isohedral, or every periodic
tiling). The engine is bit-parallel: many cells are updated in one machine word. Spaceships that fly
clear of the soup are counted and cut out, so they cannot crash back into it.

**How it classifies.** A soup ends when it dies, when it explodes (too many live cells, or spread too
wide), when it settles (its state repeats with some period up to 64), or at 20,000 generations. What
is left is split into groups of cells within two neighbour steps of each other, and every group never
seen before is run alone and classed: still life, oscillator, spaceship, puffer, gun, a set of parts
that do not interact, or other. Big still lifes and oscillators (by default more than 64 cells, or
period times cells above 4,000) are kept only as counts until a hunt has seen one twice, to keep rare
debris from filling the catalogue.

**How results are checked.** Every new periodic object is [verified](#how-finds-are-verified) and
stored with its rule range; objects already in the catalogue only gain a sighting. A GPU option
(`--gpu`) runs the start of each soup on the graphics card, and the tests require the catalogue it
leaves to equal a CPU-only hunt's.

**Command.** `jetcarp search hunt --rules "B3/S23 B36/S23" --tilings archimedean --soups 100000`.

## Bounded soup search

**What it does.** Runs seeded random soups on a finite grid of any tiling, including patches of the
Penrose, hat and spectre tilings, with wrapped or dead edges. By default each rule and tiling gets 16
soups of a 64 by 64 grid at density 0.37 for up to 2,000 generations. With `--soup-radius` only a disc
at the centre is filled, and with dead edges the spaceships that escape are caught at the edge.

**How it classifies.** At the end of each soup JETCARP's object finder splits the field into still
lifes, oscillators and spaceships (on aperiodic patches, the same patterns at their sites).

**How results are checked.** Every distinct object is verified as above before it is stored; the full
report can also be written as JSON (`--json-out`).

**Command.** `jetcarp search bounded --rules "B2/S34" --tilings hat --size L5 --boundary dead`.

## Rule census

**What it does.** Runs many rules on one tiling from the same seeded soups (soup `s` uses the random
state `seed + s`, so every rule sees the same soups) and summarises each rule. The rules can be listed,
read from a file, or taken from a whole space of B/S rules (`--all`) or a seeded sample of it
(`--sample`), optionally as Generations rules with a given state count.

**How it classifies.** Each run is *died*, *frozen* (period 1), *oscillating* (with its exact
period), *active* at the generation limit (with the fraction of cells still changing), or
*exploded* (population above half the cells, by default). Periods come from Brent's cycle-finding
method on exact states, and the generation at which a run settled is found exactly too. Life-like and
Generations rules on periodic tilings run on a bit-parallel board that reproduces the general engine
generation for generation; the tests compare the two.

**How results are checked.** The census is a measurement, not a catalogue of patterns: its result
is the report, kept with the job in the catalogue and written as text, CSV, JSON or a contact sheet of
every rule's first soup. A JSON report
holds the whole specification, so `--rerun` repeats it exactly.

**Command.** `jetcarp search census --family hex --all --birth 1-6 --survive 0-6 --csv hex.csv`.

## Ants and turmites

**What it does.** Runs ants and turmites on an unbounded periodic tiling: every turn string of the
given lengths (`--strings 2-6`), every turmite table of a given size (`--tables 2x2`) or a seeded
sample when there are too many, or rules you list. Each tiling has its own turn alphabet (`L R N U` on
squares, `L R N U L2 R2` on hexagons, `L R N` on triangles). Strings and tables that must behave like
another one already in the list are skipped: a string that repeats a shorter one, a mirror image on a
tiling where mirror images behave as such, and tables that differ only by relabelling states or
colours.

**How it classifies.** Each run, up to a million steps by default, is classed as a *highway* (the ant
builds a structure that repeats while it moves off), a *periodic orbit*, *symmetric* growth (on
squares and hexagons, fields that keep returning to mirror symmetry, as `LLRR` does), *bounded*, or
*chaotic* at the step limit.

**How results are checked.** Highways are proved, not guessed. When the ant's recent history looks
periodic with period `p`, JETCARP runs `p` more steps and checks that the ant comes back to the same
state, tile type and heading moved by a lattice vector, and that every cell it wrote matches the cell
one period further along that vector. Only the ant changes cells, so by induction the ant repeats
forever. The onset, the first step of the repeating tail, is found exactly; it reproduces the RL ant's
published highway (period 104 from step 9,977). Runs are stored in the catalogue (`jetcarp library
ants`).

**Command.** `jetcarp search ants --tilings archimedean --strings 2-5`.

## Movers on a sliding window

**What it does.** Follows a pattern that moves across an aperiodic tiling (spectre, hat, or either
Penrose tiling) for as far as you ask. A glider on an aperiodic tiling never repeats exactly, so it
cannot be proved immortal by finding a period; instead JETCARP measures how far it has been seen to
travel unchanged. Only the tiles near the live cells exist: the window is cut from the tiling's
supertile hierarchy around them and rebuilt ahead of the pattern as it moves, so memory depends on the
window, not on the distance.

**How it classifies.** The run ends *verified to a distance*, measured in rings of tiles (edge steps
from the starting tiles to the nearest live tile), in neighbour steps and in plane distance, or stops
early because the pattern died, grew past a cell limit, or repeated in place (an oscillator, not a
mover).

**How results are checked.** Every tile is placed by an exact rigid motion in integer cyclotomic
coordinates, so tiles from different windows match by equality and neighbours are found exactly;
floating point only decides which tiles to build. Live cells are always kept at least three neighbour
steps from the window's open edge, so every cell that can change sees its true neighbours and the run
equals the run on the whole infinite tiling. `jetcarp library movers track` does the same over a
series of fixed patches of growing level, checking each level against the one before. With
`--store`, a verified mover goes into the catalogue with the distance it was verified to. This is how
the hat glider was followed for a million rings (see [Findings](findings.md#the-hat-glider-is-edmonds-glider-b)).

**Command.** `jetcarp search mover --rule SpectreGlider --tiling spectre --rings 1000`.

## Large sandpiles

**What it does.** Drops N grains (up to 2<sup>64</sup> − 1) on one cell, vertex or edge of an unbounded
periodic grid, or of an aperiodic patch up to level 8, and computes the stable pile. It is built from
N's binary digits, most significant first, by doubling the pile and adding the next digit and
stabilising each time; by the abelian property this gives exactly the pile of N grains dropped at
once. A periodic grid grows as the pile does, so no grain is lost; on an aperiodic patch a grain that
reaches the open edge stops the run with an error.

**How it classifies.** It does not: the result is the pile, saved after every doubling step (which
also makes the job resumable), with a picture, a preview and metadata in the output folder's `piles/`.

**How results are checked.** The final pile and the number of topplings do not depend on the order of
topplings, and the GPU toppler must give the same pile, toppling count and sweep count as the CPU at
every step. See [The Abelian sandpile](../automata/sandpile.md#very-large-piles).

**Command.** `jetcarp search pile --tiling hat --grains 1e6`.

## Collisions

**What it does.** Places pattern B at every offset in a range (and in every phase, if asked) next to
pattern A, both from the catalogue, on a grid with dead edges, and runs each placement until it
settles. Placements where B starts within two neighbour steps of A are skipped: that is a different
starting pattern, not a collision.

**How it classifies.** A run has settled when everything left is a still life, oscillator or
spaceship and no spaceship is heading for anything. The outcome is *both survive*, *both die*, *A
survives*, *B survives*, or *new objects*; runs can also explode or fail to settle.

**How results are checked.** The products are verified and catalogued like any other find (unless
`--no-store`), and every run's final state can be drawn on a contact sheet.

**Command.** `jetcarp search collide A B --offsets -6..6`, with A and B pattern ids from
`jetcarp library list`.

## Evolutionary glider search

**What it does.** Evolves rules in which gliders exist. This is **James Edmond's method**, from
"Gliders on Aperiodic Monotilings: Cellular Automata on the Hat and Spectre"
([Edmond 2026](bibliography.md#edmond2026gliders)), reimplemented in JETCARP from his paper and a
reading of his repository, with two small functions (the mutation step and the table notation)
adapted from his code under its MIT licence. JETCARP's part is running the method on every tiling it
has, periodic and aperiodic, not only the hat and spectre.

**How it scores.** Each genome is a [priority table](../automata/priority-rules.md) of up to eight rows.
It is run from a small bank of seeds (13 for four states) on a ball of tiles out to 48 rings from a
root tile, and each
run scores the farthest ring it changed, scaled down for runs whose population swells: distance times
the peak population while still near the centre, divided by the overall peak. A genome's fitness is
its best run. Edmond's search needed filters against rules that game this score, and JETCARP keeps
them: a population cap (explosive rules score 0), the raw distance rather than a bonus for escaping,
the flatness ratio, a light-cone test that rejects runs which spread faster than the rule allows (a
sign of exploiting the patch's edge), and classification inside the patch's reliable interior. The
genetic algorithm uses a population of 256 for 800 generations, keeps the best sixteenth, picks
parents by tournaments of three, and splices and mutates their rows. A run is a pure function of its
seed.

**How results are checked.** The best distinct genomes are verified outside the search on balls of 1,
2 and 4 times the radius, where rules that only grow or loop give themselves away. Gliders that pass go to the catalogue: on a
lattice the object finder confirms an exact spaceship, and on an aperiodic tiling the mover tracker
records the distance it was verified to. The hat glider this search found turned out to be Edmond's
own glider B (see [Findings](findings.md#the-hat-glider-is-edmonds-glider-b)).

**Command.** `jetcarp search evolve --tiling hat --seed 1`.

## Explore

**What it does.** Searches over the tiling, the rule family, the rule and the starting pattern
together, across JETCARP's periodic and aperiodic tilings and its rule families, for runs that are
worth looking at. It is a quality-diversity search in the manner of MAP-Elites
([Mouret and Clune 2015](bibliography.md#mouret2015illuminating)): rather than one best answer, it
keeps an archive of the best few runs in each *niche*, a combination of rule family, group of tilings
and behaviour. The behaviour is a coarse class built from how the run ended (died, froze, short or
long cycle, still running), when it settled, how dense and how active it was late in the run, the
shape of what it left (dust, clusters, large structures, smooth regions), and whether anything moved.
Each niche keeps up to three runs, at most one per rule. New candidates are mostly fresh random draws,
variations of an elite's rule, or an elite's rule from a new starting pattern, with some effort kept
for the starting patterns and tilings tried least. A GPU loop runs the rules the graphics card
supports in large batches while a CPU loop runs the rest. Unlike the other searches it is not a job:
it has its own command and keeps its results in an SQLite database of its own.

**How it scores.** Each run is reduced to a fixed set of measurements (when it settled, its density
and activity early and late, entropy, smoothness, the sizes and number of its parts, how far things
travelled, how many movers it had) and scored from 0 to about 3. Row rules are judged as pictures.
For the rest, the core of the score is a logistic model fitted to people's keep, maybe and drop
ratings of 829 animated clips over four rounds; fitted to the first three, it reached an AUC of 0.84
for keep against drop on the fourth, which it had not seen. Most of the clips were rated by Emily
Teich, whose eye is built into the score. Learning a score from human judgements is the general
technique of [Christiano et al. 2017](bibliography.md#christiano2017deep). The ratings favour runs that
settle late, start busy, and build large connected structures rather than dust. Because the
measurements are stored, the score can be changed and recomputed without rerunning anything
(`jetcarp explore rescore`).

**How results are checked.** The elites are re-run on larger grids (64,000 cells for 4,096
generations, three times each) to confirm that they were not flukes of a small grid, and the *atlas*
that the Quick Start menu offers is chosen from the confirmed runs (`jetcarp explore atlas`). Every stored run records its tiling, rule, starting pattern and random
state, so it can be replayed exactly as a scene, but the search as a whole depends on timing and is
not reproducible run for run (see [Reproducibility](reproducibility.md#what-is-not-guaranteed-yet)).
`jetcarp explore sheet` and `jetcarp explore clips` render contact sheets and animated clips for
reviewing results by eye.

**Command.** `jetcarp explore run --db explore.sqlite --hours 8`.

Its first full run covered about 15 million runs. To our knowledge it is the first exploratory search
to treat the tiling itself as a search dimension; the evidence and related work are on
[What is new in JETCARP](contributions.md#6-tilings-as-a-dimension-of-an-exploratory-search).
