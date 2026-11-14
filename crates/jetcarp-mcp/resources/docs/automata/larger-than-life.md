# Larger than Life

Larger than Life widens Life's neighbourhood. Instead of the eight cells around it, a cell looks
at every cell within some range `r`, and is born or survives when the number of live cells there
falls in a range of values. With a large enough neighbourhood, Life's small, jagged creatures give
way to big, smooth, almost organic ones.

<figure>
<picture>
  <source srcset="../images/automata/ltl-bosco-still.png" media="(prefers-reduced-motion: reduce)">
  <img src="../images/automata/ltl-bosco.webp" alt="Bosco's Rule: a random soup collapsing into hollow blobs that drift and merge" width="480" loading="lazy">
</picture>
<figcaption>Bosco's Rule (<code>R5,C2,S33-57,B34-45</code>), 300 generations after a random start on a
wrapped 200 × 125 grid. Each cell counts the live cells in the 11 × 11 box around it. Watch the soup
die back within a few generations, leaving hollow blobs that wobble, merge and break up; the small
ones that keep their shape and travel are the "bugs" the rule is known for. Scene
<code>docs/scenes/automata/ltl-bosco.json</code>.</figcaption>
</figure>

## What it is

A Larger than Life rule has a **range** `r`, a **neighbourhood shape**, and a **birth** and
**survival** condition given as ranges of counts:

- On the square grid, the usual (Moore) shape is the box of side `2r + 1` around the cell, so a
  range-5 cell has 120 neighbours. The von Neumann shape is the diamond of cells within `r` steps
  up, down, left and right.
- A dead cell is born when the number of live cells in its neighbourhood is in the birth range; a
  live cell survives when the count is in the survival range.

