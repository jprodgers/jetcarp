# JACA: Jimmie's Annihilating Cellular Automata

JACA is a family of two-sided Life-like rules. Cells are positive, negative or empty; each sign
grows by its own birth/survival rule; and wherever the two signs touch, they destroy each other.
Started from a mixed soup, the two signs sort themselves into territories separated by glowing,
shifting fronts, and the fight between them can last thousands of generations. JACA is Jimmie
Rodgers's idea and is introduced in JETCARP.

<figure>
<picture>
  <source srcset="../images/automata/jaca-cairo-rainbow-still.png" media="(prefers-reduced-motion: reduce)">
  <img src="../images/automata/jaca-cairo-rainbow.webp" alt="JACA on Cairo pentagons: a mixed start separating into still territories with rainbow-coloured fronts" width="360" loading="lazy">
</picture>
<figcaption><code>JACA B27/S03456</code> on Cairo pentagons, 1,500 generations after a mixed start (the
early generations are shown more slowly). Watch the two signs sort themselves into territories: inside
them nothing changes, so cells grow old and take the last colour of their palette, while along the
fronts cells keep being destroyed and reborn, so age colouring turns each front into a rainbow. Scene
<code>docs/scenes/automata/jaca-cairo-rainbow.json</code> (Quick Start preset <em>JACA rainbow fronts
(Cairo)</em>).</figcaption>
</figure>

## What it is

Each cell holds one of three values: **+1**, **−1** or **0** (empty), a *balanced ternary* digit.
Every generation is computed from the current state in three steps:

1. **Contact.** A live cell with at least one neighbour of the opposite sign is a *contact* cell.
   Contact cells are destroyed: they are empty next generation.
2. **Blast.** With a blast radius `N` greater than 0, every cell within `N` neighbourhood steps of a
   contact cell is destroyed too, whatever its sign. With the default `blast 0`, only the touching
   cells go.
3. **Everything else follows its own sign's rule.** A +1 cell survives if its number of +1
   neighbours is in the +1 rule's survival list; an empty cell is born +1 if its number of +1
   neighbours is in the +1 rule's birth list; and likewise for −1. Each sign counts only neighbours
   of its own sign. An empty cell that *both* signs would fill stays empty: the two births cancel.

Two properties make the family easy to reason about. A world that contains only one sign runs that
sign's plain birth/survival rule exactly, so `JACA B3/S23` with only +1 cells is Conway's Life.
And the two signs can follow different rules: `JACA +B3/S23 -B36/S23` sets Life against HighLife.

What emerges depends on the rules, the tiling and the blast radius. Among the behaviours found so
far:

- **Territory wars.** Seeds-like rules (birth on 2, little survival) in mixed soups form territories
  that merge and push along their fronts until one sign holds the whole grid, on every tiling tried.
- **Rainbow fronts.** In rules whose territories go still while their fronts stay active, colouring
  by age turns every front into a rainbow, as in the figure above.
- **Trenches.** A blast radius of 1 to 3 leaves a strip of no-man's-land between territories: bands,
  beads, ropes, collapsing loops.
- **Annealing.** With births on 3 or more, fronts become smooth and move to reduce their curvature,
  like grain boundaries in a cooling metal.
- **Two different rules.** Bubbles of one rule growing in the other's still sea, or still crystal
  against noise, with either side winning.

<figure class="pair">
<div><picture>
  <source srcset="../images/automata/jaca-hat-war-still.png" media="(prefers-reduced-motion: reduce)">
  <img src="../images/automata/jaca-hat-war.webp" alt="JACA on the hat: teal and coral armies meeting along a glowing front" width="304" loading="lazy">
</picture></div>
<div><picture>
  <source srcset="../images/automata/jaca-square-beads-still.png" media="(prefers-reduced-motion: reduce)">
  <img src="../images/automata/jaca-square-beads.webp" alt="JACA with blast radius 1 on squares: territories separated by bands of glowing destroyed cells" width="400" loading="lazy">
</picture></div>
<figcaption>Left: a territory war on the hat tiling (<code>JACA B27/S013</code>), 1,500 generations, seen
where two armies meet; watch the front, marked by grey and pale destroyed tiles, push back and forth.
Scene <code>docs/scenes/automata/jaca-hat-war.json</code>. Right: blast radius 1 on squares
(<code>JACA B16/S1378 blast 1</code>), 1,500 generations: the borders between the signs are bands of
destroyed, glowing cells; watch them straighten and the territories trade ground. Scene
<code>docs/scenes/automata/jaca-square-beads.json</code>. Both are Quick Start presets.</figcaption>
</figure>

## Where it came from

JACA is Jimmie Rodgers's idea, from November 2026; it first appeared in JETCARP's predecessor CART
on 8 November 2026. Each of its ingredients has a precedent, and we credit them here:

