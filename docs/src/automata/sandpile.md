# The Abelian sandpile

The sandpile is a toy model of avalanches. Drop grains of sand on a grid one at a time; when a cell
holds too many, it topples and passes one grain to each neighbour, which may make them topple in
turn. The model became famous as the first example of *self-organised criticality*, and turned out to
hide a beautiful piece of algebra: the stable piles form a group, whose identity element is a
striking fractal picture. In JETCARP a tile topples when it holds as many grains as it has
neighbours, so the model runs on any tiling, with a different threshold on every tile where tiles
differ.

<figure>
<img src="../images/automata/sandpile-identity-cairo.png" alt="A four-fold symmetric pattern of coloured pentagons with large blue regions" width="640">
<figcaption>The identity element of the sandpile group on a patch of about 8,000 Cairo pentagons with
grains lost at the edges. Each colour is a number of grains, from 0 to 4; a pentagon topples at 5.
Scene <code>docs/scenes/automata/sandpile-identity-cairo.json</code>.</figcaption>
</figure>

## What it is

Each cell holds a whole number of grains. A cell with at least as many grains as it has neighbours is
*unstable*: it **topples**, giving one grain to each neighbour and keeping the rest. Grains that would
go past the edge of a finite grid are lost; the edge is a *sink*. Toppling continues until every cell
is stable.

Two facts make the model special:

- **Order does not matter.** However the topplings are ordered, the final stable pile is the same.
  This is the *abelian* property that gives the model its name, and it means adding grains and then
  stabilising behaves like addition.
- **The recurrent piles form a group.** The stable piles that keep coming back when grains are added
  at random form a finite abelian group, the *sandpile group*. Every group has an identity element:
  here, the one recurrent pile that, added to any recurrent pile and stabilised, changes nothing. On a
  square grid this identity is an intricate fractal, and its shape depends on the shape of the grid.

A different classic experiment drops a great many grains on a single cell. The result is a large,
roughly round pile with a fractal pattern inside that looks the same at every scale.

<figure class="pair">
<div><img src="../images/automata/sandpile-pile-k2.png" alt="A twelve-sided sandpile on a 2-uniform tiling" width="480"></div>
<div><img src="../images/automata/sandpile-pile-hat.png" alt="A round sandpile on the hat tiling" width="480"></div>
<figcaption>Single-source piles. Left: 20,000 grains on one tile of the 2-uniform tiling
[3.4.6.4; 4.6.12], where triangles, squares, hexagons and dodecagons topple at 3, 4, 6 and 12 grains;
the pile grows a twelve-sided outline. Scene <code>docs/scenes/automata/sandpile-pile-k2.json</code>.
Right: 10,000 grains on the hat tiling, where tiles topple at 4, 5 or 6. Scene
<code>docs/scenes/automata/sandpile-pile-hat.json</code>.</figcaption>
</figure>

## Where it came from

