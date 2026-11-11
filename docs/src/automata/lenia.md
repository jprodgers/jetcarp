# Lenia

Lenia is a continuous cellular automaton. Instead of being alive or dead, each cell holds a value
between 0 and 1; instead of counting a few neighbours, it takes a smooth weighted average over a
wide ring around it. The result looks less like a grid of pixels and more like a dish of
microorganisms: soft, round creatures that glide, rotate and divide. Bert Wang-Chak Chan, who
created it, catalogued hundreds of them and named them like biological species.

<figure>
<picture>
  <source srcset="../images/automata/lenia-orbium-still.png" media="(prefers-reduced-motion: reduce)">
  <img src="../images/automata/lenia-orbium.webp" alt="Orbium: a glowing crescent-shaped creature gliding across a wrapped square grid" width="384" loading="lazy">
</picture>
<figcaption><em>Orbium unicaudatus</em>, Lenia's best-known glider, for 600 generations from the middle
of a wrapped 64 × 48 grid. Watch it glide diagonally, keeping its shape, and come back in through the
opposite edges. Colour shows each cell's value, from dark (near 0) to pale yellow (near 1). Scene
<code>docs/scenes/automata/lenia-orbium.json</code>.</figcaption>
</figure>

## What it is

Each cell holds a value `A` between 0 and 1. Every step:

1. Each cell computes a **potential** `U`, a weighted average of the values around it. The weights
   form a *kernel* `K`: a smooth ring of radius `R` cells (or several concentric rings), normalised
   so the weights add up to 1.
2. A **growth function** `G` turns the potential into a change between −1 and +1. It is positive
   when `U` is close to a preferred value `μ` (within a width `σ`) and negative otherwise.
3. The value moves by a small time step, `A ← clip(A + G(U) / T, 0, 1)`, where `T` sets how many
   steps make one unit of time.

Life is a very coarse version of the same recipe: its "kernel" is the eight neighbours and its
"growth function" is the birth and survival lists. Lenia smooths out space, states and time, and the
creatures it supports reflect that: they look like living things rather than machines.

## Where it came from

