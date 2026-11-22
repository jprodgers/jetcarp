# 4. Building your own Quick Start

The Quick Start menu holds the examples that come with JETCARP, the entries of the packs you import,
and an *atlas*: the best runs of an exploratory search over every tiling and rule family. Each
computer's search starts from a point of its own, so your atlas holds things no one else's does. In
this tutorial you build an atlas, look through what it found, keep and hide entries, promote finds
of your own, and share them as a pack. It takes about twenty minutes, three of them searching.

You need JETCARP installed ([Installing](../getting-started/installing.md)). The search runs on your
graphics card when you have one, and on the processor otherwise, where it finds less in the time.
The [Searching for patterns](../guides/searching.md#the-exploratory-search) guide covers the
exploratory search in full.

## 1. Build an atlas

`jetcarp quickstart build` searches for as many minutes as you say, then puts its best runs into
the Quick Start:

```sh
jetcarp quickstart build --minutes 3
```

It prints:

```text
jetcarp: An explore search isn't reproducible: what it finds depends on the computer (its speed, its threads and its GPU), so the same settings find different things on another computer or another day. Each run it finds replays exactly.
explore search job 22 in /home/ada/.local/share/jetcarp/library.sqlite
GPU: AMD Radeon 8060S Graphics (RADV STRIX_HALO) (radv), Vulkan
Quick Start build (explore 22): 914 atlas entries published (914 new, 0 updated, 0 removed); 29,336 runs (136/s), 7,106 elites in 3,880 niches (413 of them confirmed), 28 of 1,359 tilings; AMD Radeon 8060S Graphics (RADV STRIX_HALO) (radv), Vulkan
  status done, 3 min 20 s run over every session, 1,476 confirmation runs, 30,844 candidates tried, 0.72 GB of memory (at most 0.72 GB this session)
  threads: 20 for runs on the processor and 10 for the GPU's batches, of 32 cores (2 left free)
  …
  family                     runs   mean   best  elites
  life_like                 9,133   0.32   1.37   2,155
  generations               6,993   0.63   2.09   1,499
  jaca                      3,292   0.54   1.76     897
  …
```

The outputs on this page come from Linux; on macOS and Windows the folders in them differ (see
[Folders](../reference/folders.md#the-three-folders)).

In three minutes it ran 29,336 worlds, each a rule of one of its 21 rule families on one of the
tilings, from a start of its own, and scored each for how lively, varied and long-lasting it was. It
kept the best of each kind it saw (7,106 *elites* in 3,880 *niches*), ran the best of those again
from other starts to confirm them (413 confirmed), and published 914 of them as atlas entries, a few
per category, rule family and tiling. The table says how each rule family did; the last figure on
the *status* line is the memory it used.

**Timings.** The minutes are the search's, and it ends on time: runs still going on the processor
when they are spent are given up, and the graphics card's last batches are sized to end with them
(one that would run on is given up too). Publishing then takes a few seconds more, since each
entry's run is replayed to where it is most interesting. This three-minute build took 3 minutes 21
seconds from start to finish. Its progress (`jetcarp jobs status`) moves every couple of seconds,
and it leaves two of the computer's cores free; the *threads* line says how it shared the rest.
Three minutes see only a couple of dozen of JETCARP's 1,359 tilings; a longer build sees more and
finds more.

Open the app (or go back to its Quick Start screen) and set *Source* to *Atlas* to see only your
build's entries:

<img src="../images/tutorials/quickstart-atlas.webp" alt="The Quick Start screen with Source set to Atlas: 914 of 1285 entries, counted by category (Waves & spirals 131, Oscillators 202, Turbulence 142, Fractals & rows 49 and more), and cards of Lenia, forest fires, Greenberg-Hastings and rock-paper-scissors waves, a turmite, JACA battles and Larger than Life turbulence running" width="700">

Your atlas will differ: every build searches from a starting point of its own.

**Stopping and carrying on.** Ctrl-C stops a build. `jetcarp jobs resume 22` carries it on to the
end of its minutes and then publishes. `--detach` runs it in the background, and `--no-gpu` keeps
it off the graphics card.

## 2. What the categories mean

Each atlas entry gets one category from how its run behaved, tested in about this order:

| Category | The run |
| --- | --- |
| *Fractals & rows*, *Quiet rows* | A rule that writes one row at a time (elementary and hexagonal row rules): with complex rows, or simple ones |
| *Sandpiles*, *Ants & turmites* | By family: the Abelian sandpile, and ants and turmites |
| *Quick deaths*, *Long fades* | Died out: early in the run, or late |
| *Frozen at once* | Stopped changing early in the run |
| *Wildfire* | A forest fire, which never settles |
| *Gliders & ships* | At least three things moving across a quiet, sparse background |
| *Battles* | A JACA rule, where two kinds of cell annihilate each other, still fighting at the end of the run (one that settled is *Static*) |
| *Waves & spirals* | Smooth waves or spirals that travel or turn |
| *Spots & stripes* | A Lenia rule settled into quiet spots or stripes |
| *Labyrinths* | Large smooth regions that have nearly stopped changing |
| *Oscillators* | Ended in a cycle of more than one generation |
| *Ash & debris* | Nearly still, in many small pieces |
| *Growth* | Still growing at the end |
| *Static* | Fine-grained noise, like a badly tuned television; or a JACA battle that settled into a cycle or froze |
| *Turbulence* | More than a fifth of the cells still changing at the end |
| *Living textures* | Still changing, and none of the above |

Four more categories hold entries from elsewhere: *Classics* and *Computing* (built-in examples),
*My finds* (your promotions, below) and *Packs* (a pack's entries that name no category of their
own). Built-in and pack entries can be in the categories above too.

## 3. Look through the results

The build is an explore job in your library. `jetcarp jobs list` lists it with the others, and
`jetcarp jobs runs` lists its stored runs, best first:

```sh
jetcarp jobs runs --limit 5
```

It prints:

```text
    15760   2.47  cycle   greenberg_hastings catalog:k6-364   GH 18/2
    16455   2.43  cycle   greenberg_hastings catalog:k6-364   GH 11/2
    26404   2.42  cycle   greenberg_hastings ih33             GH 16/1
    26205   2.41  cycle   greenberg_hastings ih33             GH 17/1
    24811   2.41  cycle   hodgepodge         cairo            Hodgepodge 166/2/1/28
5 run(s) of explore job 22
```

Each line is a run's id, its score, how it ended (`cycle`: it settled into a repeating cycle), its
rule family, its tiling and its rule. Its options choose a `--family`, a `--tiling`, a `--fate` or a
`--min-score`. With no job named, these commands use the newest explore job.

**A contact sheet** shows chosen runs at an eighth of the way, halfway and at the end:

```sh
jetcarp jobs sheet --ids 15760,26404,24811,24026 --cols 3 --px 200 --png best.png
```

It prints:

```text
4 runs, 632x928: /home/ada/best.png
```

<img src="../images/tutorials/quickstart-sheet.webp" alt="A contact sheet of four runs, three moments each, every run labelled above in JETCARP's small dot letters with its id, tiling, score, rule and measures: Greenberg-Hastings spirals on the 6-uniform tiling k6-364, Greenberg-Hastings waves in hexagonal rings on the isohedral tiling ih33, hodgepodge spirals growing out of noise on the Cairo pentagon tiling, and rock-paper-scissors bands on the 5-uniform tiling k5-176" width="632">

**Clips** animate a sample of runs from every category, to look through by eye. Each is a GIF, and
`manifest.json` says what each one is:

```sh
jetcarp jobs clips --sample 24 --out clips --page
```

It prints:

```text
24 clips in /home/ada/clips
categories: Ants & turmites 2, Ash & debris 2, Battles 2, Fractals & rows 2, Gliders & ships 2, Growth 2, Frozen at once 1, Labyrinths 1, Living textures 1, Long fades 1, Oscillators 1, Quiet rows 1, Sandpiles 1, Spots & stripes 1, Static 1, Turbulence 1, Waves & spirals 1, Wildfire 1
rating page: /home/ada/clips/index.html
```

With `--page`, the folder also gets `index.html`, a page for rating the clips. Open it in a web
browser: it needs no network, so it works from the folder (its fonts, Atkinson Hyperlegible Next
and Mono as in the app, are inside it), and it follows your system's light or dark setting. Each
clip has *Keep*, *Maybe* and *Drop* buttons and a *★* for one you would show someone. The keyboard
works too: ← and → move between clips, K, M and D rate the current one and move on, and S stars
it. The browser keeps your ratings as you go, so you can close the page and come back. When you
are done, *Save ratings* downloads them as one file, and `jobs review --from` records them:

```sh
jetcarp jobs review 22 --from ~/Downloads/ratings-clips.json
```

It prints:

```text
recorded 15 review(s) and 3 star(s) by Ada (0 for runs not in job 22, 0 already recorded); job 22 has drop 6, keep 5, maybe 4, ★ 3
```

The rater is the name typed on the page; `--rater NAME` records another. Reading the same file again
records only what is new in it.

**Reviews** record your verdict on a run, `keep`, `maybe` or `drop`, and `--reviewed` lists runs by
it, with each run's verdicts (one per rater, keep first) and a ★ if it has one:

```sh
jetcarp jobs review 22 15760 keep
```

It prints:

```text
recorded 1 review(s); job 22 has drop 6, keep 6, maybe 4, ★ 3
```

Then list the runs you kept:

```sh
jetcarp jobs runs --reviewed keep
```

It prints:

```text
    15760   2.47  cycle   greenberg_hastings catalog:k6-364   GH 18/2  [keep]
      561   0.94  froze   ltl                catalog:k6-119   R5,C2,S42-80,B54-79  [keep ★]
     2740   0.78  running generations        hat              B1/S124/C20  [keep ★]
     3949   0.58  cycle   generations        catalog:k6-171   B28/S79/C13  [keep ★]
      998   0.50  running jaca               penrose-rhomb    JACA B1346/S  [keep]
     2947   0.21  died    spectre_glider     hat              SpectreGlider  [keep]
6 run(s) of explore job 22
```

The first is yours; the others are the ones kept on the rating page.

To watch any run in the app, save it as a scene, and open that with *File → Open scene…* or
`jetcarp --scene cairo-spirals.json`:

```sh
jetcarp jobs scene 22 24811 --save cairo-spirals.json
```

It prints:

```text
run 24811 saved as a scene: /home/ada/cairo-spirals.json
```

## 4. Keep, hide and rate

**In the app**, right-click a card to rate it from one to five stars, or to hide it (see
[A tour of the app](../getting-started/tour.md#the-quick-start-screen)). A hidden entry leaves the
menu, your packs and your website exports.

**From the command line**, `jetcarp library quickstart` lists every entry that is not built in, and
hides, shows or removes one by its id:

```sh
jetcarp library quickstart --source atlas
```

It prints:

```text
catalog: /home/ada/.local/share/jetcarp/library.sqlite (914 entries)
atlas:1d5e99fc-10207          atlas     search_run Wave RPS 8/1                 RPS 8/1        Hexagon
atlas:1d5e99fc-11692          atlas     search_run WireWorld                    WireWorld      k6-426
…
```

Hide the first of them:

```sh
jetcarp library quickstart --hide atlas:1d5e99fc-10207
```

It prints:

```text
atlas:1d5e99fc-10207: hidden
```

`--hidden` lists the hidden entries, `--unhide ID` shows one again, and `--remove ID` deletes it.
An atlas entry's id is `atlas:`, the start of its search's id, and the run's id.

## 5. Promote your own finds

`jetcarp library promote` adds an entry under *My finds*. It takes a run of a search:

```sh
jetcarp library promote --search-run 24811 --job 22 --title "Spirals on the Cairo tiling"
```

It prints:

```text
promoted: Spirals on the Cairo tiling
entry:    find:run-1d5e99fc-24811 in /home/ada/.local/share/jetcarp/library.sqlite
It appears in the app's Quick Start under "My finds".
```

It also takes any scene file, such as the one from [Life on a new tiling](life-on-a-new-tiling.md):

```sh
jetcarp library promote --scene life-k2-007.json --title "Life on k2-007"
```

It prints:

```text
promoted: Life on k2-007
entry:    find:scene-3eb3915ac28926d6 in /home/ada/.local/share/jetcarp/library.sqlite
It appears in the app's Quick Start under "My finds".
```

Both are now listed as promotions:

```sh
jetcarp library quickstart --source promotion
```

It prints:

```text
catalog: /home/ada/.local/share/jetcarp/library.sqlite (2 entries)
find:run-1d5e99fc-24811       promotion search_run Spirals on the Cairo tiling  Hodgepodge 166 Cairo pentagon
find:scene-3eb3915ac28926d6   promotion scene      Life on k2-007               B3/S23         [3.4^2.6; 3.6.3.6] #2 (k2-007)
```

`jetcarp library promote ID` promotes a pattern of your library, and `jetcarp library promote-run`
a run of a script's sweep ([Scripting a rule sweep](scripting-a-sweep.md#5-keep-the-best-one)). In
the app, the pattern library's *Promote to Quick Start* does the same for a pattern.

## 6. Share it as a pack

A pack carries Quick Start entries, with everything needed to open them, to other people (see
[Packs](../guides/packs.md)). Packs are signed with your *publisher key*. `jetcarp pack key` makes
it, and a name given to it goes on your packs:

```sh
jetcarp pack key --name "Ada Example"
```

It prints:

```text
made publisher key SHA256:r7H7tVdScfeLIj9LyGlYS5f2cVBP0+5pSfyAyRFwERM
  ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILxfkgPRDPA8kH0Oke6iAfhIR+9/bTpSkkL3ozwNJzgK jetcarp publisher
  file /home/ada/.config/jetcarp/pack/publisher_ed25519
  named Ada Example
```

Then export the entries you choose, by their ids:

```sh
jetcarp pack export --to my-finds-2026.11 --feed my-finds --title "My finds" --version-label 2026.11 \
    --id find:run-1d5e99fc-24811 --id find:scene-3eb3915ac28926d6
```

It prints:

```text
publishing feed my-finds (version 1) into /home/ada/.my-finds-2026.11.store
…
exported sha256:7bc64701cdffdb027af7ee3e49cf8a2cf8b1e2b22e0ec2a661029ea3a6dde393 into /home/ada/my-finds-2026.11: 15 files, 25694 bytes
…
```

`jetcarp pack info` shows what the pack holds:

```sh
jetcarp pack info my-finds-2026.11
```

It prints:

```text
export sha256:7bc64701cdffdb027af7ee3e49cf8a2cf8b1e2b22e0ec2a661029ea3a6dde393 (feed my-finds, version 2026.11)
  My finds
  publisher SHA256:r7H7tVdScfeLIj9LyGlYS5f2cVBP0+5pSfyAyRFwERM (Ada Example), signed by its publisher; your own publisher key
  created 2026-11-22T08:20:49Z, licence CC0-1.0
  provenance             2 rows in 1 part, 3862 bytes
  quickstart             2 rows in 2 parts, 8042 bytes
  tilings                2 rows in 1 part, 7588 bytes
```

The folder `my-finds-2026.11` is the pack: zip it and send it, and the other person imports it with
`jetcarp pack import`, or adds it with *Add packs from a folder…* on the setup window's packs page
(*Researcher* and *Everything*). With `--id`, your explore searches stay out unless you name some
with `--search`. Without `--id`, the export holds your whole library but its hidden entries: every
pattern, ant run and Quick Start entry, the starter pack's included, and every explore search.
`--query` chooses patterns by a library query. A search goes in with every run it has made: a
stopped one's newest runs are sealed into its run files first, and the export says so; one still
running is refused until you stop it (`jetcarp jobs stop JOB`).

## 7. Longer searches

A build is an exploratory search and its publishing in one job. For a longer search that you look
through before publishing anything, run the two apart. The search here has a minute; give yours an
hour (`--minutes 60`):

```sh
jetcarp search explore --gpu --minutes 1 --label longer
```

It prints:

```text
jetcarp: An explore search isn't reproducible: what it finds depends on the computer (its speed, its threads and its GPU), so the same settings find different things on another computer or another day. Each run it finds replays exactly.
explore search job 23 in /home/ada/.local/share/jetcarp/library.sqlite
…
explore search 23 paused (at its budget); `jetcarp jobs resume 23` continues it from its archive (not run for run: the search is not deterministic)
```

Then publish its atlas:

```sh
jetcarp quickstart publish
```

It prints:

```text
714 atlas entries from explore job 23 (search e79a3798-9eeb-4e01-b263-2389ba10e162) in the Quick Start of /home/ada/.local/share/jetcarp/library.sqlite: 714 new, 0 updated, 0 removed
```

`jetcarp quickstart publish JOB` publishes a job's atlas (the newest explore job's when no job is
named), replacing the entries it published before. `--per-niche` sets how many entries each
category, family and tiling may have (12 by default), and `--min-score` the lowest confirmed score
(1.25). Publishing again after `jetcarp jobs resume` updates the atlas with what the search found
since.

## The setup window's build

The setup window's *Quick Start build* is `jetcarp quickstart build` with the profile's minutes: 10
for *Explorer* (which you can switch off), 30 for *Researcher* and 60 for *Everything*, changeable
from 1 to 600 on its summary page. It runs in the background while you use the app, its entries
appear in the Quick Start as soon as it publishes, and *Tools → Hunts…* lists it as *Quick Start
build*. A build cut off by closing the app or restarting the computer is carried on by *Resume*
there, by running setup again, or by `jetcarp jobs resume JOB`. See [First run and
setup](../getting-started/first-run.md).

## Where to go from here

- [Searching for patterns](../guides/searching.md) covers the other searches, which hunt for
  spaceships, oscillators and ant highways.
- [Search runs](../reference/search-runs.md) describes the files an explore search keeps.
- [Website export](../guides/website-export.md) publishes your reviewed patterns as a website.
