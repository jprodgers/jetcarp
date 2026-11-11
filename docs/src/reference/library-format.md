# Library format

The pattern library is one SQLite file, `library.sqlite` in the data folder (or wherever
`JETCARP_LIBRARY` points; see [Folders and environment variables](folders.md)). This page describes
what a record's id is made of, the geometry pins that tie records to the tiles they ran on, the
rule-semantics versions that tie them to what their rules meant, what happens when either changes,
and the tables of the file. Packs will carry the same ids, so the id
scheme below does not change: a change would be a new version line in the identity text.

## Content ids

Every pattern and every ant or turmite run has a **content id**: 32 hexadecimal digits (128 bits)
of SHA-256 over the record's *identity text*, a newline, and the *geometry pin* of its tiling:

```text
id = first 32 hex digits of SHA-256(identity text + "\n" + geometry pin)
```

The first 8 digits are the **short id**, which listings and commands accept wherever an id is asked
for, as long as no other record (or alias) starts with the same digits. Any unique prefix of 4 or more
digits works the same way, and so does a pattern's name. Ant ids are written `ant-` followed by the
content id.

### The identity text of a pattern

The identity text says what a pattern *is*, not the rule it was found in, so the same evolution found
in several rules is one pattern. It is these lines, joined by a single newline, with no newline at the
end:

```text
jetcarp-pattern-v3
model=<lifelike | wireworld | table | priority | spectre_glider | ...>
nb=<touch | edge>
steps=<steps per generation>
tiling=<tiling key>
class=<still_life | oscillator | spaceship | puffer | mover | gun | other>
period=<period>
d=<di>,<dj>
rule=<exact rule notation, or empty>
canonical=<canonical text>
phases=<phase 0>;<phase 1>;...
```

- `model` is `lifelike` for B/S and Generations rules (B/S is Generations with two states), otherwise
  the rule's family.
