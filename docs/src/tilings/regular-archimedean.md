# Regular and Archimedean tilings

The regular tilings, by triangles, squares or hexagons alone, are the grids most cellular automata
were born on. Add the eight Archimedean tilings, which mix regular polygons so that every corner looks
the same, and you have the eleven *uniform* tilings of the plane. Each gives a rule a different
neighbour graph: some tiles see three neighbours, some eighteen, and in the mixed tilings
neighbouring tiles see different numbers. JETCARP runs every rule on all eleven.

<div class="thumbs">
<figure><img src="../images/tilings/thumbs/k1-011.png" alt="3<sup>6</sup> triangular" width="140"><figcaption>3<sup>6</sup> triangular<br><code>triangle</code></figcaption></figure>
<figure><img src="../images/tilings/thumbs/k1-005.png" alt="4<sup>4</sup> square" width="140"><figcaption>4<sup>4</sup> square<br><code>square</code></figcaption></figure>
<figure><img src="../images/tilings/thumbs/k1-001.png" alt="6<sup>3</sup> hexagonal" width="140"><figcaption>6<sup>3</sup> hexagonal<br><code>hex</code></figcaption></figure>
<figure><img src="../images/tilings/thumbs/k1-007.png" alt="3.6.3.6 trihexagonal" width="140"><figcaption>3.6.3.6 trihexagonal<br><code>trihex</code></figcaption></figure>
<figure><img src="../images/tilings/thumbs/k1-002.png" alt="4.8.8 truncated square" width="140"><figcaption>4.8.8 truncated square<br><code>oct</code></figcaption></figure>
<figure><img src="../images/tilings/thumbs/k1-004.png" alt="3.12.12 truncated hexagonal" width="140"><figcaption>3.12.12 truncated hexagonal<br><code>trunchex</code></figcaption></figure>
<figure><img src="../images/tilings/thumbs/k1-003.png" alt="4.6.12 truncated trihexagonal" width="140"><figcaption>4.6.12 truncated trihexagonal<br><code>trunctrihex</code></figcaption></figure>
<figure><img src="../images/tilings/thumbs/k1-006.png" alt="3.4.6.4 rhombitrihexagonal" width="140"><figcaption>3.4.6.4 rhombitrihexagonal<br><code>rhombitrihex</code></figcaption></figure>
<figure><img src="../images/tilings/thumbs/k1-008.png" alt="3.3.3.4.4 elongated triangular" width="140"><figcaption>3.3.3.4.4 elongated triangular<br><code>elongtri</code></figcaption></figure>
<figure><img src="../images/tilings/thumbs/k1-009.png" alt="3.3.4.3.4 snub square" width="140"><figcaption>3.3.4.3.4 snub square<br><code>snubsq</code></figcaption></figure>
<figure><img src="../images/tilings/thumbs/k1-010.png" alt="3.3.3.3.6 snub hexagonal" width="140"><figcaption>3.3.3.3.6 snub hexagonal<br><code>snubhex</code></figcaption></figure>
</div>

<p style="font-size:0.9em">The eleven uniform tilings, named by their vertex configurations, with JETCARP's
family ids. Thumbnails from the <a href="catalogue.md">tiling catalogue</a>, drawn by JETCARP with each tile
in a random colour.</p>

## What it is

A tiling by regular polygons is *edge to edge* when tiles meet only along whole edges, corner to
corner. It is *uniform* (or *1-uniform*, *Archimedean*) when every vertex looks the same: the same
polygons, in the same order, meet at each corner. A vertex is described by listing the polygons around
it, so 4.8.8 means a square and two octagons, and 3.3.4.3.4 means triangle, triangle, square, triangle,
square. Repeated numbers can be written with exponents: 3<sup>6</sup> for six triangles,
3<sup>2</sup>.4.3.4 for the snub square.

There are exactly eleven: the three regular tilings 3<sup>6</sup>, 4<sup>4</sup> and 6<sup>3</sup>, and
the eight mixed ones 3.6.3.6 (trihexagonal, also called kagome), 4.8.8 (truncated square), 3.12.12
(truncated hexagonal), 4.6.12 (truncated trihexagonal), 3.4.6.4 (rhombitrihexagonal), 3.3.3.4.4
(elongated triangular), 3.3.4.3.4 (snub square) and 3.3.3.3.6 (snub hexagonal), which comes in two
mirror-image forms.

For an automaton the differences are large. With touch neighbours a triangle in the triangular tiling
has 12 neighbours, a hexagon 6; in 3.3.3.3.6 each hexagon touches 18 tiles and each triangle 9. In
3.12.12, touch and edge neighbours are the same (3 for a triangle, 12 for a dodecagon), because no two
tiles meet at a corner only. Rules that count neighbours respond to all of this.

