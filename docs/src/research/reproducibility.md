# Reproducibility

JETCARP is built so that a run can be repeated exactly: the same scene reaches the same state, bit for
bit, on one thread or thirty-two, on the CPU or the GPU, and, by design, on any operating system. A
search stopped halfway and carried on later ends exactly where an uninterrupted one would. This page
says how that is achieved, how it is tested, and what is not guaranteed yet.

## Deterministic randomness

Nothing in a run depends on the clock, the thread schedule or the platform's random numbers.

- **One generator per scene.** Every scene starts from its `rng_state`, the state of a SplitMix64
  generator written out in JETCARP's own code, so it cannot change with a dependency. Random seeds,
  reseeds and ant placements draw from it in a fixed order, and replaying the scene's commands
  advances it identically.
- **Per-cell noise.** The forest fire, the one rule that draws random numbers every step, takes them
  from a fixed function of the automaton's noise seed (drawn from the scene's generator when the
  automaton is added), the step and the cell, so the cells can be updated in any order.
- **Searches.** Soup `i` of a soup hunt uses a random state computed from the hunt's seed and `i`;
  soup `s` of a census or bounded search uses `seed + s`; the evolutionary search is a pure function
  of its seed; samples of turmite tables are seeded. A search's results never depend on how many
  threads ran it.
- **Portable arithmetic.** Tiling construction and Lenia use JETCARP's own deterministic maths for
  exponentials, powers, distances, sines, cosines and angles, built only from the IEEE 754
  operations that every processor rounds the same way (addition, multiplication, division and square
  root), in a fixed order, with no fused multiply-adds. Lenia sums each cell's kernel in a fixed order (see
  [Lenia](../automata/lenia.md#reproducible-to-the-last-bit)).

## The state hash

The *state hash* is a 64-bit FNV-1a hash of everything that determines what a world does next: the
generation number; every field's cell states and ages, sandpile heights and lost grains, and Lenia's
values by their exact bits; each automaton's bookkeeping (row cursor, step count, whether it is on);
the ants' cells and headings; and the progress of any script or reseed policy. JETCARP prints it as 16
hexadecimal digits. Two worlds with the same hash are, for practical purposes, in the same state.

Every generation is computed from the previous state alone, in parallel over cells, so the hash does
not depend on the thread count:

```text
$ jetcarp run crates/jetcarp-core/data/scenes/rule30-life-ants.json --gens 100 --threads 1
...
hash:       1adf6e3234b7f199
$ jetcarp run crates/jetcarp-core/data/scenes/rule30-life-ants.json --gens 100
...
hash:       1adf6e3234b7f199
```

`jetcarp run … --expect HASH` fails unless the run ends on that hash, which makes a check of this kind
one line in a script. The world status that scripts and MCP tools read also gives `cells_hash`, a hash
of the cell states only, for comparing with engines that do not track ages, such as HashLife.

## Scenes are the unit of reproduction

A [scene](../reference/scene-format.md) is a world's starting point plus the log of every command
applied to it, and loading it replays the log. A saved scene records the generation and hash it
reached; `jetcarp verify scene.json` replays it on the CPU and checks both. A PNG written by
`jetcarp run --png` carries the whole scene, the version and the hash in its text chunks, so a picture
can always be traced back to the run that made it.

That is why this book gives a scene for every figure. Each picture is made from a file in
`docs/scenes/` by `docs/tools/render-figures.sh` (stills) or `docs/tools/render-animations.sh`
(animations), so a reader with JETCARP can make it again, and the pictures cannot drift from the
program. Each result on the [Findings](findings.md) page names its scene in the same way.

## Pinned geometry and dynamics

Two tests pin JETCARP's behaviour to recorded values, so that a change to a tiling or a rule cannot
happen without someone noticing.

- **Geometry pins** (`crates/jetcarp-core/tests/geometry_pins.rs`, values in
  `tests/data/geometry_pins.txt`): 1,366 rows. For each of the 24 built-in tiling families, the 1,254
  entries of the periodic tiling catalogue (every k-uniform tiling with k from 1 to 6, and six
  Archimedean tilings in their catalogue form) and the 81 isohedral types, each at its default size,
  one hash of every cell polygon's coordinates bit for bit and one of the neighbour lists of both
  neighbourhoods in their clockwise order; and for seven tilings, a hash of Lenia's kernel weights.
- **Dynamics pins** (`crates/jetcarp-core/tests/dynamics_pins.rs`, values in
  `tests/data/dynamics_pins.txt`): every one of the 364 built-in Quick Start entries, which between
  them cover every rule family on many tilings, shrunk to its 4,096-cell preview grid and run for 48
  generations, must reach its pinned state hash.

The pins are rewritten (with `JETCARP_PIN_WRITE=1`) only after a deliberate change, and the change is
recorded. One was made before this release: the Penrose rhomb construction was rewritten, which
changed the P3 tiling's geometry pin and the dynamics pins of the eight Quick Start entries on it (see
the [Changelog](../about/changelog.md)). A rule family that turned out not to give the same bits on
every platform would be listed in the test as not portable, with the reason, rather than re-pinned;
that list is empty.