Life is the range-1 case. Larger ranges average over more cells, which smooths out the randomness
of small counts: patterns grow round edges and move as large coherent bodies. Kellie Evans showed
that rescaled versions of Life's gliders and replicators exist at large ranges; Evans called the
gliders "bugs" ([Evans 1996](../research/bibliography.md#evans1996larger)).

The modern generalisation, *higher-range outer-totalistic* (HROT) rules, allows any list of counts
rather than a single interval for birth and for survival
([LifeWiki: HROT](../research/bibliography.md#lifewikind_hrot)).

## Where it came from

Golly's documentation credits David Griffeath with first imagining Larger than Life in the early
1990s, to see whether Life sits near a critical point when its thresholds and range are scaled up
together ([Golly: Larger than Life](../research/bibliography.md#gollynd_ltl)). Golly cites
Griffeath's "Self-organization of random cellular automata: four snapshots" as the first reference
([Griffeath 1994](../research/bibliography.md#griffeath1994self)).

Kellie Michele Evans developed the family in a 1996 PhD thesis at the University of
Wisconsin–Madison, *Larger than Life: It's So Nonlinear*, supervised by Griffeath. Evans's preface says
the question came from Griffeath: would glider-like "digital creatures" emerge from random starts if
Life's rule were rescaled to larger neighbourhoods? The thesis studies the family with tools from
probability and from dynamics, including large-range analogues of Life's replicators and gliders
([Evans 1996](../research/bibliography.md#evans1996larger)). Evans continued the work in a series of
papers ([Evans 2001](../research/bibliography.md#evans2001larger);
[Evans 2003](../research/bibliography.md#evans2003larger);
[Evans 2003, replicators](../research/bibliography.md#evans2003replicators);
[Evans 2005](../research/bibliography.md#evans2005bosco);
[Evans 2010](../research/bibliography.md#evans2010larger);
[Evans 2016](../research/bibliography.md#evans2016larger)).

MCell's rules lexicon says the "Bugs" rule was found while exploring the parameter space on the
CAM-8 cellular automaton machine, and credits it to Evans
([MCell rules lexicon](../research/bibliography.md#wojtowicznd_rules)). Golly's help says that
"Bosco", a period-166 oscillator, was discovered by Evans, and that Dean Hickerson used MCell to
build the first bug gun from it ([Golly: Larger than Life](../research/bibliography.md#gollynd_ltl)).
The rule became known as Bosco's Rule
([LifeWiki: Larger than Life](../research/bibliography.md#lifewikind_ltl)). Other named rules include
Griffeath's Majority, Majorly and Bugsmovie, Evans's Waffle, Wójtowicz's Globe and Charles A.
Rockafellor's ModernArt (see the [catalogue](named-rules.md#larger-than-life)).

Mirek Wójtowicz's MCell (from 1999) gave the family its first widely used notation, with ranges up
to 10 ([MCell families](../research/bibliography.md#wojtowicznd_families)). Chris Rowett's
LifeViewer added Larger than Life and HROT rules in January 2019, accepting Adam P. Goucher's
lifelib notation for HROT ([Rowett 2019](../research/bibliography.md#rowett2019lifeviewer)). Golly
added a Larger than Life algorithm in version 3.0 (2017) and HROT rules in 4.0 (2020)
([Golly changes](../research/bibliography.md#gollynd_changes)).

## How JETCARP runs it

### Notation

JETCARP reads both the HROT form and MCell's form:

| Notation | Meaning |
| --- | --- |
| `R5,C2,S33-57,B34-45` | Bosco's Rule in HROT form: range 5, two states, survival on 33 to 57, birth on 34 to 45 |
| `R5,C0,M1,S34..58,B34..45,NM` | The same rule in MCell's form, where `M1` counts the cell itself, so the survival bounds are one higher |
| `R2,C2,S4-8,B5-7,NN` | The von Neumann (diamond) shape |
| `R3,C2,S2,B3,N+` | Factorio, on the cross-shaped neighbourhood (square grid only) |
| `R10,C255,S1-2,B3` | ModernArt: with more than two states, dying cells decay as in [Generations](generations.md) |

In HROT form the centre cell is never counted. Counts can be lists of values and ranges separated
by commas. Rules with weighted or custom neighbourhoods (an `NW…` suffix) are not supported yet,
so the named rule *Coexistence* cannot be run.

### On any tiling

On the square grid the Moore shape is the box and the von Neumann shape the diamond. On any other
tiling JETCARP measures range in steps over the neighbour graph: Moore means "within `r` steps over
touch neighbours" and von Neumann means "within `r` steps over edge neighbours". On the square grid
these definitions give exactly the box and the diamond, and range 1 is checked to equal Life. The
cross shape has no such reading and stays on the square grid.

Larger than Life runs on the GPU in the app and in the background host, with exactly the CPU's
result, on lattices and on aperiodic patches. On a patch each tile keeps its own list of the tiles
in reach; a patch whose lists would pass 67 million entries in all runs on the CPU.

Neighbourhood sizes change with the tiling, so a rule's thresholds usually need adjusting when it
moves. On the hat, below, Bosco's thresholds are scaled down by a quarter, and bug-like hollow
blobs appear on an aperiodic tiling.

<figure>
<picture>
  <source srcset="../images/automata/ltl-hat-still.png" media="(prefers-reduced-motion: reduce)">
  <img src="../images/automata/ltl-hat.webp" alt="A Bosco-like rule on the hat tiling: a random start clumping into drifting blobs" width="360" loading="lazy">
</picture>
<figcaption>A Bosco-like rule on the hat (<code>R5,C2,S25-43,B26-34</code>): the neighbourhood is every
tile within five steps of the touch neighbour graph. The middle of a patch of about 4,000 hats, 300
generations from a random start; watch the soup clump into blobs whose edges shift and smooth
themselves over the irregular tiles. Scene <code>docs/scenes/automata/ltl-hat.json</code>.</figcaption>
</figure>

## What is new here

Larger than Life-style range rules have run on the hat before, in Andrew Trevorrow's HatLife
([Trevorrow 2023](../research/bibliography.md#trevorrow2023hatlife)); we did not confirm whether
HatLife measures range in graph steps. Golly supports hexagonal, triangular and other
neighbourhood shapes ([Golly: Larger than Life](../research/bibliography.md#gollynd_ltl)). We do not
claim anything new for this family. Defining range as graph distance is what lets JETCARP run the
family on every tiling, including the k-uniform, isohedral and aperiodic ones.

## Try it

- Quick Start: the eight runnable named rules, for example *Bosco's Rule*, *Majority (LtL)* and
  *ModernArt*.
- `jetcarp run docs/scenes/automata/ltl-bosco.json --gens 300 --png bosco.png`
- Move Bosco's Rule to hexagons and lower its thresholds until bugs appear; the neighbourhood of
  range 5 is smaller there than on squares.

## References

- [Griffeath 1994](../research/bibliography.md#griffeath1994self);
  [Evans 1996](../research/bibliography.md#evans1996larger);
  [Evans 2001](../research/bibliography.md#evans2001larger);
  [Evans 2003](../research/bibliography.md#evans2003larger);
  [Evans 2003, replicators](../research/bibliography.md#evans2003replicators);
  [Evans 2005](../research/bibliography.md#evans2005bosco);
  [Evans 2010](../research/bibliography.md#evans2010larger);
  [Evans 2016](../research/bibliography.md#evans2016larger)
- [MCell rules lexicon](../research/bibliography.md#wojtowicznd_rules);
  [MCell families](../research/bibliography.md#wojtowicznd_families)
- [Golly: Larger than Life](../research/bibliography.md#gollynd_ltl);
  [Golly changes](../research/bibliography.md#gollynd_changes);
  [Rowett 2019](../research/bibliography.md#rowett2019lifeviewer)
- LifeWiki: [Larger than Life](../research/bibliography.md#lifewikind_ltl),
  [Higher-range outer-totalistic](../research/bibliography.md#lifewikind_hrot)
- [Trevorrow 2023](../research/bibliography.md#trevorrow2023hatlife)
