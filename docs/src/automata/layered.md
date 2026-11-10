# Layered rules

A layered rule is an elementary-style rule that grows outward from a seed instead of downward from
a top row. The "rows" are the layers of the tiling's neighbour graph around the seed: the seed, then
every tile next to it, then every tile next to those, and so on. Because any tiling has layers in
this sense, the same rule runs on squares, Cairo pentagons and the aperiodic spectre alike.

<figure>
<img src="../images/automata/layered-spectre.png" alt="A branching, fractal-looking star grown from one tile of the spectre tiling, coloured in a rainbow from top to bottom" width="720">
<figcaption><code>Layer 2/01</code> grown from one tile of a patch of about 35,000 spectres: each tile in a
new layer is the parity of its parents. Coloured by position from top to bottom. Scene
<code>docs/scenes/automata/layered-spectre.json</code>, run until the last layer is written.</figcaption>
</figure>

## What it is

Pick one or more seed tiles. The **layers** are the sets of tiles at the same distance from the
seeds, counting steps from neighbour to neighbour: layer 0 is the seeds, layer 1 the tiles that
touch them, and so on. Each generation writes the next layer.

A tile's **parents** are its neighbours in the layer before. A layered rule is *totalistic*: the
new tile's state depends only on the sum of its parents' states. The rule is a list of outputs,
one for each sum, and the list repeats, so the output for sum `s` is entry `s` mod the list's length:

- `Layer 2/01`: two states; an even sum gives 0 and an odd sum gives 1. Each tile is the parity
  (XOR) of its parents.
- `Layer 3/012`: three states; each tile is the sum of its parents mod 3.

The totalistic form is what makes this work on any tiling. On squares every cell has the same
number of parents, but on a Cairo or spectre tiling the number of parents varies from tile to tile,
and a rule that depends only on the sum stays well defined.

On the square grid with edge neighbours the layers are diamonds, and `Layer 2/01` draws Pascal's
triangle mod 2 in each quarter, the same pattern rule 90 draws from one cell
([Martin, Odlyzko and Wolfram 1984](../research/bibliography.md#martin1984algebraic)). On other
tilings the same rule draws patterns that have no simple formula.

<figure class="pair">
<div><img src="../images/automata/layered-square.png" alt="Nested Sierpinski triangles arranged in a diamond on squares" width="400"></div>
<div><img src="../images/automata/layered-cairo.png" alt="A four-fold pattern of triangles in orange and blue on Cairo pentagons" width="400"></div>
<figcaption>Left: <code>Layer 2/01</code> on squares with edge neighbours, grown from the centre; the
layers are diamonds and each quarter holds Pascal's triangle mod 2. Scene
<code>docs/scenes/automata/layered-square.json</code>. Right: <code>Layer 3/012</code> on Cairo
pentagons with edge neighbours, the sum of the parents mod 3. Scene
<code>docs/scenes/automata/layered-cairo.json</code>. Both run until the last layer is written.</figcaption>
</figure>

## Where it came from

Layered rules are Jimmie Rodgers's construction, introduced in JETCARP in 2026. They combine
the row-by-row picture of Wolfram's one-dimensional rules
([Wolfram 1983](../research/bibliography.md#wolfram1983statistical)) with a tiling's neighbour graph.

The closest earlier work we found carries one-dimensional rules into other tilings in different
ways. Maurice Margenstern embedded one-dimensional automata in hyperbolic tilings along a chosen line
of tiles ([Margenstern 2010](../research/bibliography.md#margenstern2010embedding)), and Matej
Kerekrety's web tool draws elementary rules row by row on hexagons and pentagons, using the
lattice's fixed rows ([Kerekrety 2020](../research/bibliography.md#kerekrety2020elementary)).
Neither uses graph-distance layers from a seed.

A note on the name: Abhishek Dalai's "Layered Cellular Automata"
([Dalai 2023](../research/bibliography.md#dalai2023layered)) is a different idea, two coupled layers
of computation, and is unrelated to JETCARP's layered rules.

## How JETCARP runs it

### Notation

`Layer <states>/<outputs>`, where the outputs are listed for sums 0, 1, 2, … and repeat:

| Notation | Rule |
| --- | --- |
| `Layer 2/01` | Parity: rule 90 grown on any tiling |
| `Layer 3/012` | Sum mod 3: Pascal's triangle mod 3 on any tiling |
| `Layer 2/0110` | Output 1 for sums 1 and 2, 0 for sums 0 and 3, repeating every four |

### Seeds, neighbours and edges

The layers are computed once, by a breadth-first search over the automaton's neighbourhood (touch
or edge), from every cell that is non-zero when the rule first runs. With no live cells it starts
from the centre tile and switches it on. Several seeds give layers that grow from all of them at
once and meet in the middle. The run is *done* when the last layer has been written; on a finite
patch that is when the layers reach its edge.

Touch and edge neighbourhoods give different layers. On squares, touch layers are square rings and
edge layers are diamonds; on most other tilings the two differ in more irregular ways, and so do
the pictures.

### Colour

By default each tile is coloured by state. Colour by row (`"color_by": "row"`) shades tiles by
their height in the picture instead, as in the spectre figure above.

## What is new here

To our knowledge, layered rules are a new presentation: Wolfram-style totalistic rules whose rows
are the graph-distance shells around a seed, so that they run unchanged on any tiling, including
aperiodic ones. We found no earlier source that defines them or runs them on Penrose, hat or
spectre tilings. The idea is simple enough that it may exist unpublished, for example in hobby code
or in work on rules over graphs that our search did not reach, so we claim only that we have not
seen it published. The search is described in [What is new in JETCARP](../research/contributions.md).

## Try it

- Quick Start: *Layered parity on the Spectre*, *Layered parity* and *Layered sum mod 3* (both on
  Cairo pentagons).
- `jetcarp run docs/scenes/automata/layered-spectre.json --gens 400 --png spectre-layers.png`
- Seed a few cells by hand with the Draw tool before starting a layered rule: the layers then grow
  from all of them.

## References

- [Wolfram 1983](../research/bibliography.md#wolfram1983statistical);
  [Martin, Odlyzko and Wolfram 1984](../research/bibliography.md#martin1984algebraic)
- [Margenstern 2010](../research/bibliography.md#margenstern2010embedding);
  [Kerekrety 2020](../research/bibliography.md#kerekrety2020elementary)
- [Dalai 2023](../research/bibliography.md#dalai2023layered) (unrelated, same name)
