# Tilings as neighbour graphs

A tiling covers the plane with tiles, leaving no gaps and no overlaps. For a cellular automaton, what
matters about a tiling is not the shapes of its tiles but who is next to whom: its *neighbour graph*.
JETCARP builds that graph for every tiling it supports, and every rule runs on the graph. This page
explains how the graph is made and why two very different-looking tilings can be the same thing as
far as an automaton is concerned.

<figure class="pair">
<div><img src="../images/tilings/graph-brick.png" alt="A brick and the six bricks around it" width="300"></div>
<div><img src="../images/tilings/graph-pyth.png" alt="A large square of the Pythagorean tiling and its eight neighbours" width="300"></div>
<div><img src="../images/tilings/graph-rhomb.png" alt="A Penrose rhomb and its ten touching neighbours" width="300"></div>
<figcaption>One tile (pale) and its touch neighbours on three tilings. A brick has six neighbours, like a
hexagon; a large square of the Pythagorean tiling has eight, four large and four small, though its
tiles do not meet edge to edge; this Penrose rhomb touches ten others. Scenes
<code>docs/scenes/tilings/graph-brick.json</code>, <code>graph-pyth.json</code> and
<code>graph-rhomb.json</code>, 1 generation of a rule that switches on every cell with exactly one live
neighbour.</figcaption>
</figure>

## What it is

The neighbour graph has one node for each tile and a link between every two tiles that are
neighbours. There are two kinds of neighbour (see
[How JETCARP generalises automata](../automata/generalising.md)):

- **Touch** neighbours share any part of their boundary: an edge or just a corner.
- **Edge** neighbours share a stretch of edge, not just a point.

A tile's number of neighbours is its *degree*. In a tiling with one kind of tile placed the same way
everywhere, every tile has the same degree; in mixed tilings the degree depends on the tile; in
aperiodic tilings it varies irregularly across the plane. The [tiling catalogue](catalogue.md) lists
the degrees of every tiling.

Many tilings that look different have the same graph. A brick wall in running bond has the same
neighbour graph as the hexagon tiling, and so does a herringbone pattern: each tile meets six others
in the same arrangement. Escher's lizards, on the three-fold hexagon tile, are hexagons as far as a
rule is concerned. Any rule that only counts neighbours behaves *identically* on tilings whose graphs
are the same, so a pattern found on one is a pattern on the other. This matters for honest claims.
Every isohedral tiling has the graph of one of the eleven Laves tilings, so JETCARP's 81 isohedral types
are only eleven worlds for a rule: 30 of them are the square grid, 19 the hexagonal grid, 8 the
triangular grid and 3 the Cairo tiling (see [Isohedral tilings](isohedral.md)). Spaceships first found
on isohedral tilings were Conway's glider and its relatives in a different drawing. Likewise the quadrilateral,
glide and four-fold families are squares and the Pythagorean tiling is 4.8.8. JETCARP compared all 362
combinations of its tiling families, isohedral types and 2- and 3-uniform tilings with both
neighbourhoods and found 196 distinct graphs
([Findings](../research/findings.md#isohedral-tilings-are-eleven-neighbour-graphs)).

Rules that turn, such as [Langton's ant](../automata/turmites.md), also need the *order* of a tile's
neighbours around it, so JETCARP keeps each tile's neighbours in clockwise order. And one family,
[Lenia](../automata/lenia.md), uses actual distances between tile centres, so for it the geometry
matters too.

## Where it came from

The standard reference on tilings is Branko Grünbaum and G. C. Shephard's *Tilings and Patterns*,
which defines the terms used here, such as edge-to-edge tilings and the symmetry groups of periodic
tilings ([Grünbaum and Shephard 1987](../research/bibliography.md#grunbaum1987tilings);
[2nd edition 2016](../research/bibliography.md#grunbaum2016tilings)). Treating the cells of an automaton
as the nodes of a graph is a standard idea; the earliest automata already defined their neighbourhoods
as fixed sets of nearby cells, named after John von Neumann and Edward F. Moore
([Moore 1962](../research/bibliography.md#moore1962machine)). General tools that run rules on arbitrary
polygon meshes, such as Ready, work the same way ([Ready](../research/bibliography.md#huttonnd_ready)).

## How JETCARP runs it

### Building the graph

JETCARP stores a tiling in one of three ways, depending on its kind:

| Storage | Used for | How neighbours are found |
| --- | --- | --- |
| Dense | lattices of one tile (squares, hexagons, bricks, any quadrilateral) | a fixed stencil of offsets |
| Periodic | a unit cell of several tiles repeated (Archimedean, k-uniform, isohedral, Cairo, herringbone, Pythagorean, drawn tiles) | a table of offsets for each tile in the cell |
| Explicit | finite patches of aperiodic tilings (Penrose, hat, spectre) | each tile's neighbour list, written out |

For tilings that are not edge to edge, such as herringbone, the Pythagorean tiling and tiles you draw
yourself, JETCARP measures contact from the geometry: two tiles are edge neighbours if they share a
stretch of boundary and touch neighbours if they share any point. The tests check tilings for
overlaps and gaps.

### Size and edges

A periodic world is a rectangle of whole unit cells, given either as a number of cells (roughly
square) or as a width and height in unit cells. Its edges can be *dead* (cells beyond count as empty),
*wrapped* (a torus, so the tiling has no edges at all) or *live* (cells beyond count as alive). An
aperiodic world is a patch of a chosen *level*, from 10 tiles at level 1 to about 35,000 at level 5 and
about 18 million at level 8, always the tiles nearest a fixed centre, so smaller patches sit inside
larger ones. Aperiodic patches have dead edges.

### Different tilings, same rule

Because rules only see the graph, any scene can switch tiling and keep its rules (apart from the few
families tied to one grid; see [How JETCARP generalises automata](../automata/generalising.md)).
`jetcarp tilings` and `jetcarp isohedral` print the degrees JETCARP measures.

## What is new here

Nothing on this page is new as an idea. What JETCARP adds is breadth: the same graph model for the
regular, Archimedean, k-uniform, isohedral, drawn and aperiodic tilings, so that every rule runs on all
of them. JETCARP's exploratory search also treats the tiling itself as one of the dimensions it
explores; see [What is new in JETCARP](../research/contributions.md).

## Try it

- `jetcarp tilings` lists every tiling family with its edge and touch degrees.
- Run the same rule on the brick tiling and on hexagons: it behaves the same way on both, because
  the graphs are the same.
- `jetcarp run docs/scenes/tilings/graph-rhomb.json --gens 1 --png rhomb.png`

## References

- [Grünbaum and Shephard 1987](../research/bibliography.md#grunbaum1987tilings);
  [Grünbaum and Shephard 2016](../research/bibliography.md#grunbaum2016tilings)
- [Moore 1962](../research/bibliography.md#moore1962machine);
  [Ready](../research/bibliography.md#huttonnd_ready)