<figure>
<img src="../images/tilings/archimedean-rhombitrihex.png" alt="Pale clusters of triangles, squares and hexagons scattered over the 3.4.6.4 tiling" width="720">
<figcaption>B2/S34 on the rhombitrihexagonal tiling 3.4.6.4, 200 generations after a random start.
Triangles touch 6 tiles, squares 8 and hexagons 12, so births and deaths happen at different rates on
each kind of tile. Scene <code>docs/scenes/tilings/archimedean-rhombitrihex.json</code>.</figcaption>
</figure>

## Where it came from

The regular tilings are ancient as patterns. The first systematic mathematical treatment of
edge-to-edge tilings by regular polygons, including the eight further Archimedean tilings, is Johannes
Kepler's in Book II of *Harmonices Mundi* (1619)
([Kepler 1619](../research/bibliography.md#kepler1619harmonices); English translation
[Kepler 1997](../research/bibliography.md#kepler1997harmony)). Darrah Chavey examined Kepler's results in
detail in his 1984 thesis ([Chavey 1984](../research/bibliography.md#chavey1984periodic)). Branko
Grünbaum and G. C. Shephard surveyed the subject "from Kepler to the present" in 1977 and in their book
*Tilings and Patterns*, the standard modern reference
([Grünbaum and Shephard 1977](../research/bibliography.md#grunbaum1977tilings);
[1987](../research/bibliography.md#grunbaum1987tilings)).

The dotted vertex notation (4.8.8) is commonly named after H. M. Cundy and A. P. Rollett's
*Mathematical Models* ([Cundy and Rollett 1961](../research/bibliography.md#cundy1961mathematical));
Grünbaum and Shephard use exponents. Later notations designed for generating tilings automatically
include the GomJau-Hogg notation ([Gomez-Jauregui et al. 2021](../research/bibliography.md#gomezjauregui2021gomjau)).

Cellular automata on these tilings have their own history. Carter Bays studied Life-like rules on the
triangular tiling ([Bays 1994](../research/bibliography.md#bays1994cellular)) and on hexagons and Cairo
pentagons ([Bays 2005](../research/bibliography.md#bays2005note)), and wrote an overview
([Bays 2009](../research/bibliography.md#bays2009cellular)). Hans Guttesen ran Life on six of the mixed
Archimedean tilings and found gliders on 3.6.3.6 and 3.3.4.3.4 and a glider gun on 3.6.3.6
([Guttesen 2026](../research/bibliography.md#guttesen2026game)).

## How JETCARP runs it

The triangle, square and hexagon tilings are *reshapeable* families (`triangle`, `square`, `hex`):
their tiles can be any triangle, any parallelogram or any of a family of hexagons, and the regular
shapes are the default (see [Editable tile families](editable-families.md)). The eight mixed tilings are
fixed families: `trihex`, `oct`, `trunchex`, `trunctrihex`, `rhombitrihex`, `elongtri`,
`snubsq` and `snubhex`. All eleven also appear in the [k-uniform catalogue](k-uniform.md) as its
k = 1 entries, `k1-001` to `k1-011`.

In a scene the tiling is given by its family id, a size and an edge rule, for example
`"tiling": {"family": "rhombitrihex", "size": {"cells": 9000}, "boundary": "wrap"}`. A flat-topped
version of the hexagon tiling, `hex_flat`, exists for the three-parent
[hexagonal row rules](../automata/hexagonal-rows.md).

## What is new here

The tilings are classical. On them, JETCARP's searches have found spaceships in Life-like rules on
4.8.8, 3.12.12, 4.6.12, 3.4.6.4, 3.3.3.4.4 and 3.3.3.3.6, which to our knowledge have not been reported
before; Guttesen's Life gliders on 3.6.3.6 and 3.3.4.3.4, which JETCARP also found, are his. See
[Life-like rules](../automata/life-like.md#what-is-new-here) for the details and cautions, and
[What is new in JETCARP](../research/contributions.md).

## Try it

- Quick Start, family *Tiling showcase*: a lively rule and a sandpile on each of the eleven.
- `jetcarp run docs/scenes/tilings/archimedean-rhombitrihex.json --gens 200 --png 3464.png`
- Run Life (`B3/S23`) on `trihex` from a random start and look for small gliders, as Guttesen did.

## References

- [Kepler 1619](../research/bibliography.md#kepler1619harmonices);
  [Kepler 1997](../research/bibliography.md#kepler1997harmony);
  [Chavey 1984](../research/bibliography.md#chavey1984periodic)
- [Grünbaum and Shephard 1977](../research/bibliography.md#grunbaum1977tilings);
  [Grünbaum and Shephard 1987](../research/bibliography.md#grunbaum1987tilings)
- [Cundy and Rollett 1961](../research/bibliography.md#cundy1961mathematical);
  [Gomez-Jauregui et al. 2021](../research/bibliography.md#gomezjauregui2021gomjau)
- [Bays 1994](../research/bibliography.md#bays1994cellular);
  [Bays 2005](../research/bibliography.md#bays2005note);
  [Bays 2009](../research/bibliography.md#bays2009cellular);
  [Guttesen 2026](../research/bibliography.md#guttesen2026game)
