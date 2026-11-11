# Questions on pack format 1.0, from writing an independent reader

Each place where `design/pack-spec.md` (draft 3, sections 1 to 26) or `library-format.md` was
unclear, contradictory, under-specified or hard to implement while writing `jetcarp_pack`, written
from those documents alone. Each entry gives the section, the question, what this reader does, and
what the spec could say instead. The most important come first: those that block a check or would
give two implementations different ids or verdicts. The answer to "is the `tilings` table enough to
replay without JETCARP?" is in [Level 3](#level-3-replay).

## A. Blocks a check, or changes ids

### A1. The pin's byte layout is not in the spec (sections 11, 2, 16)
- **Question.** Section 11 says the pin "can be recomputed from these columns with the byte layout
  in `crates/jetcarp-core/src/pin.rs`, which this spec will copy in full". It is not copied.
  `library-format.md` names the parts (a polygon hash, a neighbour hash, `jetcarp-geometry-v1`, floats
  by their bits) but not the bytes: field order, integer widths and endianness, how lists are
  delimited, the order of corners and offsets.
- **Chose.** Pins are not recomputed: `SKIP pin-not-recomputed`, once per version. So nothing binds a
  `tilings` row to its pin. A wrong row replays self-consistently on the wrong geometry, and level 3
  cannot tell.
- **Change.** Copy the layout, with the square grid as a worked example: its row as bytes, the two
  intermediate hashes and the pin `0145310ebd49abfe89eb01c42ecaf459`. Then every reader can bind
  geometry to pin.

### A2. The tiling key's `edges` are normalised, and the book doesn't say so (section 9, library-format.md)
- **Question.** The book says each tiling-key member's value is its RFC 8785 JSON, and its five-line
  function copies `t["edges"]` unchanged. The repository's independent vectors (`tools/jcs_vectors.py`,
  read here as data) instead write each edge as `{"mode", "curved", "nodes"}`, with `curved`
  defaulting to `true` and other members dropped. 58 of the 106 vectors with edges differ from the
  book's function. Anyone following the book gets a different id for any record on a tiling whose
  edges leave out `curved`.
- **Chose.** `jcs.tiling_key` follows the book. `tests/test_jcs.py` pins the difference: every
  mismatch is an edge without `curved`, and goes away under that normalisation.
- **Change.** State each edge's members, their order and defaults in library-format.md, and update
  the function. Also say whether `"params":{}` is written for an empty object. The vectors and the
  function both write it; the text implies a member appears "only if the tiling has" it.

### A3. Signature and key file names contain a colon, which Windows forbids (sections 4 and 7)
- **Question.** `<signer>` is the fingerprint "as `ssh-keygen -l` prints it (`SHA256:` and base64)",
  so files are named `sigs/.../SHA256:abc....sig` and `keys/SHA256:abc....json`. A colon is not
  allowed in Windows file names (NTFS reads it as an alternate data stream), so a store or export
  can't be cloned, unzipped or untarred there.
- **Chose.** As written. Because of these names, the test stores are committed as one tar
  (`testdata/golden.tar`): Git for Windows refuses to check out a path with a colon, and committed as
  folders they would have broken the whole repository on Windows.
- **Change.** Name files by the base64 part only, or replace `:` too (`SHA256_...`). This has to be
  fixed before 1.0 freezes, since JETCARP runs on Windows.

### A4. The geometry conventions are left to the engine (sections 23.1, 11)
- **Question.** Section 23.1 says axis directions and handedness "are to be copied from the engine,
  not assumed" but doesn't give them. Also not given:
  - what `tile_types.shape` (`int32`) means: a shape index, or the number of sides;
  - the order and orientation of `corners`;
  - whether `j` grows up or down. The glider's `d=-1,-1` is up and left only if `j` grows down;
  - the "engine's cyclic order" of offsets: where it starts, and which way it turns.
- **Chose.** None of this matters for B/S and Generations replay, which only count neighbours. The
  test writer's square grid uses `shape` 0, corners counter-clockwise from (0,0), and offsets
  counter-clockwise from +i. These are guesses, and the Rust importer will see different bytes for
  the same tiling.
- **Change.** Give the conventions, and the square grid's full row as an example. They matter for
  rule tables with rotations and for recomputing pins (A1).

### A5. Ant ids and targets (sections 9, 11)
- **Question.** "An ant's id is `ant-` plus the same hash." Is the `ants.id` column `ant-<hex>`, and
  so a provenance target `ant:ant-<hex>`? Or is the column the bare hex, with `ant-` only for
  display?
