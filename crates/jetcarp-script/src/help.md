# JETCARP scripts (Rhai)

Scripts are written in Rhai (https://rhai.rs): `let x = 1;`, `for n in 0..10 { ... }`,
`if a > b { ... } else { ... }`, `fn name(a, b) { ... }`, lists `[1, 2]`, maps `#{ key: 1 }`,
closures `|w, n| w.population() > 0`, strings with `+`, and `` `text ${x}` `` interpolation.
The simulation runs in Rust; the script only orchestrates. Statements end with `;`.

## Worlds

| Function | What it does |
| --- | --- |
| `world(#{ tiling: "hex", size: "screen", boundary: "dead" })` | New world. `tiling`: a family (`square`, `hex`, `hex_flat` (flat-top hexagons, for `HexT`), `triangle`, `cairo`, `oct`, `snubsq`, `spectre`, `hat`, `penrose-kd`, ...), an isohedral type `ih01`..`ih93`, or a catalog id (`k3-012`). `size`: `"screen"` (default: fill the screen at `cell_px`), a cell count, `[w, h]`, `"WxH"`, `#{cells: n}`, or `level: n` (aperiodic). `boundary`: `dead`, `wrap`, `live`. Also `rng_seed`, `screen: "WxH"`, `cell_px`, `params`, `cas: ["Life", #{rule: "W30", seed: "top_center"}]`. |
| `world()` | Square grid filling the screen, dead edges. |
| `load_scene(path)` / `scene_world(json)` | A world from a scene file or scene JSON (replayed to its saved state). |
| `w.add_ca(rule)` / `w.add_ca(rule, #{ seed: "random:0.3", neighborhood: "edge", palette: "Tide", steps_per_gen: 2, name: "a" })` | Add a CA; returns its id. Rules by notation (`B3/S23`, `W30`, `HexW3 8229`) or name (`"Brian's Brain"`). Seeds: `empty`, `random`, `random:0.2`, `center`, `top_center`, `top_random:0.5`, `random_states`, `blob:0.3:0.5`, `ants:3`, `pile:10000`, `glider_pair`, `wire_demo`, `#{random: 0.2}`; Lenia also `gauss:count:radius:peak`, `lenia:orbium`. Pattern seeds, on any tiling: `pattern:blobs`, `pattern:voronoi:0.8:by_region` (shape, density, states), or `#{shape: "blobs", count: 3, min_radius: 0.05, density: 0.4, states: "by_region"}`; shapes `all`, `disc`, `blobs`, `clusters`, `dots`, `ring`, `rings`, `stripes`, `checker`, `gradient`, `radial_gradient`, `noise`, `voronoi`, `symmetric`; states `one`, `uniform`, `range:lo:hi`, `signed:share` (JACA), `by_region`. Other options: `affects_others`, `enabled`, `drop_per_gen`, `color_by`, `agent_edge` (`bounce`/`fall_off`). |
| `w.update_ca(id, #{ rule: "B36/S23", enabled: false, palette: "Ink" })` | Change a CA (same options as `add_ca`, plus `rule`). |
| `w.remove_ca(id)`, `w.seed(seed)`, `w.seed(id, seed)`, `w.clear()`, `w.clear(id)`, `w.reset()` | Remove a CA; seed (adds to the field); clear a field; reset every CA to generation 0. |
| `w.apply(#{ op: "sandpile", id: 1, action: "stabilize" })` | Any scene command (map or JSON text). |
| `w.reseed("blobs")`, `w.reseed(id, #{ kind: "rain", drops: 30, min_size: 2, max_size: 8, density: 0.5 })` | Reseed now: `random` (`flip`), `rain` (`drops`, `min_size`, `max_size` in tiles across, `density`), or `blobs` (`count`, `min_radius`, `max_radius` as fractions of the shorter side, `live`); every running CA's field, or CA `id`. |
| `w.set_reseed([#{ mode: "blobs", when: "settled", cooldown: 200 }, #{ mode: "rain", when: #{ kind: "every", gens: 500 } }])` | Automatic reseed policies (`[]` removes them). `when`: `settled` (`activity`, `for_gens`, `cycle`, `period`) or `every` (`gens`). Saved in the scene. |
| `w.reseed_status()` | `#{ policies: [#{ since, streak, fired, last_fired }], activity, history }`: what each policy did, the last generation's changed fraction, and recent ones. |
| `w.copy()` | An independent copy (same state and hash). |

## Running

| Function | What it does |
| --- | --- |
| `w.step()`, `w.step(n)` | Run 1 or `n` generations. |
| `w.run(#{ until: "done", max_gens: 5000 })` | Run until a condition or the limit; returns `#{ generations, generation, stopped, done, done_reason }` (`stopped` is `"until"` or `"max_gens"`). `until`: `done`, `repeats` (with `max_period`, default 1000), `frozen`, `empty`, `generations` (with `n`), `population_below`/`population_above` (with `n`), `activity_below`/`activity_above` (with `fraction`, `for_gens`), `never`. Without `until`: runs `gens` generations. |
| `w.done()`, `w.done_reason()` | True when nothing can change any more: an elementary rule (`W`, `HexW`, `HexW3`, `HexT`, `HexT3`) wrote the bottom row (wrap edges: one full pass), a layered rule wrote its last layer, a sandpile is stable, all ants left, a field rule died out, or the world is frozen. |
| `w.track_changes(true)` | Count changed cells every generation (frozen worlds are then seen at once). |
| `w.jump(n)` | HashLife jump in place: `n` generations (a number, or text like `"2^40"`) on a wrap-edge lattice world whose sides are powers of two, with Life-like, Generations (B0 included), WireWorld, or Greenberg–Hastings CAs (one or several, on shared or separate fields). Same states as `w.step(n)`; live cells end with age 1. Logged as a `jump` command. `w.can_jump()` is `true` or the reason it cannot. |
| `hashlife(w, n)`, `hashlife(w, n, #{ region: [i0, j0, w, h], boundary: "dead", node_cap: 48000000 })` | HashLife on the unbounded plane: `w`'s pattern (edges ignored) `n` generations later, as a new world (default: the same frame and edges as `w`; `region: "fit"` with `margin` takes the bounding box). Compare results with `w.states_hash()`. |

## Reading

| Function | What it does |
| --- | --- |
| `w.generation()`, `w.hash()`, `w.population()`, `w.population(id)` | Generation, state hash (hex), live cells (all fields / one CA's field; grains for sandpiles). `w.states_hash()` hashes cell states only (no generation or ages), for comparing with HashLife. |
| `w.status()` | Map: generation, hash, status text, done, done_reason, period, population, populations, cells, width, height, tiling, cas. |
| `w.period()` | Exact period once seen (1 = frozen), else `()`. |
| `w.cells()`, `w.width()`, `w.height()`, `w.tiling()`, `w.rule()`, `w.cas()` | Grid facts; `width` counts every tile of a row; the first CA's rule; the CAs as maps. |
| `w.get(x, y)`, `w.get(x, y, id)`, `w.get_cell(i)` | A cell's state (x = column, y = row from the top). |
| `w.set(x, y, state)`, `w.set(x, y, state, id)`, `w.set_cell(i, s)`, `w.set_cells([[x, y, s], ...])` | Draw cells. |
| `w.pattern(".O.\n..O\nOOO", x, y)` | Draw a text picture (`O`/`*`/`#` live, digits for states). |
| `w.row(y)` | One row's states as a list. |
| `w.objects()`, `w.objects(#{ max_period: 64 })` | Still lifes, oscillators, spaceships: `#{ summary, live_cells, totals: #{still_life, oscillator, spaceship, other}, counts: [#{name, label, kind, count, period, speed}], objects: [#{name, kind, period, speed, dx, dy, size, x, y, cells}] }`. |
| `w.classify()`, `w.classify(#{ max_gens: 2000, explode: 0.5 })` | Run on and classify as `died`, `frozen`, `oscillating` (with `period`), `exploded`, or `active` (with `activity`), like `jetcarp census`. |
| `w.describe()` | The manifest descriptors: population, density, `frac_<state>`, period, distinct_rows, row_period, row_shift, periodic_from, row_class (`empty`, `uniform`, `periodic`, `shifting`, `aperiodic`), row_entropy, row_hint (`nested`, `regular`, `chaotic`). |

## Saving

| Function | What it does |
| --- | --- |
| `out_dir("label")` | Make `<Pictures>/JETCARP/<date>_<time>_label/` and return its path. `JETCARP_OUT` (or `--out`) changes the root. Relative paths below are relative to the newest output folder. |
| `w.save_png(path)`, `w.save_png(path, #{ size: [800, 600], scale: 4, max_side: 2000, outlines: true, palette: "Ink", compression: "fast" })` | PNG with JETCARP's metadata (version, generation, hash, scene). Default size: the screen, world fitted and centred. Returns `#{ path, width, height, bytes }`. |
| `w.save_scene(path)`, `w.scene()` | Scene JSON (replays to the same hash). |
| `write_text(path, s)`, `append_text(path, s)`, `write_json(path, value)`, `write_csv(path, [#{...}, ...])` | Files inside the output root (or `--out`/allowed folders). |
| `read_text(path)`, `read_json(path)`, `exists(path)`, `join(a, b)`, `to_json(v)`, `parse_json(s)` | Reading and paths. |

## Sweeps

`sweep(#{ ... })` runs one world per rule in parallel on all cores and writes one PNG per run
plus `manifest.csv`, `manifest.json`, and `run.json` into a new output folder. Returns a summary
map (folder, runs, images, kept, errors, seconds, cancelled).

| Option | Meaning |
| --- | --- |
| `rules` | A template with `{n}` (`"HexW3 {n}"`) and `from`/`to`; a series name: `"W"` (0-255), `"HexW"` (0-15), `"HexW3"` (0-19682), `"HexT"` (0-255), `"HexT sum"` (0-15), `"HexT3 sum"` (0-2186), `"HexT3 sym"`, `"HexT3"` (3^27 rules: `sample` or `from`/`to`), `"B/S"` (with `birth: "1-8"`, `survive: "0-8"`, `states`, `sample: n`); or a list of rules. `sample: n`, `sample_seed: s`: a reproducible random sample of any numbered series. |
| `tiling`, `size`, `boundary`, `level`, `rng_seed`, `screen`, `cell_px` | As for `world` (default size: the screen). Every run uses the same RNG seed. |
| `seed`, `neighborhood`, `palette`, `steps_per_gen`, ... | As for `add_ca`. |
| `until`, `max_gens`, `n`, `fraction`, `for_gens`, `max_period` | As for `w.run` (default `max_gens` 10000). |
| `png` | `false` for no images; or image options (`size`, `scale`, `max_side`, `outlines`, `palette`, `compression`: `fast`/`balanced`/`high`). Default: the screen size, balanced compression. |
| `out` | Folder label (default from the rules, e.g. `HexW3`). `folder: path` uses (and resumes) a given folder instead. |
| `keep` | Optional `|w, n| ...` called after each run (in parallel): return true/false (write the image or not) or a map of extra manifest columns (with an optional `keep`). |
| `skip` | Leave out images of dull results: a list of `"empty"` (no live cells), `"full"` (no cell in state 0), `"uniform"` (at most two distinct rows; off lattices, every cell alike). The row stays in the manifest with `kept: false` and `skipped`. |
| `scene`, `ca` | Start every run from a scene (scene JSON, a map or text) instead of an empty world: its tiling, CAs, seeds, and the edits made before its first step (drawn cells). CA number `ca` (from 0, or a name; default 0) runs the sweep's rule; options like `seed` and `palette` apply to that CA. The app's *Sweep from current world* uses this. |
| `threads`, `resume` (default true), `describe` (default true) | Worker threads; skip runs already in the folder's manifest; compute descriptors. |

**Reading a sweep back:** `manifest(folder)` returns the rows as a list of maps (one per run, sorted by index; also while a sweep is still running), e.g. `for r in manifest(s.folder) { if r.row_hint == "nested" { ... } }`. `manifest.json` itself is `#{ columns: [...], rows: [...] }`. Missing values (JSON null) are `()` in Rhai: test `r.period == ()`; Rhai has no `null`. Then `library().promote_run(s.folder, r.rule)` puts a run in the Quick Start.

File names are zero-padded to the series width: `HexW3-08229.png`, `W030.png`, `HexT3-sum-0588.png`; rule lists use
`<index>_<rule>.png`. Manifest columns: index, n, rule, file, kept, generations, stopped, done,
done_reason, hash, population, density, frac_0.. (Lenia: mass, mean_value), period, row_period,
row_shift, periodic_from, distinct_rows, row_class, row_entropy, row_hint, png_bytes, error (and
`skipped`), then any columns from `keep`.

## Pattern library

The library is the SQLite catalog: `JETCARP_LIBRARY` if set (a folder, or a `.sqlite` file), else
`<data>/catalog.sqlite` (see the book's "The pattern library" guide). Records are
verified exactly before they are stored. A pattern's id does not depend on the rule: the same evolution
found in several rules is one pattern, with `rules` (every rule it was verified in) and, for B/S
and Generations rules, `range` (`min_rule`, `max_rule`, bitmasks, `states_min`/`states_max`:
every rule it runs identically in).

| Function | What it does |
| --- | --- |
| `library()`, `library(path)` | The catalog (or another catalog inside the output root: a folder, or a `.sqlite` file). `lib.path()`, `lib.count()`, `lib.count(#{ query })`, `lib.info()` (row counts). |
| `lib.catalog(w)`, `lib.catalog(w, #{ ca: id, max_period: 64, source: "..." })` | Find the objects in `w` (`objects()`), verify each distinct still life, oscillator, and spaceship, and store it (copies raise its seen count). Returns `#{ objects, added: [ids], merged: [ids], ids, skipped, failed }`. `lib.add_from(w)` is the same. |
| `lib.search(#{ rules, tilings, soups, size, boundary, gens, density, seed, max_period, soup_radius, store, label })` | Bounded soup search (as `jetcarp search bounded`; tilings by id, vertex configuration such as `"3.4.6.4"`, or set such as `"archimedean"`); a resumable catalog job whose verified finds go into this catalog (`store: false` only reports them). Returns `#{ groups: [#{ rule, tiling, soups, objects, distinct, patterns: [#{ id, label, class, period, speed, cells, count, new }] }], new_patterns, known_patterns, new_rules, job, ids, seconds }`. |
| `hunt(#{ rules, tilings, nb, soups, size, density, shape, seed, gens, max_period, pop_cap, margin, threads, resume, limit, label, store })`, `lib.hunt(#{ ... })` | Fast hunter (as `jetcarp search hunt`; `tilings` as text or tiling maps): small soups (default 16x16 at density 0.5, `shape` `"rect"` or `"disc"`) on an unbounded plane, run bit-parallel until they settle, escaping ships cut out and counted, each object classified once per form; the hunt is a resumable catalog job (`resume: job` continues one, `limit: n` stops after n soups, `soups: 0` runs until the script is cancelled). `store`: the storage policy, `#{ max_cells: 64, max_volume: 4000, min_seen: 2 }` by default (still lifes and oscillators above 64 cells or period x cells 4,000 are kept as `unstored` rows until seen twice; ships, guns, puffers always stored) or `"all"`. Results do not depend on `threads`. Returns the progress: `#{ job, status, soups_done, soups_total, soups_per_s, elapsed_s, settled, died, exploded, unsettled, objects, by_class, distinct, new_patterns, failed, newest: [#{ id, label, class, period, rule, tiling, soup, new, score }], groups: [#{ rule, tiling, soups, by_class, other, escaped, distinct, new_patterns, ... }] }`. |
| `lib.hunt_status(job)`, `lib.hunts()`, `lib.hunt_stop(job)` | A hunt's saved progress (with its `spec`), every hunt's, and a request that a running hunt (in any process) stop at its next checkpoint. |
| `ants(#{ tilings, strings, tables, alphabet, rules, soups, size, density, seed, steps, max_period, symmetry, threads, resume, limit })`, `lib.ants(#{ ... })` | Ant and turmite search (as `jetcarp search ants`): turn strings (`strings: [2, 6]` lengths, or one length) and turmite tables (`tables: [2, 2]` states x colours) on unbounded periodic tilings, from an empty field or `soups` seeded soups; every run is classed (`highway`, `periodic`, `symmetric`, `bounded`, `chaotic`, highways proved exactly) and stored in the catalog's `ants` table by a resumable job (`resume: job`, `limit: n` runs). Returns the progress: `#{ job, status, runs_done, runs_total, runs_per_s, steps_per_s, by_outcome, newest: [#{ id, rule, tiling, outcome, period, d, onset }], groups }`. |
| `ant_run(rule)`, `ant_run(rule, #{ tiling, steps, max_period, soup: [w, h], density, seed, store })` | One rule (`"RL"`, `"LLRR"`, `"RL2N"`, `"1R1,1L0/1R1,0N0"`), no catalog unless `store: true`: `#{ id, rule, tiling_name, outcome, period, d, speed, onset, steps, cells, run: #{ proof, symmetry, reach } }`. |
| `lib.ant(id)`, `lib.ant_list(#{ outcome, tiling, rule, job, sort, limit })`, `lib.ant_census()`, `lib.ant_status(job)`, `lib.ant_jobs()`, `lib.ant_stop(job)` | Stored ant runs (one, or filtered and sorted by `score`, `onset`, `period`, `speed`, `rule`), outcome counts per tiling, a search's saved progress with its `spec`, every search's, and a stop request. |
| `lib.collide(#{ a, b, offsets, dx, dy, phases, orientation_a, orientation_b, gens, margin, rule, store })` | Collide pattern `b` into pattern `a` at every offset (`offsets: n` for -n..n, or `dx`/`dy` `[lo, hi]`) and phase of `b` (as `jetcarp search collide`), in a rule both work in; each run classed (`both_survive`, `both_die`, `a_survives`, `b_survives`, `new_objects`, `unsettled`, `explodes`, `overlap`) and new objects verified and stored (`store: false` only reports them), as a catalog job. Returns `#{ job, a, b, rule, tiling, outcomes, products: [#{ id, label, runs, new }], runs: [#{ dx, dy, phase, outcome, gens, result, products }] }`. |
| `label` (on `hunt`, `census`, `lib.search`) | Names the job (`"survey · Hexagon (touch)"`; the Hunts window shows it). Called again with the same label, the job already in the catalog is resumed, or returned as it is when done, instead of a new one being started (a labelled `lib.search` that is done is not run again), so a script of labelled jobs can simply be rerun after a crash or reboot. `jetcarp jobs report --prefix "survey · "` writes a Markdown report of them. |
| `lib.jobs()`, `lib.jobs(#{ kind, label, prefix, status })` | Jobs, oldest first, as `#{ id, kind, status, label, spec, progress, created, updated, finished }`; `prefix` matches the start of the label. |
| `census(#{ tiling, level, cells, nb, rules, birth, survive, states, all_max, sample, soups, density, gens, seed, explode, label, resume, limit, threads })`, `lib.census(#{ ... })` | Census as a resumable catalog job (as `jetcarp search census`), on any tiling: the board on periodic tilings, the World on aperiodic patches (`level`, default the tiling's default level). Rules: `rules`, or every B/S rule of the digits up to `all_max` (default 32,768), else a seeded `sample` (default 2,000). Returns `#{ job, status, rules_done, rules_total, per_s, elapsed_s }`. |
| `lib.census_rows(job)` | A census job's rule summaries in rule order: `#{ rule, notation, runs, died, frozen, oscillating, active, exploded, majority, median_period, periods, median_settle, peak_population, median_final_density, mean_activity }`. |
| `rule_neighbours(rule)`, `rule_neighbours(rule, #{ tiling, nb })` | Every rule one birth or survival count away (B0 left out), digits up to the tiling's largest neighbour count (default square; `tiling` text or a tiling map); edge rules keep their `V`. |
| `lib.rate(kind, id, rating)`, `lib.rate(kind, id, rating, #{ rater, note })` | Rate a `"pattern"`, an `"ant"` run, or a `"quickstart"` entry 1 to 5, as `"user"` (default) or `"model:<name>"`; returns the rating's id. |
| `lib.query(#{ rule, works_in, neighborhood, tiling, class, min_period, max_period, min_score, state, tag, text, discovered_by, order, limit, offset })` | Records as maps (`id`, `label`, `name`, `rule`, `rules`, `range`, `tiling_name`, `class`, `period`, `speed`, `cells`, `seen`, `score`, `state`, `tags`, `notes`, `provenance`, `phases`, ...). `rule`: verified in a rule containing this text; `works_in`: runs in this rule (inside its range, or verified there); `order`: `score` (default), `seen`, `rare`, `newest`, `updated`, `cells`, `period`, `rating`, `id`. `lib.list()` is every record. |
| `lib.works_in(rule)` | Every pattern that runs in this rule (as `query(#{ works_in })`). |
| `lib.sightings(id)` | How often it was seen, per rule and job: `[#{ rule, job, count, soups, first_seen, last_seen }]`. |
| `lib.quickstart()`, `lib.quickstart(status)` | Quick Start entries ("My finds") as `#{ id, title, description, grid, rule, rule_family, behaviour, source_kind, source_id, score, status, created }`; `status` is `auto`, `candidate`, `approved`, or `rejected`. |
| `lib.get(id)` | One record by id, unique id prefix, or name. |
| `lib.update(id, #{ name, tags, add_tags, remove_tags, notes, append_notes, rating, state })` | Curate (`state`: `new`, `reviewed`, `promoted`, `rejected`). |
| `lib.promote(id)`, `lib.promote(id, title)` | Add a Quick Start entry for it ("My finds") and mark it promoted. Returns `#{ id, entry, title }` (`entry` is `find:<id>`). |
| `lib.promote_run(folder, run)`, `lib.promote_run(folder, run, #{ title, note })` | Put one run of a sweep folder in the Quick Start ("My finds"): its tiling, rule, and start, so it replays as in the sweep. `run` is the rule text (`"HexT3 sum 588"`), `"#index"`, or `"n=588"`; the run is starred in the folder's `favourites.json`. Returns `#{ rule, index, title, entry }`. |
| `w.apply(#{ op: "paste_pattern", id: 1, pattern: #{ library: "glider" }, at: #{ cell: #{ x: 5, y: 5 } } })` | Paste a library pattern into a world. |

## Glider search and movers

| Function | What it does |
| --- | --- |
| `evolve(#{ tiling, nb, states, pop, gens, seed, radius, horizon, cap, champions, verify, stop_on_glider, store, track_gens, threads, label })` | Edmond's evolutionary glider search (`jetcarp search evolve`; defaults: spectre, touch, 4 states, population 256, 800 generations, radius 48, horizon 2500, cap 64, verify `[1, 2, 4]`), a catalog job checkpointed every generation. Writes a run folder (`label`, default `evolve-<tiling>`: fitness.csv, champions.json, gliders.json, movers.json, run.json); verified gliders go to the library unless `store: false`. Returns `#{ dir, job, best_ever, generations, evaluations, unique_evaluations, elapsed, champions: [#{ rule, fitness, glider, check }], gliders: [rules], movers: [#{ kind, rule, ids, summary }], fitness: [#{ generation, best, median, best_ever }] }`. |
| `track_mover(#{ rule, tiling, level, cells: [[cell, state], ...], nb, max_level, gens, max_pop, store, window, rings, margin, fixed_levels })` | Follow a pattern on an aperiodic patch over every larger level up to 7 (`max_level: 8` for the 17-million-tile patches; `jetcarp library movers track`), or with `window: true` (or `rings: N`) on a sliding window that generates supertiles around it (a catalog job, as `jetcarp search mover`), to `rings` edge rings or `gens` generations (`margin`: world units kept around the live cells, default 24; `fixed_levels`: Spectre levels that follow the fixed patches, default 8). Returns the report: `verified`, `summary` ("verified to distance D ..." or why it stopped), `stop`, `reach` (`rings`, `steps`, `euclid`, `heading_deg`), `generations`, `level`, `levels`, `groups`, and on a window `window` (`top_level`, `max_tiles`, `recentres`, `gens_per_s`, `rings_per_s`); with `store: true` a verified mover is added to the library (`id`). |
| `library().query(#{ class: "mover" })` | Movers in the library; each record has `mover` (distance in rings, steps, and units; generations; patch level; levels run). |

## Large sandpiles

| Function | What it does |
| --- | --- |
| `sandpile_pile(#{ tiling, grains, neighborhood, centre, level, png, px, margin, palette, reverse, preview, folder, gpu, steps, label })` | Drop `grains` grains (a number, or text like `"1e7"` or `"2^30"`, up to 2^64 − 1; default 10000) on one spot of an unbounded grid and let them settle (`jetcarp pile`), as a catalog job (kind `pile`). Built by doubling from N's binary digits on a board that grows as needed (aperiodic tilings: the exact patch, `level` up to 8, chosen automatically; an error if the pile reaches its edge). `neighborhood`: `"edge"` (von Neumann, default) or `"touch"` (Moore). `centre`: `"cell"` (default), `"vertex"`, or `"edge"` (grains split evenly among the cells meeting at that point of the centre cell; the first `N mod m` cells by angle get one more). Writes `<out>/piles/<time>_<tiling>-<N>/`: `pile.png` (the pile's box plus `margin` cells, default 4, at `px` pixels per cell, default 2; `png: false` skips it; heights coloured along `palette`, default `"Ember"`, on its background and stage, `reverse: true` for the other way round; the choice is kept in `look.json` so a resumed folder keeps it), `preview.png` (`preview` pixels on the long side, default 1024), `pile.json` (N, tiling, neighbourhood, centre, topplings, sweeps, wall time, version, per-step records), `growth.csv`, and `steps/step-KK.pile` after every doubling step (the job's checkpoint; `steps` is accepted and ignored). `folder: path` uses that folder and resumes it (its job, or a new job from its last step file) if it holds step files. `gpu: true` topples on the GPU where the host has one (CLI, app; aperiodic patches stay on the CPU). Returns `#{ folder, png, preview, tiling, grains, steps, topplings, sweeps, seconds, width, height, image_width, image_height, backend, job }`. |
| `sandpile_piles(#{ tilings, grains, thumb, cols, ... })` | The same pile on several tilings (default: square, hex, triangle, trihex, snubsq, oct, rhombitrihex, cairo), each in a subfolder of one `piles/<time>_sheet-N` folder, plus `sheet.png` (previews `thumb` pixels, default 320, in `cols` columns, default 4) and `piles.json`. Other keys go to each `sandpile_pile`. Returns `#{ folder, sheet, runs }`. |

## Media and website export

| Function | What it does |
| --- | --- |
| `media(#{ pattern \| quickstart \| ant \| missing, class, limit, mode, format, periods, gens, every, frames, force, palette })`, `lib.media(#{ ... })` | Render media into the catalog (as `jetcarp media render`): per-step PNG frames, an animation (`format`: `"auto"`, a GIF up to 320 x 320 pixels, 8 million pixels in all, and 256 colours, else an animated WebP; or `"gif"`, `"webp"`), a poster, and a thumbnail, under `<catalog folder>/media/<kind>/<id>/<alone\|seed>/`. `pattern: id` (shown alone: one seamless period, `periods: n` for more; ships in a window that moves with them, guns with their stream), `quickstart: id` (from the seed, `gens` generations, default 200, row rules one per row, a frame every `every`), `ant: id` (a highway's loop alone, other outcomes from step 0), `missing: true` (every pattern and shown Quick Start entry with no or stale media; `class: "spaceship,gun"` limits it to patterns of those classes). `mode`: `"auto"`, `"alone"`, `"seed"`. Up-to-date media are skipped unless `force: true`; `frames: false` skips the PNG frames. `palette: "Paper"` colours them with a built-in palette (its background, stage, and stops) instead of the house style; the palette is part of the render key, so media stored in another palette render again. Returns `#{ written, skipped, done: [#{ kind, id, mode, written, files: #{ anim, poster, thumb, frames, width, height } }], failed: [#{ kind, id, error }] }`. |
| `w.media(#{ gens, every, format, frames, folder, palette })` | Render a world from its current state for `gens` generations into a new `<out>/<time>_media` folder (or `folder`). Returns `#{ folder, anim, poster, thumb, frames, width, height, frames_count }`. |
| `lib.site_export(#{ to, state, min_score, min_rating, min_osc_period, class, quickstart, frames, render, palette })` | The incremental website export (as `jetcarp site export`) into the folder `to` (`min_osc_period`: leave out oscillators of shorter period): `manifest.json`, `patterns/<id>.json`, `quickstart/<id>.json`, and their media. Public by default: patterns `promoted` or `reviewed` (`state: "promoted"` or a list), Quick Start entries `auto` or `approved` (`quickstart: "none"` for none). Only changed files are written; items no longer public are deleted. `palette` renders missing media in that palette. Returns `#{ dir, patterns, quickstart, written: [paths], deleted: [paths], unchanged, rendered, media_failed }`. |

## Other

| Function | What it does |
| --- | --- |
| `params` | Map of `--set key=value` values (numbers and true/false converted). |
| `screen()` | `#{ width, height, cell_px }` of the screen sizes fill. |
| `print(x)`, `log(x)`, `debug(x)` | Log lines (shown live and returned in the summary). |
| `progress(done, total)`, `progress(done, total, item)` | Report progress to the host. |
| `now()`, `timestamp()`, `elapsed()`, `stopped()` | Local time `2026-11-05_14-30-02`; Unix seconds; seconds since start; true once cancelled. |
| `series("HexW3 {n}", 0, 99)`, `series("W")` | A rule series as a list of rule strings. |
| `tilings()`, `catalog_tilings()`, `catalog_tilings(k)`, `isohedral_tilings()`, `named_rules()` | Tiling ids, k-uniform catalog ids, isohedral ids, named rules. |
| `sweep_script(#{...})` | The `sweep(...)` call for these options, as script text. |
| `show(w)`, `show(w, label)` | Show the world in the app's main view (a copy; the script carries on). Elsewhere (CLI, MCP) it only logs a line. |
| `jetcarp_version()`, `out_root()` | Version; the output root folder. |
| `jetcarp(op)`, `jetcarp(op, #{ ... })` | Any JETCARP operation by name, with its input as a map; returns its output as a map (`jetcarp api list` lists the operations, `jetcarp api schema OP` shows an operation's input and output). The script's file rules apply (files are written inside the output root and the allowed folders), the catalog is the run's, and world operations (`world.new`, `world.step`, ...) act on one private world kept for the whole run, not on `w`. Errors are script errors with the operation's message. Examples: `jetcarp("library.query", #{ class: "spaceship", limit: 5 }).patterns`, `jetcarp("quickstart.list").entries`, `jetcarp("reference.palettes")`, `jetcarp("world.new", #{ tiling: "hex", width: 20, height: 20, cas: ["Life"] })`. |
