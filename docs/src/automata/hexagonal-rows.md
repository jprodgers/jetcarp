# Hexagonal row rules: HexW and HexT

Elementary rules build a picture row by row on the square grid, each cell reading the three cells
above it. On a grid of hexagons the rows are staggered, so a cell sits under *two* cells, or, if
the hexagons are turned to have flat tops, under three cells at two different heights. JETCARP has
a family for each: `HexW` with two parents and `HexT` with three. Both come in binary and
three-state versions.

<figure>
<img src="../images/automata/hex-hexw3.png" alt="Pascal's triangle mod 3 drawn in hexagons, in red and green" width="800">
<figcaption><code>HexW3 8229</code> from a single cell: each hexagon is the sum, mod 3, of the two
hexagons above it, which draws Pascal's triangle mod 3. Red and green are the two non-zero states.
Scene <code>docs/scenes/automata/hex-hexw3.json</code>, run until the bottom row is written.</figcaption>
</figure>

## What it is

**Two parents (`HexW`, `HexW3`).** On a grid of pointy-topped hexagons, each hexagon touches two
hexagons in the row above, one up and to the left, one up and to the right. A two-parent rule gives
the new cell's state as a function of those two. With two states there are 2 × 2 = 4 parent
patterns and so 2<sup>4</sup> = 16 rules; with three states there are 9 patterns and
3<sup>9</sup> = 19,683 rules. `HexW 6` is the exclusive or of the two parents, which draws Pascal's
triangle mod 2 just as rule 90 does on squares.

**Three parents (`HexT`, `HexT3`).** Turn the hexagons a quarter turn so they have flat tops. Now
three hexagons touch a cell's upper edges: NW and NE, half a row higher, and N, a full row higher.
The grid fills in by height in half-row *layers*, and each new layer is computed from the two layers
before it. With two states there are 8 parent patterns and 256 rules, numbered like Wolfram's
elementary rules; with three states there are 27 patterns and 3<sup>27</sup> (about 7.6 trillion)
rules. Two smaller sets are easier to explore: *totalistic* rules, which depend only on the sum
NW + N + NE (2,187 three-state rules, 16 binary ones), and left-right *symmetric* rules, which treat
NW and NE alike.

<figure class="pair">
<div><img src="../images/automata/hex-hext3-sum588.png" alt="Nested diamonds in red and green on flat-top hexagons" width="400"></div>
<div><img src="../images/automata/hex-hext3-sum314.png" alt="A chaotic pattern in pink and blue on flat-top hexagons" width="400"></div>
<figcaption>Two totalistic three-parent rules from a single cell on flat-top hexagons. Left:
<code>HexT3 sum 588</code>, NW + N + NE mod 3, grows nested diamonds. Right: <code>HexT3 sum 314</code>
turns chaotic. Scenes <code>docs/scenes/automata/hex-hext3-sum588.json</code> and
<code>hex-hext3-sum314.json</code>, run until the bottom layer is written.</figcaption>
</figure>

## Where it came from