- **Chose.** The column holds `ant-` and 32 hex digits, and targets are `ant:` plus the column
  (`ant:ant-...`).
- **Change.** Say which, with an example target.

### A6. Annotations "latest wins by version" needs history the pack doesn't record (section 24 fix 2 vs section 5)
- **Question.** Fix 2 orders a publisher's annotations by the manifest that added them, "by parent
  order". But a row doesn't say which version added it. To find out, a reader must walk parents to
  see which ancestor first listed each part. Section 5 says a reader learns a version's content
  "without walking history". Two rows for the same target and field in one version also need an
  order.
- **Chose.** `find` takes the newest by `date`, then by the value's UTF-8 bytes.
- **Change.** Give annotations a per-publisher `seq` column like `provenance` has, or record in each
  index entry the version that added the part.

### A7. Who signs key statements, and how revocation works with "times are never ordering" (sections 7, 3)
- **Question.**
  - A first key statement is "signed with namespace `jetcarp-key` like a ref". By its own key?
  - A successor "is signed by that old key". Then nothing proves the new key's holder agreed.
  - A revocation is signed by which key?
  - "Refuse refs and manifests signed by a revoked key after the revocation's date": which date?
    The statement has only `created`. Section 3 says times are never ordering, and a manifest's
    `created` is chosen by whoever holds the stolen key.
  - Revoking replaces `keys/<signer>.json`, and key statements have no `seq`, so serving the old file
    again undoes the revocation.
- **Chose.**
  - A first statement is signed by its own key, a successor by the key it supersedes, and a
    revocation by the revoked key or a confirmed successor.
  - The revocation's date is its `created`. A manifest created before it gets a WARN, after it a
    FAIL. A ref by a revoked key always fails.
  - A supersession signed only by the new key is `SKIP key-unconfirmed`, with the GitHub URL when
    `github` is set (no network here).
- **Change.** Say who signs each kind of statement, and consider two signatures on a successor. Make
  revocation sticky: a follower that has seen one keeps it, and key statements get a `seq`. Admit
  that a revoked key's own dates prove nothing.

### A8. Further machine keys: no field, and removal breaks history (section 24 fix 1)
- **Question.** "A publisher's key statement can list further authorised keys", but the field isn't
  named or shaped. Also open:
  - Do machine keys need their own `keys/` statements?
  - May they sign refs?
  - If a key is taken off the list, every manifest it signed stops verifying.
- **Chose.**
  - `"authorised": [{"key": "ssh-ed25519 ...", "fingerprint": "SHA256:...", "label": "laptop"}]`.
  - Machine keys need no statement of their own, and may sign manifests and refs.
  - Removing one does break history, as written.
- **Change.** Name the field. Keep removed keys listed with a `removed` date, as for revocation.

### A9. Publisher identity across a key change (sections 7, 23.5, 24 fix 1)
- **Question.** A feed is "(publisher fingerprint, feed name)", and events are keyed by "the
  publisher's primary fingerprint". After key R1 is superseded by R2:
  - Is the feed (R1, f) or (R2, f)?
  - Do a follower's `seq` checks carry over?
  - Should new manifests and events name R1 or R2?
- **Chose.** A publisher is the first key of its chain, and the follower state is keyed by it. New
  manifests and refs name the current key (R2). A ref naming a superseded key is
  `FAIL superseded-key`. The `key-rotation` test store does this.
- **Change.** Say the publisher's id is the first fingerprint of the chain and never changes.

### A10. Quick Start ids have no scheme (sections 2, 11)
- **Question.** A content id is "32 hex digits of SHA-256 over its identity text and geometry pin",
  and a Quick Start entry is a record, but `quickstart` has no identity column and no scheme.
