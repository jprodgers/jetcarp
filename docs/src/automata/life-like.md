# Life-like rules

Conway's Game of Life is the best-known cellular automaton, and it belongs to a large family of
rules that work the same way: each cell is alive or dead, and whether it is alive next generation
depends only on whether it is alive now and how many of its neighbours are. JETCARP runs every rule
of this family on every tiling it supports, with either of two neighbourhoods.

<figure>
<picture>
  <source srcset="../images/automata/life-square-still.png" media="(prefers-reduced-motion: reduce)">
  <img src="../images/automata/life-square.webp" alt="Life on a square grid: a random soup boiling down to still lifes, blinkers and drifting debris" width="480" loading="lazy">
</picture>
<figcaption>Conway's Life (<code>B3/S23</code>) for 500 generations after a random start on a wrapped
128 × 80 grid (the first generations are shown more slowly). Watch the soup thin out within a few
dozen generations into islands of activity that settle one by one. Colour shows how long a cell has
been alive, as in LifeViewer's age colouring
([LifeWiki: LifeViewer](../research/bibliography.md#lifewikind_lifeviewer)): pale cells are new, deep
blue ones have not changed for a long time (still lifes such as blocks and beehives). Scene
<code>docs/scenes/automata/life-square.json</code>.</figcaption>
</figure>

## What it is

A Life-like rule is fixed by two lists of neighbour counts:

- **Birth (B):** a dead cell becomes alive if its number of live neighbours is in this list.
- **Survival (S):** a live cell stays alive if its number of live neighbours is in this list.
  Otherwise it dies.

Life itself is `B3/S23`: a dead cell with exactly three live neighbours is born, and a live cell
with two or three survives. From these two lines come gliders that travel across the grid, guns that
fire them, and, as Conway sketched and later builders made explicit, machines that can compute
anything a computer can.

<figure>
<picture>
  <source srcset="../images/automata/life-glider-still.png" media="(prefers-reduced-motion: reduce)">
  <img src="../images/automata/life-glider.webp" alt="Life's glider moving diagonally and the lightweight spaceship moving sideways across a wrapped grid" width="420" loading="lazy">
</picture>
<figcaption>Life's two smallest spaceships, one generation per frame. The glider (top) takes four
generations to shift one cell diagonally (speed c/4); the lightweight spaceship (bottom) takes four to
shift two cells sideways (c/2) and wraps round the edge of the grid. Both flip through their phases as
they go. Scene <code>docs/scenes/automata/life-glider.json</code>, 80 generations.</figcaption>
</figure>

Rules of this kind are called *outer totalistic*: the new state depends on the cell itself and the
*total* of its neighbours, not on which neighbours are alive. On the square grid with the eight
surrounding cells as neighbours, counts run from 0 to 8, so there are 2<sup>18</sup> = 262,144
Life-like rules ([LifeWiki: Life-like cellular automaton](../research/bibliography.md#lifewikind_lifelikeca)).
Many have names and their own communities of explorers; JETCARP's
[named rule catalogue](named-rules.md) lists 96 of them.

## Where it came from

**Life.** John Horton Conway's Game of Life reached the public through Martin Gardner's
"Mathematical Games" column in *Scientific American* in October 1970
([Gardner 1970](../research/bibliography.md#gardner1970life)), followed by a second column in
February 1971 ([Gardner 1971](../research/bibliography.md#gardner1971life)). Conway was simplifying
ideas from von Neumann's self-reproducing automaton, and offered a $50 prize for a pattern that grows
without limit; an MIT group led by Bill Gosper won it in November 1970 with the glider gun
([LifeWiki: Conway's Game of Life](../research/bibliography.md#lifewikind_conwayslife)).
Berlekamp, Conway and Guy's *Winning Ways for your Mathematical Plays* (1982) gave Conway's sketch
that Life can hold a universal computer
([Berlekamp, Conway and Guy 1982](../research/bibliography.md#berlekamp1982winning)), and Paul
Rendell later built an explicit Turing machine in Life
([Rendell 2002](../research/bibliography.md#rendell2002turing)).

**Neighbourhoods.** The eight-cell neighbourhood is named after Edward F. Moore
([Moore 1962](../research/bibliography.md#moore1962machine)) and the four-cell one after John von
Neumann, whose 29-state automaton used it
([LifeWiki: Von Neumann neighbourhood](../research/bibliography.md#lifewikind_vonneumann)).

**The rule space.** Norman Packard and Stephen Wolfram surveyed two-dimensional automata, numbering
outer-totalistic rules and studying their behaviour
([Packard and Wolfram 1985](../research/bibliography.md#packard1985twodimensional)). David Eppstein
later classified Life-like rules by whether patterns can grow out of a bounding box and whether they
can die out ([Eppstein 2010](../research/bibliography.md#eppstein2010growth)).

**Notation.** Two orders are in use. The older survival/birth form writes Life as `23/3`; it was
common for years, and Mirek Wójtowicz's MCell rule lexicon uses it
([Wójtowicz, rules lexicon](../research/bibliography.md#wojtowicznd_rules)). The birth/survival form
`B3/S23` is the one Golly and LifeWiki use today
([LifeWiki: Rulestring](../research/bibliography.md#lifewikind_rulestring)). We found no source that
says who first wrote rules as `B…/S…`.

**Variants.** People began trying other counts almost at once. LifeWiki traces 3-4 Life (`B34/S34`)
to Robert Wainwright's newsletter *Lifeline* in December 1971
([LifeWiki: 3-4 Life](../research/bibliography.md#lifewikind_34life)). A few of the many named rules
that followed, with credits as the sources give them:

- *HighLife* (`B36/S23`), first considered by Nathan Thompson in 1994 and named by Conway; it has a
  small replicator ([LifeWiki: HighLife](../research/bibliography.md#lifewikind_highlife)).
- *Day & Night* (`B3678/S34678`), explored by Nathan Thompson from 1997; swapping live and dead
  cells leaves it unchanged ([LifeWiki: Day & Night](../research/bibliography.md#lifewikind_daynight)).
- *Seeds* (`B2/S`), first studied by Brian Silverman and named by Wójtowicz
  ([LifeWiki: Seeds](../research/bibliography.md#lifewikind_seeds)).
- *Life without Death* (`B3/S012345678`), which Tommaso Toffoli and Norman Margolus described as
  INKSPOT ([Toffoli and Margolus 1987](../research/bibliography.md#toffoli1987cellular)).
- *Vote* and *Anneal*, Gérard Vichniac's majority and twisted-majority rules
  ([Vichniac 1986](../research/bibliography.md#vichniac1986cellular)).

**Software.** Golly, by Andrew Trevorrow and Tom Rokicki, first released in 2005, made these rules
easy to explore and fast to run ([Golly credits](../research/bibliography.md#gollynd_credits);
[Golly changes](../research/bibliography.md#gollynd_changes)). Before it came the CAM-6 hardware of
Toffoli and Margolus ([1987](../research/bibliography.md#toffoli1987cellular)), Rudy Rucker and John
Walker's CelLab ([1989](../research/bibliography.md#rucker1989cellab)) and Wójtowicz's MCell. Chris
Rowett's LifeViewer now shows patterns in the browser on LifeWiki and the forums, colouring cells
by age if asked ([Rowett: LifeViewer](../research/bibliography.md#rowettnd_lifeviewer);
[LifeWiki: LifeViewer](../research/bibliography.md#lifewikind_lifeviewer);
[Rowett 2019](../research/bibliography.md#rowett2019lifeviewer)).

## How JETCARP runs it

### Notation

JETCARP reads the birth/survival form, in upper or lower case:

| Notation | Meaning |
| --- | --- |
| `B3/S23` | Life: born on 3, survives on 2 or 3, touch neighbours |
| `B36/S23` | HighLife |
| `B2/S34H` | Hexagonal Life: the `H` records that the rule is meant for hexagons ([forum thread, 2016](../research/bibliography.md#drc2016b2s34h)) |
| `B1/S0V` | Gnarl on edge neighbours: born with one live neighbour, survives with none |
| `B3,10/S2,3` | Counts of 10 or more are separated by commas |
| `B014/S2` | Oils, a rule with birth on 0 (see below) |

Any of the [named rules](named-rules.md) can be typed by name instead.

### Any tiling, two neighbourhoods

A Life-like rule needs only a neighbour count, so it runs unchanged on any neighbour graph (see
[How JETCARP generalises automata](generalising.md)). By default a cell counts its *touch*
neighbours, which share an edge or a corner; on squares that is Moore's eight.

### Edge neighbours: the V suffix

A trailing `V` switches the rule to *edge* neighbours, which share a side; on squares that is von
Neumann's four. This follows LifeWiki's convention
([LifeWiki: Rulestring](../research/bibliography.md#lifewikind_rulestring)). The neighbourhood can
also be set per automaton in a scene (`"neighborhood": "edge"`).

Counts mean different things on different tilings. A triangle has 12 touch neighbours, a hexagon in
the 3.3.3.3.6 tiling has 18, and a square in 4.8.8 has only 4. A rule tuned for one tiling is
usually a different creature on another.

<figure class="pair">
<div><img src="../images/automata/life-hex.png" alt="B2/S34 on hexagons after 60 generations" width="400"></div>
<div><img src="../images/automata/life-triangle.png" alt="B3/S23 on triangles after 300 generations" width="400"></div>
<figcaption>Left: Hexagonal Life (<code>B2/S34H</code>), 60 generations from a random start; every
hexagon has six neighbours. It has spaceships of its own, which wildmyron posted in the forum thread
that named the rule ([drc, Hickerson and wildmyron 2016](../research/bibliography.md#drc2016b2s34h)).
Scene <code>docs/scenes/automata/life-hex.json</code>. Right: Life's own
rule <code>B3/S23</code> on triangles with touch neighbours, where each cell has twelve neighbours,
300 generations. Scene <code>docs/scenes/automata/life-triangle.json</code>.</figcaption>
</figure>

### B0 rules

If 0 is in the birth list, every dead cell with no live neighbours is born, so the empty background
switches on. Without survival on the maximum count it switches off again next generation, and the
whole picture flashes. Golly avoids this by running such a rule as two complementary rules that
alternate on even and odd generations, so that what you see is the rule's real behaviour without the
strobing ([Golly: QuickLife](../research/bibliography.md#gollynd_quicklife)); Golly credits David
Eppstein with the idea ([Golly credits](../research/bibliography.md#gollynd_credits)). JETCARP does
the same, and its tests check the result against running the rule by brute force. B0 rules also run
under HashLife.

### Speed

On lattice tilings JETCARP can step Life-like rules on the GPU as well as the CPU, with identical
results, and HashLife can jump wrap-edge lattice worlds far ahead.

## What is new here

Life-like rules on other tilings are not new: Carter Bays studied them on triangles, hexagons and
pentagons ([Bays 1994](../research/bibliography.md#bays1994cellular);
[Bays 2005](../research/bibliography.md#bays2005note)), Life has been run on Penrose tilings
([Hill, Stepney and Wan 2005](../research/bibliography.md#hill2005penrose);
[Owens and Stepney 2010](../research/bibliography.md#owens2010game)) and on the hat
([Trevorrow 2023](../research/bibliography.md#trevorrow2023hatlife)), and H. S. Guttesen studied Life
on the Archimedean tilings ([Guttesen 2026](../research/bibliography.md#guttesen2026game)).

What JETCARP adds is a search for **spaceships** (patterns that move) across many tilings and rules
at once. Some of what it found was already known. Guttesen reported a period-4 glider and a glider
gun for Life on the kagome tiling (3.6.3.6) and a period-8 glider on the snub square tiling; JETCARP's
soup searches found the same kagome glider (cell for cell, in one of its phases) and, by comparison
with Guttesen's figure, the same snub-square glider independently, and the credit for them is Guttesen's.

Beyond those, JETCARP's catalogue holds spaceships in Life-like rules on the 4.8.8, 3.12.12, 4.6.12,
3.4.6.4, 3.3.3.4.4 and 3.3.3.3.6 tilings, on 3.6.3.6 in rules other than Life, and on several 2- and
3-uniform tilings. Each ship was checked in three ways before it was claimed:

- Against every Archimedean, 2-uniform, 3-uniform and isohedral tiling, with both neighbourhoods. A
  ship that sees exactly the same neighbour counts in a corridor of a simpler tiling belongs to that
  tiling, and so does one whose live tiles evolve the same way there in some rule.
- For being two smaller ships flying together.
- On large patches, with wrapped edges and with dead edges.

Ten catalogue entries turned out to be pairs of smaller ships flying side by side, eight of them two
of Guttesen's kagome gliders, and they are not claimed; every Life candidate on the kagome tiling was
such a pair. Eight ships found on 3-uniform tilings run unchanged on a simpler tiling, and they are
listed there. A looser test, which asks only that the live tiles evolve the same way whatever the
neighbour counts of the empty tiles around them, found two more catalogue entries that are the same
ship as another, and eight ships that run on a simpler tiling than the one they were found on. That
leaves 60 distinct objects. Thirteen are a smaller ship with a passenger, or a
bound pair, and are listed with the smaller ship. Twenty-eight, six of those thirteen among them, carry a
flag: they are small and turn up often in soups, run in a well-known rule, are built on Guttesen's
gliders, or run on 4.8.8 or 3.12.12 in the rules where EricG reported many unlisted spaceships in 2013
([EricG 2013](../research/bibliography.md#ericg2013tiling2)), so others may well have seen them,
although we found no report. To our knowledge, the remaining 25 single ships, on 3.4.6.4, 3.3.3.4.4,
3.3.3.3.6 and on 2- and 3-uniform tilings, have not been reported before. Spaceships on these tilings
are not new as such: EricG found ships in Life-like rules on every Archimedean tiling, and Martin
Büttner found one in Life on a 2-uniform tiling in 2014 ([Code Golf 2014](../research/bibliography.md#codegolf2014life));
only these particular ships are claimed. Only two of the 60 run in
Life itself: both are on the snub square tiling, both are built around Guttesen's glider, and both are
flagged. The rest are in other rules.

The searches behind those 60 ran Life itself on only a few tilings, though. A census of Life on all 92
Archimedean, 2-uniform and 3-uniform tilings, 50,000 random soups on each, found Life spaceships on 28
of them: Conway's glider and spaceships on the square grid, Guttesen's two gliders, a ship on 3.3.3.4.4
that is probably the one EricG reported in 2013, the period-20 ship on k2-007 that is very likely
Büttner's, and ships on 5 of the 2-uniform and 19 of the 3-uniform tilings. To our knowledge, nobody
had reported running Life on any of those 2- and 3-uniform tilings but k2-007. Most of the census's
ships are common ships flying in pairs, or ships of a simpler tiling running in a corridor; those that
passed every check are listed in
[Life on the Archimedean and k-uniform tilings](../research/findings.md#life-on-the-archimedean-and-k-uniform-tilings).

Ships found on isohedral tilings are not claimed at all. Each isohedral tiling has the neighbour graph
of one of the eleven Laves tilings, and every "ship" JETCARP found on one is a known ship of the square
or triangular grid in another drawing, Conway's glider among them. The full list, with every check, is
in [Findings](../research/findings.md); the claims are on
[What is new in JETCARP](../research/contributions.md).

## Try it

- Quick Start: *Life on the square grid*, *Life on the 12-neighbour triangle grid*, *B2/S34 on the
  hat*, and one entry for every named rule, each on its usual grid.
- From the command line:
  `jetcarp run docs/scenes/automata/life-square.json --gens 500 --png life.png`.
- Change the tiling of any Life-like entry in the app and watch the same rule on a new graph.

## References

- [Gardner 1970](../research/bibliography.md#gardner1970life);
  [Gardner 1971](../research/bibliography.md#gardner1971life);
  [Berlekamp, Conway and Guy 1982](../research/bibliography.md#berlekamp1982winning);
  [Rendell 2002](../research/bibliography.md#rendell2002turing)
- [Moore 1962](../research/bibliography.md#moore1962machine);
  [Packard and Wolfram 1985](../research/bibliography.md#packard1985twodimensional);
  [Eppstein 2010](../research/bibliography.md#eppstein2010growth)
- [Toffoli and Margolus 1987](../research/bibliography.md#toffoli1987cellular);
  [Vichniac 1986](../research/bibliography.md#vichniac1986cellular);
  [Rucker and Walker 1989](../research/bibliography.md#rucker1989cellab)
- [Golly: QuickLife](../research/bibliography.md#gollynd_quicklife);
  [Golly credits](../research/bibliography.md#gollynd_credits);
  [Golly changes](../research/bibliography.md#gollynd_changes);
  [Rowett 2019](../research/bibliography.md#rowett2019lifeviewer)
- [Bays 1994](../research/bibliography.md#bays1994cellular);
  [Bays 2005](../research/bibliography.md#bays2005note);
  [Hill, Stepney and Wan 2005](../research/bibliography.md#hill2005penrose);
  [Owens and Stepney 2010](../research/bibliography.md#owens2010game);
  [Trevorrow 2023](../research/bibliography.md#trevorrow2023hatlife);
  [Guttesen 2026](../research/bibliography.md#guttesen2026game)
- LifeWiki: [Life-like cellular automaton](../research/bibliography.md#lifewikind_lifelikeca),
  [Rulestring](../research/bibliography.md#lifewikind_rulestring),
  [Conway's Game of Life](../research/bibliography.md#lifewikind_conwayslife),
  [3-4 Life](../research/bibliography.md#lifewikind_34life),
  [HighLife](../research/bibliography.md#lifewikind_highlife),
  [Day & Night](../research/bibliography.md#lifewikind_daynight),
  [Seeds](../research/bibliography.md#lifewikind_seeds)
