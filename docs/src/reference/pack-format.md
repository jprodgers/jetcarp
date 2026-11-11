# Pack format

A **pack** is how JETCARP shares data: patterns, ant and turmite runs, Quick Start scenes, the
tilings and rules they run on, and optionally search runs, pictures and ratings. This page is the
normative definition of pack format **1.0**, the first format JETCARP keeps readable for good. It
is written so that a program can read, check and merge packs without JETCARP; JETCARP itself
publishes and imports them with `jetcarp pack` (see [Command line](#the-command-line)).

The words MUST, MUST NOT, SHOULD and MAY are used as in RFC 2119. Notes in *italics* explain, and
add no rules.

A pack makes four promises:

1. **Readable without JETCARP.** Every table is a Parquet file and every other document is JSON,
   so pyarrow, Polars, DuckDB, R's arrow package or Spark read a pack directly, and DuckDB can
   query one in place over HTTPS.
2. **Checkable without JETCARP.** Every file is named by its SHA-256, every version is signed with
   an SSH signature, every record's id is the hash of an identity text the pack carries, and every
   periodic tiling's geometry pin follows from the geometry the pack carries.
3. **Readable for good.** A pack written by JETCARP 0.4 is read by every later JETCARP. The test
   suite reads the [golden packs](#golden-packs) kept in the repository on every run.
4. **Mergeable.** Two packs that hold the same pattern hold the same facts about it, so merging is a
   union, and what varies (who found it, how often it was seen, its name) is kept per publisher.

## Terms

| Term | Meaning |
| --- | --- |
| store | a folder (or URL prefix) holding a pack's files in the [layout](#the-store) below |
| object | an immutable file in a store, named by the SHA-256 of its bytes |
| manifest | the JSON object describing one version of a pack (like a Git commit) |
| index | a JSON object listing a table's Parquet parts (like a Git tree) |
| part | one Parquet file of a table |
| ref | a signed pointer to a feed's newest manifest |
| feed | a named sequence of versions published by one publisher |
| publisher | whoever signs a feed's versions, known by a key fingerprint |
| export | one version laid out with readable names, for a single download |
| record | a pattern, an ant run or a Quick Start entry |
| content id | a record's id: 32 hex digits of SHA-256 over its identity text and geometry pin |
| target | a record or search run named in a table: `pattern:<id>`, `ant:<id>`, `quickstart:<id>`, `search_run:<uuid>:<run id>` |

## Encodings

- **Text** is UTF-8 without a byte-order mark. Line ends inside identity texts are `\n`. Writers
  SHOULD write text in Unicode NFC; readers MUST NOT normalise it, and every hash is over the bytes
  as they are. *JETCARP's identity texts are ASCII.*
- **Hashes** are written `sha256:` and 64 lower-case hex digits of SHA-256. A reader MUST refuse a
  hash with another prefix where it needs the object, and a malformed one anywhere.
- **JSON documents** (manifests, indexes, refs, key statements, `jetcarp-store.json`):
  - MUST be written in [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785) canonical form (members
    sorted by their names' UTF-16 code units, no whitespace, RFC 8785's string escapes), so a
    document has one spelling and one hash. A reader MUST refuse a manifest or index whose bytes
    are not that form, and SHOULD refuse a ref or key statement that is not; JETCARP refuses all
    four.
  - MUST NOT hold a number written with a fraction or an exponent (`1.0` and `1e5` included), or
    an integer beyond ±2^53 (that bound included). Decimal quantities are strings, written as
    ECMAScript writes the number (`"0.35"`, `"2"`).
  - nest arrays and objects at most 32 deep, and are at most 16 MiB.
  - A reader MUST ignore members it does not know (a later minor version may add them).
- **JSON in table cells** (scenes, ranges, render settings and the like) is RFC 8785 canonical
  JSON too, but it MAY hold non-integer numbers: a cell is data, not a pack document.
- **Times** are RFC 3339 strings in UTC with `Z` and whole seconds (`2026-11-11T09:02:38Z`):
  every time in a pack, in documents and in table columns alike (`raters.consent_date` too; a
  calendar date alone is written as its midnight, `2026-11-01T00:00:00Z`). Times are
  information: no rule of this format orders anything by a time, except where it breaks a tie
  that nothing else breaks ([latest wins](#annotations)).
- **Parquet tables:**
  - zstd compression; statistics and page indexes (column and offset indexes) written;
  - each part's rows sorted as its table says ([below](#tables-indexes-and-parts));
  - Parquet logical types as each table lists them: `string` is UTF-8 binary, `int32`, `int64`,
    `bool`, `float64`, `list<...>` the three-level list (JETCARP names its element `element`), and
    `struct<...>` a group. Readers SHOULD accept another integer width whose values fit, `float`
    for `float64`, and large string and list types;
  - writers SHOULD write no key-value metadata at all (no writer version, no time, no
    `ARROW:schema`), and readers MUST ignore any there is;
  - JETCARP sets `created_by` to `jetcarp-pack 1`. Other writers MAY leave theirs, and readers
    MUST NOT depend on it.

  *JETCARP writes zstd level 9, Parquet 1.0 data pages, dictionary encoding where it helps, row
  groups of at most 16,384 rows or 32 MiB of row text, and parts closed at 224 MiB, so the same
  rows give the same bytes with the same writer. Dedupe never depends on that: records are merged
  by content id.*
- **Licences** are SPDX identifiers (`CC0-1.0`, `CC-BY-4.0`). JETCARP's own feeds take only
  `CC0-1.0`.

## The store

```text
jetcarp-store.json                          {"format":"jetcarp-store","version":"1.0"}
refs/<feed>.json                            a feed's signed pointer
objects/sha256/<aa>/<hex>.<ext>             every immutable file (manifests, indexes, Parquet, media)
sigs/sha256/<aa>/<hex>/<signer>.sig         detached signatures on the manifest <hex>
keys/<signer>.json                          publishers' key statements
views.sql                                   optional, not part of the format: DuckDB views
receipts/                                   reserved (verification receipts; not written by 0.4)
torrents/                                   reserved for 1.1
```

- `<hex>` is the object's 64 hex digits and `<aa>` their first two, so there are at most 256
  folders, each far below 10,000 files.
- `<ext>` is one of `json`, `parquet`, `png`, `gif`, `webp`, given by what names the object: an
  index or a manifest's `tables` is `json`, a part, search segment, score sidecar or reference
  patch is `parquet`, and a media row's `media_type` gives its file's ([media](#media)). The hash
  covers the bytes only.
- `<signer>` names a key in file names: the 64 lower-case hex digits of the key's SHA-256 digest,
  the same digest its `SHA256:` fingerprint writes in base64. (No colon, which Windows forbids, and
  no names that differ only in case.) In Python:
  `hashlib.sha256(base64.b64decode(blob)).hexdigest()` over the key's base64 blob, or
  `base64.b64decode(fp[7:] + "=").hex()` from a fingerprint `fp`.
- `<feed>` is 1 to 64 letters, digits and `-`. A store holds at most one feed of a name.
- A store is served as plain files: a reader needs only `GET` (and `Range` to resume and to query
  in place). Nothing in a store is executable, and nothing is run from one.
- A writer MUST NOT change an object once written. A reader MUST check an object's hash before
  using it. A reader MUST ignore top-level paths it does not know (a writer's temporary `.tmp`
  folder among them).
- **No name comes from a pack's data** except as this page builds it: from hashes (checked to be 64
  hex digits), the extension list, feed and signer names, and, in exports, table names, checked
  partition values, numbers, uuids and ids ([exports](#exports)). A path in any other form is
  refused.
- Names never extend a sibling's name with a byte below `/` (`data/` beside `data.json`): within a
  folder there are only same-length hex names or the fixed names above.
- **Reachability.** An object is reachable from a ref through its manifest, the manifest's parents
  (recursively), indexes, parts and the objects their rows name. Hosts MAY prune unreachable
  objects, but MUST keep any manifest given a DOI or listed in a reachable manifest's `retain`. A
  reader MUST NOT need a parent to read a version (every manifest is a full snapshot) and treats a
  missing parent as history it does not have.

## The manifest

One manifest is one complete version of a pack, an object (`.json`) whose hash is the version's
id. The minimal golden pack's, pretty-printed (stored without whitespace):

```json
{
  "created": "2026-11-11T00:00:00Z",
  "creators": [{"name": "Golden Publisher A", "role": "creator"}],
  "description": "Golden pack: minimal. Made by JETCARP's tests; never rewritten.",
  "doi": "",
  "engine": {
    "name": "JETCARP",
    "rule_semantics": {"ant": "1", "cyclic": "1", "...": "1", "wireworld": "1"},
    "schemes": {"ant": "jetcarp-ant-v1", "geometry": "jetcarp-geometry-v1",
                "pattern": "jetcarp-pattern-v3", "tables": "jetcarp-tables-1"},
    "version": "0.4.0"
  },
  "feed": "golden-a",
  "format": "jetcarp-pack",
  "licence": "CC0-1.0",
  "parents": [],
  "publisher": "SHA256:ACp7het+5at0PuMIyNhhqe8OXAJ9oehORSHEyzwQYcg",
  "related": [],
  "requires": [],
  "retain": [],
  "retracts": [],
  "searches": {},
  "stats": {"ants": 0, "bytes": 63913, "patterns": 3, "quickstart": 1},
  "tables": {"annotations": "sha256:432e...", "patterns": "sha256:5669...", "...": "..."},
  "title": "minimal",
  "version": "1.0",
  "version_label": "golden-1"
}
```

| Member | Meaning |
| --- | --- |
| `format`, `version` | always `jetcarp-pack`; `major.minor` ([versions](#versions-and-compatibility)) |
| `requires` | features a reader MUST understand to use this pack; a reader that does not know one MUST refuse the pack. Format 1.0 defines none, and a 1.0 reader refuses any; later minor versions list theirs on this page |
| `feed` | the feed this version belongs to |
| `title`, `description`, `version_label` | for people; `version_label` is free text, never an order |
| `created` | when this version was made |
| `licence`, `creators`, `doi`, `related` | citation metadata, DataCite-shaped: creators `{"name", "orcid"?, "role"}` (`creator`, `contributor`, `curator`...), a creator's `orcid` only if they chose it; `related` a list of `{"relation": "IsSupplementTo", "id": "doi:..."}` |
| `parents` | manifest hashes of the versions this one follows: the feed's previous version first, then any merged; none for a first version |
| `publisher` | the fingerprint (`SHA256:...`, as `ssh-keygen -l` prints it) of the publisher's current key ([keys](#signatures-and-keys)) |
| `engine` | what made the data: `name`, `version`, the id `schemes` (`pattern`, `ant`, `geometry`, and `tables`, the name of the set of table schemas of this format, informational), and `rule_semantics`, every rule family's semantics version ([rule semantics](#rule-semantics-versions)) |
| `tables` | for each table, the hash of its index; a table that is absent is empty. A reader ignores tables it does not know |
| `searches` | `{search uuid: search index hash}` for the search runs it includes ([search runs](#search-runs)) |
| `retracts` | records withdrawn: `[{"target", "publisher", "reason", "date"}]`, cumulative ([retractions](#retractions)) |
| `retain` | earlier manifest hashes hosts MUST keep (cited versions) |
| `stats` | counts for people (`patterns`, `ants`, `quickstart` rows, and `bytes` of its tables' parts and search runs); not authoritative |
| `slice_of`, `filter` | optional: the manifest says it names a subset of another manifest's objects (`filter` describes which, for people); informational |

**Every manifest is a full snapshot.** A version's content is what its manifest and indexes name;
no reader walks history to read it. An unchanged table keeps its index hash, so a new version
costs only its new indexes and parts.

### Retractions

`retracts` is how append-only data withdraws a record: `target` is a [target](#terms), `publisher`
the fingerprint of the publisher whose assertions it withdraws (the version's publisher when it
first listed it), and `reason` and `date` are for people. The list is cumulative: every later
version of the feed lists every retraction its parents list, and a merge keeps its parents' with
their publishers. A reader shows a retracted record as retracted, with the reason; an importer
flags it and never deletes the user's copy. A retraction applies only to its publisher's
assertions: a record another publisher also published stays theirs.

## Refs

```json
{"feed": "golden-a", "format": "jetcarp-ref", "manifest": "sha256:83f9...", "publisher": "SHA256:ACp7...",
 "seq": 1, "signature": "-----BEGIN SSH SIGNATURE-----\n...\n-----END SSH SIGNATURE-----\n",
 "updated": "2026-11-11T00:00:00Z", "version": "1.0"}
```

- `refs/<feed>.json` names the feed's newest manifest. `seq` is 1 for a feed's first version and
  goes up by at least one with every update; `publisher` is the publisher's current key.
- A follower keeps the last `(seq, manifest)` it accepted for the feed (the pair publisher id and
  feed name). A ref with the same `seq` and manifest is no update; one with the same `seq` and
  another manifest, or a lower `seq`, MUST be refused (no rollback).
- `signature` is an SSH signature (namespace `jetcarp-ref`) over the RFC 8785 bytes of the ref
  without its `signature` member, by the publisher's current key or a machine key its statement
  authorises. A follower MUST refuse a ref whose signature does not verify.
- A reader MUST ignore ref members it does not know (1.1 adds `mirrors` and `torrents`).

*JETCARP 0.4 reads refs to find a store's newest version and checks their signatures; following
feeds, and keeping the last `seq`, arrive with networking in 0.6.*

## Signatures and keys

- **Signatures** are SSH signatures in the `sshsig` format that `ssh-keygen -Y sign` writes, with
  the message hashed by SHA-512 (as `ssh-keygen` does), armored, ending with a newline (a reader
  accepts one without). Format 1.0 uses **Ed25519 keys only** (`ssh-ed25519`); a reader MAY count
  a signature by another kind of key as no signature. Namespaces: `jetcarp-manifest`,
  `jetcarp-ref`, `jetcarp-key`, and `jetcarp-receipt` (reserved).
- **A manifest's signature** is a detached file, `sigs/sha256/<aa>/<hex>/<signer>.sig`, over the
  manifest's exact bytes, so signing never changes a manifest's hash and others can add
  endorsements later. A reader takes the signing key from the signature itself, and MUST check
  that the file's `<signer>` is that key's. The publisher's own signature is at the name the
  manifest's `publisher` gives, so a reader over HTTP fetches it without listing a folder.
- Anyone can check one with OpenSSH:

  ```sh
  echo "publisher namespaces=\"jetcarp-manifest\" ssh-ed25519 AAAA..." > allowed
  ssh-keygen -Y verify -f allowed -I publisher -n jetcarp-manifest \
      -s sigs/sha256/83/83f9.../<signer>.sig < objects/sha256/83/83f9....json
  ```

- **Key statements** (`keys/<signer>.json`):

  ```json
  {"created": "2026-11-11T00:00:00Z", "fingerprint": "SHA256:ACp7...", "format": "jetcarp-key",
   "key": "ssh-ed25519 AAAA... comment", "name": "Golden Publisher A", "revoked": false,
   "signature": "-----BEGIN SSH SIGNATURE-----\n...", "version": "1.0"}
  ```

  | Member | Meaning |
  | --- | --- |
  | `key`, `fingerprint` | the OpenSSH public key and its `SHA256:` fingerprint, which MUST match |
  | `name`, `orcid`, `github` | who the publisher is: present only if they chose to be named |
  | `created` | when it was made (information only) |
  | `supersedes` | present on a successor: the fingerprint of the key it succeeds |
  | `revoked` | `true` makes the statement a revocation of `key` |
  | `authorised` | present when the publisher signs on several machines: `[{"key", "fingerprint", "label"?, "revoked"?}]`, further keys that may sign the publisher's manifests and refs; entries are never removed, and one marked `revoked: true` signs nothing |
  | `signature` | an SSH signature (namespace `jetcarp-key`) over the RFC 8785 bytes of the statement without `signature` |

- **Who signs a statement.** A first statement (no `supersedes`) is signed by its own key. A
  successor is signed by the key it supersedes, whose own statement must stand. A revocation is
  signed by the revoked key itself or by a successor (a key whose statement supersedes it and is
  signed by it).
- **Revocation.** A revoked key signs nothing: no manifest, ref or successor counts by it, whatever
  the dates say (no rule depends on time). A successor stands after its predecessor's revocation
  only if it signed that revocation itself. Revocation is permanent: a reader that has seen one
  MUST keep the key revoked, so a store that serves an older statement again cannot undo it
  (JETCARP keeps them in the library's `pack_revoked`).
- **The publisher's id** is the fingerprint of the first key of its chain of statements, and never
  changes. Manifests and refs name the publisher's current key in `publisher`; a reader finds the
  id by following `supersedes`. The `publisher` column of every table holds the id. *JETCARP 0.4
  makes one key per publisher and never rotates it, so its manifests, refs and rows all name that
  key.*
- **Signed by its publisher** means: the publisher's key statement stands, its key is not revoked,
  and a signature on the manifest by that key, or by a machine key it authorises, verifies.

## Tables, indexes and parts

### Indexes

```json
{"children": [], "format": "jetcarp-index",
 "parts": [{"bytes": 9123456, "first": "00a1...", "last": "ffe0...", "object": "sha256:...",
            "partition": {"class_group": "spaceship", "tiling_group": "k3"}, "rows": 131072}],
 "schema": "jetcarp-patterns 1", "table": "patterns", "version": "1.0"}
```

- `table` names the table; `schema` its column layout and version, `jetcarp-<table> <n>` with the
  table's name exactly as written (so `score_models` is `jetcarp-score_models 1`;
  [every schema id](#schema-ids) is listed below). A reader MUST refuse a schema number it does not
  know for a table it needs.
- `parts`: each Parquet part with its `rows`, its size in `bytes`, its `partition` (an object, empty
  for an unpartitioned table) and `first` and `last`, the value of the table's **first** sort column
  in the part's first and last row, as text (an integer in decimal), for skipping.
- `children`: further indexes of the same table whose parts add to these. At most 10,000 parts and
  10,000 children per index, and at most 8 levels (the manifest's index is level 1). An index
  reached more than once is read once, and a part listed more than once counts once.
- `compacts` (reserved): an index MAY list parts whose rows its own parts hold again (compaction).
  A follower that has those rows MAY skip the new parts. JETCARP 0.4 never writes it and ignores
  it.
- **A table is the union of its parts.** Each part's rows are sorted by the table's sort columns
  (text by its UTF-8 bytes, integers by value, `false` before `true`); parts are not sorted across
  each other. *JETCARP breaks ties by the rows' RFC 8785 text.*
- **Parts are append-only.** A new version keeps its parents' parts and adds parts holding only
  rows the parents lack. A part SHOULD be at most 256 MiB and MUST be at most 4 GiB.
- **Fact and event tables.** In a fact table (`tilings`, `rules`, `patterns`, `ants`, `quickstart`,
  `aliases`) a key's rows are the same wherever they appear: a reader keeps one and MAY check the
  others are identical (JETCARP's `--strict` does). In an event table, rows are not deduplicated
  across parts: a writer MUST NOT add a row its parents already have (JETCARP adds only missing
  rows), and identical rows in two parts count twice.
- **Schema evolution.** Adding a nullable column is a minor change: the schema id stays, and readers
  MUST ignore columns they do not know and read a missing nullable column as null. Removing a
  column or changing what one means is a new schema number.

### Schema ids

Every schema id format 1.0 uses, so none has to be derived:

| Schema id | Of | Written in |
| --- | --- | --- |
| `jetcarp-tilings 1` | the `tilings` table | its index's `schema` |
| `jetcarp-rules 1` | the `rules` table | its index's `schema` |
| `jetcarp-patterns 1` | the `patterns` table | its index's `schema` |
| `jetcarp-verifications 1` | the `verifications` table | its index's `schema` |
| `jetcarp-sightings 1` | the `sightings` table | its index's `schema` |
| `jetcarp-provenance 1` | the `provenance` table | its index's `schema` |
| `jetcarp-annotations 1` | the `annotations` table | its index's `schema` |
| `jetcarp-ants 1` | the `ants` table | its index's `schema` |
| `jetcarp-quickstart 1` | the `quickstart` table | its index's `schema` |
| `jetcarp-media 1` | the `media` table | its index's `schema` |
| `jetcarp-aliases 1` | the `aliases` table | its index's `schema` |
| `jetcarp-ratings 1` | the `ratings` table | its index's `schema` |
| `jetcarp-raters 1` | the `raters` table | its index's `schema` |
| `jetcarp-score_models 1` | the `score_models` table | its index's `schema` |
| `jetcarp-search 1` | a [search index](#search-runs) (`table` `search`) | its `schema` |
| `jetcarp-explore-runs 1` | an explore search's run segments | the search index's `run_schema` |
| `jetcarp-explore-scores 1` | an explore search's score sidecars | the search index's `score_schema` |
| `jetcarp-reference-patch 1` | an aperiodic family's [reference patch](#reference-patches) | nowhere: a `tilings` row names the object |

The documents' `format` members are `jetcarp-store` (`jetcarp-store.json`), `jetcarp-pack` (a
manifest), `jetcarp-index` (a table index and a search index alike), `jetcarp-ref` and
`jetcarp-key`, each with `version` `1.0`; a manifest's `engine.schemes.tables` is
`jetcarp-tables-1`.

### Partitions

The `patterns` table is partitioned by `class_group` and `tiling_group`; `ants` and `quickstart`
by `tiling_group`. A partition's keys are named exactly as its columns, and a part holds one value
of each, which every row of the part repeats in its own columns.

- `class_group` is the pattern's class (`still_life`, `oscillator`, `spaceship`, `puffer`,
  `mover`, `gun`, `other`).
- `tiling_group` is fixed for good: `k1` to `k6` for catalogue tilings (family `periodic` with a
  catalogue id, from the catalogue's `k`; `k1` holds the 11 Archimedean tilings in their catalogue
  form), `isohedral` for the 81 isohedral types (`ih01` to `ih93`), and otherwise the family id
  (`square`, `hex`, `oct`, `trihex`, `cairo`, `hat`, `spectre`, `penrose-kd`, `penrose-rhomb`...).
  A family added later gets its own id as its group; groups are never renamed or merged.
- A partition value is 1 to 64 lower-case letters, digits, `-` and `_`, starting with a letter or
  digit; a reader MUST refuse any other.

## Ids

- **A pattern's id** is the first 32 hex digits of SHA-256 over the UTF-8 bytes of
  `identity + "\n" + pin`: its identity text (scheme `jetcarp-pattern-v3`, specified in
  [Library format](library-format.md#the-identity-text-of-a-pattern), including the one spelling
  of its `tiling=` line) and the 32-hex [geometry pin](#geometry-pins) of its tiling. The `phases=`
  line lists every cell of each phase, whatever its state (a Generations pattern's ageing cells
  too), and `cells`, `min_cells` and `max_cells` count them the same way.
- **An ant's id** is `ant-` and the same hash over its `jetcarp-ant-v1` identity text
  ([Library format](library-format.md#the-identity-text-of-an-ant)); its `start=` line is JSON, so
  the empty start is `start="empty"`. The `ants.id` column holds `ant-<hex>`, and a target names it
  `ant:ant-<hex>`.
- **A Quick Start entry's id** is `qs-` and the first 32 hex digits of SHA-256 over its `scene`
  column's text, `"\n"`, and its `pin`. Every target that names an entry (in `provenance`, `media`,
  `ratings`, `annotations` and `retracts`) is `quickstart:qs-<hex>`, never a library's own name for
  it. *JETCARP's library keys its entries by name (a pattern's id, a scene's file stem): a publish
  writes the pack id in its place (a selection or retraction may name either), and an import maps
  it back, to the entry the library has with that pack id or else to a new entry kept under it.*
- A reader checks an id with one hash. In Python:
  `hashlib.sha256((identity + "\n" + pin).encode()).hexdigest()[:32] == id`.
- That proves the id matches the stated identity. That the identity is the canonical form of a real
  evolution needs re-running and canonicalising ([conformance](#conformance-levels) levels 3 and 4).
- If an identity scheme is ever revised (`jetcarp-pattern-v4`), packs keep their identity texts, so
  old ids stay checkable, and the new version maps old ids to new ones in `aliases` with reason
  `scheme-change`.

## Geometry pins

A pin is the first 32 hex digits of SHA-256 over the text `jetcarp-geometry-v1`, `\n`, the
**polygon hash**, `\n`, the **neighbour hash** (each 64 hex digits). The two hashes are SHA-256
over these bytes (integers little-endian `u32` unless marked, floats as the `u64` of their IEEE 754
bits, so `-0.0` and `0.0` differ):

```text
polygon hash   = SHA-256("jetcarp-geometry-v1 polygons\n" + body)
  lattice:   "lattice", a1.x, a1.y, a2.x, a2.y (f64), types (u32),
             then per tile type: shape (u32), corners (u32), corners × (x, y) (f64)
  aperiodic: "patch", tiles (u32), then per tile: shape (u32), corners (u32), corners × (x, y)
neighbour hash = SHA-256("jetcarp-geometry-v1 neighbours\n" + body)
  lattice:   for edge, then touch: per tile type: slots (u32), slots × (di i32, dj i32, k u32)
  aperiodic: for edge, then touch: per tile: degree (u32), degree × neighbour tile (u32);
             then per tile: open (one byte, 1 on the patch's open boundary, else 0)
```

The strings are their ASCII bytes with no terminator. A lattice tiling's values are its `tilings`
row's columns (`a1`, `a2`, `tile_types`, `edge`, `touch`), in their order; an aperiodic family's are
its reference patch's rows, in `tile` order ([reference patches](#reference-patches)). The square
grid's row gives:

```text
a1 = [1, 0], a2 = [-0.0, 1], tile_types = [{shape 0, corners (0,0) (1,0) (1,1) (0,1)}]
edge  = [[(0,-1,0) (1,0,0) (0,1,0) (-1,0,0)]]
touch = [[(-1,-1,0) (0,-1,0) (1,-1,0) (1,0,0) (1,1,0) (0,1,0) (-1,1,0) (-1,0,0)]]
polygon hash   f5a761f20c098aba5f00361946f6b309ccbb17ab5f6aca2474d0aa3e64a8075b
neighbour hash 12a567c8a5480b1b7ca4a8d4ccd2439565888a698ace2567508ed2c954c0bbdb
pin            0145310ebd49abfe89eb01c42ecaf459
```

`crates/jetcarp-pack/tests/data/pin_vectors.jsonl` holds the full `tilings` rows (as JSON) of
twelve periodic tilings, and JETCARP's tests check that the layout above gives each one's hashes.

**Conventions** (what a reader needs to rebuild a periodic tiling's cells and neighbours):

- World units: a tile's edge is about 1. The plane is drawn **y down** (as on a screen): `x` grows to
  the right and `y` downward.
- A lattice cell `(i, j, k)` is tile type `k` placed at `i·a1 + j·a2`: its corners are
  `tile_types[k].corners`, each plus `i·a1 + j·a2`. On the square grid `i` grows rightward and `j`
  downward (so the glider's `d=-1,-1` is up and to the left).
- `shape` is the index of the tile type's prototile in the engine's list of shapes for that tiling:
  two tile types with one shape are congruent copies placed differently. It means nothing more,
  but it is part of the pin.
- `corners` are in the prototile's own order, moved into place: clockwise on the y-down screen for
  a tile type as drawn, counter-clockwise for a mirrored one.
- A tile type's `edge` list holds the tiles that share a stretch of its boundary, and `touch` those
  that share at least a point; each neighbour once, and the relation is symmetric. A cell's
  neighbours are `(i + di, j + dj, k')` for each `(di, dj, k')` in its type's list.
- Their order is the engine's cyclic order, which the pin covers: clockwise on the y-down screen,
  starting at the tile's first corner. Each neighbour sits at where it meets the tile's boundary,
  walked from corner 0 in corner order (an edge's index plus the fraction along it; a stretch at
  its middle, a point at that point), and tiles meeting it at one point go by the angle of their
  centres about the tile's. For a mirrored tile type the list is reversed, so it is clockwise too.
  Only rules that depend on neighbour order (rule tables with rotations, ants and turmites) need
  this; the others count neighbours.

## Rule-semantics versions

Geometry pins catch changed tiles; rule-semantics versions catch a changed rule. `engine.rule_semantics`
maps every rule family JETCARP has to its version, a positive integer written as a string; the
families and their versions are listed in [Library format](library-format.md#rule-semantics-versions),
and every one is `"1"` in JETCARP 0.4.0. Each `rules` row, each pattern and each ant carries its
family and the version it was verified under (`family`, `semantics`). An importer flags a record
whose version differs from its own engine's for that family, or whose family it does not have,
as it flags a changed pin ([importing](#importing)).

## Tables

Columns are `name: type`; a `?` marks a nullable column. Text columns that hold JSON say so.
Sorting and partitions are given with each table. The design rule: **a record's facts depend only
on its id**, so two packs give identical fact rows; **everything that varies is an event row**,
append-only and keyed by publisher (who found it, when, verifications, counts, names, curation),
so merging never conflicts.

### `tilings` (facts, sorted by `pin_key`)

Enough geometry to re-run periodic patterns, and to check every pin.

| Column | Meaning |
| --- | --- |
| `pin_key: string` | the tiling's identity key: the `tiling=` text of identity lines |
| `pin: string`, `polygon_hash: string`, `neighbour_hash: string` | its geometry pin and the two hashes it is made of |
| `name: string` | display name (`Square`, `Snub square (3.3.4.3.4)`) |
| `kind: string` | `lattice` or `aperiodic` |
| `family: string` | the tiling family |
| `params: string?`, `edges: string?` | its parameters and reshaped edges as JSON, as the pin key has them (null when it has none), so `pin_key` can be rebuilt from `family`, `params` and `edges` |
| `tiling_group: string` | [partition group](#partitions) |
| `a1: list<float64>?`, `a2: list<float64>?` | lattice vectors (lattice only) |
| `tile_types: list<struct<shape: int32, corners: list<list<float64>>>>?` | each tile type's prototile index and corners placed in the unit cell (lattice only) |
| `edge: list<list<struct<di: int32, dj: int32, k: int32>>>?` | for each tile type, its edge neighbours as offsets (lattice only) |
| `touch: list<list<struct<di: int32, dj: int32, k: int32>>>?` | the same for its touch neighbours |
| `reference_level: int32?`, `reference: string?`, `reference_tiles: int32?` | aperiodic: the level its pin is computed on, the hash of its [reference patch](#reference-patches), and its tile count |

#### Reference patches

An aperiodic family's pin hashes a fixed reference patch (level 4 of each family: 4,000 tiles of the
hat and of either Penrose tiling, 4,401 of the Spectre). A pack with aperiodic records includes each
family's reference patch as a Parquet object (schema `jetcarp-reference-patch 1`, sorted by `tile`),
named by its `tilings` row's `reference`, so a reader can check the pin and locate sites on it:

| Column | Meaning |
| --- | --- |
| `tile: int32` | the tile's index, 0 first |
| `shape: int32` | its prototile index |
| `corners: list<list<float64>>` | its corners in world units |
| `edge: list<int32>`, `touch: list<int32>` | its neighbours' indexes, in the engine's order |
| `open: bool` | it lies on the patch's open boundary |

### `rules` (facts, sorted by `notation`, `neighbourhood`, `steps_per_gen`)

| Column | Meaning |
| --- | --- |
| `notation: string` | JETCARP's [rule notation](rule-notation.md), normalised (the neighbourhood is the next column, never a `V` suffix) |
| `neighbourhood: string` | `touch` or `edge` |
| `steps_per_gen: int32` | how many times the rule is applied per generation |
| `family: string`, `semantics: string` | rule family and its semantics version |
| `states: int32` | cell states (2 for B/S rules; 0 when not fixed) |
| `birth: int64?`, `survive: int64?` | B/S and Generations: bit masks of live-neighbour counts (bit n for n neighbours), so these rules need no parser |

### `patterns` (facts, sorted by `id`; partitioned by `class_group`, `tiling_group`)

| Column | Meaning |
| --- | --- |
| `id: string` | content id |
| `identity: string` | the identity text it hashes |
| `pin_key: string`, `pin: string` | its tiling (a row of `tilings`): the pin key and pin |
| `model: string`, `neighbourhood: string`, `steps_per_gen: int32` | as in the identity |
| `class: string` | `still_life`, `oscillator`, `spaceship`, `puffer`, `mover`, `gun`, `other` |
| `period: int64`, `dx: int32`, `dy: int32` | period and displacement per period in lattice steps, as the identity's `period=` and `d=` |
| `speed: string?` | `c/4 diagonal` and the like |
| `cells: int32`, `min_cells: int32`, `max_cells: int32` | cells of phase 0, and the fewest and most over the period (a mover's over its whole track) |
| `phases: list<list<struct<i: int32, j: int32, k: int32, s: int32>>>` | one period, phase 0 first, in the canonical orientation: a typed copy of the identity's `phases=` line, which it MUST equal |
| `canonical: string` | the canonical picture text |
| `rule: string?` | the exact rule, for puffers, movers, guns and `other` (else null) |
| `site: struct?` | aperiodic only: the pattern's local tiles (below) |
| `gun: string?` | guns: JSON `{"ship", "ship_label", "ship_period", "ship_displacement", "ship_speed", "per_period", "ship_phases"}`, the emitted ship in the gun's orientation |
| `family: string`, `semantics: string` | its rule's family, and the version it was verified under |
| `class_group: string`, `tiling_group: string` | the partition columns |

The identity's `canonical=`, `class=` and `nb=` lines MUST equal `canonical`, `class` and
`neighbourhood`. A pattern's rule is its `rule` when set, else that of its [verification](#verifications-events-sorted-by-pattern_id-rule-publisher-date)
marked `main`; every pattern has one.

The typed `site` of an aperiodic pattern:

```text
site: struct<
  patch: struct<family: string, params: string?, level: int32>,  -- where it was found
  anchor: int32, anchor_shape: int32, anchor_point: list<float64>,   -- the anchor tile there
  radius: int32,
  nb: string?,                                    -- touch | edge
  tiles: list<struct<shape: int32, dist: int32,   -- dist: steps from the nearest pattern cell
                     point: list<float64>, corners: list<list<float64>>>>,
  graph: struct<                                  -- null for geometric sites
    rule: string, degrees: bool, chirality: bool,
    cells: list<struct<shape: int32, flip: bool, degree: int32,
                       states: list<int32>,       -- the tile's state in each phase
                       neighbours: list<int32>>>, -- graph labels of neighbouring pattern tiles
    around: list<struct<tiles: list<int32>, count: int32>>>?>
```

`patch` and `anchor` say where one copy was found (its level and position), which two packs may
give differently for one pattern; the rest is fixed by the id. `params` is the patch's parameters
as JSON (`level` among them when the patch gave its level that way).

### `verifications` (events, sorted by `pattern_id`, `rule`, `publisher`, `date`)

| Column | Meaning |
| --- | --- |
| `pattern_id: string`, `rule: string` | the pattern, and the rule notation it was verified in (with the pattern's neighbourhood and steps per generation, a row of `rules`) |
| `publisher: string` | the publisher's id |
| `main: bool` | the rule the publisher runs the pattern in by default (one per pattern and publisher) |
| `periods: int64`, `generations: int64` | how far it was run, every generation matching |
| `field: string` | where: `dead-edged frame WxH` or `patch hat level 6` |
| `sites: int32?` | aperiodic: the sites it was run at |
| `engine_version: string`, `pin: string`, `semantics: string` | what checked it |
| `date: string` | when |
| `range: string?` | on the main row of B/S and Generations patterns: the rule range as JSON (`bits`, `birth_min`, `birth_max`, `survive_min`, `survive_max`, `states_min`, `states_max`: every rule whose masks lie between the bounds runs the pattern as stored) |
| `mover: string?` | on the main row of aperiodic movers: how far the tracker followed it (JSON) |

### `sightings` (events, sorted by `pattern_id`, `rule`, `publisher`, `source`)

| Column | Meaning |
| --- | --- |
| `pattern_id: string`, `rule: string`, `publisher: string` | |
| `source: string` | the uuid of the search that saw it, or `unattributed` for copies the publisher counted without a search |
| `source_kind: string?` | the search's kind (`hunt`, `explore`, `manual`...) |
| `count: int64`, `soups: int64` | copies seen and soups they came from |
| `first_seen: string`, `last_seen: string` | |

Rows are **increments**: a new version adds a row holding only what grew since its parents. The
total for a (pattern, rule, publisher, source) is the sum of its rows (their earliest `first_seen`
and latest `last_seen`), and a pattern's total sightings the sum over its rows.

### `provenance` (events, sorted by `target`, `publisher`, `seq`)

| Column | Meaning |
| --- | --- |
| `target: string` | `pattern:<id>`, `ant:<id>` or `quickstart:<id>` |
| `publisher: string`, `seq: int32` | the publisher's history of the record, 0 first |
| `kind: string` | `found`, `imported`, `reverified` |
| `finder: string?`, `orcid: string?` | only if the finder chose to be named |
| `search_kind: string?`, `search: string?` | the kind and uuid of the search that found it |
| `machine: string?` | operating system and CPU architecture (`linux-x86_64`), never a host name |
| `engine_version: string`, `pin: string`, `date: string` | |
| `source: string?` | what found it (`search`, `hunt`, `collide A + B`, a scene's file name, `import catalog.sqlite`): a file name only, never a path |
| `pack: string?` | the manifest hash a record was imported from |
| `run: string?` | on row 0: the run that found it, as JSON (its source, tiling with size, random state, soup, density, generation, engine and date), so JETCARP can replay the find |

### Annotations

`annotations` (events, sorted by `target`, `publisher`, `field`, `date`): per-publisher curation,
latest wins.

| Column | Meaning |
| --- | --- |
| `target: string`, `publisher: string` | |
| `field: string` | `name`, `label`, `tags`, `notes`, `state`, `rating`, `score`, `discovered_by`; readers ignore fields they do not know |
| `value: string` | JSON: a string (`name`, `label`, `notes`, `state`, `discovered_by`), a list of strings (`tags`), an integer 1 to 5 (`rating`), a decimal as a string (`score`), or `null` (the field was cleared) |
| `score_model: string?` | for `score`: the score model and its version |
| `date: string` | |
| `rev: int64` | the publisher's ref `seq` of the version that added the row |

**Latest wins.** A reader shows, for each target and field, one row of the publisher it chooses to
follow: the one with the highest `rev`; on a tie (versions on parallel branches), the later `date`,
then the greater `value` by its UTF-8 bytes. Annotations of different publishers are never merged
into one. *JETCARP writes `name`, `tags`, `notes`, `state` (when not `new`), `rating` and
`discovered_by` (when not `search`), dated by the record's last change; it does not publish its
interest score, which it computes again.*

### `ants` (facts, sorted by `id`; partitioned by `tiling_group`)

| Column | Meaning |
| --- | --- |
| `id: string`, `identity: string`, `pin_key: string`, `pin: string` | `ant-` and its content id, its identity text, its tiling |
| `rule: string`, `kind: string` | the ant or turmite rule notation; `string` (a turn string) or `table` |
| `family: string`, `semantics: string` | `ant` or `turmite`, and its version |
| `states: int32`, `colors: int32` | |
| `start: string` | the start, as in the identity (JSON) |
| `outcome: string` | `highway`, `periodic`, `symmetric`, `bounded`, `chaotic` |
| `period: int64?`, `dx: int32?`, `dy: int32?`, `onset: int64?`, `speed: float64?` | highways and periodic runs: the period in steps, the displacement per period in lattice steps, the first step of the periodic tail, and (highways) the distance per step in cell sizes (world units over the square root of a tile's mean area, to 6 decimals) |
| `steps: int64`, `cells: int64` | how many steps it was run, and its coloured (non-zero) cells at the end |
| `run: string` | the run with its proof and limits (JSON, below) |
| `tiling_group: string` | partition column |

`run` is a JSON object with two members, each an object:

```text
{"limits": {"max_period", "steps", "symmetry"},
 "run":    {"outcome", "steps", "cells", "reach", "period"?, "d"?, "onset"?, "speed"?,
            "proof"?: {"at", "period", "d", "cells"},
            "symmetry"?: {"count", "first_step", "last_step", "last_cells", "reflection",
                          "translation"}}}
```

- `limits`: how the run looked. `steps`, the steps it ran before it was classed as it is (a
  highway test under way may add one period); `max_period`, the longest highway or orbit period
  it looked for; `symmetry`, whether it tested fields for mirror symmetry (true when absent).
- `run`: what it found. `outcome`, `steps` and `cells` as the columns; `reach`, the unit cells the
  ant reached, `[i0, j0, i1, j1]` relative to its start. A highway or periodic run adds `period`,
  `d` (`[dx, dy]`), `onset` and (a highway) `speed`, as the columns, and `proof`, the exact test
  that succeeded: the step `at` which it started, the `period`, the displacement `d`, and the
  `cells` it compared. A run that saw mirror-symmetric fields with the ant on its start cell adds
  `symmetry`: how many (`count`), the `first_step` and `last_step`, the last one's coloured cells
  (`last_cells`), its `reflection` `[a, b, c, d]` (a matrix in lattice coordinates) and its
  `translation` `[x, y]`.
- Members that do not apply are absent. Integers are integers and `speed` a number. A reader
  ignores members it does not know, as everywhere.

*An importer that cannot read one ant's `run`, `start` or `outcome` leaves that ant out, with a
note naming it and the member, and imports the rest of the pack (an id that does not hash still
refuses the pack, as a pattern's does).*

### `quickstart` (facts, sorted by `id`; partitioned by `tiling_group`)

| Column | Meaning |
| --- | --- |
| `id: string` | `qs-` and the hash of its scene and pin ([ids](#ids)) |
| `title: string`, `description: string` | |
| `source: string?` | `pattern:<id>` or `ant:<id>` it was made from, or null |
| `scene: string` | the scene as self-contained JSON ([Scene file format](scene-format.md)) |
| `scene_format: string` | the scene format version, in decimal (`1`) |
| `rule: string`, `rule_family: string`, `semantics: string` | its first automaton's rule, family and version |
| `behaviour: string?` | `still life`, `spaceship`... (free text) |
| `pin_key: string`, `pin: string` | its tiling's pin key and pin |
| `tiling_group: string` | partition column |

A scene in a pack MUST NOT depend on anything outside the pack and the JETCARP version named in
`engine`: it names palettes only from the frozen registry below (or writes colours out, or holds
the palette itself in its `palettes`). Scenes MAY hold Rhai scripts; a reader MUST NOT run them
unless the user enables them for that pack.

The **frozen palette registry** ([Palettes](palettes.md)): a scene may name these, and each name
means its palette for good; a later JETCARP may add names to the list, never change or reuse one:

<!-- palette-registry -->
`Ember`, `Tide`, `Prism`, `Mono`, `Ink`, `Aurora`, `Synthwave`, `Moss`, `Candy`, `Lava`, `Glacier`,
`Phosphor`, `Koi`, `Desert`, `Thermal`, `Deep Sea`, `Paper`, `Blueprint`, `Neon`, `Autumn`,
`Verdigris`, `Marigold`, `Coral`, `Orchid`, `Citrus`, `Riso`, `Bone`, `Ultramarine`,
`Stained Glass`, `Opal`.
<!-- /palette-registry -->

### Media

`media` (events, sorted by `target`, `kind`, `from_seed`): pictures and animations, one file each.

| Column | Meaning |
| --- | --- |
| `target: string`, `kind: string` | the record, and `gif`, `webp`, `poster` or `thumb` |
| `from_seed: bool` | shown from the seed, or alone |
| `object: string` | the file's hash: an animation is one animated WebP or GIF (frame lists are never packed) |
| `media_type: string` | `image/png` (extension `png`), `image/gif` (`gif`) or `image/webp` (`webp`); a reader refuses any other |
| `width: int32`, `height: int32`, `frames: int32`, `bytes: int64` | |
| `render_key: string`, `render: string` | what it was rendered with (a key, and the settings as JSON); another key means out of date |

Media are optional in every pack and can always be rendered again from the records.

### `aliases` (facts, sorted by `old_id`)

`old_id: string`, `target: string`, `reason: string` (`geometry-change` or `scheme-change`),
`date: string`: an old id of a record. There are no aliases for CART's ids.

### Ratings

`ratings` (events, sorted by `rater`, `target`, `date`): human judgements, such as the taste
ratings that train an interest score. A pack holding only ratings and score models is a **taste
pack**.

| Column | Meaning |
| --- | --- |
| `rater: string` | a pseudonymous rater id: `rater-` and 16 random hex digits, made once per rater and kept for good |
| `target: string` | `search_run:<search uuid>:<run id>`, `pattern:<id>`, `ant:<id>`, `quickstart:<id>` |
| `scale: string` | the rubric: `keep-maybe-drop 1` or `stars-1-5 1` |
| `verdict: string` | a value on that scale: `keep`, `maybe`, `drop`; or `1` to `5` |
| `note: string?` | the rater's note, only if the rater agreed to share notes |
| `round: string?` | the rating session or round (`review`, `ratings:ratings1`) |
| `date: string` | |

`raters` (sorted by `rater`): one row for every rater the ratings name, and for every member of a
group.

| Column | Meaning |
| --- | --- |
| `rater: string` | the id |
| `consent_date: string` | when the rater consented, a [time](#encodings) like every other (a group: its latest member's) |
| `consent_scope: string` | always `public-permanent` in 1.0 |
| `display_name: string?`, `orcid: string?` | only if the rater chose to be named; never on a group |
| `members: list<string>?` | a group's member ids (each with its own row); groups do not nest |
| `notes: bool` | the rater agreed to share notes |

A group's ratings are one verdict agreed by several people; a reader shows its members. A group's
ratings go in a pack only when every member consented. JETCARP never writes another person's
ratings without their recorded consent; by default it publishes only the publisher's own.

### `score_models` (events, sorted by `model`, `version`)

`model: string`, `version: string`, `kind: string` (`logistic 1`), `features: list<string>`,
`weights: list<float64>`, `intercept: float64`, `fitted_on: string` (the training set, for
people), `rater: string?`, `metrics: string` (JSON: AUC and so on). A score model shares taste
without the ratings it was fitted on. *JETCARP 0.4 reads this table and keeps nothing of it.*

## Search runs

A pack MAY include explore searches' runs in their own format ([Search runs](search-runs.md):
segments of schema `jetcarp-explore-runs 1`, score sidecars `jetcarp-explore-scores 1`). `searches`
in the manifest maps a search's uuid to its **search index**:

```json
{"children": [], "created": "...", "engine_version": "0.4.0", "format": "jetcarp-index",
 "kind": "explore", "parts": [{"bytes": 77187976, "engine_version": "0.4.0", "first": "1",
 "last": "1000000", "object": "sha256:...", "rows": 1000000, "sealed": "...",
 "segment": "part-000001"}], "remeasure": {...}, "run_schema": "jetcarp-explore-runs 1",
 "schema": "jetcarp-search 1", "score_schema": "jetcarp-explore-scores 1",
 "scores": [{"created": "...", "engine_version": "...", "parts": [{"bytes": 1, "object": "sha256:...",
 "rows": 3, "segment": "part-000001"}], "version": 1}], "search": "<uuid>", "spec": {...},
 "table": "search", "updated": "...", "version": "1.0"}
```

- `table` is `search` and `schema` `jetcarp-search 1`; `search` MUST be the uuid the manifest maps.
- `parts`: the sealed segments in id order, each a Parquet object, with its `segment` name
  (`part-` and digits), `rows`, `bytes`, and `first` and `last` run ids in decimal.
- `scores`: each rescore, with the sidecar of each segment it changed. A run's score is the one in
  its segment's newest sidecar, or the segment's own.
- `spec` and `remeasure`: the search's settings and import record, copied from its folder's manifest
  with every local path stripped and every non-integer number written as a string (as ECMAScript
  writes it). A reader that needs the numbers reads such a string back as a number: JETCARP does,
  for the settings it knows to be numbers and for the record's seconds.
- **What a search carries.** Its runs (every sealed segment and score sidecar), its settings and its
  record, and nothing of its working database: everything a stopped search needs is rebuilt from
  the runs (its archive of elites, offered in id order with each run's newest score; coverage and
  family totals; the identities tried; the elites confirmed, from the confirmation runs'
  `parent`). Not carried: the time it ran and its random-number states, so a resumed import starts
  its random states again from its seed (the identities tried keep it from repeating a candidate).
- `children` lists further search indexes of the same search whose `parts` and `scores` add to these
  (the same limits as table indexes).
- Every object a search index names is Parquet (`.parquet`). Runs not yet sealed are never in a pack.
- Ratings of a search's runs are `ratings` rows with targets `search_run:<uuid>:<run id>`.

## Importing

An importer of a version:

1. Checks the manifest's `format`, `version` and `requires`, and refuses what it does not know
   (another major version, an unknown required feature).
2. Checks the signatures ([keys](#signatures-and-keys)) and refuses a version with a signature that
   does not verify. JETCARP refuses one not signed by its publisher unless told to allow unsigned
   packs. Through a feed, it also checks the ref's `seq` and signature.
3. Checks every object's hash and size (every part, segment, sidecar, media file and reference
   patch) before storing anything, and refuses a mismatch.
4. Checks every id against its identity and pin, the typed phases against the identity, and every
   tiling's pin against its geometry columns or reference patch.
5. Merges by content id: a fact row for an id it has is ignored (or checked identical); event
   rows are added under their publisher; retractions become flags.
6. Flags (`needs_reverify`) every record whose pin, or rule family's semantics version, differs from
   its own engine's, keeping its id and pin until it is verified again.
7. Verifies a random sample again (JETCARP: 3,000 new patterns by default, seeded by the manifest
   hash, within a time budget), and keeps the result.
8. Never runs anything from the pack.

*What JETCARP 0.4 keeps:* a new pattern is put in canonical form, which must give the pack's id;
it keeps its finder and find date (its provenance rows from the pack: of the publisher whose row 0
is earliest, then the manifest's publisher, then by fingerprint), followed by an `imported` row
naming the manifest. Its curation is the latest annotations of the manifest's publisher, then of
others for fields still empty. Sightings are kept per search (a job made for each uuid), the larger
of the publishers' totals, so importing a pack again changes nothing; copies counted without a
search add to the record's seen count. Ratings are kept under the rater's display name when the
rater chose one (a group's: its members' names joined by ` & `), else the rater id. A pattern's
rule is written as the library writes rules: the notation with `V` for B/S and Generations rules
(and JACA's edge form) on edge neighbours (`B2/S23V`), as a search that found it here would write
it. A search's segments and sidecars are copied into the library's search folders, and it becomes
a paused explore job with the search's own settings, its working database rebuilt from its runs
([what a search carries](#search-runs)), so its status, runs, report and atlas read as a stopped
search's and it can be resumed. Retractions are kept in `pack_retractions`, the result in
`pack_imports` ([Library format](library-format.md#tables)); no receipt is written.

## Exports

An export is one version laid out for people, for a single download (a folder, or an uncompressed
tar of it, conventionally named `<feed>-<version_label>`):

```text
pack.json                                         the manifest's exact bytes
index/<hex>.json                                  every index object of the version
history/<hex>.json                                earlier manifests (parents...), when the store had them
tables/<table>/<key>=<value>/.../part-NNNNNN.parquet
tilings/reference/<family>.parquet                aperiodic reference patches
searches/<uuid>/runs/<segment>.parquet            search segments
searches/<uuid>/scores-<n>/<segment>.parquet      score sidecars
media/<kind>/<id>/<media kind>[-seed].<ext>       media files (`-seed`: shown from the seed)
sigs/<signer>.sig                                 the manifest's signatures
keys/<signer>.json                                the signers' key statements
views.sql                                         DuckDB views over the files (not part of the format)
LICENSE, README.md, CITATION.cff                  made from the manifest
```

- A table's parts are numbered from 1 within each partition folder, in the order its index lists
  them (children in order); partition folders are `<key>=<value>` in the partition's key order
  (`class_group=spaceship/tiling_group=k3`). The partition columns are also in the files, so a tool
  that adds folder names as columns (DuckDB's `hive_partitioning`, pyarrow's `partitioning="hive"`)
  sees each twice with the same value; read the files without it, or drop one.
- The indexes give every file's hash, and media rows and `tilings` rows the rest, so an export is
  checked exactly like a store. Importers accept a store or an export.
- *JETCARP makes an export's data files as hard links to the store's objects when both are on one
  file system, and copies them otherwise (another file system, or one without hard links). A
  hard-linked export shares its files with the store: do not edit them in place.*

## Limits

A reader SHOULD apply these limits, and MUST NOT crash or hang on input beyond them:

| Item | Limit |
| --- | --- |
| object | 4 GiB |
| manifest, index, ref or key statement | 16 MiB |
| parts or children per index | 10,000 |
| nesting of indexes | 8 levels |
| JSON nesting | 32 levels |
| string cell | 1 MiB |
| cells in a record's phase | 1,000,000 |
| period | 2^32 |
| Parquet footer | 16 MiB, Thrift nesting 32, 1,024 columns |
| decompressed size of a part | the sum of its pages' uncompressed sizes (as their headers say) at most 64× the part's size, checked before decoding |
| decoded size of a part | its data pages' values at most 64 per byte of the part (plus 2^20), and its decoded columns at most 64× its size plus 64 MiB |

- File names are never taken from a pack: only as [the store](#the-store) and [exports](#exports)
  build them.
- Verification on import runs within a time budget, and one record is left unchecked when it is too
  large for its own budget (JETCARP: a period above 4,096 or above 10,000 cells).
- *JETCARP reads every page header before decoding a part, and catches a decoder that fails on
  malformed input, so a hostile part is a refusal, never a crash.*

## Personal data

- `finder`, `orcid`, `name`, `github` and raters' `display_name` appear only for people who chose to
  be named. JETCARP records a finder only when `JETCARP_FINDER` is set, and `machine` is the
  operating system and architecture only.
- Local paths MUST NOT appear anywhere in a pack. *JETCARP writes a file's name in place of every
  absolute path (`/x/y`, `~/`, `C:\`, `\\server\`, `file://`) in text it publishes, publishes no
  library jobs (which hold paths), and its tests scan every file and every table cell it writes.*
- Ratings go in a pack only with each rater's recorded consent, which covers permanent public
  release: a published pack cannot be recalled from mirrors and torrents.
- **Withdrawal.** A person may ask a publisher to stop distributing their data. The publisher
  retracts it in the next version, and hosts MAY prune old versions on request. Copies already
  downloaded cannot be recalled, and consent says so.

## Licences

Each manifest's `licence` covers the objects that version adds. A merge lists its parents in
`parents`, and their licences stay as they were. JETCARP's own feeds take `CC0-1.0` only, so merged
data never carries attribution duties; other publishers may choose any open licence for their own.

## Versions and compatibility

- **Format `major.minor`.** A reader MUST refuse an unknown major, and MUST read a higher minor,
  ignoring unknown members, tables and columns.
- **`requires`** is how a minor version adds something a reader must not ignore: the feature's name
  goes in `requires`, and a reader that does not know it refuses the pack.
- **Upgrades.** A new format version is written as a new version whose parent is the old manifest;
  old versions stay readable and citable.
- **Verification receipts** (`receipts/`, namespace `jetcarp-receipt`) are reserved: a receipt
  will be `receipts/sha256/<aa>/<manifest hex>/<receipt hex>.json`, named by the SHA-256 of its
  own bytes and naming the manifest it checked. JETCARP 0.4 writes none and keeps its sample's
  result in the library.

### Golden packs

The repository keeps example packs, `crates/jetcarp-pack/tests/golden/`, with their expected
manifest hashes, and every JETCARP's test suite reads all of them: every hash, signature and id
checked, every record imported, every pattern verified again. A new format version adds golden
packs; none is ever deleted or rewritten.

| Pack | Holds | Manifest |
| --- | --- | --- |
| `minimal` | three Life patterns, a sighting, a Quick Start entry, ratings by the publisher, a consented pseudonymous rater and a group of both | `sha256:83f9a55da2225566a271e13a4f9ef8a0bf1b6aa51e9c275be5cb79710d56d9e6` |
| `retraction` | a second version retracting a pattern, adding one and renaming another (`rev` 2) | `sha256:f13d5a72ea30960dcc56f70ba26d18d6d2bded342b93e54192069ef1adaac4c6` |
| `merge` | another publisher's version merging `minimal`: two parents | `sha256:0e6fdf99d648cefb59417eefc81c88884e9150ee4c28cce5e2a0494cb858157f` |
| `searches` | an explore search's two segments and a score sidecar, with verdicts | `sha256:69836e167a7a2207aa5a9759714b9478134d5df36226cbf866c317adda2f19a4` |
| `media` | a poster and a thumbnail | `sha256:318ed4ed00f83228bb2204aee87ee578d7dd0c5f1c07ddd337fc2b2271f406f3` |
| `aperiodic` | patterns on a Penrose kite-and-dart patch, with the reference patch | `sha256:f0add939fb55f54c0c6b3fc2ac302e4174ce1eff2a98e6ce37f4669e3b3027dd` |
| `export` | `minimal` as an export | `sha256:83f9a55da2225566a271e13a4f9ef8a0bf1b6aa51e9c275be5cb79710d56d9e6` |

Their keys are Ed25519 keys from fixed seeds and their raters are invented. Every time in them is
fixed, including the times a search stamps on what it finds (a find's date and its verification's),
so making them again gives the same bytes; a test makes and publishes the aperiodic one twice, in
different seconds, and compares every file.

### Conformance levels

| Level | A reader can | Needs |
| --- | --- | --- |
| 0 Browse | read every table and document | a Parquet reader and a JSON parser |
| 1 Integrity | check every object hash, manifest hash, content id and periodic pin | SHA-256 |
| 2 Authenticity | check signatures, key statements and ref sequence numbers | `ssh-keygen -Y verify` or an sshsig library |
| 3 Replay | re-run periodic patterns from `tilings` and `rules` and confirm class, period and displacement | a small simulator (B/S and Generations from the masks) |
| 4 Canonical | recompute identity texts from raw cells, symmetry search included | JETCARP, or a reimplementation of [Library format](library-format.md#canonical-form)'s canonical form |

JETCARP is level 4. **Replay** (level 3):

- Patterns live on an unbounded plane of dead cells; a replay MUST use a field large enough that
  nothing reaches its edge within the periods checked (`verifications.field` says what JETCARP
  used).
- A pattern runs in its rule ([patterns](#patterns-facts-sorted-by-id-partitioned-by-class_group-tiling_group)):
  the `rules` row of that notation with the pattern's neighbourhood and steps per generation.
  Each generation applies the rule `steps_per_gen` times.
- Each listed neighbour counts once. Cell states mean the same in every pack for a family: B/S, 1
  is alive; Generations, 1 is alive and 2 and up are the dying states in order
  ([Generations](../automata/generations.md)); other families as their automata pages define,
  each pinned by its semantics version.
- A rule with birth on 0 (B0) is run as two complementary rules alternating on even and odd
  generations, as [Life-like rules](../automata/life-like.md#b0-rules) describes.
- Aperiodic patterns are not level 3 in 1.0: a site lists its tiles' corners and the pattern
  graph's neighbours among pattern tiles only. Replaying them is JETCARP's (level 4).
- Search runs replay only in JETCARP: re-creating a run's soup needs its random-number generator
  and soup builder, not specified in 1.0. The runs stay fully queryable without it.

## Examples (not normative)

Check a store's tables and ids with pyarrow (level 1, without signatures):

```python
import hashlib, json, pathlib, pyarrow.parquet as pq

root = pathlib.Path("store")
def obj(h, ext):
    p = root / "objects" / "sha256" / h[7:9] / f"{h[7:]}.{ext}"
    b = p.read_bytes()
    assert "sha256:" + hashlib.sha256(b).hexdigest() == h, p
    return p, b
ref = json.loads((root / "refs" / "golden-a.json").read_text())
manifest = json.loads(obj(ref["manifest"], "json")[1])
index = json.loads(obj(manifest["tables"]["patterns"], "json")[1])
for part in index["parts"]:
    path, _ = obj(part["object"], "parquet")
    for row in pq.read_table(path, columns=["id", "identity", "pin"]).to_pylist():
        h = hashlib.sha256((row["identity"] + "\n" + row["pin"]).encode()).hexdigest()[:32]
        assert h == row["id"], row["id"]
```

Spaceships in an export with DuckDB:

```sql
SELECT id, period, dx, dy, speed, tiling_group
FROM read_parquet('jetcarp-starter-2026.11/tables/patterns/class_group=spaceship/**/*.parquet')
ORDER BY period;
```

## The command line

| Command | Does |
| --- | --- |
| `jetcarp pack key [--name N] [--orcid O] [--github G]` | makes or shows the publisher's Ed25519 key (`<settings>/pack/publisher_ed25519`; never `~/.ssh` unless `--key FILE` points there) and records who the publisher is |
| `jetcarp pack rater NAME [--show-name] [--notes] [--withdraw]` | records a rater's consent, with a pseudonymous id made once |
| `jetcarp pack publish --feed NAME --to DIR [--id ID]... [--query JSON] [--no-searches] [--retract pattern:ID=reason] [--merge STORE] [--views]` | publishes the library, or a selection, as a new signed version of a feed |
| `jetcarp pack export --to DIR [--from STORE]` | lays one version out for a single download |
| `jetcarp pack check DIR` | checks a store or export without importing it |
| `jetcarp pack import DIR [--verify sample\|full\|none] [--allow-unsigned] [--strict]` | imports a store or export |

`--created TIME` fixes a version's time, so the same library publishes to byte-identical files.
The operations are `pack.key`, `pack.rater`, `pack.publish`, `pack.export`, `pack.check` and
`pack.import`.

## Questions settled while building it

Step 10 settled these, from building the reader and writer and from an independent Python reader
written from the design alone. Each is part of the rules above.

| Question | Settled |
| --- | --- |
| The pin's byte layout | given in full, with the square grid's hashes and twelve tilings' rows as test vectors |
| The tiling key's edges | each written as the engine reads it (`mode`, `curved` default true, `nodes` default empty, nodes' `t` and `d` only); [Library format](library-format.md#the-identity-text-of-a-pattern) says so and its function does it; `params` is written whenever the spec has one, even `{}` |
| Colons in file names | `<signer>` is the hex of the key's SHA-256 digest |
| Geometry conventions | y down; `i` right, `j` down on the square grid; `shape` a prototile index; corners clockwise unless mirrored; neighbours clockwise from corner 0 |
| Ant ids and targets | `ant-<hex>` in `ants.id`; targets `ant:ant-<hex>` |
| Latest-wins annotations | by `rev`, the adding version's ref `seq`; then date, then value bytes |
| Who signs key statements; revocation and time | self, predecessor, key-or-successor; no rule depends on time; revocation permanent |
| Machine keys | `authorised` in the key statement; they sign manifests and refs; never removed, revoked in place |
| Publisher identity across a key change | the first key of its chain; manifests and refs name the current key |
| Quick Start ids | `qs-` and the hash of the scene's text and pin |
| `first`/`last` of several sort columns | the first sort column's value, as text |
| Sort order | within each part; text by UTF-8 bytes, integers by value |
| A rule's spelling on import | the pack's `rules` table writes the notation and the neighbourhood apart; the importer writes the library's spelling (`B2/S23V` on edge neighbours) |
| A search's working database | not in a pack: rebuilt on import from the runs; the time it ran and its random states are not carried |
| Decimals in a search's settings and record | strings in the pack, numbers again in JETCARP |
| Exports made of hard links | links where the file system allows, else copies |
| Partition keys and columns | keys named as their columns (`class_group`, `tiling_group`); `class_group` is the class; ants and Quick Start by `tiling_group` only; Hive folders name the same columns |
| `family` and `semantics` | on `patterns`, `rules`, `ants`, `quickstart` and every verification |
| `created_by` and key-value metadata | `created_by` free (JETCARP's `jetcarp-pack 1`); writers SHOULD write no key-value metadata, readers ignore it |
| The same `seq` again | the same manifest is no update; another manifest, or a lower `seq`, a refusal |
| Retractions in snapshots and merges | cumulative, each with its `publisher` |
| Event rows twice in a merge | parts are a union by hash; a writer adds only rows its parents lack; identical rows in two parts count twice |
| What "decompressed" means | page headers' uncompressed sizes, before decoding; plus limits on decoded values and bytes |
| Index children | at most 10,000, 8 levels; an index read once, a part counted once |
| Exports' indexes | in `index/`, with earlier manifests in `history/` and key statements in `keys/` |
| The search index | defined above: `table` `search`, its parts and scores Parquet |
| Finding keys and signatures over HTTP | the publisher's key statement and signature have names the manifest gives; listing endorsements and receipts waits for 1.1 |
| Media files' extensions | `image/png` `png`, `image/gif` `gif`, `image/webp` `webp`; nothing else |
| Generations populations | every listed cell counts, ageing cells too |
| Sightings as counts | increments per version, totals by sum |
| Seen counts without a search | a `sightings` row with source `unattributed` |
| The main rule | `verifications.main`; one per pattern and publisher |
| A site's anchor | `anchor`, `anchor_shape`, `anchor_point` in `site`; `patch` and `anchor` may differ between packs |
| A find's run | `provenance.run` on row 0; `machine` and `pack` columns |
| The publisher's own ratings, groups, consent | the rater book (`raters.json`); a group needs every member's consent; rater ids `rater-` and 16 hex |
| Integers ±2^53, numbers with fractions | the bound included; any fraction or exponent refused |
| Canonical refs and keys | MUST be canonical; JETCARP refuses one that is not |
| A signature's final newline | written as `ssh-keygen` writes it; accepted without |
| Unknown tables | ignored; `engine.schemes.tables` informational |
| Text in NFC | SHOULD; never normalised by readers |
| Local paths | written as file names; JETCARP's rule and test above |
| JSON in cells | RFC 8785 text, non-integer numbers allowed; shapes of `gun` and `range` given |
| `requires` names | none in 1.0; later ones listed here |
| Empty key-statement members | omitted |
| Reachability and pruning | parents are reachable; a reader never needs one; `retain` is for hosts |
| Cell-state meanings | the automata pages, pinned by semantics versions |
| Level 3: which rule, steps, B0, repeated offsets, aperiodic | as [above](#conformance-levels) |
| Ed25519 only | packs are signed with Ed25519 keys; another kind may count as no signature |
