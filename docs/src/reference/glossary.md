# Glossary

The terms this book uses, in alphabetical order, each with the page that explains it.

## A

**Affects others.** An automaton's setting for sharing a field: on, it acts on the shared field with
the other automata of its kind; off, it keeps a private field drawn alongside. On by default for
binary rules. See [Several automata at once](../guides/several-automata.md).

**Age.** The number of generations a cell has been in its current state. Shown by
[age colouring](palettes.md#age-colouring) and included in the state hash.

**Agent.** An automaton that moves over the tiling instead of updating every cell: an ant or turmite.

**Annihilation.** In [JACA](../automata/jaca.md), the destruction of live cells of opposite sign that
touch, and of every cell within the *blast radius* of them.

**Aperiodic tiling.** A tiling with no translation symmetry: it never repeats. JETCARP has the
[Penrose](../tilings/penrose.md) kites and darts and rhombs, and the [hat and the spectre](../tilings/hat-spectre.md).

**Archimedean tiling.** An edge-to-edge tiling by regular polygons in which every corner looks the
same, named by its *vertex configuration*, such as 4.8.8. There are eleven, three of them the regular
tilings. See [Regular and Archimedean tilings](../tilings/regular-archimedean.md).

**Atlas.** The selection of confirmed, high-scoring runs from the [exploratory search](../research/search-methods.md#explore)
that the Quick Start menu offers.

## B

**B/S notation.** A Life-like rule written as the neighbour counts for birth and survival: `B3/S23`
is Life. See [Rule notation](rule-notation.md#life-like-rules-bs).

**B0 rule.** A rule in which a dead cell with no live neighbours is born, so empty space switches on.
See [Life-like rules](../automata/life-like.md#b0-rules).

**Background.** A palette's colour for empty cells. See also *stage*.

**Blast radius.** In JACA, how many neighbour steps around each annihilation are also cleared.

**Boundary.** What lies past the edge of a finite grid: *dead* (missing cells, counted as empty),
*wrap* (the grid is a torus) or *live* (counted as alive). Aperiodic patches have dead edges.

**Bounded soup search.** Soups run on a finite grid of any tiling. See
[Search methods](../research/search-methods.md#bounded-soup-search).

## C

**Alias.** An old id of a record, kept so that lookups by it still find the record: CART's ids, or an
id from before a geometry change. See [Library format](library-format.md#aliases).

**Canonical form.** A pattern written from its canonical starting phase, in its canonical orientation,
at the origin: the form its identity is computed from, so one pattern has one id however it was found
or stored. See [Library format](library-format.md#canonical-form).

**Catalogue (catalog).** The SQLite database of verified patterns, ant runs, Quick Start entries and
jobs, `library.sqlite` in the data folder; also called the *pattern library*. See
[The pattern library](../guides/pattern-library.md) and [Library format](library-format.md).

**Cell.** One tile of the tiling, holding a state.

**Census.** A run of many rules from the same soups, classifying each rule as died, frozen,
oscillating, active or exploded. See [Search methods](../research/search-methods.md#rule-census).

**Checkpoint.** The saved state of a job, written after every step, from which it can be resumed.

**Colour by.** An automaton's colouring setting: `auto`, `age`, `state` or `row`. See
[Palettes](palettes.md#how-a-palette-colours-cells).

**Command.** One change to a world: add an automaton, paint cells, step, reseed and so on. A scene
keeps the log of commands since its start. See [Scene file format](scene-format.md#commands).

**Compound.** A "spaceship" that turns out to be two or more smaller ships travelling together. See
[Findings](../research/findings.md).

**Congruence.** Two ships are congruent, and counted as one object, if one rotation, reflection and
translation carries the live tiles of one onto the other's at every generation. See
[What is new in JETCARP](../research/contributions.md#how-we-checked).

**Content id.** A library record's id: 32 hexadecimal digits of SHA-256 over its identity text and
its tiling's geometry pin. Its first 8 digits are the *short id*. See
[Library format](library-format.md#content-ids).

**Continuous field.** A field whose cells hold a value between 0 and 1, as in [Lenia](../automata/lenia.md).

## D

**Degree.** The number of neighbours a tile has, in the chosen neighbourhood. It varies from tile to
tile on mixed and aperiodic tilings. See [Tilings as neighbour graphs](../tilings/neighbour-graphs.md).

**Derivative.** A ship that is a smaller ship pulling a passenger along, or a bound pair of gliders.

**Displacement.** How far a spaceship moves in one period, as a lattice vector.

## E

**Edge neighbours.** Tiles that share a stretch of edge: the von Neumann neighbourhood on squares.
Written with a `V` on a B/S rule. See [How JETCARP generalises automata](../automata/generalising.md).

**Elementary rule.** One of Wolfram's 256 one-dimensional rules, drawn row by row (`W30`). See
[Elementary rules](../automata/elementary.md).

**Explore.** JETCARP's exploratory, quality-diversity search over tilings, rules and starting patterns
(`jetcarp search explore`). See [Search methods](../research/search-methods.md#explore).

## F

**Family.** A kind of rule with its own notation: Life-like, Generations, Larger than Life, turmites,
and so on. Also used of tilings (`square`, `hex`, `spectre`, …).

**Field.** The array of cell values an automaton works on: binary, multi-state, height (sandpile) or
continuous. Several automata can share one.

**Flag A, Flag B.** On the [Findings](../research/findings.md) page, spaceship candidates that are most
likely already known (A), or need checking before any claim (B).

## G

**Generation.** One step of time, in which every cell's next state is computed from the current
states. Agents and some automata take several steps per generation.

**Generations.** The family of rules with dying states, written `B2/S/C3`. See
[Generations](../automata/generations.md).

**Geometry pin.** A hash of the geometry a library record ran on: a periodic tiling's unit cell (its
polygons and neighbour offsets), or an aperiodic record's patch. Every record stores one; when the
engine's pin differs, the record is flagged for re-verification. Not to be confused with the test
*pins*. See [Library format](library-format.md#geometry-pins).

**Glider.** A small spaceship. On aperiodic tilings, a pattern that travels with a constant speed and
heading although it never repeats exactly.

## H

**HashLife.** An algorithm that jumps a pattern far ahead by remembering how blocks of cells evolve.
See [HashLife and very long runs](../guides/hashlife.md).

**Highway.** A turmite's endless, repeating, travelling structure, such as the one Langton's ant builds
after 9,977 steps. JETCARP proves highways exactly. See [Langton's ant and turmites](../automata/turmites.md).

**Host (background host).** The part of JETCARP that keeps scenes running with no app window, as a live
wallpaper or screensaver. See [Wallpapers, screensavers and the background host](../guides/wallpapers.md).

**HROT.** "Higher-range outer-totalistic": the Larger than Life notation `R5,C2,S33-57,B34-45`.

**Hunt (soup hunt).** The fast search of small random soups on an unbounded plane. See
[Search methods](../research/search-methods.md#soup-hunt).

## I

**Identity text.** What a pattern's content id is the hash of: its cell-state model, neighbourhood,
tiling, class, period, displacement and canonical evolution, as lines of text in a fixed form
(`jetcarp-pattern-v3`). See [Library format](library-format.md#the-identity-text-of-a-pattern).

**Isohedral tiling.** A tiling by one tile in which a symmetry carries any tile to any other.
Grünbaum and Shephard sorted them into 93 types, IH1 to IH93; JETCARP has the 81 that can be told
apart by the tile's shape alone. See [Isohedral tilings](../tilings/isohedral.md).

## J

**JACA.** Jimmie's Annihilating Cellular Automata: cells of sign +1 and −1, each following its own B/S
rule, destroyed where they meet. See [JACA](../automata/jaca.md).

**Job.** A search stored in the catalogue with its settings, progress and checkpoint, so that it can
be stopped and resumed. See [Search methods](../research/search-methods.md#jobs).

## K

**k-uniform tiling.** An edge-to-edge tiling by regular polygons with exactly k kinds of corner.
JETCARP has all 1,248 with k up to 6, from Brian Galebach's catalogue. See
[k-uniform tilings](../tilings/k-uniform.md).

## L

**Larger than Life.** Life-like rules over a larger range of neighbours. See
[Larger than Life](../automata/larger-than-life.md).

**Lattice.** The translations that map a periodic tiling onto itself; a *lattice step* is one of them,
the unit in which a spaceship's speed is given.

**Laves tiling.** The dual of a uniform tiling. Every isohedral tiling has the neighbour graph of one
of the eleven Laves tilings, which is why JETCARP's 81 isohedral types give rules only eleven worlds.
See [Findings](../research/findings.md#isohedral-tilings-are-eleven-neighbour-graphs).

**Layer, layered rule.** The tiles at one graph distance from a seed; a layered rule writes one layer
per generation from the layer before. See [Layered rules](../automata/layered.md).

**Level.** The number of substitution steps used to build an aperiodic patch; higher levels are larger.

**Life-like rule.** A two-state rule that counts live neighbours, written in B/S notation. See
[Life-like rules](../automata/life-like.md).

## M

**MAP-Elites.** A quality-diversity search method that keeps the best solutions in each niche of a
behaviour space; the basis of `jetcarp search explore`.

**MCP.** The Model Context Protocol, through which AI assistants can drive JETCARP (`jetcarp mcp`). See
[MCP tools](mcp.md).

**Mover.** A pattern that travels across an aperiodic tiling; it is *verified to a distance*. See
[Search methods](../research/search-methods.md#movers-on-a-sliding-window).

**Multi-state field.** A field whose cells hold a state number, for rules with more than two states.

**My finds.** The Quick Start entries you add yourself, from patterns, runs and scenes.

## N

**Neighbour graph.** A tiling seen as tiles and the links between neighbours, which is all a counting
rule sees. See [Tilings as neighbour graphs](../tilings/neighbour-graphs.md).

**Neighbourhood.** Which tiles count as a cell's neighbours: *touch* or *edge*.

**Niche.** In the exploratory search, a combination of rule family, group of tilings and behaviour
class; the archive keeps the best few runs of each.

**Notation.** The text that names a rule, such as `B3/S23` or `Cyclic 14/1`. See [Rule notation](rule-notation.md).

## O

**Object finder.** The part of JETCARP that splits a field into still lifes, oscillators, spaceships
and other objects (`jetcarp objects`).

**Onset.** The step at which a turmite's highway begins.

**Operation.** One of JETCARP's named actions (`scene.run`, `search.start`, `jobs.status`, …), the
same for the command line, scripts, the app and MCP; `jetcarp api list` lists them.

**Oscillator.** A pattern that returns to its starting state after a period, without moving.

## P

**Pack.** A planned format for sharing catalogues and search results, not yet in this release. See
[Packs](../guides/packs.md).

**Palette.** A list of colour stops with a background and a stage colour. See [Palettes](palettes.md).

**Patch.** A finite piece of an aperiodic tiling, built to some level.

**Pattern library.** See *catalogue*.

**Period.** The number of generations after which an oscillator, spaceship or highway repeats.

**Periodic tiling.** A tiling that repeats in two directions: it has a lattice of translations.

**Pins.** Recorded hashes that JETCARP's tests compare against: *geometry pins* for every tiling and
*dynamics pins* for every built-in Quick Start entry. See
[Reproducibility](../research/reproducibility.md#pinned-geometry-and-dynamics).

**Provenance.** A library record's history: how it was found or imported, by whom, by which search,
on which engine and geometry, and when it was checked again. See
[Library format](library-format.md#provenance).

**Priority table.** James Edmond's rule family: ordered rows of conditions on neighbour counts, the
first matching row deciding the next state. See [Priority rules](../automata/priority-rules.md).

**Profile (screensaver profile).** A named, complete setup for the screensaver: what to show (a
scene or a Quick Start entry), and optionally its palette, colouring, reseeding, cell size and speed. See [Wallpapers, screensavers and the background host](../guides/wallpapers.md).

## Q

**Quick Start.** The app's menu of ready-to-run scenes: the built-in classics, the atlas and *My finds*.

## R

**Receipt.** A planned record of a verification check on a pack, not yet in this release. See
[Reproducibility](../research/reproducibility.md#planned-verification-receipts-for-packs).

**Reseed.** Putting new cells into a world that has settled, by hand or by an automatic policy. See
[Reseeding](../guides/reseeding.md).

**Rhai.** The scripting language JETCARP runs (`jetcarp script`). See [Scripting (Rhai) API](scripting.md).

**Ring.** One step of edge-neighbour distance from a starting tile; distances on aperiodic tilings are
counted in rings.

**Rotation.** What a wallpaper or screensaver shows over time: a list of profiles, scenes or Quick
Start entries, each with the conditions that end it. See [Rotations](../guides/rotations.md).

**Rule.** How a cell's next state follows from its own state and its neighbours'.

**Rule range.** Every Life-like or Generations rule in which a pattern runs through exactly the same
phases, stored with each catalogued pattern. See [Search methods](../research/search-methods.md#how-finds-are-verified).

**Rule table.** A rule written as a list of explicit transitions. See [Rule tables](../automata/rule-tables.md).

## S

**Scene.** A JSON file that holds a run: tiling, automata, random state and command log. See
[Scene file format](scene-format.md).

**Seed.** What an automaton puts on its field at the start: a random soup, one cell, ants, a pile of
grains and so on. Also the number that starts a random-number generator.

**Settled.** A run that has stopped changing, or repeats.

**Short id.** The first 8 hexadecimal digits of a content id, accepted by commands wherever an id is,
when no other record starts with them.

**Sighting.** A record that a job saw a catalogued pattern.

**Sliding window.** The few tiles of an infinite aperiodic tiling kept around a moving pattern,
rebuilt as it travels.

**Soup.** A random starting pattern, usually a small square of random cells.

**Spaceship.** A pattern that returns to its starting shape after a period, moved by a lattice vector.
Its speed is written as a fraction of one lattice step per generation, such as c/4, and its direction
in the lattice's own terms: *orthogonal* along one of the lattice's shortest steps (on a rectangular
lattice, one of its two mirror axes), *diagonal* halfway between two of them (square and hexagonal
lattices only), and *oblique* otherwise. A lattice with no mirror axes gets no word. The fraction
counts steps in the lattice's basis: the larger of the displacement's two coordinates, over the
period. On a hexagonal lattice, whose basis vectors are 60° apart, that is the convention Golly's
hexagonal rules use, so (1, 1), halfway between two shortest steps, also counts as one step.

**Stage.** A palette's colour for the area outside the tiling.

**State.** The value a cell holds: dead or alive, a number, a height or a level.

**State hash.** A 64-bit hash of everything that determines a world's future. Equal hashes mean equal
states, in practice. See [Reproducibility](../research/reproducibility.md#the-state-hash).

**Still life.** A pattern that does not change.

**Strategy.** A kind of search run as a job (`hunt`, `bounded`, `census`, `ants`, `mover`, `pile`,
`collide`, `evolve`, `explore`, and `reverify`, which checks library records again).

**Supertile.** A cluster of tiles that the substitution rules of an aperiodic tiling treat as one
larger tile.

## T

**Tiling.** A covering of the plane by tiles without gaps or overlaps. See the [tiling catalogue](../tilings/catalogue.md).

**Touch neighbours.** Tiles that share an edge or just a corner: the Moore neighbourhood on squares.

**Turmite.** A Turing-machine-like agent that reads a cell's colour, writes a new one, turns and steps.
Langton's ant is the simplest. See [Langton's ant and turmites](../automata/turmites.md).

**Turn string.** A one-state turmite written as one turn per colour, such as `RL` or `LLRR`.

## V

**Verified.** Of a pattern: run again alone and compared cell by cell with its stored phases for
several periods before it is catalogued. Of a mover: *verified to a distance*, the number of rings it
was followed without changing. See [Search methods](../research/search-methods.md#how-finds-are-verified).

**Vertex configuration.** The polygons around a corner of a tiling by regular polygons, in order:
4.8.8 is a square and two octagons.