- **Chose.** Any short name (`[A-Za-z0-9._-]`) is accepted and not checked (`SKIP quickstart-id`).
- **Change.** Give an id scheme (such as the hash of the scene's RFC 8785 form and pin), or say these
  ids are names.

## B. Would make implementations disagree

### B1. `first`/`last` for sort keys of several columns (section 8.1)
- **Question.** These are "the smallest and largest sort key in the part", but a key of several
  columns (verifications: `pattern_id`, `rule`, `publisher`) has no string form given.
- **Chose.** The first sort column's value, and `""` for an empty part.
- **Change.** Say so, or use a JSON array.

### B2. Sort order: within a part, and by what (sections 8, 23.4)
- **Question.** Append-only parts can't be sorted across a table.
- **Chose.** Rows are sorted within each part: text by UTF-8 bytes, integers by value.
- **Change.** Say "within each part".

### B3. Partition keys and columns don't match (sections 8.1, 8.2, 11, 20)
- **Question.**
  - An index entry's partition key is `class`, but the column is `class_group`, beside a separate
    `class` column. How do they relate?
  - Section 8.2 partitions ants and Quick Start entries "by `class`", but neither table has a class
    column.
  - Exports' Hive folders `class=spaceship/` would make DuckDB and pyarrow add a `class` column that
    collides with the file's own.
- **Chose.**
  - Partition key `class` means column `class_group`, and `class_group` should equal `class`
    (WARN if not).
  - Ants and Quick Start are partitioned by `tiling_group` only.
- **Change.** Name each partition key exactly as its column (`class_group=...`), say what
  `class_group` holds, and fix 8.2 for ants and Quick Start.

### B4. `family` and `semantics` on patterns and ants (section 10 vs section 11)
- **Question.** Section 10 says every record carries its family's version ("the `semantics`
  column"), and that ants need the same. Section 11's `patterns` and `ants` columns have neither
  `family` nor `semantics`.
- **Chose.** Optional nullable `family` and `semantics` on both. A version that differs from the
  manifest's `rule_semantics` is reported as `INFO needs-reverify`.
- **Change.** Add them to section 11.

### B5. `created_by` can't be set with pyarrow, and replacing it hides the real writer (section 3)
- **Question.** "`created_by` set to `jetcarp-pack 1`". pyarrow, the spec's own example library, has
  no option for it. Parquet readers also use `created_by` to work around known bugs of named
  writers, and a fixed string hides which writer wrote the file.
- **Chose.** The test writer rewrites the footer's Thrift field. The reader WARNs on another value
  and doesn't refuse.
- **Change.** Make it SHOULD, or move `jetcarp-pack 1` into a key-value entry and leave
  `created_by` alone.

### B6. Which key-value metadata is allowed (section 3)
- **Question.** "No writer-version or timestamp key-value metadata." pyarrow and arrow-rs write
  `ARROW:schema` by default, which differs between writers, so identical rows give different bytes.
  Is it allowed?
- **Chose.** The writer turns it off (`store_schema=False`). The reader WARNs on any other key.
- **Change.** Say whether `ARROW:schema` is allowed.

### B7. The same `seq` again (section 6)
- **Question.** "MUST refuse a ref whose `seq` is not greater than the last it accepted" makes an
  unchanged ref, the normal result of polling, a refusal.
- **Chose.**
  - The same `seq` and manifest is no update (INFO).
  - The same `seq` with another manifest is `FAIL seq-reused`.
  - A lower `seq` is `FAIL seq-rollback`.
  - `seq` must be 1 or more (section 26).
- **Change.** Say so.

### B8. Retractions in snapshots and merges (section 5)
- **Question.**
  - Is `retracts` cumulative (every later version lists all the publisher's retractions, as a full
    snapshot should), or only this version's?
  - In a merge, retractions by the other parent's publisher have nowhere to go: entries have no
    publisher.
- **Chose.** Cumulative, by the manifest's publisher.
- **Change.** Say cumulative, and give each entry a `publisher`.

### B9. Event rows twice in a merge (sections 5, 11)
- **Question.** "The total sightings of a pattern are the sum over rows." If a merge lists two parts
  that both hold the same event row, is it counted twice?
- **Chose.** Parts are a union by object hash, so an identical part counts once. Identical rows in
  different parts are kept.
- **Change.** Say whether event rows are a set (deduplicated) or a merge must not list them twice.

### B10. The decompression limit: what "decompressed" means (sections 17, 22 item 10)
- **Question.** Is "decompressed size of a part" Parquet's uncompressed page bytes (what decision 10
  measured) or the decoded data? Dictionary and run-length pages legitimately decode to far more
  than 64 times their size: a `class` column holding one value is an example.
- **Chose.**
  - Refuse when Parquet's `total_uncompressed_size` is over 64 times the stored size, before
    decoding.
  - While decoding, 64 rows at a time, stop at 64 times the stored size plus 64 MiB. That catches a
    dictionary bomb.
- **Change.** Name the measure, and add a limit on decoded size.

### B11. Index children: limit, union and visiting (sections 8.1, 17)
- **Question.** The limit of 10,000 is for parts per index; children aren't limited. A child listed
  twice: union or duplicate? A DAG 8 deep with 10,000 children per index has 10^32 paths if walked
  naively.