Hexagonal grids have a long history in cellular automata. Marcel Golay used hexagonal cellular
logic for image processing in 1969 ([Golay 1969](../research/bibliography.md#golay1969hexagonal)).
Norman Packard and Stephen Wolfram's survey of two-dimensional automata counted the rules on the
hexagonal neighbourhood alongside the square ones
([Packard and Wolfram 1985](../research/bibliography.md#packard1985twodimensional)), and the hexagonal
lattice gas of Frisch, Hasslacher and Pomeau showed why hexagons matter for physics: their symmetry
is enough to recover fluid flow, where the square grid's is not
([Frisch, Hasslacher and Pomeau 1986](../research/bibliography.md#frisch1986latticegas)).

**The two-parent rules are Wolfram's.** In a note in *A New Kind of Science*, Wolfram observes that
if the cells on successive steps are arranged "like hexagons or staggered bricks", each cell depends
on two cells instead of three; Wolfram counts 16 two-colour rules and 19,683 three-colour rules, of which
1,734 are fundamentally different ([Wolfram 2002](../research/bibliography.md#wolfram2002new), p. 885).
`HexW` and `HexW3` are exactly this family. Matej Kerekrety's 2020 web tool also draws the 16 binary
rules on hexagons, along with the 256 elementary rules on squares and pentagons
([Kerekrety 2020](../research/bibliography.md#kerekrety2020elementary)).

**The three-parent rules are a known kind of rule in a new presentation.** A cell that depends on
two cells one layer back and one cell two layers back is a *second-order* one-dimensional automaton
on a staggered lattice. Second-order rules are standard: *A New Kind of Science* uses them to build
reversible rules (p. 437 and the note on p. 1017), and the Wolfram Language's `CellularAutomaton`
supports staggered ranges and rules of any order
([Wolfram 2002](../research/bibliography.md#wolfram2002new);
[Wolfram Language documentation](../research/bibliography.md#wolframnd_cellularautomaton)). The 16
`HexT` rules of the form g(NW, NE) XOR N are exactly the reversible second-order versions of the 16
two-parent rules.

## How JETCARP runs it

### Notation

| Notation | Family | Rule number |
| --- | --- | --- |
| `HexW 6` | two parents, 2 states | 0 to 15; the output for parents (left, right) is digit left·2 + right of the number in base 2 |
| `HexW3 8229` | two parents, 3 states | 0 to 19,682; digit left·3 + right in base 3 |
| `HexT 90` | three parents, 2 states | 0 to 255; bit NW·4 + N·2 + NE |
| `HexT3 n` | three parents, 3 states | 0 to 3<sup>27</sup> − 1; digit NW·9 + N·3 + NE in base 3 |
| `HexT3 sum 588` | three parents, 3 states, totalistic | 0 to 2,186; digit NW + N + NE in base 3 |
| `HexT sum 6` | three parents, 2 states, totalistic | 0 to 15 |
| `HexT3 sym n` | three parents, 3 states, symmetric | 0 to 3<sup>18</sup> − 1 |

`HexT 90` is NW XOR NE, Pascal's triangle mod 2 again; `HexT 150` is the XOR of all three parents;
`HexT 4` copies N straight down.

### Grids

The two-parent rules run on the ordinary hexagon tiling (`hex`). The three-parent rules need the
flat-top hexagon tiling (`hex_flat`), whose rows are the half-row layers. As with the
[elementary rules](elementary.md), each generation writes one more row or layer from a seed at the
top (`top_center` for one cell, `top_random` for a random row), with dead, live or wrapped edges, and
the run is *done* when the bottom is reached. These families are tied to rows of hexagons and do not
run on other tilings; the any-tiling version of the idea is the [layered family](layered.md).

### Sheets and sweeps

`jetcarp sheet --kind hexw`, `hexw3`, `hext`, `hext3` or `hext3sum` draws a contact sheet of
consecutive rules, and scripts can sweep whole sets. A first run classified all 256 `HexT` rules and
all 2,187 `HexT3 sum` rules from a single cell with JETCARP's row descriptors, which label the rows a
rule leaves behind as uniform, periodic, shifting or aperiodic (and the aperiodic ones as nested,
regular or chaotic). For `HexT` that gave 44 uniform, 127 periodic, 46 shifting and 39 aperiodic
rules, three of them chaotic (153, 165 and 195). The labels are a quick heuristic, not a proof.

<figure>
<img src="../images/automata/hex-hexw-sheet.png" alt="Contact sheet of the 16 binary two-parent hexagon rules" width="880">
<figcaption>All 16 binary two-parent rules, <code>HexW 0</code> to <code>HexW 15</code>, from a single cell:
<code>jetcarp sheet --kind hexw --count 16</code>.</figcaption>
</figure>

## What is new here

The two-parent rules are not new: they are Wolfram's two-cell rules, drawn on hexagons as Wolfram
describes, and Kerekrety's tool draws the binary ones too.

The three-parent rules are built from standard ingredients, but we found no earlier source that
presents them as a family of hexagon rules, numbers all 256 binary and 3<sup>27</sup> three-state
rules in this way, adds the totalistic and symmetric subsets, or catalogues them. To our knowledge,
presenting and cataloguing the `HexT` and `HexT3` family is new with JETCARP. The search behind this
is summarised in [What is new in JETCARP](../research/contributions.md).

## Try it

- Quick Start: *Hex rule 6 (XOR)*, *Hex ternary 8229 (sum mod 3)*, *Hex3 rule 90 (NW xor NE)*, *Hex3
  ternary sum 588 (sum mod 3)* and *Hex3 ternary sum 314*. On a hexagon rule's card, `<` and `>`
  step through the rule numbers.
- `jetcarp sheet --kind hext3sum --from 0 --count 64 --png sheet.png` draws 64 totalistic
  three-parent rules.
- `jetcarp run docs/scenes/automata/hex-hext3-sum588.json --gens 200 --png diamonds.png`

## References

- [Wolfram 2002](../research/bibliography.md#wolfram2002new) (p. 885 note on two-cell
  neighbourhoods; p. 437 and the p. 1017 note on second-order rules);
  [Wolfram Language: CellularAutomaton](../research/bibliography.md#wolframnd_cellularautomaton)
- [Kerekrety 2020](../research/bibliography.md#kerekrety2020elementary)
- [Golay 1969](../research/bibliography.md#golay1969hexagonal);
  [Packard and Wolfram 1985](../research/bibliography.md#packard1985twodimensional);
  [Frisch, Hasslacher and Pomeau 1986](../research/bibliography.md#frisch1986latticegas)
