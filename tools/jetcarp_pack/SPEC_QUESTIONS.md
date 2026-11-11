# Questions on pack format 1.0, from an independent reader

This reader was first written from the design (`design/pack-spec.md`, sections 1 to 26). Its
questions were then settled in the normative spec, `docs/src/reference/pack-format.md` (its last
section lists the settlements). The reader now implements that page, and was cross-checked against
JETCARP's golden packs and against `jetcarp pack check` and `jetcarp pack import` run as a black box
on this reader's own test stores. This file records:
1. what the cross-check found (each disagreement, and which side it points at);
2. questions the settled spec still leaves;
3. every earlier question, how pack-format.md settled it, and whether the reader now agrees.

## 1. The cross-check

**JETCARP's golden packs** (`crates/jetcarp-pack/tests/golden/*`, levels 0 to 3). Every other check
passes:
- every hash, signature, content id and Quick Start id;
- every pin, recomputed from the geometry columns, and the Penrose kites-and-darts pin from its
  4,000-tile reference patch;
- every JSON cell in its one spelling;
- every file of the export where the layout puts it;
- retractions cumulative, and no event row added twice;
- every periodic pattern replayed (the aperiodic ones skip: they are not level 3).

What fails (pinned in `tests/test_rust_golden.py`):

| # | Pack | Finding | Points at |
| --- | --- | --- | --- |
| X1 | minimal, retraction, merge, export | a `provenance` row targets `quickstart:golden-glider`, but the entry's id is `qs-5dbeecb02a20a394da2f9539ed051897` | **JETCARP**: the row names the library's own Quick Start key, not the pack id. The spec is clear: targets are `quickstart:<id>`, ids `qs-<hex>` |
| X2 | minimal, retraction, merge, searches, export | `raters.consent_date` is `2026-11-01`, a date | **JETCARP**, or the spec if a date is meant: "Times are RFC 3339 strings in UTC with Z and whole seconds", and consent_date is "when the rater consented". JETCARP accepts a time there on import. Either write times, or say consent_date is a calendar date |
| X3 | media | its two PNG objects are not in the repository | **the repository**: the root `.gitignore` ignores `*.png` outside `docs/`, so the files were never committed. A fresh clone's golden media pack is incomplete; JETCARP's own golden test can pass only where the files exist untracked. Add `!/crates/jetcarp-pack/tests/golden/**/*.png` and commit them |

**JETCARP on this reader's stores** (`jetcarp pack check` and `jetcarp pack import --verify full`,
each into a fresh library). JETCARP's full re-verification passed every record. That includes the
block, blinker and LWSS this reader's writer put in canonical form by inference from the book's
glider, so the inferred canonical form matches JETCARP's level 4 on all four. All ten refusals were
refused for the reason expected, by both programs:
- a tampered object, a bad signature, an unsigned version;
- an unknown `requires`, an unknown major version;
- a revoked publisher key, a pin that does not follow from its geometry;
- a successor of a self-revoked key, a machine key revoked in place;
- a rolled-back `seq` (this reader only, with its state file: see X7).

These stores were accepted by both programs, as the spec says they should be:
- minimal, export and retraction;
- merge, both feeds: a merge by an authorised machine key, its ref signed by that key;
- key-rotation and revoked-by-successor.

Disagreements:

| # | Store | Finding | Points at |
| --- | --- | --- | --- |
| X4 | group-raters | JETCARP refuses the pack: `score_models: schema 'jetcarp-score_models 1' (this JETCARP reads 'jetcarp-score-models 1')` | **the spec**: its rule is `jetcarp-<table> <n>`, which gives `jetcarp-score_models 1` for table `score_models`; JETCARP spells it with a hyphen. No golden pack holds `score_models`, so either can still be chosen. With the hyphen the rest of the store imports (3 ratings). Change: list every schema id in one table in the spec |
| X5 | ant-run | JETCARP refuses the whole pack: `ant run: invalid type: null, expected struct AntRun` | **the spec**, and JETCARP's reaction: `ants.run` is "the run with its proof and limits (JSON)" with no shape, so no other writer can make one JETCARP reads. Change: give its members, as for `gun` and `range`; and refuse or flag one ant, not the whole pack. The message also hides which ant and member failed |
| X6 | merge | Both accept a version signed only by an authorised machine key. This reader WARNs: the spec says "the publisher's own signature is at the name the manifest's `publisher` gives, so a reader over HTTP fetches it without listing a folder", which is not true then | **the spec**: say that a machine key's signature sits at the publisher's name too, or that the ref names the signer, or that a publisher's own key always signs |
| X7 | negative/rollback | JETCARP 0.4 imports an older version after a newer one. It keeps no `seq`, as the spec says (feeds arrive in 0.6), and the older version brings back a record the newer one retracted. The retraction is kept in `pack_retractions`, so the record stays flagged | nobody yet; worth a line in the spec: 0.4 does not refuse rollbacks |
| X8 | retraction | JETCARP does not import a retracted record the library lacks (1 pattern added, `retracted 0`); the retraction is recorded | **the spec**, a clarification: "an importer flags it and never deletes the user's copy" does not say whether a retracted record the user lacks is added flagged or left out |