- The **tiling key** is the JSON of the tiling's identity: its family, shape parameters and edges,
  and `"boundary":"dead"`, but not its size. For example `{"family":"square","boundary":"dead"}`,
  or `{"family":"periodic","params":{"catalog":"k3-019"},"boundary":"dead"}`. The level of an
  aperiodic patch is its size, so it is left out too, even when it is given as a `level` parameter
  (as the Spectre's default parameters give it): the Spectre's key is
  `{"family":"spectre","boundary":"dead"}` on every level.
- The tiling key has one spelling, so any program can write it and get the same id:
  - Its own members come in a fixed order: `family`, then `params` and `edges` (each only if the
    tiling has them), then `boundary`. This is the order every id has used, so this one object does
    not sort its members as RFC 8785 would (which would put `boundary` first).
  - Every member's value is [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785) canonical JSON:
    object keys sorted by their UTF-16 code units at every depth, no whitespace, strings escaped as
    RFC 8785 says, and numbers as ECMAScript writes them (`1`, not `1.0`; `0.000001` but `1e-7`;
    `1e+21`; minus zero as `0`).
  - Tilings at their default shapes have no numbers in their parameters, so the rule for numbers
    never changes their keys; a reshaped tiling's numbers get their one spelling, for example
    `{"family":"cairo","params":{"a":[1e-7,0.000001],"b":1},"boundary":"dead"}`.
  - JETCARP's tests check the key against RFC 8785's own samples and against 600 lines written
    with an independent implementation, the Python `rfc8785` package
    (`crates/jetcarp-core/tests/data/jcs_vectors.txt`, made by `tools/jcs_vectors.py`).

  In Python, with the `rfc8785` package and a tiling `t` as JETCARP writes it (in a scene, say),
  read from JSON:

  ```python
  import rfc8785
  def tiling_key(t):
      jcs = lambda v: rfc8785.dumps(v).decode()
      out = ['"family":' + jcs(t["family"])]
      out += [f'"{k}":' + jcs(t[k]) for k in ("params", "edges") if t.get(k) is not None]
      return "{" + ",".join(out + ['"boundary":"dead"']) + "}"
  ```

  (On the four aperiodic families, take `level` out of `params` first, and leave `params` out if
  nothing is left.)
- `d` is the displacement per period in lattice steps, in the canonical orientation (`0,0` for still
  lifes and oscillators).
- `rule` is the exact rule only for puffers, movers, guns and `other`, whose stored phases do not
  cover their whole evolution; it is empty for the rest.
- `canonical` is the canonical picture of phase 0 on one-tile lattices (rows such as `ooo/o/.o`),
  a `k@i,j` list on lattices with several tile types, or on aperiodic patches `graph ...` or
  `site r2 ...` followed by 32 hex digits of SHA-256 of the pattern graph's canonical code or the
  site's description.
- `phases` lists every phase of one period in order, phase 0 first (puffers and movers: their
  starting configuration only). Each phase is its live cells `i,j,k,s` (lattice column, row, tile
  type, state; on aperiodic patches `i` is the tile's label in the pattern's graph or site), separated
  by single spaces and sorted by row `j`, then column `i`, tile type `k` and state `s`.

For Conway's glider on the square grid the text is

```text
jetcarp-pattern-v3
model=lifelike
nb=touch
steps=1
tiling={"family":"square","boundary":"dead"}
class=spaceship
period=4
d=-1,-1
rule=
canonical=ooo/o/.o
phases=0,0,0,1 1,0,0,1 2,0,0,1 0,1,0,1 1,2,0,1;1,-1,0,1 0,0,0,1 1,0,0,1 0,1,0,1 2,1,0,1;0,-1,0,1 1,-1,0,1 0,0,0,1 2,0,0,1 0,1,0,1;0,-1,0,1 1,-1,0,1 -1,0,0,1 0,0,0,1 1,1,0,1
```

and with the square grid's pin `0145310ebd49abfe89eb01c42ecaf459` its id is
`5a080db389c0cab3caf884d2596a1e70` (short id `5a080db3`). JETCARP's tests pin both values.

### Canonical form

The identity is computed from the record's cells, never from how they happen to be stored, so one
pattern has one id whichever phase, orientation or position it was found or written in:

- **Lattice tilings.** Of every stored phase in every symmetry of the tiling (8 on the square grid, 12
  on the hexagonal grid, the tiling's point group with its tile-type permutation on the Archimedean
  and k-uniform tilings, glide reflections included), moved to the origin, the smallest is the
  canonical start; the period is rewritten from that phase, in that orientation, with the
  displacement turned to match. Rule tables are not isotropic, so for them only the starting phase
  and the position count. The symmetries are all of them: the search goes through every rotation
  or reflection that can map the unit cell's lattice onto itself (the cell vectors' images are
  lattice vectors of the same lengths, so their coordinates are bounded), with every translation
  that carries the cell's first tile onto a tile, the translations inside the cell included, and
  JETCARP's tests check the result on every periodic tiling it builds against an independent scan
  of the tiles' corners.
- **Unit cells that hold the tile pattern more than once** (41 tilings: flat-top hexagons, the glide
  and fourfold families, and 38 isohedral types, such as IH58, whose 1 × 2 cell holds two unit
  squares). Some of their symmetries, such as IH58's quarter turn, do not map the cell's lattice
  onto itself, so the minimum is taken over the whole symmetry group, applied to each cell through
  its centroid. On such a cell a spaceship moving half a cell per period is seen again only after it
  has moved a whole cell, so its period as found can depend on its direction; its period is first
  made its shortest over every translation of the tiling, and in the canonical orientation it is
  the shortest multiple of that after which the ship has moved by a cell vector. Conway's glider on
  IH58 gets one id (period 8, as the cell sees it) in all 8 orientations and both rows of the cell.
  Puffers and guns there use the symmetries that keep their displacement a cell vector, and rule
  tables only the identity.
- **Aperiodic patches, pattern graphs** (outer-totalistic rules). The pattern's tiles, as a labelled
  graph, are labelled canonically again from the stored graph (minimised over the starting phase and,
  on chiral tilings such as the hat, a global mirror image), so the patch's rotations and reflections
  and the pattern's position on the patch do not count.