Bert Wang-Chak Chan introduced Lenia in "Lenia — Biology of Artificial Life", posted in December 2018
and published in *Complex Systems* in 2019. The paper reports over 400 "species" in 18 families, gives
them Linnaean-style names such as *Orbium* and *Scutium*, and studies their form and behaviour the way
a biologist would ([Chan 2019](../research/bibliography.md#chan2019lenia)).

Chan places Lenia at the end of a line of generalisations of Life: Life itself
([Gardner 1970](../research/bibliography.md#gardner1970life)), Chan's own multi-state program
"Primordia" from around 2005, Larger than Life ([Evans 2001](../research/bibliography.md#evans2001larger);
[Evans 2003](../research/bibliography.md#evans2003larger); see [Larger than Life](larger-than-life.md)),
Marcus Pivato's RealLife, the continuum limit of Larger than Life
([Pivato 2007](../research/bibliography.md#pivato2007reallife)), and Stephan Rafler's SmoothLife
([Rafler 2011](../research/bibliography.md#rafler2011generalization)). Chan writes that SmoothLife and
Lenia were developed independently and look strikingly alike, an example of what Chan calls convergent
evolution, and traces a "phylogeny of the glider" from Life's glider through Larger than Life's bugs
and SmoothLife's smooth glider to Lenia's creatures ([Chan 2019](../research/bibliography.md#chan2019lenia)).

Chan later extended Lenia to more dimensions, kernels and channels
([Chan 2020](../research/bibliography.md#chan2020expanded)), and Flow-Lenia made it conserve mass
([Plantec et al. 2022](../research/bibliography.md#plantec2022flowlenia)). Chan's reference code, in
several languages, is open source under the MIT licence ([Chan, Lenia code](../research/bibliography.md#chakazulnd_lenia)).
Lenia has also become a test bed for automated discovery: intrinsically motivated goal exploration
with learned goal spaces ([Reinke, Etcheverry and Oudeyer 2020](../research/bibliography.md#reinke2020intrinsically)),
quality-diversity search with MAP-Elites ([Faldor and Cully 2024](../research/bibliography.md#faldor2024toward))
and search guided by foundation models ([Kumar et al. 2025](../research/bibliography.md#kumar2025automating)).

## How JETCARP runs it

### Notation

JETCARP uses Chan's parameters and the numbering of kernels and growth functions from Chan's code:

| Parameter | Meaning |
| --- | --- |
| `R` | kernel radius, in cells |
| `T` | time resolution: each step moves values by `G / T` |
| `b` | kernel peaks, one per ring (`b=1` for one ring, `b=1/2,1` for two) |
| `m`, `s` | the growth function's centre μ and width σ |
| `kn` | kernel core: 1 polynomial (default), 2 exponential, 3 step, 4 staircase |
| `gn` | growth function: 1 polynomial (default), 2 Gaussian, 3 step |

`Lenia R=13,T=10,b=1,m=0.15,s=0.015` is Orbium's rule. The *Orbium unicaudatus* pattern itself comes
from the `animals.json` file in Chan's repository, used under its MIT licence; the seed
`{"lenia": {"name": "orbium"}}` places it.

### On any tiling

Lenia's kernel is a ring in the plane, so JETCARP measures it with straight-line distances between
tile centres, in units of the mean tile size (the square root of the mean tile area). On squares this
is exactly Chan's kernel. On hexagons, triangles and every Archimedean, k-uniform and isohedral tiling
the kernel is precomputed per tile type; on aperiodic patches each tile gets its own list of kernel
weights, and cells beyond the patch edge count as 0. This is the one family where JETCARP uses
geometric distance instead of steps in the neighbour graph (see
[How JETCARP generalises automata](generalising.md)).

<figure>
<picture>
  <source srcset="../images/automata/lenia-hat-still.png" media="(prefers-reduced-motion: reduce)">
  <img src="../images/automata/lenia-hat.webp" alt="Lenia on the hat tiling: a dense random start breaking up into worm-like loops" width="360" loading="lazy">
</picture>
<figcaption>Orbium's rule on the middle of a patch of about 35,000 hat tiles, 300 generations after a
dense random blob: watch the blob break up into worms and loops that settle into slowly shifting
shapes. Lenia measures distances between tile centres, so the hats' geometry, not just their
neighbour graph, shapes the result. Scene <code>docs/scenes/automata/lenia-hat.json</code>.</figcaption>
</figure>

### Reproducible to the last bit

Lenia's values are floating-point numbers, and the usual maths library functions can differ in their
last bit from one operating system to another. JETCARP computes Lenia's exponentials, powers and
distances with its own portable arithmetic, using only operations that IEEE 754 defines exactly, and
sums each cell's kernel in a fixed order. The state hash includes every value's exact bits, so a
Lenia scene replays to the same hash on any machine and any number of threads, and the tests pin
hashes to check it.

## What is new here

Lenia is Chan's. Running it on other tilings by measuring the kernel in the plane is a natural step,
and we make no novelty claim for it.

## Try it

- Quick Start: *Orbium (Lenia)*.
- `jetcarp run docs/scenes/automata/lenia-hat.json --gens 300 --png lenia-hat.png`
- Try Orbium's rule on hexagons or triangles from a random blob, and adjust `m` and `s` until
  something survives.

## References

- [Chan 2019](../research/bibliography.md#chan2019lenia);
  [Chan 2020](../research/bibliography.md#chan2020expanded);
  [Plantec et al. 2022](../research/bibliography.md#plantec2022flowlenia);
  [Chan, Lenia code](../research/bibliography.md#chakazulnd_lenia)
- [Reinke, Etcheverry and Oudeyer 2020](../research/bibliography.md#reinke2020intrinsically);
  [Faldor and Cully 2024](../research/bibliography.md#faldor2024toward);
  [Kumar et al. 2025](../research/bibliography.md#kumar2025automating)
- [Rafler 2011](../research/bibliography.md#rafler2011generalization);
  [Pivato 2007](../research/bibliography.md#pivato2007reallife);
  [Evans 2001](../research/bibliography.md#evans2001larger);
  [Evans 2003](../research/bibliography.md#evans2003larger);
  [Gardner 1970](../research/bibliography.md#gardner1970life)
