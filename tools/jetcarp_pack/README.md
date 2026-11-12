# jetcarp_pack: an independent reader for pack format 1.0

A second implementation of JETCARP's pack format. It implements the normative spec,
`docs/src/reference/pack-format.md`, with `library-format.md` for identity texts, and the automata
pages it points to for cell states. It was first written from the design alone. Since then it has
been cross-checked against JETCARP's golden packs and against `jetcarp pack check` and
`jetcarp pack import` run as a black box. No JETCARP source was read to answer a format question;
only the book, the test vectors and the golden packs. [SPEC_QUESTIONS.md](SPEC_QUESTIONS.md) records
every question found, how the spec settled it, and the cross-check's disagreements.

Python 3.10 or later, [pyarrow](https://arrow.apache.org/docs/python/), and the standard library.
Level 2 runs `ssh-keygen -Y verify` (OpenSSH 8.1 or later).

## Running it

```sh
python3 -m venv ~/venvs/jetcarp-pack && ~/venvs/jetcarp-pack/bin/pip install pyarrow
cd tools/jetcarp_pack
~/venvs/jetcarp-pack/bin/python -m jetcarp_pack check testdata/minimal      # levels 1, 2 and 3
```

`pip install tools/jetcarp_pack` also installs a `jetcarp-pack` command. Every command reads a
store, or an export (a folder holding `pack.json`).

| Command | Level | What it does |
| --- | --- | --- |
| `info STORE` | 0 | the refs (feed, seq, publisher, manifest) and key statements |
| `tables STORE` | 0 | a version's tables: schema, parts, rows, partitions |
| `rows STORE TABLE [--where col=value] [--columns a,b] [--limit N]` | 0 | rows as JSON |
| `find STORE ID` | 0 | a pattern or ant by id, unique prefix of 4+ digits, alias or name, with its events, latest annotations and any retraction |
| `integrity STORE [--no-scan]` | 1 | object, manifest and index hashes; content ids; pins; table consistency |
| `authenticity STORE [--state FILE [--accept]]` | 2 | signatures, key statements, refs and `seq` |
| `replay STORE [--periods N]` | 3 | re-runs periodic patterns from `tilings` and `rules` |
| `check STORE [--level 1-3]` | 1-3 | levels 1 to N in turn |

Browse commands take `--manifest sha256:...` or `--feed NAME` when a store has several refs.
`--json` prints the report as JSON; `-v` adds INFO lines. The exit status is 0 when nothing failed,
1 when a check failed or the pack was refused, and 2 for a folder that cannot be opened. Every
finding names its level, a stable code, where it was found and why:

```
FAIL L1 pin: tilings {"family":"square","boundary":"dead"}: pin is 0145310e..., but the geometry gives d748e571...
FAIL L2 seq-rollback: refs/test-retraction.json: seq 1 is below 2, the last accepted; refusing a rollback
```

## What each level checks

**Level 0, browse.** Opens a version and reads every table and document. Even browsing checks each
object's hash before using it, refuses an unknown major version, `requires` or schema number, and
applies the limits below.

**Level 1, integrity.**
- **Hashes.** Every object hashes to its name; an export's `index/` and `history/` do too.
  Manifests, indexes and `jetcarp-store.json` must be RFC 8785.
- **Parts.** Each part matches its index entry: bytes, rows, partition (values checked), `first`
  and `last`, and its sort order. Each column has its type; a nullable column left out reads as
  null.
- **Ids.** Every pattern, ant and Quick Start id is recomputed from its identity text (or scene)
  and pin. The identity's lines are checked against the typed columns.
- **Pins.** Every pin is recomputed with the spec's byte layout: from a lattice tiling's
  geometry columns, or from an aperiodic family's reference patch. Each `pin_key` is rebuilt from
  `family`, `params` and `edges`. Each neighbour must be listed once, symmetrically.
- **Content.** Fact rows agree across parts. Every JSON cell is in its one spelling, and no text
  cell holds a local path. The other rules are checked as the spec gives them: consent (a group's
  members too; each rater's latest `raters` row stands, and a note needs a row that shares notes),
  annotations' `rev`, the main verification, the members of `gun`, `range` and `ants.run`,
  sightings' sources, retractions with their publishers, search indexes, media files.
- **Across versions.** Every parent's retractions are kept, and no part a version adds repeats an
  event row its parents have. In an export, every file is where the layout puts it.

**Level 2, authenticity.**
- **Key statements.** A first statement is signed by its own key, a successor by its
  predecessor, and a revocation by the key or a successor. The file name is the key's SHA-256 in
  hex, and Ed25519 is the only key type.
- **Revocation.** A revoked key signs nothing, whatever the dates. A successor stands after a
  revocation only if it signed it. With `--state`, revocations are kept, so a store serving the
  old statement again cannot undo one.
- **Manifests.** A version needs a signature by its publisher's standing key or a machine key it
  authorises and has not revoked. The signing key is taken from the signature, and must be the one
  the file is named for.
- **Refs.** A ref is canonical and signed by the publisher's current key, or a machine key it
  authorises. With `--state FILE`, a lower `seq` is a rollback, and the same `seq` for another
  manifest is refused; `--accept` records what passed. Signatures are checked by
  `ssh-keygen -Y verify` against an allowed-signers file holding only the expected key.

**Level 3, replay.** Each still life, oscillator and spaceship on a lattice tiling is re-run on the
neighbour graph its `tilings` row gives, on an unbounded dead plane:
- in its rule (its `rule`, else its main verification's), and in every other rule a verification
  names;
- with B/S and Generations from the masks, `steps_per_gen` applications a generation, and B0 B/S
  rules as two complementary rules alternating on even and odd generations.

Every generation for `--periods` periods (default 2) must equal its stored phase moved by the
displacement so far, the period must be the smallest, and the class must follow. The masks are
checked against the notation. Aperiodic patterns, other families, B0 rules the alternation does not
define, puffers, guns and other classes are skipped with a reason. Each record has a budget of
100,000 generations, 1,000,000 live cells and 20 seconds.

## Limits

The limits in pack-format.md's "Limits" table are all enforced:
- Objects over 4 GiB and documents over 16 MiB are refused before reading.
- JSON nesting over 32 is checked before parsing. Duplicate keys, NaN, numbers with a fraction or
  exponent, and integers beyond ±(2^53 − 1) are refused (a pack writes a larger integer as a
  string of its decimal digits).
- An index may list 10,000 parts and 10,000 children, 8 levels deep, and each index is read once.
- A Parquet footer is checked before pyarrow parses it: 16 MiB, Thrift nesting 32, 1,024 columns.
- Then every page header is read: the pages' uncompressed sizes may total at most 64 times the
  part's size, and the data pages' values at most 64 per byte plus 2^20.
- Decoding goes 64 rows at a time and stops at 64 times the stored size plus 64 MiB.
- String cells are limited to 1 MiB, phases to 1,000,000 cells and periods to 2^32.

Paths are built only from checked hashes, the extension list, feed and signer names, and (in
exports) table names and checked partition values. Symbolic links and non-files anywhere are refused
without being opened. An input no check foresaw is a failure, never a traceback.

## Tests and test stores

```sh
cd tools/jetcarp_pack && ~/venvs/jetcarp-pack/bin/python -m unittest
```

Without pyarrow, `python3 -m unittest` still runs the RFC 8785 tests and skips the rest. The tests
cover:
- RFC 8785's own examples and the repository's 600 `rfc8785` vectors;
- the spec's square-grid pin, the 12 pin vectors and the golden Penrose reference patch;
- every refusal and limit;
- B0 alternation against a brute-force run of the real rule;
- replay on a two-tile cell;
- JETCARP's golden packs, every one of which must pass every level with no FAIL and no WARN
  (`tests/test_rust_golden.py`).

`tests/packwriter.py` is a writer for tests only. It writes Parquet with pyarrow and no key-value
metadata, sets `created_by` to `jetcarp-pack 1` by patching the footer, and signs with Ed25519 keys
made by `ssh-keygen` in a temporary folder. `tests/make_testdata.py` wrote the stores in
`testdata/`. `testdata/EXPECTED.json` gives each one's manifest, refs, test keys and expected
outcome. Every name in them is invented.

| Store | Expected |
| --- | --- |
| `minimal` | accept: square grid (the spec's row), `B3/S23`, `B2/S/C3`, glider (the book's id `5a080db3`), block, blinker, a Quick Start entry, events |
| `export` | accept: `minimal`'s version as an export |
| `merge` | accept: feeds `test-a` and `test-b`; `test-a` seq 2 merges both, signed by A's authorised laptop key |
| `retraction` | accept: version 2 retracts the blinker and renames the glider (`rev` 2) |
| `group-raters` | accept: a taste pack, with a group rater whose two members consented |
| `ant-run` | accept: Langton's ant alone, its `run` JSON in the shape the spec gives |
| `key-rotation` | accept: key R2 supersedes R1; version 2 and its ref are by R2 |
| `revoked-by-successor` | accept: R1's revocation is signed by its successor R2 |
| `negative/tampered-object`, `bad-signature`, `unsigned`, `unknown-requires`, `unknown-major`, `revoked-key`, `bad-pin`, `successor-of-self-revoked`, `revoked-machine-key` | refuse, each for its reason |
| `negative/rollback` | refuse (`seq-rollback`) after accepting `retraction`'s ref |

JETCARP 0.4 (`jetcarp pack check` and `pack import --verify full`) accepts and refuses them as
expected, `group-raters` and `ant-run` included since the cross-check's fixes (SPEC_QUESTIONS.md).
The one exception is `negative/rollback`: JETCARP 0.4 keeps no `seq`, so it accepts the rolled-back
version, as pack-format.md says it does. The test ant's outcome uses this writer's own start direction
and turn sense, and the canonical forms of the block, blinker and LWSS are inferred from the book's
glider. JETCARP's re-verification confirms the canonical forms; it imports the ant as packed (its
id is the same as JETCARP's own Langton's ant's, in the golden `ants` pack). Running
`make_testdata.py` again makes new keys, so every signature and manifest hash changes.
