# Isotropic rules: Hensel's notation

A [Life-like rule](life-like.md) looks only at how many of a cell's eight neighbours are alive. An
*isotropic non-totalistic* rule also looks at where they are: two live neighbours side by side can
mean something different from two on opposite sides. What it does not look at is which way the
arrangement faces. Arrangements that are turns or mirror images of each other count as one, so the
rule treats every direction alike, which is what *isotropic* means. Alan Hensel's notation names
each arrangement with a letter after its count: `B2-a/S12` is born on two live neighbours unless
they are a corner and the edge beside it, and survives on one or two.

<figure>
<img src="../images/automata/isotropic-letters.png" alt="A table of small three-by-three blocks: one column for each count of live neighbours from 0 to 8, one row for each letter, each block a cell and its live neighbours" width="364" loading="lazy">
<figcaption>Hensel's letters, as JETCARP reads them. Columns: 0 to 8 live neighbours. Rows: no
letter (0 and 8), then c, e, k, a, i, n, y, q, j, r, t, w and z. Each block is the cell (purple) and
its live neighbours (orange), those for 1 to 4 drawn as Hensel's table draws them; every turn and
reflection of a block has the same letter. The blocks for 5 to 7 are those for 3 to 1 with live and
dead swapped.
Scene <code>docs/scenes/automata/isotropic-letters.json</code>, at generation 0.</figcaption>
</figure>

## What it is

A cell on the square grid touches eight neighbours: four across its edges and four at its corners.
Their live and dead states make 256 neighbourhoods. Counted up to the square's eight symmetries
(four turns, each with or without a reflection) they fall into 51 arrangements: one each for 0 and
8 live neighbours, and 2, 6, 10, 13, 10, 6 and 2 for 1 to 7. An isotropic rule says, for each of the
51, whether a dead cell is born and whether a live cell survives, so there are 2<sup>102</sup> such
rules. The Life-like rules are the 2<sup>18</sup> among them that treat every arrangement of a count
alike.

Hensel gave each arrangement a letter. Those of 1 to 4 live neighbours are drawn above; those of 5,
6 and 7 are the arrangements of 3, 2 and 1 *dead* neighbours, under the same letters. Four letters
carry their meaning in Hensel's table: **c** (corner), **e** (edge), **k** (knight, for a knight's
move) and **a** (adjacent); for the rest, Hensel wrote that he tried to make them mnemonic but that
many are somewhat arbitrary. With two live neighbours, for example:

| Letter | The two live neighbours |
| --- | --- |
| `2c` | two corners on the same side |
| `2e` | two edges, round a corner |
| `2k` | a corner and an edge a knight's move from it |
| `2a` | a corner and the edge beside it |
| `2i` | two opposite edges |
| `2n` | two opposite corners |

| Live neighbours | Letters |
| --- | --- |
| 0 and 8 | none |
| 1 and 7 | `c e` |
| 2 and 6 | `c e k a i n` |
| 3 and 5 | `c e k a i n y q j r` |
| 4 | `c e k a i n y q j r t w z` |

A rule is written as a B/S rule with letters after any count. A count with letters takes only those
arrangements (`B2ce`); a `-` after the count takes every arrangement but those listed (`B2-a`, the
same as `B2ceikn`); and a count with no letters takes them all, as in a Life-like rule, so Life is
`B3/S23` whether or not its letters are written out. A third part makes it a [Generations](generations.md) rule:
`B2a/S/C3`. Only state-1 neighbours count, as for Generations rules; dying cells do not.

<figure>
<img src="../images/automata/isotropic-tlife.png" alt="Small still lifes and oscillators scattered on a dark grid, with a few gliders and other spaceships among them" width="800" loading="lazy">
<figcaption>tlife (<code>B3/S2-i34q</code>) 300 generations after a random soup on a wrapped square
grid: Life's birth, and survival on two live neighbours unless they face each other across the
cell, on three, and on four in the <code>q</code> arrangement. Small still lifes and oscillators
settle, and gliders and other spaceships travel among them (colour shows age: the youngest cells
are brightest). Scene <code>docs/scenes/automata/isotropic-tlife.json</code>.</figcaption>
</figure>

## Where it came from