## 2. Questions the settled spec still leaves

1. **Schema ids** (X4): list them. `jetcarp-score_models 1` or `jetcarp-score-models 1`?
2. **`ants.run`** (X5): its shape; also what `ants.speed` (`float64?`) measures (cells per step?).
3. **Machine-key signatures over HTTP** (X6).
4. **`consent_date`** (X2): a time, as every other time, or a date?
5. **The export's part numbering** with child indexes: "in the order its index lists them (children
   in order)" leaves open whether an index's own parts come before its children's. *Chose:* own
   parts first, then each child's, depth first. The golden export has no children, so it is
   untested.
6. **`raters`** is listed neither as a fact table nor as an event table. *Chose:* facts, keyed by
   `rater`.
7. **The settlements table** says `family` and `semantics` are on "every verification". The
   `verifications` table, and the golden packs, carry `semantics` only.
8. **`verifications.range`** in the golden packs also has `min_rule` and `max_rule`, beyond the
   listed members. *Chose:* extra members are allowed (readers ignore members they do not know).
   Say so for cell JSON too.
9. **Retracted records the importer lacks** (X8): added flagged, or left out?

## 3. The earlier questions, settled

"Reader" says whether this reader now does what pack-format.md says. Section letters are from the
first version of this file.

### A. Blocked a check, or changed ids

| # | Question | How pack-format.md settled it | Reader |
| --- | --- | --- | --- |
| A1 | The pin's byte layout | Given in full, with the square grid's hashes and twelve tilings' rows as vectors | Agrees. Recomputed for every lattice tiling and from aperiodic reference patches. The layout as written gives all 12 vectors and the Penrose patch's hashes; this was right first time |
| A2 | The tiling key's edges | Each edge `{mode, curved (true), nodes (empty)}`, nodes' `t` and `d` only; `params` written whenever the spec has one, even `{}` | Agrees: all 600 `rfc8785` vectors match. `pin_key` is also rebuilt from `family`, `params` and `edges` |
| A3 | Colons in file names | `<signer>` is the 64 hex digits of the key's SHA-256 digest | Agrees. The test stores are plain folders again; no colon anywhere |
| A4 | Geometry conventions | y down; `i` right, `j` down; `shape` a prototile index; corners clockwise unless mirrored; neighbours clockwise from corner 0, each once, symmetric | Agrees. Each neighbour listed once and the relation being symmetric are checked; the test writer's square grid is the spec's row |
| A5 | Ant ids and targets | `ant-<hex>` in `ants.id`; targets `ant:ant-<hex>` | Agrees (as chosen) |
| A6 | Latest-wins annotations | `rev`, the adding version's ref `seq`; then date, then value bytes | Agrees (`find`); `rev` checked to be 1 or more |
| A7 | Who signs key statements; revocation and time | Self, predecessor, key-or-successor; no rule depends on time; a revocation is permanent | Agrees. Dates no longer matter; revocations are kept in the state file and outlast a store serving the old statement; a successor stands after its predecessor's revocation only if it signed it |
| A8 | Machine keys | `authorised: [{key, fingerprint, label?, revoked?}]`; never removed, revoked in place; they sign manifests and refs | Agrees |
| A9 | Publisher identity across a key change | The first key of its chain; manifests and refs name the current key | Agrees |
| A10 | Quick Start ids | `qs-` and the hash of the scene's text, "\n" and the pin | Agrees; checked |

### B. Would have made implementations disagree