- **Aperiodic patches, geometric sites** (rule tables, and patterns too symmetric or too scattered for
  a graph). The phases are rotated to the start with the smallest description. One gap: the site's
  anchor tile is taken as stored. JETCARP's own searches always choose the canonical anchor, and an
  importer that calls JETCARP's canonicalisation rebuilds the site from its patch, which fixes it; a
  site re-anchored by another tool would get another id.

Two records that are the same live-cell evolution on *different* tiles keep different ids: on another
tiling, or at an inequivalent position of one tiling (where no symmetry of the tiling carries one onto
the other). An id names an evolution on its tiles.

### The identity text of an ant

```text
jetcarp-ant-v1
rule=<rule notation, such as "Ant RL" or "Turmite 1R1,1L0/1R1,0N0">
tiling=<tiling key>
start=<the start as JSON: "empty", or the start cells>
```

An ant and its mirror image (`RL` and `LR`) are two records, as the searches store them.

## Geometry pins

A record means something only on the tiles it ran on. Every pattern, ant run and Quick Start entry
stores the **geometry pin** of its tiling: 32 hex digits of SHA-256 over the geometry that its
behaviour depends on, whatever grid size it was built on.

- **Periodic tilings** (every lattice): the unit cell. The *polygon hash* covers the lattice vectors
  and, for each tile type, its shape and corners in the unit cell; the *neighbour hash* covers the
  offset table of both neighbourhoods (edge, then touch): for each tile type, its neighbour slots in
  order.
- **Aperiodic tilings** (hat, Spectre, Penrose): one fixed reference patch of the family, level 4 of
  each (4,000 tiles of the hat and of either Penrose tiling, 4,401 of the Spectre), whatever level
  the record was found on: every tile's shape and corners in tile order, then the neighbour lists of
  both neighbourhoods and which tiles lie on the open boundary. Level 4 is the smallest level that
  holds every local configuration of the next one (each tile with its neighbours and theirs, two
  steps out), so a change to any tile shows in the pin. A pattern's identity on these tilings
  does not depend on where it sits on the patch, so the same pattern found on two levels is one
  record; the record keeps the patch it was found on, where it is verified again. The reference
  levels are fixed, not the families' default levels, so a change of default never changes a pin.

