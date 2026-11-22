# Reproducibility

JETCARP is built so that a run can be repeated exactly: the same scene reaches the same state, bit for
bit, on one thread or thirty-two, on the CPU or the GPU, and, by design, on any operating system. A
search stopped halfway and carried on later ends exactly where an uninterrupted one would, the
exploratory search aside ([below](#what-is-not-guaranteed-yet)). This page
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
  threads ran it, and `jetcarp library soup ID` rebuilds the soup a hunt's find came from.
- **Portable arithmetic.** Tiling construction and Lenia use JETCARP's own deterministic maths for
  exponentials, powers, distances, sines, cosines and angles, built only from the IEEE 754
  operations that every processor rounds the same way (addition, multiplication, division and square
  root), in a fixed order, with no fused multiply-adds. Lenia sums each cell's kernel in a fixed order (see
  [Lenia](../automata/lenia.md#reproducible-to-the-last-bit)).

## The state hash

The *state hash* identifies a world's present state. Since JETCARP 0.4.3 it is version 2: SHA-256,
256 bits, printed as 64 hexadecimal digits, over everything that decides what the world does next:

- the generation number and the world's random-number generator;
- every field's cell states and ages, sandpile heights and lost grains, and Lenia's values by their
  exact bits;
- each automaton's rule (with its family's [rule-semantics version](../reference/library-format.md#rule-semantics-versions)),
  neighbourhood, settings and start (which a reseed uses again), its bookkeeping (row cursor, step
  count, whether it is on, a forest fire's noise seed, a layered rule's layers), and its ants' cells,
  headings and states;
- the rule tables, the script and where it is, the reseed policies and what they have done, the
  number of manual reseeds, and the period check's progress;
- a digest of the scene's tiling, so that two worlds on different tilings never share a hash.

Left out are what only shows (names, palettes, JACA's glow), what follows from the rest (a B0 rule's
background, which is the rule run from its start for the automaton's step count; compiled
tables and neighbourhoods), and the history: the command log is not hashed, so two ways of reaching
the same state give the same hash. Also left out are the counts of changed cells kept for recent
generations, which depend on whether a front end or the GPU counted them, so a world reopened from
its saved scene could not bring them back. A world saved as a scene
and reopened at any generation therefore has exactly the hash of the world that ran straight
through, then and at every later generation. Tests check both halves: that changing any one piece of
the state changes the hash, and that reopened worlds with scripts, reseed policies, ants, a B0 rule
on a patch, a layered rule and a forest fire hash as unbroken ones. The code lists every piece of a
world, so a new one cannot be added without being hashed or marked as one of those left out.

**Version 1**, which JETCARP 0.4.0 to 0.4.2 wrote, is a 64-bit FNV-1a hash of the generation, every
field and part of the automata's bookkeeping, printed as 16 hexadecimal digits. Records written then
carry no version; JETCARP keeps version 1 unchanged and verifies them with it, so old scenes, figures
and packs still check. A record written since says `"hash_version": 2`. Where a person only reads a
hash, as in the app's menu bar, JETCARP shows its first 16 digits; everywhere it is kept, compared or
copied (`jetcarp run` and `verify`, records, PNG metadata, `--json`, the MCP tools, *Copy state
hash*) it gives all 64.

Every generation is computed from the previous state alone, in parallel over cells, so the hash does
not depend on the thread count. On one thread, in a copy of JETCARP's repository (which holds this
scene):

```sh
jetcarp run crates/jetcarp-core/data/scenes/rule30-life-ants.json --gens 100 --threads 1
```

It ends with:

```text
...
hash:       bf2ec44f9443a2d100124c0133475711a1f98cfe48927add833a4d63e8bea8b9
```

On every core:

```sh
jetcarp run crates/jetcarp-core/data/scenes/rule30-life-ants.json --gens 100
```

It ends with the same hash:

```text
...
hash:       bf2ec44f9443a2d100124c0133475711a1f98cfe48927add833a4d63e8bea8b9
```

`jetcarp run … --expect HASH` fails unless the run ends on that hash, which makes a check of this kind
one line in a script. It takes either version, told apart by length: 16 hexadecimal digits are
version 1 (`--expect 1adf6e3234b7f199` passes for this run too), 64 are version 2, and anything else
is refused. The world status that scripts and MCP tools read also gives `cells_hash`, a hash
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

## B0 rules whose background is live

JETCARP 0.4.3 changed where one kind of B0 rule starts (#238). A rule with birth on 0 whose
background turns live for good at the first step, because it survives on every neighbour count the
tiles have (`S8` on the square grid), now starts on the live background at generation 0, as Golly
reads such a rule, and the two agree generation for generation. Before, it started on the empty
world. So a scene from 0.4.2 or earlier with such a rule replays to other states, and its recorded
hash no longer matches: the rule's family, `lifelike_b0`, went to version 3, and a library flags
records of B0 rules for `jetcarp library reverify`. B0 rules whose background keeps flashing, and
every other rule, replay as before. The pinned scenes that moved are the Quick Start entries of
AntiLife (`B0123478/S01234678`) and InverseLife (`B0123478/S34678`), both live for good on the
square grid; the version change alone moved the hashes of the other B0 entries, whose states are
unchanged.

## Larger than Life rules with birth on 0

JETCARP 0.4.3 also began keeping two-state [Larger than Life](../automata/larger-than-life.md#rules-with-birth-on-0)
rules with birth on 0 relative to their background, as B0 Life-like rules are, so they match Golly
from generation 0 (#238). Before, such a rule ran as written, and at its first step every cell with
no live cell in reach was born. So a scene from 0.4.2 or earlier with such a rule replays to other
states, and its recorded hash no longer matches: the family, `ltl`, went to version 3, and a
library flags Larger than Life records for `jetcarp library reverify`. Larger than Life rules
without birth on 0, and those with more than two states, replay as before. No Quick Start entry has
such a rule, so no pinned state moved: the version change alone moved the hashes of the eight Larger
than Life entries (their version 1 hashes, of the states alone, are the same before and after), and
the rule-semantics scenes gained two, `ltl:b0` and `ltl:live`.

## Pinned geometry and dynamics

Three tests pin JETCARP's behaviour to recorded values, so that a change to a tiling or a rule cannot
happen without someone noticing.

- **Geometry pins** (`crates/jetcarp-core/tests/geometry_pins.rs`, values in
  `tests/data/geometry_pins.txt`): 1,366 rows. For each of the 24 built-in tiling families, the 1,254
  entries of the periodic tiling catalogue (every k-uniform tiling with k from 1 to 6, and six
  Archimedean tilings in their catalogue form) and the 81 isohedral types, each at its default size,
  one hash of every cell polygon's coordinates bit for bit and one of the neighbour lists of both
  neighbourhoods in their clockwise order; and for seven tilings, a hash of Lenia's kernel weights.
- **Dynamics pins** (`crates/jetcarp-core/tests/dynamics_pins.rs`, values in
  `tests/data/dynamics_pins.txt`): every one of the 371 built-in Quick Start entries, which between
  them cover every rule family on many tilings, shrunk to its 4,096-cell preview grid and run for 48
  generations, must reach its pinned state hash.
- **Rule-semantics pins** (`crates/jetcarp-core/tests/semantics_pins.rs`, values in
  `tests/data/semantics_pins.txt`): one small fixed scene for each of the 25 rule families of the
  engine's [rule-semantics versions](../reference/library-format.md#rule-semantics-versions), started
  from cells the test chooses itself (for ants, the sandpile, Lenia and the row rules, a start of the
  engine's), must reach its pinned state after 12 to 600 generations. Each
  row also records the family's version. A change in what a family's rules do fails the test, and the
  test refuses to pin the new state until the family's version has been raised; a library then flags
  the records verified under the old version for checking again, as it does after a geometry change.

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
- **More families** (`families.rs`): JACA (with blast radii 0 to 64, its glow and its counts),
  B0 Life-like rules (from an odd step count too), Margolus blocks on even and odd sides,
  isotropic rules (two-state, Generations and B0, from an odd step count too, and the isotropic
  tables of B/S rules against the B/S stepper),
  Larger than Life on lattices and patches (two-state rules with B0 too, from an odd step count,
  and the CPU going on from where the GPU stopped),
  the forest fire (at probabilities 0 and 1, and past 2^32 steps),
  sandpiles (grains dropped and lost, open patch edges, piles past 2^32 grains),
  on lattices and patches with every edge, through scripts, reseeds and period detection, and
  every Quick Start entry of these families. Everything the state hash covers must agree, and so
  must JACA's glow and counts.
- **The exploratory search's batches** (`batch.rs`): populations, changes, hashes and frames of many
  worlds stepped together.
- **Soup hunts** (`crates/jetcarp-search/tests/gpu_hunt.rs`): every soup's opening run must be identical, and a hunt with the GPU must
  leave exactly the catalogue a CPU-only hunt leaves.
- **Large sandpiles** (`pile.rs`): the same pile, toppling count and sweep count at every step.

These tests are skipped, with a note, on a machine with no usable graphics adapter.

## Interrupted searches end where uninterrupted ones would

All but the exploratory search, which continues from its saved archive rather than resuming exactly
([below](#what-is-not-guaranteed-yet)). Every search in JETCARP runs as a *job* in the catalogue (see [Search methods](search-methods.md)). A
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
  resuming exactly as the other jobs do. Its results depend on the computer's number of threads:
  its rounds of runs on the processor are sized to them, so that it scales with the machine, and
  a computer with another number of threads (or another `--threads`) makes other runs from the
  same seed and settings. A run on the processor past its time cap is kept with the
  generations it reached, which depend on the machine. On the processor alone (`--cpu`), a search
  that is not stopped makes the same runs, ids and confirmations whatever the length of its steps
  and however its threads take up its runs, for the same seed, settings and number of threads (its
  rounds of runs are sized to its threads), as long as no run meets its time cap. Each run it stores
  can be replayed exactly from its tiling, rule, seed, random state and generations. The tilings and
  families a search was told to search are kept in its spec, and a search made before 0.4.3 keeps
  the schedule it had ([Search methods](search-methods.md#explore)).
- **Before 1.0.** The scene format, and the behaviour of a rule or a tiling, may still change between
  releases. Such changes are deliberate, show up in the pins and are listed in the changelog, but an
  old scene may then reach a different state. The Penrose rhombs already lay out their tiles
  differently from those of CART, JETCARP's predecessor.
- **HashLife jumps** reach the same cells as stepping but reset every live cell's age to 1, so their
  state hash differs from the stepped run's; compare `cells_hash` instead.
- **Hashes are not proofs.** Two different states sharing a version 2 hash would need a collision
  of SHA-256, which nobody knows how to find, so equal hashes stand for equal states. A hash still
  says only that two states are equal, not how either was reached, and the 64-bit version 1 is not
  cryptographic: two states could share one by chance, or by design. Where it matters, as in pattern
  verification, JETCARP compares the states themselves.

## Checking packs, and receipts to come

<div class="checked">

**Packs** (since JETCARP 0.4.2), the format for sharing catalogues and search results, are checked
when they are imported ([Packs](../guides/packs.md#import-a-pack)): every file against its SHA-256,
the version against its publisher's signature, every pattern's id against the pattern, and the
pack's geometry pins and rule meanings against the running engine (a pattern from another engine
version is kept and flagged for `jetcarp library reverify`). Then 3,000 of the new patterns are run
again (fewer if the pack has fewer), chosen at random but seeded by the pack's version id, so the
same pack always checks the same sample. If all pass, fewer than about 0.1% of the pack's patterns
are wrong, with 95% confidence (the "rule of three"), whatever its size. `--verify full` runs every
new pattern.

**Planned, not yet in JETCARP: receipts.** Each check would write a small *receipt*: the
pack's manifest hash, the mode and sample seed, the records checked and passed and any that failed,
the JETCARP version, the geometry pins, the operating system and processor, the date, and who ran it.
Later, receipts could be signed and collected in a public, append-only list, so that a pack can show
how many independent machines have verified it. Receipts are only meaningful because of everything
above: a receipt from one platform must mean the same as one from another.

</div>
