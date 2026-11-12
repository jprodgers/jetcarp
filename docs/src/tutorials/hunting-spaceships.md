# 3. Hunting for spaceships

In this tutorial you hunt for spaceships in Conway's Life on a tiling made of triangles and
squares, look at the ship you find, check whether it is new, and then crash two copies of it into
each other. It takes about ten minutes, and the hunt itself a few seconds.

You need JETCARP installed ([Installing](../getting-started/installing.md)). Everything here runs
in a terminal. The [Searching for patterns](../guides/searching.md) guide covers every option met
here and more.

## 1. Choose a rule and a tiling

Life's rule, `B3/S23`, means a cell is born with exactly three live neighbours and survives with two
or three. On the square grid it has been searched for fifty years. On other tilings the same rule
behaves differently, because each tile has a different number and arrangement of neighbours.

This tutorial uses `k2-017`, one of the twenty 2-uniform tilings: a tiling of triangles and squares
with two kinds of vertex, `3³.4²` and `3².4.3.4`. The
[k-uniform tilings](../tilings/k-uniform.md) chapter explains the names, and
`jetcarp tilings` lists every tiling JETCARP knows.

## 2. Run a hunt

```text
$ jetcarp search hunt --tilings k2-017 --soups 5000 --label k2-017
hunt job 1 in /home/ada/.local/share/jetcarp/library.sqlite
  status done, 2 s run, 3357 settled, 788 died, 703 exploded, 152 unsettled
  B3/S23 on [3^3.4^2; 3^2.4.3.4] #2 (k2-017): 5000 soups, 13674 objects (oscillator 5824, spaceship 6900, still life 950), 20 distinct, 20 new
```

The hunt dropped 5,000 small random soups on an unbounded plane of this tiling and ran each until
it settled, died, exploded or ran out of time. It classed the 13,674 objects left behind, and
6,900 of them were spaceships. Every new pattern was verified exactly before it was stored in
your [pattern library](../guides/pattern-library.md).

## 3. Look at the ship

```text
$ jetcarp library list --class spaceship
16bc37c99a1397bc3ab3656a351b6008  c/4 orthogonal spaceship 6c #16bc37  B3/S23  [3^3.4^2; 3^2.4.3.4] #2 (k2-017)  p4  6c  seen 6900  …
```

All 6,900 spaceships were one ship. `jetcarp library show` gives its record; any unique start of
the id will do:

```text
$ jetcarp library show 16bc37
id:          16bc37c99a1397bc3ab3656a351b6008
label:       c/4 orthogonal spaceship 6c #16bc37
rule:        B3/S23 (Touch neighbours, 1 step(s) per generation)
tiling:      [3^3.4^2; 3^2.4.3.4] #2 (k2-017) (12 tile type(s) per unit cell)
class:       spaceship, period 4
works in:    B3/S23 to B3,5,6,8,9,10,11,12/S1,2,3,7,9,10,11,12 (8,192 B/S rules)
moves:       c/4 orthogonal : lattice vector (-1, 0) per period = (-1.932, 0.000) world units
cells:       6 (phases 6..9), bounding box 2x1
seen:        6900
…
verified:    16 periods (64 generations) on dead-edged frame 32x16 unit cells, …
```

Read it from the top:

- **Period 4, c/4.** Every four generations the ship returns to its own shape, one lattice step
  along. Its speed is a quarter of the fastest speed possible on this tiling, like Life's glider on
  squares.
- **6 to 9 cells.** Its four phases have between six and nine live tiles.
- **Works in 8,192 rules.** It behaves the same in every rule between `B3/S23` and
  `B3,5,6,8,9,10,11,12/S1,2,3,7,9,10,11,12`, because its tiles never see the neighbour counts where
  those rules differ.
- **Verified.** JETCARP ran it alone for 16 periods and checked that it came back to itself every
  time, moved by its displacement.

Draw its phases, larger than the default:

```text
$ jetcarp library render 16bc37 --png ship.png --px 240
```

To watch it move, make a scene with it in the middle and open that in the app:

```text
$ jetcarp library paste 16bc37 --size 120x40 --out ship.json
$ jetcarp --scene ship.json
```

## 4. Is it new?

The hunt says "new" when *your* library had not seen a pattern before. Whether anyone has seen it
before is a different question. Look the ship up in the book's
[Findings](../research/findings.md), which lists every ship JETCARP's own searches found and how
each was checked: `16bc37c9` is there, **flagged as common**. It turned up in 76% of soups on this
tiling, and the same ship runs unchanged on the 3-uniform tiling `k3-053`. A ship that easy to find
has very likely been seen by anyone who has run Life here, so JETCARP does not claim it as a
discovery. The checks a find must pass before it is called new are in
[What is new in JETCARP](../research/contributions.md).

To hunt for rarer ships, search more tilings and more rules. `--tilings kuniform:2` hunts all twenty
2-uniform tilings in one job, `--rules` takes several rules at once, and `--forever` keeps going
until you stop it. `jetcarp jobs report` then summarises what the job found, rule by rule and
tiling by tiling.

## 5. Crash two ships together

`jetcarp library paste` places library patterns on one grid. Each placement is
`ID@X,Y`, with an orientation `oK` if you want one. X and Y count tiles, not unit cells: the
120 × 40 unit-cell grid here is 1,440 tiles wide, because each row of a unit cell holds twelve
tiles. Put one ship a third of the way across, facing right (`o1`), and another two thirds across,
facing left:

```text
$ jetcarp library paste 16bc37@480,20,o1 16bc37@960,20 --size 120x40 --boundary dead --out meet.json
scene: meet.json (2 patterns, 1 CA(s), hash 65c7546103b51c38)
```

Run it and look at the population as they close in:

```text
$ jetcarp run meet.json --gens 70
…
field 0:    shared binary, 12 live: Life [B3/S23]
$ jetcarp run meet.json --gens 80
…
status:     Empty
field 0:    shared binary, 0 live: Life [B3/S23]
```

They meet at about generation 76 and destroy each other completely. Move the second ship down a
row or two (`16bc37@960,22`) and they pass each other unharmed instead. Open `meet.json` in the
app (`jetcarp --scene meet.json`) to watch it.

JETCARP can also try the collisions for you. This runs one ship into another at every offset from
−2 to 2 in each direction, and draws every outcome on one contact sheet; it takes a couple of
minutes:

```text
$ jetcarp search collide 16bc37 16bc37 --offsets -2..2 --phases 0 --gens 256 --sheet hits.png
outcomes: both_die 2, both_survive 10, new_objects 2, overlap 11
products: 1 distinct verified object(s), 0 new to the library
  8d1704ebc63a9486c11ef219d24376d1  still life 6c #8d1704 (from 2 run(s))
```

Two offsets destroy both ships, ten let both survive, eleven start with the ships overlapping, and
two leave a six-cell still life behind.

## Where to go from here

- **Aperiodic tilings.** Translations don't exist on the hat or the Penrose tilings, so their soup
  searches run on finite patches: `jetcarp search bounded --tilings hat --size L4 --rules B3/S23`.
- **Other rule families.** Generations rules (`B2/S/C3`) and Larger than Life rules have spaceships
  too.
- **Gliders by evolution.** `jetcarp search evolve` evolves rules until one carries a glider, as
  James Edmond did for the spectre.
- **Share what you find.** [Packs](../guides/packs.md) carry patterns to other people with
  everything needed to check them.