Floats are hashed by their exact bits, and the tiling code uses JETCARP's own deterministic maths, so
pins are the same on every platform. The pin is the first 32 hex digits of SHA-256 of the line
`jetcarp-geometry-v1`, the polygon hash and the neighbour hash (64 hex digits each), separated by
newlines. The engine's regression test, `crates/jetcarp-core/tests/geometry_pins.rs`, pins every
tiling's polygons and neighbours separately (see
[Reproducibility](../research/reproducibility.md#pinned-geometry-and-dynamics)).

### When the geometry changes

The table `engine_pins` keeps the running engine's pin for every tiling the library uses. When a
library is opened, pins cached by another JETCARP version (or by a build tested against other geometry
pins) are computed again, and every record whose stored pin differs from the engine's is **flagged**
(`needs_reverify`). Flagged records are kept, listed, and counted by `jetcarp library info`; they are
never deleted or failed silently.

`jetcarp library reverify` (the operation `library.reverify`) checks them again as a job, resumable
with `--job`: a pattern is verified again exactly on the current geometry, an ant run again from its
start (it must end the same way), and a Quick Start scene built and, if it records a state hash,
replayed to it. `--all` checks every record, not only the flagged ones.

- A record that **passes** gets the engine's pin, its flag is cleared, and its provenance gains a
  `reverified` row. If the geometry really changed, its id changes with the pin: the record moves to
  its new id with its rules, sightings, Quick Start entry, media, ratings and history, and the old id
  becomes an alias (`geometry-change`), so links to it keep working.
- A record that **fails** stays flagged and is reported, with why. Only `--drop` deletes such records.

## Rule-semantics versions

Geometry pins catch changed tiles; rule-semantics versions catch a changed rule: a parser fix, or a
change in how a family counts neighbours or states, steps, or draws its randomness, anything that
makes the same rule, start and seed reach another state on the same tiles. The engine gives every rule
family a version, a positive integer written as a string, in one table
(`RULE_SEMANTICS` in `crates/jetcarp-core/src/semantics.rs`). Every family is at `"1"` in JETCARP
0.4.0. A family's version goes up by one when the meaning of any rule in it changes; a change that
keeps every rule's meaning, such as a faster step or a new way of writing an old rule, keeps it. The
version is not part of a record's id.

| Family | Rules | Version |
| --- | --- | --- |
| `lifelike` | B/S: `B3/S23`, `B3/S23V` | 1 |
| `generations` | `B2/S/C3` | 1 |
| `ltl` | Larger than Life: `R5,C2,S33-57,B34-45` | 1 |
| `elementary` | `W30` | 1 |
| `hexw` | two-parent hexagon rows: `HexW 6`, `HexW3 1234` | 1 |
| `hext` | three-parent hexagon rows: `HexT 30`, `HexT3 sum 777` | 1 |
| `layered` | `Layer 2/01` | 1 |
| `jaca` | `JACA +B3/S23 -B36/S23 blast 2` | 1 |
| `ant` | turn strings: `Ant RL`, `Turmite LLRR` | 1 |
| `turmite` | state tables: `Turmite 1R1,1L0/1R1,0N0` | 1 |
| `wireworld` | `WireWorld` | 1 |
| `cyclic` | `Cyclic 14/1` | 1 |
| `greenberg_hastings` | `GH 8/1` | 1 |
| `rps` | rock-paper-scissors: `RPS 3/3` | 1 |
| `forest_fire` | `ForestFire 0.01/0.0001` | 1 |
| `hodgepodge` | `Hodgepodge 100/2/3/28` | 1 |
| `margolus` | `Margolus BBM` | 1 |
| `lenia` | `Lenia R=13,T=10,b=1,m=0.15,s=0.015` | 1 |
| `sandpile` | `Sandpile` | 1 |
| `jvn29` | von Neumann's 29 states: `JvN29` | 1 |
| `priority` | priority tables: `Priority 4: …` | 1 |
| `spectre_glider` | `SpectreGlider` | 1 |
| `table` | rule tables by name: `Table parity` | 1 |

There is one family for each kind of rule the engine runs ([Rule notation](rule-notation.md) lists
their forms), with two splits: B/S rules and Generations are apart, though a pattern's identity calls
both `lifelike`; and an ant is a turn string (one state, each colour repainted the next), a turmite a
state table. The families that the identity's `model` line names have the same names; its `other` is
split here into its families.

- **Records.** Every pattern carries its rule's family (`family`) and the version of that family's
  meaning it was verified under (`semantics`). A pattern written before versions existed has
  version `1`.
- **On opening.** A library flags (`needs_reverify`) every pattern whose version is not the running
  engine's for its family, or whose family the engine does not have, exactly as it flags a changed
  pin (see [When the geometry changes](#when-the-geometry-changes)). `jetcarp library reverify`
  checks them again; a pattern that passes gets the engine's version (and keeps its id).
- **Tests.** The test `crates/jetcarp-core/tests/semantics_pins.rs` runs one small fixed scene per
  family and pins the state it reaches, with the version it was pinned at
  (`tests/data/semantics_pins.txt`; see
  [Reproducibility](../research/reproducibility.md#pinned-geometry-and-dynamics)). A change in a
  family's meaning fails it, with a message to raise that family's version in the table, and
  writing the pins again (`JETCARP_PIN_WRITE=1`) refuses a family whose state changed while its
  version did not.

Packs will carry the engine's table as `rule_semantics`, so an importer can flag records made under
another version in the same way.

## Provenance

Every record has a history in the table `provenance`. Row 0 says how it came into the library:

| Column | Holds |
| --- | --- |
| `kind` | `found` (by a search, a script or a person) or `imported` |
| `finder`, `orcid` | who, from `JETCARP_FINDER` and `JETCARP_ORCID` (empty when unset) |
| `search_kind`, `job_uuid` | the job that found it (its kind, such as `hunt`, and its uuid) |
| `pack_hash` | the pack it came in (for packs) |
| `machine` | operating system and CPU architecture, such as `linux-x86_64` (never the host name) |
| `engine_version`, `geometry_pin` | the JETCARP version and the pin it was verified with |
| `source` | what found it: `search`, `collide A + B`, a scene path, `import <file>` |
| `date` | when (UTC) |

Row 0 is written once and never rewritten (the file refuses an update to it). Later events append
rows 1, 2, ...: today `reverified`. The records brought in from CART's catalog and from two early development catalogs are the one
exception to how row 0 is written: theirs is their discovery, and their row 1 the import (see
[Records from CART](#records-from-cart)).

## Aliases

The table `id_aliases` maps old ids to records: `geometry-change` (an id from before a geometry
change). Every lookup by id, prefix or short id also searches the aliases.

## Records from CART

CART's catalog was brought into a fresh library once, before this release, by a development tool
(`crates/jetcarp-store/examples/import_cart`), and every record was verified again on JETCARP 0.4.0 on
the way in. The tool was removed once the import was made: it was added in commit `ef3eed3`, and
commit `f3492e8` is the last that has it. What the import did:

- **Records.** Each record was put in canonical form, which gives it its content id, and verified again
  in its own rule and in every rule CART had verified it in; its rule range was computed again. A
  record that failed in every rule was left out, and one that failed in some rules came in with the
  others only. Records that CART stored twice, as two orientations or phases of one pattern, became
  one record: their seen counts and sightings add up, the name and run of the one found first are
  kept (another name goes into the notes), and so are all their notes and tags.
- **Kept.** Names, notes, tags, curation, seen counts, and sightings per rule and job with their
  dates; CART's jobs, as finished jobs with their census rows, and the objects a hunt counted without
  storing them, as counts in its job's progress. Scores and labels were made again. Media were not
  kept (they are made again from the records), nor were the stage-1 settings and score models.
- **Ids.** CART's 64-bit ids were not kept, and no alias names them: this book cites the new ids.
- **Dates.** A record's provenance row 0 is its discovery in CART: `found`, by Jimmie Rodgers (with
  ORCID iD), at the moment of the find in its run (the earliest of the records merged into it), by
  the job that saw it first, with engine version `CART 0.1.0` and the geometry pin it was verified
  with in 0.4.0; the record's own provenance gives the same date. Row 1, `imported`, is the
  import.
- **Two more catalogs.** The same tool brought in two catalogs that JETCARP's development builds
  wrote in CART's format before the library had a schema: the Life census and the mining hunts. Their
  records were verified and merged in the same way, and their row 0 states the engine version the
  records give (`0.4.0`).
- **Counts.** The library holds 723,736 records: 415,086 from CART's catalog (its 416,392 records,
  of which 2,498 merged into 1,192), 308,595 from the census (whose other 487 records were already
  there and gained their rules), and 55 from the hunts (86 already there). 295 of them are
  spaceships.

## Tables

`PRAGMA user_version` is the schema version, 2, and `PRAGMA application_id` marks the file as a
JETCARP library. A library of schema 1 (made by development builds before the 0.4 release) is brought
up to 2 the first time it is opened: its ratings gain `verdict` and `source`, and its media rows, which
named files in per-record folders, are dropped, since media are always made again from their records.
A library made by a newer JETCARP is refused with a message to update. Catalogs from before schema 1
(CART's, or a JETCARP build before 0.4, called `catalog.sqlite`) are not opened or migrated: JETCARP 0.4
starts a fresh library (see [Records from CART](#records-from-cart)). A library made before
[rule-semantics versions](#rule-semantics-versions) (of either schema) gains them the first time it is
opened: its patterns gain `family`, worked out from each one's rule, and `semantics`, version `1`. The
schema number stays 2, since schema 2 gained them before the 0.4.0 release.

| Table | One row per |
| --- | --- |
| `meta` | library fact: `schema`, `library_uuid`, `created`, `created_by`, `builtin_version` |
| `engine_pins` | tiling in use: its pin key (its identity key), the engine's pin, the polygon and neighbour hashes, which engine computed them, when |
| `rules` | rule as run: notation, neighbourhood, steps per generation, family, birth and survival masks |
| `patterns` | pattern: content id, tiling, pin key and geometry pin, class, period, displacement, cells, canonical form, rule and rule range, its rule's `family` and that family's `semantics` version, seen count, score, curation, `needs_reverify`, `engine_version`, and the record's phases and verification |
| `pattern_rules` | rule a pattern was verified in, with the verification and engine version |
| `sightings` | pattern seen by a job in a rule: copies, soups, first and last seen |
| `unstored` | object a hunt counted but did not store as a record |
| `jobs` | search: uuid, kind, spec, status, progress, checkpoint, owner and heartbeat, engine version |
| `census_rows` | rule of a census job and its summary |
| `provenance` | event in a record's history (see above) |
| `id_aliases` | old id of a record (see above) |
| `ants` | ant or turmite run: content id, rule, tiling, pin, start, outcome, highway period and displacement, curation, `needs_reverify` |
| `quickstart` | Quick Start entry: its scene, title, status, pin and `needs_reverify` |
| `quickstart_usage` | how often the app opened a Quick Start entry (local only) |
| `ratings` | rating of a pattern, ant run, Quick Start entry or search run (see below) |
| `media` | picture or animation rendered for a record: its kind and mode, the SHA-256 and extension of its file, its render key (see below) |

## Ratings of search runs

People judge an exploratory search's runs with a verdict, *keep*, *maybe* or *drop*
(`jetcarp jobs review`). A verdict is a row of `ratings` like any other, so that it stays with the
library when the search's folder is deleted or shared:

| Column | Holds |
| --- | --- |
| `target_kind` | `search_run` |
| `target_id` | `<search uuid>:<run id>`: the search's folder name and the run's id in its [run records](search-runs.md) |
| `verdict` | `keep`, `maybe` or `drop` (empty for the 1 to 5 ratings of other records) |
| `rating` | the rating that stands for the verdict: keep 5, maybe 3, drop 1 |
| `rater` | who judged: a name, `user` or `model:<name>` |
| `note` | what was seen, in a sentence |
| `source` | where it came from: `review` (one at a time), `ratings:<folder>` (a folder of rating files from the clip-rating page; the taste rounds of [the search imported from CART](search-runs.md#the-search-imported-from-cart) are `ratings:ratings1` to `ratings:ratings4`); a folder by its name only, never where it was on the disk |
| `created` | when (UTC): the rating file's time, or the file's own when it holds none |

Reading a folder of rating files again records only the ratings not already recorded (the same run,
verdict, note, rater, time and source).

## Media files

The pictures and animations rendered for records (`jetcarp media render`) are files in the `media`
folder beside the library file, named by the SHA-256 of their bytes: `media/ab/<sha256>.<ext>`, where
`ab` is the hash's first two hexadecimal digits and the extension is `png`, `gif`, `webp` or `txt`.
Identical renders, such as a pattern alone and the Quick Start entry made from it, are one file.

The table `media` has a row per record, kind and mode: `target_kind` (`pattern`, `quickstart`,
`ant`) and `target_id`, `kind` (`frames`, `gif` or `webp`, `poster`, `thumb`), `from_seed` (shown
from the seed, or alone), the file's `hash` and `ext`, the `key` it was rendered with (a hash of the
inputs, options and house style: another key means the file is out of date), `width`, `height`,
`frames`, `bytes`, and `render` (JSON: the window, scale, delay, palette and so on).

A sequence of frames is a PNG file per frame and a **frame list**, the `txt` file the `frames` row
names: the line `jetcarp-frames 1`, then the SHA-256 of each frame, one per line, in order. The
frames stay ordinary PNG files, and a loop's repeated phases are stored once.

Deleting a record's media deletes its rows only, never a file, because another row may name the same
file; `jetcarp media gc` (the operation `media.gc`) deletes the files no row names (with `--dry-run`,
lists them) and leftover temporary files, leaving files younger than ten minutes in case a render in
another program has not recorded them yet. Keep one library file per folder: two libraries in one
folder share its `media` folder, and `media gc` for one would delete the other's files (they can be
rendered again). Files are written to a temporary name and renamed, so a
file is never seen half written. The website export (`jetcarp site export`) and
`jetcarp media export` copy files out under readable names.
