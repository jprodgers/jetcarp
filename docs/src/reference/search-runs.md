# Search runs

An [exploratory search](../research/search-methods.md#explore) measures millions of runs: one
candidate (a tiling, a rule and a starting pattern) run once, with its fate, its measures and its
score. It keeps them in a folder of its own, `searches/<uuid>/` beside the
[pattern library](library-format.md) (see [Folders and environment variables](folders.md)). This page
describes that folder for anyone who wants to read the runs with other tools: the runs are Parquet
files, which pyarrow, DuckDB, Polars and pandas read without JETCARP.

## The folder

```text
searches/<uuid>/
  search.sqlite                    the search's working state, and its newest runs
  runs/part-000001.parquet         sealed runs: a million each, never changed again
  runs/part-000002.parquet
  scores-1/part-000001.parquet     a rescore's scores for segment 1 (only when it changed them)
  manifest.json                    what the folder holds
```

The uuid is the search's, made with the job; `jetcarp jobs status JOB` prints the folder.

**Sealing.** A running search adds its runs to its working database, `search.sqlite`. Once a million
runs are waiting there, the oldest million are *sealed*: written to the next **segment**,
`runs/part-NNNNNN.parquet`, and deleted from the working database. Stopping a search seals nothing:
its newest runs, fewer than a million, stay in the working database and the next session adds to
them, so every segment but the last holds a million runs however often the search is stopped and
continued. `jetcarp jobs seal JOB` (a stopped job) seals the rest, so that the folder holds every run
in Parquet; do that before sharing a folder. Importing an old CART explore database
(`jetcarp search explore --import FILE`) writes segments straight from it, after checking its runs on
this engine (see [Re-measuring an import](#re-measuring-an-import)), and a search folder from
before JETCARP 0.4's segments is sealed the first time it is opened.

**A run's id** is its place in one sequence that never restarts: segment 1 holds the lowest ids,
segment 2 the next, and the working database the newest. Elites, confirmation runs (`parent`) and
reviews name runs by these ids.

**Crash safety.** A segment is written to `part-NNNNNN.parquet.tmp`, synced, read back and checked,
renamed, and added to the manifest (itself written to a temporary file and renamed); only then are
its runs deleted from the working database. If the process stops at any point of that, the next time
the search is opened JETCARP removes temporary files and segments that the manifest does not list
(their runs are still in the working database), and deletes from the working database the runs that a
listed segment holds. A reader never sees a run twice or misses one, even while a seal is under way in
another process.

## The manifest

`manifest.json` is plain JSON:

| Field | Holds |
| --- | --- |
| `format` | `jetcarp-search-manifest 1` |
| `search`, `kind` | the search's uuid, and `explore` |
| `engine_version` | the JETCARP version that last wrote the manifest |
| `run_schema`, `score_schema` | `jetcarp-explore-runs 1` and `jetcarp-explore-scores 1`: the column layouts below |
| `created`, `updated` | UTC times |
| `spec` | the job's settings (tilings, families, grid sizes, generations, seeds, ...). An imported database is named by its file name only, so a shared folder never shows where it came from on the publisher's disk |
| `segments` | one per segment, in id order: `file` (relative to the folder), `sha256` and `bytes` of the file, `rows`, `first_id` and `last_id`, `sealed` (UTC) and `engine_version` |
| `scores` | one per rescore that changed a score: `version` (1, 2, ...), `created`, `engine_version`, and `files`, one per segment it rescored: `segment`, `file`, `sha256`, `bytes`, `rows` |
| `remeasure` | only in an imported search: how its runs were checked on this engine and which were run again (see [below](#re-measuring-an-import)) |

## Re-measuring an import

The runs of an imported database were measured by the engine of their day, and where something has
changed since (a tiling's geometry, the way a rule runs) they no longer replay to the measures stored
with them. So before an import copies anything it checks them on this engine. With
`--remeasure auto` (the default) it runs up to 32 runs of every tiling again at their recorded
settings. The sample holds each rule family's first run on the tiling (a database's engine may have
changed while its search ran, and the oldest runs are the likeliest not to reproduce), then takes the
families in turn and, within each family, its kinds of starting pattern, with the confirmation runs
as one more group; it is drawn the same way every time. Each is compared exactly with what is stored:
fate, settling generation, period, every measure, score and behaviour class.

Every run of a tiling whose sampled runs differ in two or more rule families is then run again: a
change to the tiling shows in all its families. When the sampled runs that differ are all of one
family, the way that rule runs has changed rather than the tiling, and only that family's runs on the
tiling are run again. They are run again before the copy, many at a time, and their new measures are
kept in the working database (a stopped import goes on from the last of them); the copy then stores
each with its new measures under the same id, and the other runs as they are. `--remeasure all` runs
every run again, and `--remeasure none` copies them all as they are.

The check runs the sampled runs on the CPU, but Larger than Life on the GPU when the job has one
(`--gpu`): a search runs those on a GPU only, and on the CPU each takes seconds. Each tiling's largest
batch of sampled runs that a GPU can run (other than Larger than Life) is run on the GPU as well. If
the GPU measured every one of those as the CPU did, the runs re-measured go to the GPU for every rule
it runs, the rest to the CPU; if not, only Larger than Life does. Without a GPU the CPU runs
everything, Larger than Life included, which is slow when there are many.

The manifest's `remeasure` says what was done:

| Field | Holds |
| --- | --- |
| `mode` | `auto`, `all` or `none` |
| `engine_version`, `checked` | the JETCARP that checked, and when (UTC) |
| `sample_per_tiling`, `tilings`, `sampled`, `sampled_differ` | the check: runs sampled per tiling at most, tilings checked, runs sampled, and those that measured differently |
| `backend` | what ran the rules a GPU can run: `cpu`, or the GPU and why |
| `remeasured` | one per tiling run again: `tiling`, `family` (only when just that family's runs were run again), `why` (how many sampled runs differed, in what, and in which families), `runs`, `changed` (runs whose measures changed) and `failed` (runs that could not run again, kept as they were) |
| `to_remeasure`, `runs`, `changed` | runs to re-measure (as the check counted them; 0 with `all`), runs re-measured, and those whose measures changed |
| `check_seconds`, `remeasure_seconds` | how long the check took, and the re-measuring |
| `done` | whether the import has copied every run |

A run was re-measured when its tiling is listed in `remeasured` (with its family, when one is named):
runs have no column of their own for it. The job's progress (`jetcarp jobs status JOB`) and report
show the same, while the import runs and after.

## Segments

Each segment is a Parquet file compressed with zstd (level 9). Its runs are in id order, in row
groups of 131,072 runs, with statistics for every row group and every data page (the column and
offset indexes), so a reader that wants a few runs or the best scores skips most of the file. Floats
are stored with byte-stream-split encoding and ids and times as deltas. The file's key-value metadata
says `jetcarp.schema` (`jetcarp-explore-runs 1`), `jetcarp.search` (the uuid), `jetcarp.segment`
(its own name) and `jetcarp.engine_version`.

Every column is required (never null) except `parent`.

| Column | Type | Meaning |
| --- | --- | --- |
| `id` | int64 | The run's id |
| `stage` | int8 | 1: a screening run; 2: a confirmation run, the same candidate again on a bigger grid with another random seed |
| `parent` | int64, null for screening runs | The screening run a confirmation run confirms |
| `tiling` | string | The tiling key: `square`, `hex`, `trihex`, `catalog:k3-019`, `ih28`, `spectre`, ... |
| `nb` | int8 | The neighbourhood: 0 touch (tiles that share a corner count), 1 edge |
| `fam` | string | The rule family: `life_like`, `generations`, `ltl`, `jaca`, `cyclic`, ... |
| `rule` | string | The rule, in [rule notation](rule-notation.md) |
| `seed` | string | The starting pattern, as JSON (a scene's seed) |
| `seed_kind` | string | Its kind: `soup`, `mutated` (an elite's seed, changed), `blobs`, `symmetric`, `point` (one cell), `circuit` (WireWorld), `confirm` (a confirmation run), ... |
| `cells` | int32 | About how many cells the grid has |
| `gens` | int32 | Generations run |
| `steps` | int32 | Steps per generation |
| `rng` | uint64 | The random seed: with the columns above it replays the run exactly |
| `origin` | string | How the candidate was made: `random`, `mutate`, `seed`, `cover`, `named`, `confirm` |
| `fate` | string | How it ended: `died`, `froze`, `cycle` (a repeating cycle) or `running` |
| `settle` | int32 | The generation it settled (died, froze or entered its cycle); the run's length if it never did |
| `period` | int32 | The period of its final cycle (1 frozen, 0 none seen) |
| `score` | float32 | The interest score, about 0 to 3 (higher is more interesting), as sealed; a rescore's sidecar may hold a newer one |
| `bin` | uint32 | The behaviour class (fate, settling time, density, activity, shape, movers, packed), the niche's third part |
| `density` | float32 | Live cells at the end, as a fraction of cells |
| `density_late` | float32 | Mean density over the last quarter of the run |
| `activity_late` | float32 | Mean fraction of cells changing per generation over the last quarter |
| `activity_early` | float32 | The same over the first quarter |
| `pop_cv` | float32 | Coefficient of variation of the population over the last half |
| `growth` | float32 | log2 of the late population over the early population |
| `peak_density` | float32 | The highest density reached, as a fraction of cells |
| `entropy` | float32 | Shannon entropy of the final states (bits), divided by log2 of the number of states seen |
| `bits` | float32 | Bits per cell to store the last frame, deflated |
| `same_nb` | float32 | Fraction of neighbour pairs in the same state |
| `smooth` | float32 | `same_nb` relative to shuffled cells: 0 like noise, 1 fully smooth |
| `comps_per_live` | float32 | Connected groups of live cells per live cell (1 is dust; small for a few large blobs) |
| `largest` | float32 | The largest group's share of the live cells |
| `size_entropy` | float32 | Entropy of the group sizes (log2-spaced buckets), bits |
| `patchiness` | float32 | Standard deviation of density over coarse spatial bins, relative to the mean density |
| `travel` | float32 | Cells that changed at all over the last frames, over the mean changes per generation, divided by the window: near 1/window for flicker in place, near 1 for changes that travel |
| `movers` | int32 | Moving objects seen |
| `mover_speed` | float32 | Their mean speed, in tiles per generation |
| `ts` | int64 | When the run was stored, in seconds since 1970 (UTC) |

The 18 columns from `density` to `mover_speed`, with `settle` and `period`, are the run's 20 measures;
the working database stores them as an 80-byte blob of 20 little-endian 32-bit floats (`feats`) in
that order, `settle` and `period` first. On CART's 15.3 million runs the segments take 77 bytes a run,
1.18 GB in all, against 5.17 GB for the same runs in SQLite.

## Score sidecars

Rescoring (`jetcarp jobs rescore JOB`, after the score function changes) never rewrites a segment. For
each segment whose scores changed it writes a **score sidecar**, `scores-<n>/part-NNNNNN.parquet` (the
segment's own file name; `n` counts the rescores: 1, 2, ...), holding two columns, `id` (int64) and
`score` (float32), for every run of that segment, in the segment's row order and row groups. A run's
score is the one in the newest sidecar of its segment, or the segment's own if none has it. The
manifest's `scores` list says which sidecars exist; the runs still in the working database are
rescored in place.

## Runs not yet sealed

The working database's `runs` table holds the newest runs, with the columns above except that the 20
measures are the `feats` blob. Reading it while JETCARP runs is safe (open it read only); to read
every run as Parquet instead, seal the search first (`jetcarp jobs seal JOB`). The search's other
tables (its archive of elites, coverage, the identities tried) are internal to JETCARP.

Reviews of runs (keep, maybe, drop) are not in the folder: they are ratings in the library, with
target kind `search_run` and target id `<uuid>:<run id>` (see
[Library format](library-format.md#ratings-of-search-runs)), so a search folder can be deleted or
shared without them.

## Reading the runs

**DuckDB.** The glob reads every segment (and never a `.tmp` file):

```sql
SELECT fam, count(*) AS runs, avg(score) AS mean, max(score) AS best
FROM read_parquet('runs/part-*.parquet')
WHERE stage = 1
GROUP BY fam ORDER BY runs DESC;
```

With the newest score of each run, after rescores:

```sql
WITH newest AS (
  SELECT id, arg_max(score, version) AS score
  FROM (SELECT id, score,
               CAST(regexp_extract(filename, 'scores-([0-9]+)', 1) AS INTEGER) AS version
        FROM read_parquet('scores-*/part-*.parquet', filename = true))
  GROUP BY id)
SELECT r.id, r.tiling, r.rule, coalesce(n.score, r.score) AS score
FROM read_parquet('runs/part-*.parquet') r LEFT JOIN newest n USING (id)
WHERE r.stage = 1
ORDER BY score DESC, r.id
LIMIT 20;
```

**pyarrow**, reading the segments the manifest lists:

```python
import json
import pyarrow as pa
import pyarrow.parquet as pq

manifest = json.load(open("manifest.json"))
runs = pa.concat_tables(
    pq.read_table(s["file"], columns=["id", "fam", "tiling", "fate", "score"])
    for s in manifest["segments"])
```

**Polars**, lazily, reading only the columns a query needs:

```python
import polars as pl

ships = (pl.scan_parquet("runs/part-*.parquet")
         .filter((pl.col("stage") == 1) & (pl.col("movers") >= 3))
         .select("id", "tiling", "rule", "score", "movers")
         .sort("score", descending=True)
         .head(20)
         .collect())
```

A run found this way opens in JETCARP with `jetcarp jobs scene JOB RUN`, which writes the scene that
replays it.
