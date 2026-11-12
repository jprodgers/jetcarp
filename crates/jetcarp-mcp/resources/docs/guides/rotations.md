# Rotations

A *rotation* is what the screensaver or the background host shows over time: a list of items, each a
[screensaver profile](wallpapers.md#screensaver-profiles), a scene file, a Quick Start entry or a whole
series of rules, played in order, shuffled, one a day or one an hour. Each item has the conditions that end it,
such as a number of minutes, the world settling, or a time of day, and can be limited to certain days
and hours. A rotation can also have a short script that decides when to move on. This guide covers the
built-in rotations, using one, and making your own.

## The built-in rotations

| Rotation | What it shows |
| --- | --- |
| *Wolfram series* | All 256 elementary rules, each drawn line by line to the bottom of the screen, then the next (4-pixel cells, in Ink) |
| *A classic a day* | A different classic each day, the same one all day, restarting if it ends: Life, Langton's ant, a forest fire, cyclic spirals, the turmite LLRR, Penrose waves and Hodgepodge. The background host runs this one unless told otherwise |
| *Classics, hourly* | The same classics, one each hour of the clock, in a new random order each time through, restarting if one ends |
| *Until it settles* | Life, HighLife (`B36/S23`) and `B368/S245` with no reseeding, each until it settles (at most 30,000 generations), then the next |

## Use a rotation

- **Screensaver mode in the app:** in the *Screensaver* pane, set *Show* to *A rotation* and choose it
  under *Rotation*, or start the app with `jetcarp --screensaver --rotation "Wolfram series"`.
- **The background host:** choose it under *Rotation* in the pane's *Background host* section, or run
  `jetcarp host rotation "Classics, hourly"` (add `--screen N` for one screen of several).
  `jetcarp host start --rotation NAME` runs one for that run only.
- **The system's screen saver:** `jetcarp host saver install --rotation NAME`, or *Screen saver runs* in
  the app. On macOS the rotation is copied into the screen saver, so install it again after changing
  the rotation (see [Wallpapers](wallpapers.md#the-systems-screen-saver)).

While a rotation runs, the ◀ and ▶ buttons in the pane, *Next* and *Previous* in the host's menu,
`jetcarp host next` and `previous`, and `jetcarp app next` for the app's screensaver skip between items.
A rotation that names an item that no longer exists (a deleted profile, say) leaves it out; one that
cannot be found at all shows Life instead.

## When a rotation moves on

*In order* and *Shuffled* rotations move on when an item ends: any of its endings holds (minutes,
generations, settling, finishing, a time of day), after its hold. An item with no endings takes its
profile's minutes, else the rotation's default, and with neither it runs until something else moves
on. The minutes count only while the rotation runs, so a pause doesn't use up an item's time.

*One a day* and *One an hour* rotations follow the clock instead, so the same item shows all day (or
all hour), and a program started again shows the same item:

- **One a day** shows item *d* mod *n* on day *d* (counting the days since 1 January 1970, by the
  computer's own date) of a rotation with *n* items, and the next one tomorrow, at midnight.
- **One an hour** shows one item each hour of the clock: every item once, in an order shuffled from
  the clock, then every item again in another order (never the same item twice in a row), changing
  on the hour.
- **An item that finishes starts again.** When nothing in it can change any more (Langton's ant has
  walked off the edge, a world has died), or one of its endings holds, it starts again from its
  beginning, once it has run at least a minute. It doesn't give way to the next item.
- **Skipping** with *Next* or *Previous* (or `jetcarp host next`) shows another item until the day or
  the hour changes; then the clock's choice comes back.
- An item that cannot be shown at all (a scene that won't build) gives way to the next, as in any
  rotation, until the day or the hour changes.

## Make a rotation in the app

In the *Screensaver* pane, set *Show* to *A rotation*. Under the rotation's name are *Edit*, *New*,
*Duplicate* and, for one of your own, *Delete* (for an edited built-in rotation, *Revert to built-in*).
The editor has:

- **Name**, and **Order**: *In order*, *Shuffled* (a new random order each time through), *One a
  day* (one item per calendar day, the next one tomorrow) or *One an hour* (one item per hour of the
  clock); see [When a rotation moves on](#when-a-rotation-moves-on).
  *Default* sets the minutes for items with no ending of their own; untick it and such items run until
  something else moves on.
- **Settings for every item:** a cell size, a speed, a palette, whether periodic tilings are rebuilt to
  fill the screen, and *No reseeding*. These apply to every item that is not a stored profile.
- **The items.** *Add profile*, *Add scene…*, *Add series* (one item per rule, for example every
  Wolfram rule) or a Quick Start entry's handle typed into the box (such as `rule:B3/S23`,
  `langtons-ant`, an atlas entry's `atlas:…` or one of your own `find:…`; press Enter). Each item can
  be moved and removed, and has:
  - **Ends (any of these)**, from *Add ending*: *After minutes*, *After generations*, *When settled* (the
    same test the [reseed policies](reseeding.md#automatic-policies) use, with an optional limit in
    generations), *On low activity* (under a share of cells changing, for a number of generations),
    *When finished* (a rows rule reaches the bottom, or nothing can change any more), and *At a time
    of day*. With no ending, an item takes its profile's minutes, else the rotation's default.
  - **Hold:** seconds to keep showing the finished world before moving on.
  - **Only at certain times:** the days of the week, and hours *Between* two times (which may cross
    midnight). Outside them the item is skipped.
  - **Settings for this item**, as above, for this item alone.
  - For a series: the **Rules** (a series name such as `W` or `HexW`, a template with `{n}`, or a list),
    the **Tiling**, **From** and **To**, and the **Seed**.
- **Script:** *Add a script* (see [below](#a-rotations-script)).

*Save* keeps it, *Save and start* runs the screensaver on it at once, and *Close* discards the edits.
Editing a built-in rotation saves your own copy under the same name.

## Rotation files

Each rotation of your own is a JSON file in the `rotations` folder of the settings folder (see
[Folders](../reference/folders.md)), so rotations can be written by hand, kept and shared. The app,
the host and the Windows screen saver read the same files; the macOS screen saver gets a copy when it
is installed. This one shows Brian's Brain until it settles,
then Langton's ant for ten minutes, and from 10 pm to 6 am also the 256 elementary rules:

```json
{
  "name": "Evenings",
  "description": "Brian's Brain until it settles, then a Langton's ant for ten minutes; the Wolfram rules at night.",
  "order": "in_order",
  "default_minutes": 5.0,
  "items": [
    {"source": {"kind": "profile", "name": "Brian's Brain"},
     "end": [{"kind": "settled", "latest": 20000}], "hold_s": 3.0},
    {"source": {"kind": "quick_start", "id": "langtons-ant"},
     "end": [{"kind": "minutes", "minutes": 10.0}],
     "look": {"cell_px": 4.0, "speed": 240.0}},
    {"source": {"kind": "series", "rules": "W", "from": 0, "to": 255},
     "end": [{"kind": "done"}], "hold_s": 3.0,
     "schedule": {"from": "22:00", "to": "06:00"},
     "look": {"cell_px": 4.0, "palette": "Ink"}}
  ],
  "script": "if s.population == 0 { \"next\" }"
}
```

| Field | Meaning |
| --- | --- |
| `name`, `description` | Shown in the app and the host's menu |
| `order` | `in_order` (default), `shuffle`, `daily` or `hourly` |
| `default_minutes` | Minutes for items with no ending of their own (5 if left out); `null` lets them run until something else moves on |
| `look` | Settings for every item that is not a stored profile: `cell_px`, `speed` (generations per second), `refill`, `gpu`, `palette`, `color_by`, and `reseed` (a list of [reseed policies](reseeding.md); `[]` for none). The app's editor sets all but `gpu` and `color_by`, and of `reseed` only *No reseeding* |
| `items` | The items, in order |
| `script` | Rhai code run about once a second (see below) |

An item has a `source`, which is one of `{"kind": "profile", "name": …}`, `{"kind": "scene", "path": …}`,
`{"kind": "quick_start", "id": …}` and `{"kind": "series", "rules": …, "from": …, "to": …,
"family": …, "boundary": …, "seed": …}` (a series becomes one item per rule, on a 200 × 140 grid of the
family, `square` and dead edges by default). It may also have a `label`, a list of `end` conditions, a
`hold_s` in seconds, its own `look`, and a `schedule` with `days` (`mon` to `sun`) and `from` and `to`
times. The end conditions are `{"kind": "minutes", "minutes": …}`, `{"kind": "generations", "gens": …}`,
`{"kind": "settled", "latest": …}` (`latest` optional), `{"kind": "low_activity", "below": …,
"for_gens": …}`, `{"kind": "done"}` and `{"kind": "clock", "at": "07:30"}`.

## A rotation's script

A rotation's script runs about once a second while the rotation plays. It sees a map `s` describing
what is showing now, and a map `state` in which it can keep anything it likes between runs:

| In `s` | Meaning |
| --- | --- |
| `index`, `count`, `label`, `n` | The item's number (from 0), how many items there are, its label, and its rule number in a series (else `()`) |
| `generation`, `seconds` | Generations and seconds since the item started |
| `population`, `cells`, `activity` | Live cells, all cells, and the share of cells that changed last generation (or `()` when it was not measured) |
| `done`, `settled` | Whether nothing can change any more, and whether the world has settled |
| `hour`, `minute`, `weekday` | The local time; `weekday` 0 is Monday |

Its answer decides what happens: nothing (or `"continue"`) carries on, `"next"` and `"previous"` move
on, `"reseed"` reseeds the world now, a number goes to that item, and `#{ item: "label" }` goes to the
item with that label. The script runs with tight limits on how much work and memory it may use, and
its `print` output is discarded. New scripts start from this one:

```rhai
// Runs about once a second. `s` describes what is showing, `state` keeps
// anything you like between runs. Answer "next", "previous", "reseed",
// an item number, or #{item: "label"}. Anything else just continues.
if s.generation > 20000 { "next" }
```

This is a different, much smaller language environment from [`jetcarp script`](../reference/scripting.md):
none of the scripting API's functions are available in it.

More: [Wallpapers, screensavers and the background host](wallpapers.md) and [Reseeding](reseeding.md).