- **Chose.** At most 10,000 children; each index read once; part entries deduplicated by object
  hash. The top index is depth 1.
- **Change.** Say these.

### B12. Exports have no indexes (section 20)
- **Question.** "The indexes map each exported file back to its object hash", but the export layout
  has no index objects, and `pack.json` names only the index hashes.
- **Chose.** This reader reads stores only.
- **Change.** Include the indexes (`indexes/<hex>.json`), or a path-to-hash map, in exports.

### B13. The search index (section 12)
- **Question.** `searches` maps a uuid to "an index of kind `search`", but index objects have no
  kind, and their part entries carry no extension. A JSON spec object can't be found on a static
  host without guessing `.json` or `.parquet`.
- **Chose.** Each listed object is looked for as `.parquet`, then `.json`, and checked by hash
  only.
- **Change.** Define the search index (its `table` value, each entry's role), and give entries an
  `ext` or `media_type`.

### B14. Finding keys, signatures and feeds over plain HTTP (sections 4, 6, 7)
- **Question.** "A reader needs only `GET`." But static hosts don't list folders, so a reader can't
  discover:
  - a successor key (`keys/` with `supersedes` = X);
  - endorsements (`sigs/.../<manifest>/`);
  - the feeds (`refs/`) or receipts.
- **Chose.** Local folders are listed.
- **Change.** Have the ref (or a small listing file) name the key chain and the signature files.

### B15. Media files' extensions (sections 4, 11)
- **Question.** `media.object` needs an extension to be found, which "the entry that names the
  object" should give, but the media row has only `media_type`.
- **Chose.** `image/png` gives `png`, `image/gif` gives `gif`, `image/webp` gives `webp`; any other
  type is a FAIL.
- **Change.** Give the mapping.

### B16. Population of Generations patterns (section 11, library-format.md)
- **Question.** Do `cells`, `min_cells` and `max_cells`, and the identity's "live cells", count
  ageing cells (state 2 and up)?
- **Chose.** Yes: every cell listed in a phase.
- **Change.** Say so. It changes Generations ids, since phases list the cells.

## C. Under-specified, smaller

- **C1. Integers within ±2^53 (section 3).** Inclusive, or I-JSON's ±(2^53 − 1)? *Chose:*
  inclusive. Any number written with a fraction or exponent is refused, `1.0` and `1e5` included.
- **C2. Canonical bytes for refs and key statements (section 3).** Must the files themselves be
  RFC 8785, given that signatures cover the canonical form? *Chose:* a FAIL for manifests and
  indexes, whose hashes are ids, and a WARN for refs and keys.
- **C3. A signature string's trailing newline (sections 6, 7).** *Chose:* `ssh-keygen`'s exact
  output, with its final newline; the reader accepts either.
- **C4. Unknown tables (sections 8.3, 15).** *Chose:* ignored, as INFO. The relation between an
  index's `schema` (`jetcarp-patterns 1`) and `engine.schemes.tables` (`jetcarp-tables-1`) is not
  explained.
- **C5. `params` of a tiling without parameters (section 11).** `"{}"` or `"null"`? And does an
  aperiodic tiling's `params` column keep `level`? *Chose:* `"{}"`, and `level` is allowed (it is
  dropped when comparing with the key).
- **C6. The `tilings` table can't rebuild `pin_key`.** It has `family` and `params` but no `edges`.
  *Chose:* `pin_key` is checked for its one spelling on its own.
- **C7. Optional tables' types and sort orders (section 11).** `raters` gives no types; `raters`,
  `aliases` and `score_models` give no sort order. *Chose:* strings; sorted by `rater`, `old_id`,
  and `model, version`.
- **C8. Group raters (section 25 item 4).** A group's `consent_date`? Groups of groups? A group's
  `consent_scope`? *Chose:* the latest member's date; no nesting; `public-permanent`. A group with
  a `display_name` or ORCID is a FAIL.
- **C9. Rater ids (section 11).** "Random, per rater, stable", in no given format. *Chose:*
  `rater-` and 16 hex digits.
- **C10. Ant `start=` (library-format.md).** "The start as JSON: `"empty"`": quoted
  (`start="empty"`) or bare? *Chose:* quoted, since it is JSON.
- **C11. Receipts (sections 4, 14).** `<receipt-hex>` is the hash of what? *Chose:* name syntax
  only. The receipt must name its folder's manifest and verify against `verifier`'s key.
- **C12. Text in NFC (section 23.4).** Does this apply to identity texts? Normalising before hashing
  would change ids. *Chose:* WARN only, and ids are hashed from the bytes as given.
