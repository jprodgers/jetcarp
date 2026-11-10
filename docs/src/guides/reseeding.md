# Reseeding

Most worlds settle down sooner or later: a Life soup turns to ash, Brian's Brain dies out, a sandpile
stops. *Reseeding* puts new life into a world: once, by hand, or automatically, through *policies*
that fire when the world has settled or every so many generations. It is what keeps a wallpaper or a
screensaver lively for hours. Reseeding works the same way in the app, on the command line, in
scripts, through MCP and in scene files, and every reseed is planned from the world's own random state,
so a reseeded run replays exactly.

## The three modes

| Mode | What it does | Settings (defaults) |
| --- | --- | --- |
| `random` | Every cell flips with a probability: dead cells come alive, live or dying cells die | `flip` (0.05) |
| `rain` | Drops fall on random cells: small, roughly round patches in which each dead cell comes alive with a probability. Rain only adds cells | `drops` (24), `min_size` and `max_size` in tiles across (2 and 10; small drops are more likely), `density` (0.5) |
| `blobs` | A few large patches with a wobbly outline; every cell inside is set alive or dead at a set ratio, overwriting what was there | `count` (1), `min_radius` and `max_radius` as fractions of the grid's shorter side (0.06 and 0.18), `live` (0.35) |

Sizes are measured in typical tiles, so a drop six tiles across looks the same on squares, hexagons,
mixed tilings and aperiodic patches; on wrapped grids, drops and blobs wrap across the edges. A
reseed writes into the field of every running automaton (sandpiles excepted), or into one
automaton's field if you name it. New live cells are in state 1, with age 1.

On a [JACA](../automata/jaca.md) field, whose live cells are +1 or −1, each mode also takes a `sign`:
`either` (the default: each drop or blob is all +1 or all −1, at random; in random mode, each cell),
`mixed` (each new cell +1 or −1 at random), `alternate` (drops and blobs alternate, and the first sign
changes with every reseed), `pos` or `neg`. Other fields ignore it.

## Reseed now

- **In the app:** *Seed → Reseed now → Random*, *Rain* or *Blobs*, or the *Reseed now* buttons in the
  *Reseed* pane. Their settings, and which automaton they aim at, are under *Reseed now
  settings* in the same pane (*Defaults* puts them back).
- **On the command line:** `jetcarp run scene.json --reseed-now rain --gens 500` reseeds once before
  running. The mode is a name (`random`, `rain`, `blobs`, optionally with a JACA sign: `rain:neg`) or
  JSON: `--reseed-now '{"kind":"rain","drops":40}'`.
- **In a script:** `w.reseed("blobs")` reseeds every running automaton's field, and
  `w.reseed(id, #{ kind: "rain", drops: 30, min_size: 2, max_size: 8 })` one automaton's.

A reseed by hand also restarts the cooldown of every automatic policy.

## Automatic policies

A policy has a mode, a trigger (`when`), a cooldown, and optionally a target automaton (`ca`; without
one it reseeds every running automaton). There are two triggers:

- **`every`** fires every `gens` generations (1 means every generation: a continuous drizzle).
- **`settled`** fires when the world has settled. It is checked every 16 generations once the cooldown
  is over, and fires when any one of these holds:
  - fewer than `activity` of the cells (default 0.002, that is 0.2%) changed in each of the last
    `for_gens` generations (default 60);
  - the number of cells changing each generation has repeated with a period of at most `cycle`
    (default 12, at most 64) over those generations, which means only oscillators and gliders are
    left (Life's ash keeps about 1% of its cells blinking, so a threshold alone would never see it
    settle);
  - an exact repeat of the whole state with a period of at most `period` was seen (off by default);
  - the running fields are empty;
  - `latest` generations have passed since the policy last fired, whatever the activity (off by
    default). This makes sure a screensaver never stalls.

The **cooldown** is the number of generations after a policy fires (or is installed, or a reseed by
hand) during which it cannot fire, so that a new seed has time to develop; a settled policy starts
counting quiet generations only once it is over.

Policies run independently of each other, and that can surprise you: a frequent `every` policy keeps
the world busy, so a `settled` policy beside it may never see it settle. Here Life ran for 3,000
generations with blobs when settled and ten drops of rain every 500 generations:

```text
$ jetcarp run crates/jetcarp-core/data/scenes/life.json --gens 3000 \
    --reseed '[{"mode":"blobs","when":"settled","cooldown":200},{"mode":{"kind":"rain","drops":10},"when":{"kind":"every","gens":500}}]'
...
reseed:     blobs when settled: fired 0 times, last at -
reseed:     rain every 500: fired 6 times, last at 3000
```

Giving the settled policy a `latest` (*Anyway after* in the app) makes it fire anyway.

## The Reseed pane

*Seed → Reseed* opens the *Reseed* pane. At the top is a graph of the share of cells that changed in
each of the last 512 generations, on a log scale, with the settled thresholds in orange, automatic
reseeds in pink and reseeds by hand in blue; beside it are the current activity, the population and
the generation.

**Presets** install a ready-made set of policies with one click. Their drop counts are tuned for a
grid of 200 × 140 squares and scaled to the size of yours:

| Preset | Policies |
| --- | --- |
| Screensaver | Blobs when the world settles (or after 2,400 generations at the latest, cooldown 150), and a light rain every 400 generations: keeps any rule lively without turning it to noise |
| Gentle drizzle | A few small rain drops every 30 generations |
| Continuous rain | One or two tiny drops every 3 generations |
| Storm | Many large drops every 8 generations |
| Blobs when settled | Two large patches at 35% live when the world settles, or after 4,000 generations at the latest (cooldown 150) |
| Full refresh when settled | Flip 25% of all cells when the world settles (cooldown 300) |

Type a name and press *Save as preset* to keep the current policies as a preset of your own (saved in
the app's settings and shown with a ★); *Delete…* removes one.

**Automatic policies** lists the policies, each with a checkbox that switches it on and off, its mode
and settings, the *Sign* when a JACA field is involved, *When* (*Settled*, with *Activity below*,
*Repeating up to*, *Exact period up to* and *Anyway after*; or *Every N gens*, and *Continuous* for
every 3 generations), the *Cooldown*, and which automaton it reseeds. Each shows how often it has
fired. *Add policy* and *Remove all* do what they say. Your edits are applied a moment after you stop
changing them, as one logged command, so they are saved with the scene.

*Seed → Auto reseed* switches all the policies on or off at once; when there are none, it installs the
Screensaver preset.

## From the command line

`jetcarp run --reseed` installs policies before the run: one policy as a JSON object, or a list. The
mode and the trigger can be given by name, with their defaults (`"rain"`, `"settled"`), or as objects
with any of their settings. The report has a line per policy saying how often it fired, and `--save`
keeps the policies in the saved scene, where `jetcarp verify` replays them.

## In a script

```rhai
let w = load_scene("life.json");
w.set_reseed([
    #{ mode: "blobs", when: #{ kind: "settled", latest: 2400 }, cooldown: 150 },
    #{ mode: #{ kind: "rain", drops: 3 }, when: #{ kind: "every", gens: 400 } },
]);
w.step(5000);
print(w.reseed_status());   // how often each policy fired, and recent activity
w.set_reseed([]);           // remove them
```

## In a scene

A scene keeps its policies in its command log (a `set_reseed` command), or, to start with them, in
its top-level `reseed` list. Written out in full, a policy looks like this:

```json
{
  "mode": {"kind": "rain", "drops": 10, "min_size": 2.0, "max_size": 10.0, "density": 0.5},
  "when": {"kind": "settled", "activity": 0.002, "for_gens": 60, "cycle": 12, "latest": 2400},
  "ca": 2,
  "cooldown": 200,
  "enabled": true
}
```

Every field but `mode` and `when` may be left out. See
[Scene file format](../reference/scene-format.md#script-and-reseeding).

## Reseeding and reproducibility

Which cells a reseed touches depends only on the tiling and the world's random state, not on the
cells' current states, and the policies' progress is part of the state hash. So a reseeded run gives
the same hash at any thread count and on the CPU or the GPU, and settled policies are checked only
every 16 generations so that the GPU can run whole batches between checks (see
[Reproducibility](../research/reproducibility.md)).

## Screensavers and rotations

A [screensaver profile](wallpapers.md#screensaver-profiles) can carry reseed policies of its own,
which replace the scene's while it shows; their drop counts are scaled to the size of the screen's
grid, as the presets are. A [rotation](rotations.md) can give its items reseed policies, or none at
all.
