# Scene file format

A scene is a JSON file that holds everything needed to reproduce a run: the tiling, the starting
state of the random-number generator, the automata with their rules and seeds, and the log of every
command applied since. Loading a scene rebuilds the tiling, adds the automata, and replays the log,
so the world comes back in exactly the state it was saved in; a saved scene also records the
generation and state hash it reached, so `jetcarp verify` can check that it still does. Scenes are
what the app saves and opens, what `jetcarp run` takes, and what every figure in this book is made
from. How to use them is in [Scenes: saving, sharing, reproducing](../guides/scenes.md); this page
describes the file.

## An example

A scene can be very short: anything left out takes its default. This one ships with JETCARP
(`crates/jetcarp-core/data/scenes/rule30-life-ants.json`) and appears in Quick Start:

```json
{
  "name": "Rule 30 meets Life and ants",
  "description": "Rule 30 grows from one cell, Life rewrites what it leaves behind, and three ants take 100 steps per generation. All three share one binary field, so they interact.",
  "tiling": {"family": "square", "size": {"rect": {"w": 240, "h": 160}}, "boundary": "dead"},
  "rng_state": 30,
  "cas": [
    {"rule": "W30", "seed": "top_center"},
    {"rule": "B3/S23", "seed": "empty"},
    {"rule": "Ant RL", "steps_per_gen": 100, "seed": {"ants": {"count": 3}}}
  ]
}
```

- `tiling`: a square grid of 240 by 160 cells whose edges are dead (cells past the edge count as
  empty).
- `rng_state`: the starting state of the scene's random-number generator. The seeds that use random
  numbers (here the positions of the second and third ants) draw from it, so the same number gives
  the same start every time.
- `cas`: three automata, run in this order every generation. Rule 30 starts from one cell in the top
  row; Life starts with nothing of its own; the ants are three agents, one at the centre and two at
  random cells, each taking 100 steps per generation. All three rules are binary, so by default they
  share one field and act on each other's cells.

Running it for 100 generations and saving it (`jetcarp run … --gens 100 --save out.json`) writes the
same scene with every default filled in, the command log, and a record:

```json
{
  "jetcarp_version": "0.4.0",
  "format": 1,
  "name": "Rule 30 meets Life and ants",
  "description": "…",
  "tiling": {"family": "square", "size": {"rect": {"w": 240, "h": 160}}, "boundary": "dead"},
  "rng_state": 30,
  "cas": [
    {"id": 1, "name": "Rule 30", "rule": "W30", "neighborhood": "touch", "enabled": true,
     "affects_others": true, "steps_per_gen": 1, "palette": "Ember", "seed": "top_center"},
    {"id": 2, "name": "Life", "rule": "B3/S23", "neighborhood": "touch", "enabled": true,
     "affects_others": true, "steps_per_gen": 1, "palette": "Ember", "seed": "empty"},
    {"id": 3, "name": "Langton's ant", "rule": "Ant RL", "neighborhood": "edge", "enabled": true,
     "affects_others": true, "steps_per_gen": 100, "palette": "Ember",
     "seed": {"ants": {"count": 3}}}
  ],
  "commands": [{"op": "step", "gens": 100}],
  "record": {"generation": 100, "hash": "1adf6e3234b7f199", "jetcarp_version": "0.4.0",
             "dead_boundary": true}
}
```

(JETCARP writes the file with one field per line; it is folded here to fit.) `jetcarp verify out.json`
replays it and answers `ok: generation 100 hash 1adf6e3234b7f199`.

## The top-level fields

