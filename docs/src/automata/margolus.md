# Margolus block rules

Most cellular automata update each cell from its neighbourhood. Block rules work differently: the
grid is cut into 2 × 2 blocks, every block is replaced as a whole by looking it up in a table, and
the cut moves by one cell between steps so that information can cross block boundaries. This
arrangement, the *Margolus neighbourhood*, makes it easy to build rules that are reversible and
conserve particles, which is why it was invented: to make physics-like models of computation.

<figure>
<picture>
  <source srcset="../images/automata/margolus-tron-still.png" media="(prefers-reduced-motion: reduce)">
  <img src="../images/automata/margolus-tron.webp" alt="Tron: nested rectangles of coloured stripes spreading from a small seed" width="400" loading="lazy">
</picture>
<figcaption>Tron (<code>Margolus Tron</code>) for 100 generations after a small random blob, two
generations a frame. A block whose four cells agree is complemented, so the background flashes every
step (only even generations are drawn, to spare the eyes), and the blob grows into nested rectangles
that fill the grid with stripes. Colour shows age. Scene
<code>docs/scenes/automata/margolus-tron.json</code>.</figcaption>
</figure>

## What it is

The square grid is partitioned into 2 × 2 blocks. On even steps the blocks start at even
coordinates; on odd steps they start at odd coordinates, so each new block straddles four old ones.
A block of four binary cells has 16 possible states, and a block rule is a table giving the new
state for each of them.

Two properties are easy to see in the table:

- The rule is **reversible**, meaning every state has exactly one predecessor and the evolution can
  be run backwards, exactly when the table is a one-to-one map of the 16 block states.
- The rule **conserves particles** when every block keeps its number of live cells.

Some well-known block rules:

- **Billiard ball machine.** A lone ball (live cell) crosses its block to the opposite corner; two
  balls on a diagonal leave along the other diagonal; everything else stays. Balls fly diagonally
  and bounce off each other like hard spheres, which is enough to build logic gates.
- **Critters.** A block with exactly two live cells stays; otherwise every cell is complemented, and
  a block that had three live cells is also turned half a turn. Reversible, and full of gliders.
- **Tron.** A block whose four cells agree is complemented. Rectilinear patterns grow from any seed.
- **HPP gas** (in its diagonal Margolus form). Every particle crosses its block to the opposite
  corner, except that two particles on a diagonal leave along the other diagonal: a toy gas.
- **Single Rotation.** A block with exactly one live cell turns 90 degrees clockwise; all others stay.
  A remarkably simple reversible rule with many spaceships.

<figure>
<picture>
  <source srcset="../images/automata/margolus-rotation-still.png" media="(prefers-reduced-motion: reduce)">
  <img src="../images/automata/margolus-rotation.webp" alt="Single Rotation: a small cloud of cells spreading out as particles fly away" width="400" loading="lazy">
</picture>
<figcaption>Single Rotation (<code>Margolus SingleRotation</code>) for 400 generations after a small
random blob: the blob loosens into a gas, and spaceships leave the cloud in several directions. The rule
is reversible, so the cloud never settles. Scene
<code>docs/scenes/automata/margolus-rotation.json</code>.</figcaption>
</figure>

## Where it came from