Alan Hensel described the notation on the pages for his Java Life applet. On the copy of his "Rule
Notation" page dated 15 May 2001 he writes that he extended his applet's notation to rules "that
depend on the configuration of the neighbors, not just their counts", kept to symmetrical ones, and
counted 51 neighbourhoods; he assigned each a letter, mnemonic where he could, allowed a `-` before
the letters to name the arrangements left out, and gave Just Friends as an example, birth on two
neighbours except when they are adjacent and survival on one or two
([Hensel 2001](../research/bibliography.md#hensel2001notation)). His applet wrote survival first, so
the page has `12/2-a` for today's `B2-a/S12`. That page's table uses seventeen letters, among them
`x` for two opposite corners. A second page of the same date has the revised table of thirteen
letters, which reuses letters between counts (`2n` for the opposite corners) and which JETCARP
follows ([Hensel 2001b](../research/bibliography.md#hensel2001table)).

In a 2012 forum thread on the notation, Paul Tooke wrote that the scheme had been revised after Johan
Bontes suggested combining some of the mnemonics, and that the revised table is the one in the help
file of Bontes's program Life32, apart from turns and reflections. EricG reported that Life32 swaps
`r` and `y` for three, four and five neighbours, and Tooke that Hensel's applet does not
([EricG and Tooke 2012](../research/bibliography.md#ericg2012nontotalistic)). JETCARP reads the
letters as Hensel's table draws them.

Golly added the rules to its QuickLife and HashLife algorithms in version 2.8 (August 2016); version
3.0 (August 2017) added them with birth on 0, and to its Generations algorithm
([Golly changes](../research/bibliography.md#gollynd_changes)). Its help describes the notation as
based on Hensel's, gives its canonical form, and credits Just Friends to David Bell
([Golly: QuickLife](../research/bibliography.md#gollynd_quicklife)). LifeWiki keeps a list of named
isotropic rules, from which JETCARP's other named ones are taken (see the
[named rule catalogue](named-rules.md#isotropic)).

## How JETCARP runs it

### Notation

| Notation | Meaning |
| --- | --- |
| `B2-a/S12` | Just Friends: birth on two, except a corner with the edge beside it; survival on one or two |
| `B3-cnqy/S23-a` | birth on three, except `c`, `n`, `q` and `y`; survival on two, or three except `a` |
| `B2a/S/C3` | a Generations rule with three states |
| `B01e/S8` | birth on 0, on one edge neighbour, and survival on eight (see [Generations and B0 rules](#generations-and-b0-rules)) |
| `B3aceijknqry/S2aceikn3aceijknqry` | Life with every letter written: read as `B3/S23` |

Case does not matter, letters may come in any order, and a count may appear twice (its letters add
up). JETCARP writes every rule back in the canonical form Golly's documentation gives: each count's
letters in alphabetical order, or `-` and the letters left out where that is shorter (the plain form
when they are the same length), and a count with every letter as its bare digit. So `B2kicen/S21`
comes back as `B2-a/S12`, and a rule whose counts all have every letter or none is the Life-like or
Generations rule of those counts, in its family. A letter that its count does not take is refused
with the letters it does: `B1a/S` gives *"'a' is not one of Hensel's letters for 1 live neighbour;
1 takes c, e"*. The [rule notation reference](../reference/rule-notation.md#isotropic-rules-hensels-notation)
lists every form.

### The letters, checked

JETCARP's table of letters was read from the diagrams on Hensel's revised page, cell by cell. A test
checks that the letters of each count are different arrangements, that together they are all 51,
and that 5 to 7 are 3 to 1 with live and dead swapped. Then the stepper is checked four ways:

- **Against the letters.** Every generation, every cell's next state must be what the rule's letters
  say for its neighbourhood, worked out separately in the test.
- **Against the B/S rules.** Given a Life-like rule's table (every letter of each count), the
  isotropic stepper must give the B/S stepper's states and ages, for two-state, Generations and B0
  rules, with every kind of edge. A von Neumann rule (`B2/S013V`) is an isotropic rule that reads only
  the four edge neighbours; written out letter by letter, it must step as the rule with a `V`.
- **Isotropy.** A start turned or reflected by any of the square's eight symmetries must reach the
  same state, turned or reflected the same way.
- **Against Golly.** Golly 5.0's command-line program, `bgolly`, runs the same soups: 73 rules (Just
  Friends written two ways, `B3-cnqy/S23-a`, Life written with letters two ways, two B/S rules with B0,
  and 66 random isotropic rules: two-state, B0 and Generations, written with letters, `-` forms and
  bare digits in shuffled orders), two soups each, compared cell by cell and state by state at
  every generation up to 100, and Golly must name every rule as JETCARP does. For every two letters of a count, as birth and as survival, some rule
  takes one and not the other while its soup has a cell with the first, so a letter read wrongly
  would show at the first generation. It does: with the arrangements of `3r` and `3y` swapped (as in
  Life32) the comparison fails at the first generation in 120 of its 146 soups, and with `4j` and
  `4r` swapped in 88 (`golly_agrees_on_isotropic_rules`, `jetcarp-api/tests/golly.rs`, ignored where
  Golly isn't installed; CI runs it).

### Square grid only

The letters name places among the square grid's eight touching neighbours, so these rules run on the
square grid only, with touch neighbours: a `V` is refused, and an automaton's neighbourhood setting is
ignored. A rule on another tiling is refused with the grid it needs. Other tilings have neighbourhoods
of their own; the hexagonal grid has an isotropic notation of its own, which JETCARP does not read
yet. (See [How JETCARP generalises automata](generalising.md).)

Past a dead edge a neighbour is dead; past a live edge it is live; with wrapped edges the grid is a
torus.

### Generations and B0 rules

A Generations rule's dying cells count up to the state count and die, as for any Generations rule.

A two-state rule with birth on 0 is kept relative to its background, as Life-like
[B0 rules](life-like.md#b0-rules) are: on the square grid the background is all dead and all live in
turn, or live for good from the first step when the rule survives on 8, and a cell is lit where the
real state differs from it. That background depends only on birth on 0 and survival on 8, the two
counts with one arrangement each. A rule that survives on 8 starts on the live background, as
Golly reads it, and Golly agrees with JETCARP generation by generation from the start for both kinds.
A Generations rule with birth on 0 runs unemulated, as Life-like Generations rules do (Golly does not
run Generations rules with birth on 0).

### Searches and the library

The soup hunter runs isotropic rules on the square grid, on the CPU and the GPU: its bit-parallel
engine reads the eight neighbour planes through a small decision diagram for birth and one for
survival, instead of counting them, and the GPU hunter's shader carries the same diagrams, so a hunt
leaves the same catalogue with or without the GPU. The object finder, puffer detection and the
library take the rules too. The rules are isotropic, so an object turned or reflected is the same
object, and the finder's eight symmetries apply. A record's identity uses the same cell model as a
Life-like rule's (`lifelike`), so a still life is the same pattern whichever rule it was found in;
its rule is kept with it. A record on the square grid's touching neighbours has an **isotropic rule
range**, whatever B/S, Generations or isotropic rule found it: the births and survivals its
evolution needs, and those it must not have, among the 51 arrangements, so every isotropic rule
between the two runs it exactly as stored. It is worked out when the pattern is verified, shown with
the record (`jetcarp library show`, the app, the MCP tools), stored in the library and in packs
(format 1.1), and `--works-in` matches a rule against it, so Life's glider is found in tlife. A
record also has a B/S rule range when a B/S rule gives its evolution; an object of an isotropic rule
whose evolution needs a count's arrangements to differ has only the isotropic one. The exploratory
search samples isotropic rules when a search names them (`jetcarp search explore --families
isotropic`), on the GPU when there is one and on the processor when there is not, and leaves
them out otherwise, the named ones too ([Search methods](../research/search-methods.md#explore)). HashLife
(`jetcarp hashlife`) does not run them.

### On the GPU

Isotropic rules run on the GPU in the app, the background host and `jetcarp run`, with exactly the
CPU's result: the rule's 512-bit table travels with each step, and B0 rules follow the same
background. The tests compare the state hash every few generations on even and odd sides with every
kind of edge, from odd step counts and with several steps per generation, and give the GPU the
isotropic tables of B/S rules to compare with the CPU's B/S stepper.

Explore searches run them in the GPU's batches too, many short runs at once, two-state, B0 and
Generations: each cell reads its eight touching neighbours by place, from a list made as the
processor finds them, so a run gives the same cells on the GPU as on the processor. The tests
compare every cell of every generation for random rules over all 102 letters, written in shuffled
notation too, on every kind of edge, and the GPU's self-test runs an isotropic batch before the
GPU is used.

### Copy as RLE

*Copy as RLE* writes two-state isotropic rules in the canonical form, which Golly reads and names the
same way, and their B0 rules where Golly reads them the same: always when the background is live for
good, at even generations when it flashes (the Golly tests check it).

### Reproducibility

Isotropic rules are a [rule-semantics](../reference/library-format.md#rule-semantics-versions) family
of their own, `isotropic`, at version 1, with a pinned scene, and the state hash covers the rule.

<figure>
<img src="../images/automata/isotropic-just-friends.png" alt="Many short lines and small shapes on a dark grid, with a few brighter moving shapes" width="800" loading="lazy">
<figcaption>Just Friends (<code>B2-a/S12</code>) 300 generations after a random soup on a wrapped
square grid. A pair of live cells survives, so the soup settles into short lines and small still
lifes, with a few oscillators (brighter: colour shows age). Scene
<code>docs/scenes/automata/isotropic-just-friends.json</code>.</figcaption>
</figure>

## What is new here

Nothing. The notation and the rules belong to the people credited above.

## Try it

- Quick Start: *Just Friends*, *tlife*, *qlife*, *LeapLife*, *Salad*, *Goat Flock* and *Logic Rule*.
- `jetcarp run docs/scenes/automata/isotropic-tlife.json --gens 300 --png tlife.png`
- `jetcarp search hunt --rules "B3/S2-i34q" --tilings square --soups 500` hunts tlife's soups, and
  finds its glider and other spaceships.
- `jetcarp search explore --families isotropic --minutes 30` explores random isotropic rules near
  the B/S rules, on the square grid.
- Type a Life-like rule with one letter changed, such as `B3/S23-a` (Life, but a live cell with
  three neighbours in a corner and its two edges dies), and watch how far Life's patterns still hold.

## References

- [Hensel 2001](../research/bibliography.md#hensel2001notation);
  [Hensel 2001b](../research/bibliography.md#hensel2001table)
- [EricG and Tooke 2012](../research/bibliography.md#ericg2012nontotalistic)
- [Golly: QuickLife](../research/bibliography.md#gollynd_quicklife);
  [Golly changes](../research/bibliography.md#gollynd_changes)