- **C13. Local paths (sections 11, 18).** "MUST be stripped", but what counts as one? *Chose:* a
  heuristic (`/home/`, `/Users/`, `~/`, `C:\`...): FAIL in `source` columns, WARN elsewhere.
- **C14. JSON in cells.** No shape is given for `gun` (schema `jetcarp-gun 1`), `range`, `render`,
  `metrics` or `scene_format`. *Chose:* checked as JSON only, with floats allowed (cells are not pack
  documents).
- **C15. `requires` names.** 1.0 defines none, so any entry refuses the pack. A registry of names
  would help.
- **C16. The example key statement has `"github": ""`,** while the text says `name`, `orcid` and
  `github` are "present only if the publisher chose to be named". *Chose:* the writer leaves them
  out.
- **C17. Reachability and pruning (section 4).** Are parents "reachable from a ref"? If so, nothing
  but orphans can ever be pruned. *Chose:* parents are checked when present; one that is missing is
  INFO. A missing `retain` is a FAIL.
- **C18. Section 23.2 points to the wrong page.** "Multi-state families: as their rule notation page
  defines", but `rule-notation.md` says it "only says how to write them"; the meanings are on the
  automata pages. *Chose:* Generations from `automata/generations.md`.

## Level 3, replay

**Is the `tilings` table enough to rebuild neighbourhoods without JETCARP?** For lattice tilings
and rules that only count neighbours (B/S and Generations), yes. Cell (i, j, k)'s neighbours are
(i + di, j + dj, k') for each offset in its type's list. That is the whole graph, and replay needs
nothing else: no corners, no axes, no order. `tests/test_replay.py` replays the glider on the square
grid and on a two-tile unit cell, where its period becomes 8 with displacement (-1, -2), and a
Brian's Brain ship. The gaps:

1. **Nothing ties the offsets to the pin** (A1). A wrong `tilings` row replays consistently on the
   wrong graph.
2. **Which rule?** Patterns of periodic classes carry no rule (`rule` is null). The rule comes from
   `verifications.rule`, matched to `rules` by (notation, the pattern's `neighbourhood`,
   `steps_per_gen`), so a pattern without verifications can't be replayed. *Chose:* every rule it
   was verified in; `SKIP no-rule` when there are none.
3. **`steps_per_gen` above 1** isn't explained for B/S rules. *Chose:* the rule is applied that many
   times per generation.
4. **B0 rules** switch on the whole dead plane, which section 23.3 requires. JETCARP runs them as two
   alternating rules (`life-like.md`), and the spec doesn't describe how. *Chose:* `SKIP b0`.
5. **Repeated or one-way offsets.** May an offset appear twice, and count twice? Must the relation
   be symmetric? *Chose:* counted as listed (each listed neighbour counts), and a one-way relation
   is WARNed.
6. **Order-sensitive families** (rule tables with rotations) need the cyclic order's start and
   direction (A4), and other families need their automata pages. Level 3 here is B/S and
   Generations only, from the masks; the masks are also checked against the notation.
7. **Aperiodic tilings: not enough.** A `site` gives tiles' corners and the pattern graph's
   neighbours among pattern tiles only. Running births outside the pattern means rebuilding
   adjacency from float corners, with a tolerance. The reference patch of section 24 fix 3 has no
   format or location. *Chose:* `SKIP aperiodic`. *Change:* list each site tile's neighbours (as
   indices into `tiles`) for both neighbourhoods.
8. **Field size.** "Large enough that nothing reaches its edge" is met by keeping live cells
   sparsely on an unbounded plane, with a budget per record (100,000 generations, 1,000,000 live
   cells, 20 s).

## Level 4 notes (not implemented; met while making test records)

The canonical form can be partly inferred from the book's one example. Over every phase in the
square grid's 8 symmetries, move each to the origin (smallest i and j 0) and take the one whose
cells, sorted by (j, i, k, s), are smallest. That reproduces the glider's text and id exactly. Not
inferable:
- whether "moved to the origin" means the bounding box or the first cell (the same for every test
  pattern here);
- the picture for states above 1;
- the `k@i,j` list for several tile types;
- empty rows in a picture;
- the order of candidates on other tilings.

The test stores' block, blinker and LWSS use this inference, so their ids are a prediction that the
Rust importer's level 4 will test. The test ant's outcome uses this writer's own start direction and
turn sense (section 23.2 leaves them to the ant pages), and its highway onset (step 9,977) and period
(104) match the known values.