Edward Fredkin and Tommaso Toffoli's "Conservative logic" (1982) set out reversible, conservative
computation, including a billiard-ball model in which logic signals are hard balls and gates are
their elastic collisions ([Fredkin and Toffoli 1982](../research/bibliography.md#fredkin1982conservative)).
Norman Margolus, in the Information Mechanics Group at MIT with Toffoli, found a very simple
reversible automaton that runs the billiard-ball model, using the partitioning scheme now named after
Margolus ([Margolus 1984](../research/bibliography.md#margolus1984physicslike)). Margolus later wrote that the
partitioning technique was first developed for that purpose, and that Toffoli had built the first
universal invertible automaton earlier by another route
([Margolus 1999](../research/bibliography.md#margolus1999crystalline)).

Toffoli and Margolus built dedicated cellular automaton hardware at MIT, the CAM machines, and wrote
*Cellular Automata Machines* (1987) around the CAM-6; a whole chapter is devoted to the Margolus
neighbourhood ([Toffoli and Margolus 1987](../research/bibliography.md#toffoli1987cellular)). Critters
and Tron are usually credited to that book. Margolus describes Critters as a universal invertible
automaton that evolves interesting complexity ([Margolus 1999](../research/bibliography.md#margolus1999crystalline)).
Toffoli and Margolus reviewed invertible automata in 1990
([Toffoli and Margolus 1990](../research/bibliography.md#toffoli1990invertible)).

The HPP lattice gas is older than the Margolus neighbourhood: J. Hardy, Y. Pomeau and O. de Pazzis
introduced it on the square lattice in 1973 and analysed it in 1976
([Hardy, Pomeau and de Pazzis 1973](../research/bibliography.md#hardy1973time);
[1973b](../research/bibliography.md#hardy1973timeevolution);
[Hardy, de Pazzis and Pomeau 1976](../research/bibliography.md#hardy1976molecular)). Margolus recalls
that the MIT group rediscovered it about a decade later
([Margolus 1999](../research/bibliography.md#margolus1999crystalline)). The hexagonal gas of Frisch,
Hasslacher and Pomeau removed its unphysical extra conservation laws
([Frisch, Hasslacher and Pomeau 1986](../research/bibliography.md#frisch1986latticegas)), and Margolus,
Toffoli and Vichniac described automaton hardware for fluid dynamics the same year
([Margolus, Toffoli and Vichniac 1986](../research/bibliography.md#margolus1986cellularautomata)).

The Single Rotation rule was posted in November 2013 on the blog *Random thoughts and fancy math* by
its author, who writes as dmishin; the post doubts that so simple a rule had not been found before
but notes that its author had not seen it elsewhere
([dmishin 2013](../research/bibliography.md#dmishin2013single)).

## How JETCARP runs it

### Notation

| Notation | Meaning |
| --- | --- |
| `Margolus 0,8,4,3,2,5,9,7,1,6,10,11,12,13,14,15` | The billiard ball machine, as a table of 16 outputs |
| `Margolus BBM`, `Margolus Critters`, `Margolus Tron`, `Margolus SingleRotation`, `Margolus HPP` | The built-in tables by name |
| `MS,D0;8;4;3;2;5;9;7;1;6;10;11;12;13;14;15` | MCell's form of the same table |

The table lists the new state of the block for block states 0 to 15, numbering the cells as MCell
does: upper left 1, upper right 2, lower left 4, lower right 8. The named tables are built in
JETCARP's code from the published descriptions of each rule, not copied from rule files, and were
checked against those descriptions.

### Square grid only

Block rules depend on cutting the grid into 2 × 2 squares on two alternating offsets, which has no
natural counterpart on other tilings, so they run on the square grid only (see
[How JETCARP generalises automata](generalising.md)). Which offset comes next is part of the saved
state, so scenes replay exactly. With wrapped edges the blocks wrap across the seam when a side has
even length; with dead or live edges, or an odd side, a block that would cross the edge is left
unchanged, so conserving rules still conserve. Use even sides for a true torus.

### Checks

JETCARP's tests run Critters forward 101 generations and then backward with the inverse table, and
recover the starting pattern exactly; check that the billiard ball machine keeps its number of balls;
and check the lone ball, the Single Rotation square and Tron's flashing background. Block rules run on
the CPU.

## What is new here

Nothing. These rules, and the idea of the Margolus neighbourhood, belong to the people credited above.

## Try it

- Quick Start: *Billiard ball machine*, *Critters*, *Tron*, *Single Rotation* and *HPP gas (Margolus)*.
- `jetcarp run docs/scenes/automata/margolus-tron.json --gens 101 --png tron.png`
- Draw a single cell in Single Rotation and watch it circle a 2 × 2 square with period 4.

## References

- [Fredkin and Toffoli 1982](../research/bibliography.md#fredkin1982conservative);
  [Margolus 1984](../research/bibliography.md#margolus1984physicslike);
  [Margolus 1999](../research/bibliography.md#margolus1999crystalline)
- [Toffoli and Margolus 1987](../research/bibliography.md#toffoli1987cellular);
  [Toffoli and Margolus 1990](../research/bibliography.md#toffoli1990invertible)
- [Hardy, Pomeau and de Pazzis 1973](../research/bibliography.md#hardy1973time);
  [Hardy, Pomeau and de Pazzis 1973b](../research/bibliography.md#hardy1973timeevolution);
  [Hardy, de Pazzis and Pomeau 1976](../research/bibliography.md#hardy1976molecular);
  [Frisch, Hasslacher and Pomeau 1986](../research/bibliography.md#frisch1986latticegas);
  [Margolus, Toffoli and Vichniac 1986](../research/bibliography.md#margolus1986cellularautomata)
- [dmishin 2013](../research/bibliography.md#dmishin2013single)
