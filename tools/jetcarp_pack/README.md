# jetcarp_pack: an independent reader for pack format 1.0

A second implementation of JETCARP's pack format, written only from the pack specification
(`design/pack-spec.md` sections 1 to 26, with section 24's fixes taken as agreed) and the book's
`library-format.md`, `rule-notation.md` and the Generations and Life-like pages that the spec
includes by reference. No JETCARP source was read to answer a format question. Where the two
implementations disagree, the spec is unclear or wrong; [SPEC_QUESTIONS.md](SPEC_QUESTIONS.md) lists
every such place found while writing this one.

Python 3.10 or later, [pyarrow](https://arrow.apache.org/docs/python/), and the standard library.
Level 2 runs `ssh-keygen -Y verify` (OpenSSH 8.1 or later).

## Running it

```sh
python3 -m venv ~/venvs/jetcarp-pack && ~/venvs/jetcarp-pack/bin/pip install pyarrow
cd tools/jetcarp_pack
mkdir -p /tmp/golden && tar -xf testdata/golden.tar -C /tmp/golden
~/venvs/jetcarp-pack/bin/python -m jetcarp_pack check /tmp/golden/minimal      # levels 1, 2 and 3
```

`pip install tools/jetcarp_pack` also installs a `jetcarp-pack` command.

| Command | Level | What it does |
| --- | --- | --- |
| `info STORE` | 0 | the store's refs (feed, seq, publisher, manifest) and key statements |
| `tables STORE` | 0 | a version's tables: schema, parts, rows, partitions |
| `rows STORE TABLE [--where col=value] [--columns a,b] [--limit N]` | 0 | rows as JSON |
| `find STORE ID` | 0 | a pattern or ant by id, unique prefix of 4+ digits, alias or name, with its events, latest annotations and any retraction |
| `integrity STORE [--no-scan]` | 1 | object, manifest and index hashes; content ids; table consistency |
| `authenticity STORE [--state FILE [--accept]]` | 2 | signatures, key statements, refs and `seq` |
| `replay STORE [--periods N]` | 3 | re-runs periodic patterns from `tilings` and `rules` |
| `check STORE [--level 1-3]` | 1-3 | levels 1 to N in turn |

Browse commands take `--manifest sha256:...` or `--feed NAME` when the store has several refs.
`--json` prints the report as JSON; `-v` adds INFO lines. The exit status is 0 when nothing failed,
1 when a check failed or the pack was refused, and 2 for a store that cannot be opened.

Every finding names its level, a stable code, where it was found and why:

```
FAIL L1 object-hash: patterns part sha256:f0efb3724ca7: objects/sha256/f0/f0ef....parquet hashes to sha256:45d9..., not its name
FAIL L2 seq-rollback: refs/test-retraction.json: seq 1 is below 2, the last accepted; refusing a rollback
```

## What each level checks

**Level 0, browse.** Opens a version and reads every table and document. Even browsing checks each
object's hash before using it (section 4), refuses an unknown major version or an unknown
`requires` (section 15) and an unknown schema number, and applies the limits below.

**Level 1, integrity.**
- Every file under `objects/` is named `<sha256>.<ext>` in its two-digit folder and hashes to its
  name. Every manifest and index named by a ref, a parent or `retain` is checked; manifests and
  indexes must be in RFC 8785 form (their hashes are ids).
- Each part matches its index entry: bytes, rows, partition, `first`/`last`, and its sort order (by
  UTF-8 bytes). Each column has the type section 11 gives; unknown columns are ignored.
- Every pattern id is `sha256(identity + "\n" + pin)[:32]` and every ant id `ant-` plus the same.
  The identity text is parsed and its lines compared with the typed columns (`phases`, `period`,
  `d`, `class`, ...). Each record's `pin_key` must be a `tilings` row with the same pin, and every
  tiling key must be in its one spelling (RFC 8785 values in the fixed member order).
- Fact rows with the same key must be identical across parts (promise 4). Events, ratings and
  their consent rules (every rater has a `raters` row; a group's members have all consented),
  retractions, media objects and the manifest's fields are checked as section 11 describes.

  Not checked: geometry pins (the spec does not yet give the byte layout), Quick Start ids (no
  identity scheme is given), and search runs beyond their hashes.

**Level 2, authenticity.**
- Key statements: the file name is the key's fingerprint, the fingerprint is the key's, and the
  signature (namespace `jetcarp-key`) is by the key itself, or for a successor by the key it
  `supersedes`. Further machine keys are read from `authorised` (section 24 fix 1; the field name is
  this reader's choice). Revocations, forks (two successors) and unconfirmed supersessions are
  reported.
- Manifests: at least one detached signature (`jetcarp-manifest`) by the publisher's key or one of
  its authorised keys; other valid signatures are endorsements. A key revoked before the manifest
  was made fails it.
- Refs: signed (`jetcarp-ref`) by the publisher's current key, pointing to a manifest of the same feed
  and publisher. With `--state FILE`, a `seq` below the last accepted is a rollback and the same
  `seq` for another manifest is refused; `--accept` records what passed.
- Receipts, if any, name their folder's manifest and are signed by their verifier.

  Signatures are checked by `ssh-keygen -Y verify` against an allowed-signers file holding only the
  expected key. GitHub confirmation of a lost key is not done (no network).

**Level 3, replay.** For every still life, oscillator and spaceship on a lattice tiling, in each rule
it was verified in: the neighbourhood is built from the `tilings` row's `edge` or `touch` offsets,
and the rule from the `rules` row's masks (B/S and Generations), on an unbounded dead plane.
Every generation for `--periods` periods (default 2) must equal the stored phase moved by the
displacement so far. The period must be the smallest, and the class must follow from period and
displacement. The masks are also checked against the rule's notation. Patterns on aperiodic
tilings, other rule families, B0 rules, puffers, guns and other classes are skipped with a reason.
Each record has a budget (100,000 generations, 1,000,000 live cells, 20 seconds).

## Limits (section 17)

Objects over 4 GiB and documents over 16 MiB are refused before reading. JSON nesting is limited to
32, checked before parsing, so a deep document cannot exhaust Python's stack. Duplicate keys, NaN,
non-integers and integers beyond 2^53 are refused. An index may list 10,000 parts and 10,000
children, nested 8 deep, and each index is read once however often it is listed. A part whose
Parquet pages decompress to more than 64 times its size is refused before decoding. Decoding goes
64 rows at a time and stops at 64 times the stored size plus 64 MiB, which catches dictionary
bombs. String cells are limited to 1 MiB, phases to 1,000,000 cells and periods to 2^32.

Paths are built only from validated hashes, the extension allowlist, feed names and fingerprints.
Symbolic links and non-files (a FIFO, a device) anywhere in the store are refused without being
opened.

## Tests and test stores

```sh
cd tools/jetcarp_pack && ~/venvs/jetcarp-pack/bin/python -m unittest
```

99 tests. Without pyarrow, `python3 -m unittest` still runs the 20 RFC 8785 tests and skips the
rest. The tests cover:
- RFC 8785's own number table (Appendix B) and examples (sections 3.2.2 and 3.2.3), and the
  repository's 600 vectors from the Python `rfc8785` package, read as data;
- every refusal: a tampered object, bad and misdirected signatures, a rolled-back `seq`, unknown
  `requires`, an unknown major version or schema number;
- the limits above, and replay on a two-tile unit cell.

`tests/packwriter.py` is a tiny writer used only by the tests. It writes Parquet with pyarrow and
patches the footer's `created_by` to `jetcarp-pack 1`, which pyarrow cannot set. It signs with
Ed25519 keys made by `ssh-keygen` in a temporary folder. `tests/make_testdata.py` wrote the golden
stores, for the cross-check with JETCARP's importer, as one uncompressed tar,
`testdata/golden.tar` (440 KB). They aren't committed as folders because the spec's signature and key
file names hold a colon (`SHA256:...`), and Git for Windows refuses to check out such paths, which
would break the whole repository there ([SPEC_QUESTIONS.md](SPEC_QUESTIONS.md) A3). The tests unpack
the tar into a temporary folder. `testdata/EXPECTED.json` lists each store's manifest, refs, test
keys and expected outcome.

| Store | Expected |
| --- | --- |
| `minimal` | accept: square grid, `B3/S23` and `B2/S/C3`, glider (the book's id `5a080db3`), block, blinker, Langton's ant, events |
| `merge` | accept: feeds `test-a` and `test-b`; `test-a` seq 2 merges both (two parents), signed by A's authorised laptop key |
| `retraction` | accept: version 2 keeps version 1's index hashes and retracts the blinker |
| `group-raters` | accept: a taste pack with a group rater whose two members have consented |
| `key-rotation` | accept: key R2 supersedes R1; version 2 and its ref are by R2 |
| `negative/tampered-object` | refuse (`object-hash`) |
| `negative/bad-signature` | refuse (`bad-signature`) |
| `negative/unknown-requires` | refuse (`unknown-requires`) |
| `negative/unknown-major` | refuse (`unknown-major`) |
| `negative/rollback` | refuse (`seq-rollback`) after accepting `retraction`'s ref |

All names are invented. The geometry columns of the square grid's `tilings` row (corners, axes,
neighbour order) are this writer's guesses, since section 23.1 leaves them to the engine; its pin is
the book's. The block, blinker and LWSS identities use the canonical form that the book's glider
implies, which may not be JETCARP's; the ant's outcome uses this writer's own start direction and
turn sense.

Running `make_testdata.py` again makes new keys, so every signature and manifest hash changes.
The committed stores are the golden ones.