- **Two-player Life.** Mark Levene and George Roussos's two-player game of life (2003) has black and
  white tokens; opponents weaken each other's survival through a difference count, and a tie at
  birth is settled by a coin flip ([Levene and Roussos 2003](../research/bibliography.md#levene2003twoplayer)).
- **+1, −1 and 0, with cancelled births.** The CodinGame bot game "Game of Life or Death", described
  on the Unsupervision Blog in November 2025, encodes the two players as 1 and −1; each side treats
  the other's cells as dead, and a cell both sides would switch on stays dead
  ([Unsupervision Blog 2025](../research/bibliography.md#unsupervision2025playable)). This is the
  closest precedent for JACA's encoding, own-sign counting and cancelled births. Touching cells of
  opposite sides are not destroyed there.
- **Matter and antimatter Life.** In September 2018 KittyTac proposed, on the ConwayLife.com forums, a
  Life with two kinds of cell where contact turns both cells into energy, and wildmyron implemented
  it as the "Energetic" rule, in which the energy decays into photons
  ([KittyTac and wildmyron 2018](../research/bibliography.md#kittytac2018rulerequest);
  [LifeWiki: Rule:Energetic](../research/bibliography.md#lifewikind_energetic)). This is the closest
  precedent for annihilation on contact. There the products are part of the state and can create
  cells, and both kinds follow one fixed rule.
- **Physics.** Opposite particles that annihilate when they meet, A + B → 0, are a classic problem in
  the physics of diffusion ([Toussaint and Wilczek 1983](../research/bibliography.md#toussaint1983particle));
  Chopard and Droz treat such reaction–diffusion processes with cellular automata
  ([Chopard and Droz 1998](../research/bibliography.md#chopard1998cellular)).

## How JETCARP runs it

### Notation

| Notation | Meaning |
| --- | --- |
| `JACA B3/S23` | Both signs follow Life |
| `JACA +B3/S23 -B36/S23` | +1 follows Life, −1 follows HighLife |
| `JACA B3/S23 blast 2` | Life for both, and each annihilation clears everything within two steps |
| `JACA B13/S234V` | `V` on a B/S part: edge neighbours instead of touch neighbours |

The blast radius runs from 0 to 64. Rules with birth on 0 are refused. Counts of 10 or more are
written with commas, as in other B/S rules.

### On any tiling

JACA uses only neighbour counts and neighbourhood steps, so it runs on every tiling (see
[How JETCARP generalises automata](generalising.md)); the blast radius is measured in steps of the
automaton's own neighbourhood. It runs on the CPU; there is no GPU or HashLife version yet.

### Seeds, colours and counts

A JACA field is stored as a three-state field (states 1 and 2 are +1 and −1), so drawing, scenes
and scripts treat it like any other multi-state rule. The usual start is a mixed soup
(`{"signed": {"density": 0.3, "negative": 0.5}}`: 30% of cells live, half of them negative); a
*halves* start puts +1 cells on the left and −1 on the right. Each sign has its own palette
(`palette` and `palette_neg`), coloured by age. Destroyed cells glow briefly and fade; the glow is
drawn only and is not part of the state or its hash. The app's card shows the number of cells of each
sign and how many were touching and destroyed; the world status that scripts and the MCP server read
also reports cancelled births.

### Presets

The Quick Start's JACA family has 5 named rules and 71 presets. Each preset is a whole scene: tiling,
neighbourhood, a rule per sign, blast radius, start and palette pair. Together they cover 24
tilings, from squares and hexagons through the Archimedean and k-uniform tilings to two isohedral
tilings, both Penrose tilings, the hat and the spectre. (For a rule, the isohedral IH28 is the Cairo
tiling and IH41 the square grid, and the brick, herringbone and Pythagorean presets play on the
hexagonal, hexagonal and 4.8.8 graphs; see [Isohedral tilings](../tilings/isohedral.md).) They came from a six-hour search over about
130,000 configurations: random rule pairs, mutations and crossovers of the best, and transplants of
good pairs to other tilings, scored on balance between the signs, contact along fronts, territory
changing hands and activity, then reviewed by eye as contact sheets.

## What is new here

Each ingredient of JACA has been published before, as the list above shows, and we do not claim
"the first signed Life" or "the first matter–antimatter Life". To our knowledge, JACA's particular
combination is new: an independent birth/survival rule for each sign, counting only same-sign
neighbours; destruction of every live cell that touches the other sign; a blast radius measured in
neighbourhood steps; cancelled double births; an exact reduction to the plain rule when one sign is
alone; and all of this on any periodic or aperiodic tiling. The searches behind this statement, and
their known gaps (MCell's weighted rules and parts of the ConwayLife forums were not searched), are
described in [What is new in JETCARP](../research/contributions.md).

The colouring is not part of the claim: colouring cells by age and fading destroyed cells have
precedents in other programs.

## Try it

- Quick Start, family *JACA*: *JACA Life*, *JACA Life vs HighLife*, *JACA war on the hat tiling*, *JACA
  rainbow fronts (Cairo)*, *JACA bead borders (square, blast 1)*, and the other presets.
- `jetcarp run docs/scenes/automata/jaca-hat-war.json --gens 3000 --png hat-war.png`
- Try a halves start with two different rules, and watch which front advances.

## References

- [Levene and Roussos 2003](../research/bibliography.md#levene2003twoplayer)
- [Unsupervision Blog 2025](../research/bibliography.md#unsupervision2025playable)
- [KittyTac and wildmyron 2018](../research/bibliography.md#kittytac2018rulerequest);
  [LifeWiki: Rule:Energetic](../research/bibliography.md#lifewikind_energetic)
- [Toussaint and Wilczek 1983](../research/bibliography.md#toussaint1983particle);
  [Chopard and Droz 1998](../research/bibliography.md#chopard1998cellular)