Per Bak, Chao Tang and Kurt Wiesenfeld proposed the sandpile in 1987 as an explanation of how systems
can organise themselves into a critical state, in which avalanches of every size occur and their sizes
follow a power law ([Bak, Tang and Wiesenfeld 1987](../research/bibliography.md#bak1987selforganized);
[1988](../research/bibliography.md#bak1988selforganized)). Deepak Dhar showed in 1990 that the final
state does not depend on the order of topplings, so that adding grains defines commuting operators
and the recurrent configurations form an abelian group
([Dhar 1990](../research/bibliography.md#dhar1990selforganized)); Lionel Levine and James Propp credit
the name "sandpile group" to Dhar ([Levine and Propp 2010](../research/bibliography.md#levine2010sandpile)).
Majumdar and Dhar later linked the model to spanning trees and the q → 0 Potts model
([Majumdar and Dhar 1992](../research/bibliography.md#majumdar1992equivalence)).

The same dynamics have been invented several times under other names, including chip-firing; in
combinatorics the main early reference is Björner, Lovász and Shor's "Chip-firing games on graphs"
([1991](../research/bibliography.md#bjorner1991chipfiring); see
[Levine and Propp 2010](../research/bibliography.md#levine2010sandpile)).

Michael Creutz drew attention to the identity element and its fractal structure on the square grid
([Creutz 1991](../research/bibliography.md#creutz1991abelian)); Caracciolo, Paoletti and Sportiello
call it the Creutz identity ([Caracciolo, Paoletti and Sportiello 2008](../research/bibliography.md#caracciolo2008explicit)),
and Le Borgne and Rossin studied it combinatorially
([Le Borgne and Rossin 2002](../research/bibliography.md#leborgne2002identity)). Single-source piles on
the square grid were studied by Srdjan Ostojic ([Ostojic 2003](../research/bibliography.md#ostojic2003patterns)),
and Wesley Pegden and Charles Smart proved that the rescaled piles converge to a limit
([Pegden and Smart 2013](../research/bibliography.md#pegden2013convergence)). Off the square grid,
Fersula, Noûs and Perrot computed identities on triangular and hexagonal grids and on Penrose tilings
([Fersula, Noûs and Perrot 2020](../research/bibliography.md#fersula2020sandpile)).

## How JETCARP runs it

### The rule on any tiling

The rule is written `Sandpile`. A tile topples when its number of grains reaches its number of
neighbours, so the threshold is the tile's degree in the neighbour graph: 4 on squares and 6 on
hexagons with edge neighbours, but anything from 3 to 12 on a 2-uniform tiling and 4 to 7 on the
spectre. Sandpiles use edge neighbours by default; with touch neighbours a square topples at 8. Dead
edges, and the open boundary of an aperiodic patch, are sinks. A wrapped grid has no sink, so it has
no identity and cannot absorb more grains than it can hold.

Starting piles include `{"pile": {"grains": N}}` (N grains on the centre tile) and random heights;
`steps_per_gen` sets how many toppling sweeps make one generation, so you can watch an avalanche
spread or let it settle quickly.

A sandpile world runs on the GPU in the app and in the background host, on lattices and aperiodic
patches, with exactly the CPU's grain counts, grains dropped and grains lost. A script step that
waits for the pile to be *done* runs on the CPU, which checks whether any tile can still topple.

### The identity

JETCARP computes the identity of the sandpile group as stab(2m − stab(2m)), where m is the pile in
which every tile holds one grain fewer than its threshold and stab means "topple until stable". Its
tests check that the result is idempotent, stab(I + I) = I, on squares, hexagons, Cairo pentagons and
the spectre. The app's *Identity* button and the scene command `{"op": "sandpile", "action":
"identity"}` compute it on any tiling with sink edges.

### Very large piles

`jetcarp pile` drops N grains on one tile of an *unbounded* grid. It never builds a tower of N grains:
it reads N's binary digits from the most significant end and repeats s ← stab(2s + bit · δ), where δ is
one grain on the centre tile. The abelian property makes the result the same as stabilising all N
grains at once, while heights stay small at every step. The board grows as the pile does, so no grain
is ever lost, and on aperiodic tilings the pile is built on an exact patch of up to about 17 million
tiles. Piles can be toppled on the GPU on periodic tilings, and are saved after every doubling step so
a long run can be resumed. The app's *Sandpile…* window builds one too, as a catalog job: it draws
the pile as it grows, and a stopped pile carries on from where it stopped (in the middle of a step
too), from the window, the *Hunts…* window, or `jetcarp jobs resume`. See the guide [Large sandpiles](../guides/large-sandpiles.md).

## What is new here

Sandpile identities have been computed before on triangular, hexagonal and Penrose grids (see above),
and on hexagonal and 4.8.8 grids in Eli T. Johnson's master's thesis
([Johnson 2019](../research/bibliography.md#johnson2019sandpile)); single-source piles on the square
grid have a long literature. To our knowledge, JETCARP is the first to show sandpile identities and
large single-source piles on the hat and spectre monotiles, on the k-uniform tilings with k of 2 or
more, and on the Laves tilings other than the regular grids. The last are what the isohedral tilings
amount to: a sandpile sees only the neighbour graph, and the 81 isohedral types have only the eleven
graphs of the Laves tilings (see [Isohedral tilings](../tilings/isohedral.md)), so an identity on an
isohedral type is an identity on its Laves tiling. Our search found no published sandpile work on the
hat or spectre at all. The
doubling method follows directly from the abelian property and is presumably known, so we do not
claim it. The search is described in [What is new in JETCARP](../research/contributions.md).

## Try it

- Quick Start: *Abelian sandpile*, *Sandpile identity on Cairo pentagons*, *Sandpile on the Spectre*,
  *Sandpile on hexagons*, and a sandpile on every tiling in the *Tiling showcase*.
- `jetcarp run docs/scenes/automata/sandpile-identity-cairo.json --png identity.png`
- `jetcarp pile --tiling spectre --grains 1e6` builds a million-grain pile on the spectre.

## References

- [Bak, Tang and Wiesenfeld 1987](../research/bibliography.md#bak1987selforganized);
  [Bak, Tang and Wiesenfeld 1988](../research/bibliography.md#bak1988selforganized);
  [Dhar 1990](../research/bibliography.md#dhar1990selforganized);
  [Majumdar and Dhar 1992](../research/bibliography.md#majumdar1992equivalence)
- [Björner, Lovász and Shor 1991](../research/bibliography.md#bjorner1991chipfiring);
  [Levine and Propp 2010](../research/bibliography.md#levine2010sandpile)
- [Creutz 1991](../research/bibliography.md#creutz1991abelian);
  [Le Borgne and Rossin 2002](../research/bibliography.md#leborgne2002identity);
  [Caracciolo, Paoletti and Sportiello 2008](../research/bibliography.md#caracciolo2008explicit)
- [Ostojic 2003](../research/bibliography.md#ostojic2003patterns);
  [Pegden and Smart 2013](../research/bibliography.md#pegden2013convergence)
- [Fersula, Noûs and Perrot 2020](../research/bibliography.md#fersula2020sandpile)