| # | Question | How pack-format.md settled it | Reader |
| --- | --- | --- | --- |
| B1 | `first`/`last` of several sort columns | The first sort column's value, as text | Agrees (as chosen) |
| B2 | Sort order | Within each part; text by UTF-8 bytes, integers by value, `false` before `true` | Agrees |
| B3 | Partition keys and columns | Named as their columns (`class_group`, `tiling_group`); `class_group` is the class; ants and Quick Start by `tiling_group`; values 1 to 64 of `[a-z0-9_-]` | Agrees; a bad value is refused |
| B4 | `family` and `semantics` | On patterns, rules, ants, Quick Start entries (and `semantics` on verifications) | Agrees (see question 7 above) |
| B5 | `created_by` | Free; JETCARP writes `jetcarp-pack 1`; readers MUST NOT depend on it | Agrees: no longer checked. The test writer still sets it |
| B6 | Key-value metadata | Writers SHOULD write none; readers ignore any | Agrees: no longer checked |
| B7 | The same `seq` again | The same manifest is no update; another manifest, or a lower `seq`, is refused | Agrees (as chosen) |
| B8 | Retractions | Cumulative, each with its `publisher` | Agrees; dropping a parent's retraction is a FAIL |
| B9 | Event rows twice in a merge | A writer adds only rows its parents lack; identical rows in two parts count twice | Agrees; a new part repeating a parent's event row is a FAIL |
| B10 | What "decompressed" means | The pages' uncompressed sizes from their headers, before decoding; values per byte; decoded bytes | Agrees. Page headers are read with a small Thrift reader (pyarrow does not show them), and the footer's size and Thrift depth are checked before pyarrow parses it |
| B11 | Index children | At most 10,000, 8 levels; each index read once, each part counted once | Agrees |
| B12 | Exports' indexes | `index/`, `history/`, `keys/` | Agrees: exports are read, and every file is checked against the layout |
| B13 | The search index | Defined: `table` `search`, schema `jetcarp-search 1`, Parquet parts and score sidecars | Agrees: hashes, sizes, segment names, run ids and local paths checked. Runs are not read further |
| B14 | Keys and signatures over HTTP | The publisher's statement and signature have names the manifest gives; listing waits for 1.1 | Agrees, except question 3 above |
| B15 | Media extensions | `png`, `gif`, `webp`; nothing else | Agrees; `bytes` checked against the file |
| B16 | Generations populations | Every listed cell counts, ageing cells too | Agrees (as chosen) |

### C. Smaller

| # | Question | How pack-format.md settled it | Reader |
| --- | --- | --- | --- |
| C1 | ±2^53 | Inclusive; any fraction or exponent refused | Agrees |
| C2 | Canonical refs and key statements | MUST be canonical; JETCARP refuses them | Agrees: now refused (was a WARN) |
| C3 | A signature's final newline | As `ssh-keygen` writes it; accepted without | Agrees |
| C4 | Unknown tables; `engine.schemes.tables` | Ignored; informational | Agrees |
| C5 | `params` of a tiling without parameters | Null when the key has none, as the key has them | Agrees (the column, not `"{}"`) |
| C6 | Rebuilding `pin_key` | The `edges` column was added | Agrees; checked |
| C7 | Optional tables' types and sorts | Given | Agrees (`raters`: see question 6 above) |
| C8 | Group raters | A group's `consent_date` is its latest member's; no nesting; no name or ORCID; every member consented | Agrees; also `notes` must be agreed for a note |
| C9 | Rater ids | `rater-` and 16 hex digits | Agrees; checked |
| C10 | Ant `start=` | JSON: `start="empty"` | Agrees |
| C11 | Receipts | Reserved; named by their own SHA-256 | Agrees: no longer read (0.4 writes none) |
| C12 | NFC | SHOULD; readers never normalise | Agrees: WARN only, hashes over bytes as given |
| C13 | Local paths | `/x/y`, `~/`, `C:\`, `\\server\`, `file://`; never in a pack | Agrees: those forms, in every text cell and the manifest, are a FAIL |
| C14 | JSON in cells | RFC 8785 text, decimals allowed; `gun` and `range` shapes given | Agrees: every JSON cell must be in its one spelling; `gun` and `range` members checked |
| C15 | `requires` names | None in 1.0 | Agrees |
| C16 | Empty key-statement members | Omitted | Agrees (WARN on an empty one) |
| C17 | Reachability and pruning | Parents reachable; a reader never needs one; `retain` for hosts | Agrees |
| C18 | Cell-state meanings | The automata pages | Agrees |

### Level 3

| Question | How pack-format.md settled it | Reader |
| --- | --- | --- |
| Which rule | The pattern's `rule`, else its main verification's; every pattern has one | Agrees. Also replayed in every other verified rule; a pattern with neither is a level 1 FAIL |
| `steps_per_gen` | Applied that many times per generation | Agrees |
| B0 rules | Two complementary rules alternating on even and odd generations | Agrees, for B/S rules. A B0 rule that survives with every neighbour live, a Generations B0 rule, or B0 with several steps a generation is skipped: the alternation is not defined for them. The alternation is tested against a brute-force run of the real rule |
| Repeated or one-way offsets | Each neighbour once, symmetric; each listed neighbour counts once | Agrees; checked at level 1 |
| Aperiodic patterns | Not level 3 in 1.0 | Agrees: skipped with the reason |

**Is the `tilings` table enough to replay without JETCARP?** Yes, for lattice tilings and the rules
level 3 covers. The offsets give the whole neighbour graph, and the pin now ties them to the record:
replay runs on exactly the geometry the id was made on. Replay is checked on the square grid and on
a two-tile unit cell (where the glider's period becomes 8). The spec's square-grid row, with its
`-0.0`, is reproduced exactly. Rule tables with rotations and ants would need the cyclic order,
which is now specified but not used by this reader.