| Field | Type | Default | Meaning |
| --- | --- | --- | --- |
| `jetcarp_version` | text | empty | The JETCARP version that wrote the file |
| `format` | number | `1` | The scene format version (see [Versions](#versions)) |
| `name`, `description` | text | empty | Shown in the app and in Quick Start |
| `tiling` | object | required | The tiling (see [below](#tiling-tiling)) |
| `rng_state` | number, or text | `0` | The starting state of the scene's random-number generator (SplitMix64, written out in JETCARP's own code so that it never changes with a dependency): a whole number from 0 to 2^64 − 1, as text beyond 2^53 − 1 (see [Large numbers](#large-numbers)) |
| `tables` | list | none | [Rule tables](../automata/rule-tables.md#jetcarps-own-format) that the scene's `Table <name>` rules use, beyond the built-in one |
| `cas` | list | none | The automata, in the order they run (see [below](#automata-cas)) |
| `palettes` | list | none | Palettes of the scene's own: `{"name": …, "stops": [[r, g, b], …], "background": [r, g, b], "stage": [r, g, b]}`, the last two optional (see [Palettes](palettes.md#choosing-a-palette)) |
| `script` | object | none | A scripted sequence that starts with the scene (see [below](#script-and-reseeding)) |
| `reseed` | list | none | Automatic reseed policies that start with the scene |
| `commands` | list | none | The command log, replayed in order after the automata are added (see [below](#commands)) |
| `view` | any | none | The app's camera and display state; the engine ignores it |
| `record` | object | none | The generation and hash when saved (see [below](#record-and-verification)) |

Fields that JETCARP does not know are ignored when a scene is read.

### Large numbers

Most programs read a JSON number as a double (JavaScript does, and so does the canonical JSON that
packs use), and a double holds every whole number exactly only up to 2^53. So an integer outside
±(2^53 − 1) is written as a string of its decimal digits, as I-JSON
([RFC 7493](https://www.rfc-editor.org/rfc/rfc7493#section-2.2)) recommends, and a smaller one as a
number:

```json
"rng_state": "17705299566207515059"
"rng_state": 30
```

A random state can be anything up to 2^64 − 1, so most of the states JETCARP makes itself (an
atlas entry's, a search's soups) are written as text. JETCARP reads either form wherever the scene
holds a whole number: `rng_state`, the `state` of `set_rng_state`, the `gens` of `step` and `jump`
(a jump can be 2^60 generations), the record's `generation`, a pile seed's `grains`, a sandpile's
`add_random` count, and the generation counts of scripts and reseed policies. Scenes written before
this rule, with every number a number, read as they always have, and either form gives the same
world and the same state hash. The same rule holds for every JSON file JETCARP writes, and for the
answers of its API and MCP server.

## Tiling (`tiling`)

| Field | Meaning |
| --- | --- |
| `family` | The tiling's id, as `jetcarp tilings` lists them: `square`, `hex`, `hex_flat`, `triangle`, `cairo`, the Archimedean tilings (`oct`, `trihex`, `snubsq`, …), the drawn families (`lizard`, `brick`, `quad`, …), the aperiodic `penrose-kd`, `penrose-rhomb`, `hat` and `spectre`, an isohedral type such as `ih28` (`jetcarp isohedral` lists the 81), or `periodic` for catalogued and custom periodic tilings |
| `params` | The family's shape parameters, such as `{"catalog": "k3-011"}` for a k-uniform tiling of the [catalogue](../tilings/catalogue.md); left out for the defaults |
| `edges` | For tiles drawn with the tile editor: the shape of each edge |
| `size` | `{"rect": {"w": 240, "h": 160}}` for exactly that many lattice unit cells, `{"cells": 2000}` for about that many cells in a roughly square region, or `{"level": 4}` for the substitution level of an aperiodic patch |
| `boundary` | `"dead"` (the default: cells past the edge are missing, so they count as empty), `"wrap"` (a torus) or `"live"` (cells past the edge count as alive). Aperiodic patches always have dead edges |

## Automata (`cas`)

Every field but `rule` may be left out; a saved scene writes them all.

| Field | Default | Meaning |
| --- | --- | --- |
| `rule` | required | The rule, in [notation](rule-notation.md). Names such as `Life` are not read here |
| `id` | the next free number | How commands and scripts refer to this automaton |
| `name` | the rule's catalogue name, else its notation | A label, also usable in scripts |
| `neighborhood` | the rule's default | `"touch"` or `"edge"`; a `V` in the rule picks edge too. Ignored by rules that fix their own |
| `enabled` | `true` | A disabled automaton does not step |
| `affects_others` | `true` for binary rules, else `false` | Whether it shares the field with the other automata of its kind, or keeps a private one that is only drawn alongside. Sandpiles always keep their own |
| `steps_per_gen` | `1` | Steps this automaton takes each generation (1 to 100,000) |
| `palette` | Ember (Tide for JACA) | Its [palette](palettes.md) |
| `palette_neg` | Ember | JACA only: the palette of −1 cells |
| `glow` | `true` | JACA only: draw the fading glow of destroyed cells |
| `color_by` | `"auto"` | `"auto"`, `"age"`, `"state"` or `"row"` (see [Palettes](palettes.md#how-a-palette-colours-cells)) |
| `seed` | the rule's default | What it puts on the field when the world is built or reset (see [Seeds](#seeds)) |
| `drop_per_gen` | `0` | Sandpiles: grains dropped on the centre cell each generation |
| `agent_edge` | `"bounce"` | Ants at a dead edge: `"bounce"` (turn around) or `"fall_off"` (leave the grid) |

The automata step in list order, each `steps_per_gen` times, every generation. On a shared field a
later automaton sees what an earlier one did in the same generation. See
[Several automata at once](../guides/several-automata.md).

### Seeds

A seed is either a word or an object with one key:

| Seed | Meaning |
| --- | --- |
| `"empty"` | Nothing |
| `{"random": {"density": 0.3}}` | Each cell live with this probability (sandpiles: a random stable height) |
| `"center"` | One live cell at the centre |
| `{"blob": {"radius": 0.3, "density": 0.3}}` | Random live cells in a central disc; the radius is a fraction of half the shorter side |
| `"top_center"`, `{"top_random": {"density": 0.5}}` | One cell, or a random row, at the top (row rules) |
| `"random_states"` | A random state in every cell (multi-state rules) |
| `{"ants": {"count": 3}}`, `{"ant_at": {"cell": 120, "heading": 0}}` | Ants: one at the centre and the rest at random cells, or one on a given cell facing a given edge slot |
| `{"pile": {"grains": 10000}}` | Sandpile grains on the centre cell |
| `{"signed": {"density": 0.3, "negative": 0.5}}`, `{"halves": {"density": 0.3}}` | JACA: a mixed soup, or +1 on the left half and −1 on the right |
| `"glider_pair"` | The Spectre glider's two-tile seed near the centre |
| `"wire_demo"`, `{"circuit": {"sites": 12, "electrons": 6}}` | WireWorld: clock loops on squares, or wiring along the borders of random regions on any tiling |
| `{"cells": {"cells": [[120, 1], [121, 3]]}}` | Explicit `[cell, state]` pairs (sandpiles: grains added) |
| `{"gaussian": {"count": 3, "radius": 8, "peak": 1}}` | Lenia: Gaussian bumps |
| `{"lenia": {"name": "orbium"}}` | Lenia: a built-in creature at the centre (`"scale"` optional) |
| `{"pattern": {"shape": "blobs"}}` | A shape (blobs, clusters, stripes, rings, symmetric seeds and more) with an optional `fill` |

The command line, scripts and MCP tools also accept short forms such as `random:0.2` or `ants:3`
(the `reference.seeds` operation lists them); a scene file needs the forms above. Without a `seed`,
each rule takes its usual one: a 30% random soup for B/S rules, one ant for turmites, one cell in the
top row for elementary rules, a pile for sandpiles, and so on. A B/S rule added to a shared field
that already has live cells starts from those cells instead.

## Commands

The `commands` list is the world's history since its base: each entry is an object whose `op` names
the command. The app's buttons, the command line, scripts and the MCP tools all change a world
through these, which is why replaying them reproduces it.

| `op` | Fields | Effect |
| --- | --- | --- |
| `step` | `gens` | Run generations |
| `jump` | `gens` | Advance with HashLife (one automaton on a wrapped power-of-two grid); the cells match `step`, but every live cell ends with age 1 |
| `add_ca`, `remove_ca`, `update_ca` | `ca`; `id`; `id`, `change` | Add an automaton, remove one, or change some of its fields |
| `seed`, `clear`, `set_cells` | `id` with `seed`, nothing, or `cells` | Add a seed to an automaton's field, clear it, or paint `[cell, state]` pairs |
| `paste_pattern` | `id`, `pattern`, `at`, `orientation`, `phase` | Paste a [pattern library](../guides/pattern-library.md) pattern; the log keeps its cells inline, so the scene does not need the library to replay |
| `reseed`, `set_reseed` | `ca`, `mode`; `policies` | Reseed now, or replace the automatic reseed policies |
| `sandpile` | `id`, `action` | Sandpile tools: `{"add_everywhere": {"grains": 3}}`, `{"add_random": {"count": 1000}}`, `"stabilize"`, `"identity"` |
| `add_table`, `set_palette` | `table`; `palette` | Add a rule table or a palette |
| `set_script`, `script_goto` | `script`; `step` | Install a script (or remove it with `null`), or jump to one of its steps |
| `set_tiling`, `set_rng_state`, `reset` | `tiling`; `state`; nothing | Start over on a new tiling, with a new random state, or from the seeds. These start a new base, so a saved scene holds them as its starting fields rather than in its log |

## Script and reseeding

A `script` is a list of steps, each naming the automata that run during it and a trigger that moves
on to the next:

```json
"script": {
  "steps": [
    {"name": "Rule 30 fills the grid", "run": ["Rule 30"], "until": {"when": "generations", "n": 149}},
    {"name": "Day & Night until it calms", "run": ["Day & Night"],
     "until": {"when": "activity_below", "fraction": 0.002, "for_gens": 20}, "max_gens": 400},
    {"name": "Ants", "run": ["Langton's ant"], "until": {"when": "never"}}
  ],
  "repeat": false
}
```

Automata are named by `id` or by `name`. The triggers are `generations` (`n`), `population_below`
and `population_above` (`n`), `activity_below` and `activity_above` (`fraction`, `for_gens`),
`repeats` (`max_period`), `done` and `never`; `max_gens` is an optional limit on a step. Triggers are
checked from the world's own state after every generation, so a scripted scene replays exactly. The
built-in scene `script-five.json` is a complete example. Reseed policies (`reseed`) are described in
[Reseeding](../guides/reseeding.md).

## Record and verification

`record` is written when a scene is saved:

| Field | Meaning |
| --- | --- |
| `generation` | The generation the world had reached |
| `hash` | The world's state hash then, 16 hexadecimal digits |
| `jetcarp_version` | The version that saved it |
| `dead_boundary` | Whether some cells have missing neighbours (dead edges or a patch boundary) |

`jetcarp verify scene.json` (or the `scene.verify` operation) replays the scene and reports whether it
reaches the same generation and hash. The hash covers every field's states and ages, sandpile heights,
Lenia's exact values, the generation number, ants, and the progress of scripts and reseed policies,
so two worlds with the same hash are in the same state; see
[Reproducibility](../research/reproducibility.md). A PNG written by `jetcarp run --png` carries the
same information in text chunks: `jetcarp:scene` (the whole scene), `jetcarp:version`,
`jetcarp:generation`, `jetcarp:hash` and `jetcarp:dead_boundary`.

## Versions

Each scene names its `format`, now 1. A scene with a higher format than the running JETCARP knows is
refused (*"scene format 2 is newer than this JETCARP supports (1)"*); an older or equal one is read,
with any field it lacks taking its default and any field JETCARP does not know ignored. The
`jetcarp_version` field records who wrote the file but does not change how it is read.

The format is not yet declared stable: that is planned for JETCARP 1.0, together with the pack format,
the command line, the scripting API and the MCP tools. Until then a release may still change it.

A change to a rule or a tiling's construction changes the state a scene reaches even when the file
reads the same. JETCARP's tests pin the geometry of every tiling and the dynamics of every built-in
scene so that such a change cannot happen by accident (see
[Reproducibility](../research/reproducibility.md#pinned-geometry-and-dynamics)). One such change was made
on purpose before this release: the Penrose rhomb (P3) construction was rewritten, so `penrose-rhomb`
lays out its tiles differently from CART's, JETCARP's predecessor, and what ran on it in CART does not
replay to the same hashes.

## JSON Schema

There is no JSON Schema for scenes yet. The scene types are defined in JETCARP's Rust code
(`crates/jetcarp-core/src/world.rs`, with the tiling in `topology.rs`), and the operation that reads a
scene (`jetcarp api schema scene.load`) accepts any JSON object for it. A schema for scenes and packs
is planned.