JETCARP's continuous-integration workflow builds and tests everything, pins included, on Linux
(x86_64), Windows (x86_64) and macOS (Apple silicon). A pass on all three means the same scene reaches
the same state on each.

## The CPU and the GPU agree

The GPU is an option, never a different answer. Wherever JETCARP can step a world on the GPU, the
tests in `crates/jetcarp-gpu/tests` require it to match the CPU exactly:

- **Worlds** (`equality.rs`): Life-like and Generations rules with both neighbourhoods, WireWorld, the
  cyclic, rock-paper-scissors, Greenberg–Hastings and hodgepodge rules and the Spectre glider, on ten
  tilings with dead, wrapped and live edges, compared after runs of 1, 2, 13, 60 and 180 generations:
  the state hash, the generation, the status and the command log must all agree. Further tests cover
  aperiodic patches, several automata with several steps per generation, commands between GPU runs,
  scripts and period detection, priority tables, reseed policies, ages, and rules the GPU cannot run
  (which must fall back to the CPU, never run differently).
- **The exploratory search's batches** (`batch.rs`): populations, changes, hashes and frames of many
  worlds stepped together.
- **Soup hunts** (`hunt.rs`): every soup's opening run must be identical, and a hunt with the GPU must
  leave exactly the catalogue a CPU-only hunt leaves.
- **Large sandpiles** (`pile.rs`): the same pile, toppling count and sweep count at every step.

These tests are skipped, with a note, on a machine with no usable graphics adapter.

## Interrupted searches end where uninterrupted ones would

Every search in JETCARP runs as a *job* in the catalogue (see [Search methods](search-methods.md)). A
job runs in steps of a few seconds and writes each step's finds, its progress and a checkpoint in one
transaction, so stopping it, a crash or a reboot loses at most the step in flight. Results are merged
in a fixed order (soups by index, for example, with each new pattern's record taken from the lowest
soup index that found it), so they do not depend on the batching either.

The tests in `crates/jetcarp-search/tests/jobs.rs` and beside it check, for every kind of job, that one
stopped and resumed, on the same runner or another, ends with exactly the results of one that ran
through: hunts, ant searches, bounded soup searches, collisions, sandpiles, censuses (the same
report), movers on a sliding window, and evolutionary searches. They also check that a job left
running by a crashed process is seen as stalled and can be resumed, that two runners never own one
job, and that a runner that has lost its job writes nothing more.

## Results are verified exactly

Hashes find candidates quickly; they never decide what is stored. A soup hunt notices that a soup has
settled when its state hash repeats, but every object it keeps is then run again alone and compared
cell by cell with its stored phases (see [Search methods](search-methods.md#how-finds-are-verified)).
Turmite highways are proved by an exact argument, not by a hash. Each method's verification is
described with the method.

## What is not guaranteed yet

- **Platforms.** The portability of the pins is a design property checked by the CI runs above.
  macOS on Intel and Linux on ARM are built but not tested, and other platforms have not been tried.
- **Graphics adapters.** The GPU equality tests have run on the author's machines and, in CI, on
  Mesa's software renderer; a GPU driver that miscomputes could still give a different answer. A
  run made with `--gpu` can always be checked on the CPU with `jetcarp verify`.
- **The exploratory search as a whole.** `jetcarp search explore` runs a CPU loop and a GPU loop side
  by side and shares its archive between them, so two searches with the same settings do not try the
  same candidates in the same order, and a stopped search continues from its saved archive rather than
  resuming exactly as the other jobs do. Each run it stores can be replayed exactly from its tiling,
  rule, seed and random state.
- **Before 1.0.** The scene format, and the behaviour of a rule or a tiling, may still change between
  releases. Such changes are deliberate, show up in the pins and are listed in the changelog, but an
  old scene may then reach a different state. Scenes on Penrose rhombs saved by CART, JETCARP's
  predecessor, already do.
- **HashLife jumps** reach the same cells as stepping but reset every live cell's age to 1, so their
  state hash differs from the stepped run's; compare `cells_hash` instead.
- **Hashes are not proofs.** A 64-bit hash is not cryptographic, and two different states could share
  one. Where it matters, as in pattern verification, JETCARP compares the states themselves.

## Planned: verification receipts for packs

<div class="checked">

**Planned, not in JETCARP 0.4.0.** Packs, the format for sharing catalogues and search results, come
in a later release. The plan is that importing a pack checks the SHA-256 of every file against its
manifest and the pack's geometry pins against the running engine, then re-runs a random sample of its
records, chosen on the importing machine from a seed it records: 3,000 by default, and if all pass,
fewer than about 0.1% of the pack's records are wrong, with 95% confidence (the "rule of three"),
whatever its size. A full re-run is available on request. Each check writes a small *receipt*: the
pack's manifest hash, the mode and sample seed, the records checked and passed and any that failed,
the JETCARP version, the geometry pins, the operating system and processor, the date, and who ran it.
Later, receipts could be signed and collected in a public, append-only list, so that a pack can show
how many independent machines have verified it. Receipts are only meaningful because of everything
above: a receipt from one platform must mean the same as one from another.

</div>
