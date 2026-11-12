# Packs: importing, exporting, checking

A *pack* is how JETCARP shares data: patterns, ant and turmite runs, Quick Start entries, search
runs, pictures and ratings, in files that any program can read. Every file is named or listed by
its SHA-256 hash, and every version is signed by its publisher, so whoever receives a pack can check
that nothing was changed on the way and who made it. JETCARP also checks every pattern's id against
the pattern itself, and runs a sample of the patterns again before trusting them.

This guide covers checking and importing a pack you were given, then publishing and exporting your
own. The files themselves are described in [Pack format](../reference/pack-format.md).

## Stores and exports

A pack comes in one of two layouts, and every command here takes either:

- A **store** is a folder that keeps every version of one or more *feeds*. A feed is a named series
  of versions, such as `penrose-finds`. Files in a store are named by their hashes
  (`objects/sha256/…`), so a new version adds only the files that changed, and the store can be
  copied to any static web host as it is.
- An **export** is one version laid out with readable names for a single download: a `README.md`, a
  `CITATION.cff`, a `LICENSE`, the manifest `pack.json`, and the tables as Parquet files under
  `tables/`.

To send someone a pack, send the export folder, for example as a zip. JETCARP 0.4 imports from a
folder, so unpack a zip or tar first.

## Check a pack

`jetcarp pack check` reads a pack and checks all of it, without importing anything. It checks the
format, the publisher's signature, the hash and size of every file, and every id:

```text
$ jetcarp pack check penrose-finds-2026.11
sha256:a968036b…: feed penrose-finds, by SHA256:yNFIDrcM3AiT87AjIHMwLfCRBHMOW4KZm5X6pwQx1A8 (Ada Example), checking every object
…
checked sha256:a968036b… (feed penrose-finds, SHA256:yNFIDrcM… Ada Example, signed by its publisher)
  9 objects checked (213869 bytes)
```

The first part is the *version id*: the hash of the manifest, which in turn lists the hash of every
file. Two packs with the same version id are the same pack. The `SHA256:…` value is the publisher's
key fingerprint, the same form `ssh-keygen -l` prints. A name after it is one the publisher gave
themselves, and nothing more: trust a publisher by their key, for instance one they list on their
website.

If any file was changed, the check fails and says which:

```text
Error: part sha256:d7762c49… is altered or truncated
```

A pack that its publisher didn't sign is refused unless you add `--allow-unsigned`. A store with
several feeds needs `--feed NAME`, and `--manifest sha256:…` picks a version other than the newest.

## Import a pack

`jetcarp pack import` runs the same checks and then merges the pack into your library:

```text
$ jetcarp pack import penrose-finds-2026.11
…
imported sha256:a968036b… (feed penrose-finds, SHA256:yNFIDrcM… Ada Example, signed by its publisher)
  9 objects checked (213869 bytes)
  patterns: 3 added, 0 merged, 0 flagged needs_reverify
  sightings 3, ratings 0, ants 0, Quick Start 0, media 0, searches 0, retracted 0
  verified again (sample): 3 of 3 passed, 0 skipped, 0 failed
```

What happens to each kind of record:

- **Patterns** are merged by id. A pattern you don't have is added with its finder, the date it was
  found, and the publisher's name, tags and notes for it. A pattern you already have keeps your own
  name, tags, notes, rating and state: importing never overwrites your curation, so a later version
  that renames a pattern changes nothing for one you already had. Sightings are counted per search,
  so importing the same pack twice changes nothing.
- **Patterns from another engine version.** A pattern whose tiling geometry or rule meaning differs
  from this version of JETCARP's is kept and flagged `needs_reverify`. `jetcarp library reverify`
  checks it again.
- **Sample verification.** After merging, JETCARP runs 3,000 of the new patterns again (fewer if the
  pack has fewer), chosen at random but seeded by the version id, so the same pack always checks the
  same sample. `--verify full` runs every new pattern, `--verify none` skips this step,
  `--sample N` changes the sample size, and `--verify-seconds S` caps the time (600 by default).
- **Search runs** become a paused explore job, so `jetcarp jobs status`, `jobs runs` and the atlas
  work on them as on a search you ran yourself, and `jetcarp jobs resume` continues it.
- **Quick Start entries** appear in the Quick Start menu.
- **Ratings** are kept under each rater's name, as the pack gives it.
- **Retractions.** A publisher can withdraw a record in a later version. Importing that version flags
  the record as retracted, with the reason, and never deletes your copy.

`--strict` refuses a pack that has two different rows for one record id, instead of keeping the
first. `--library FILE` imports into another library file than your usual one.

**JETCARP never runs code from a pack.** A pack holds data. A scene's `script` is a list of steps,
naming which automata run and when to move on ([Scene file format](../reference/scene-format.md#script-and-reseeding)),
not a program.

## Your publisher key

Every pack you make is signed with your *publisher key*, an Ed25519 key that JETCARP keeps in its
settings folder. `jetcarp pack key` makes one the first time and shows it after that:

```text
$ jetcarp pack key
made publisher key SHA256:yNFIDrcM3AiT87AjIHMwLfCRBHMOW4KZm5X6pwQx1A8
  ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGPv65+hMLaYJAVXB/chf5oAx0smWQlLyOVymcinItHV jetcarp publisher
  file /home/ada/.config/jetcarp/pack/publisher_ed25519
  not named (packs show the key only; --name to be named)
```

Your packs show only the key until you give a name. `--orcid` and `--github` add those too, and
they appear in each pack's `CITATION.cff`:

```text
$ jetcarp pack key --name "Ada Example" --orcid 0000-0002-1825-0097
```

The key is an ordinary OpenSSH key, so OpenSSH can check JETCARP's signatures too. In an export's
folder, with the publisher's public key in a file `allowed` as `ada ssh-ed25519 AAAA…`:

```text
$ ssh-keygen -Y verify -f allowed -I ada -n jetcarp-manifest -s sigs/*.sig < pack.json
Good "jetcarp-manifest" signature for ada with ED25519 key SHA256:yNFIDrcM3AiT87AjIHMwLfCRBHMOW4KZm5X6pwQx1A8
```

**Back up the key file.** A new key is a new publisher: later versions of your feeds would be
signed by a stranger. `--key FILE` signs with an OpenSSH Ed25519 key of your own instead.

## Publish your library

`jetcarp pack publish` writes your library, or part of it, as a new version of a feed in a store.
The store folder is made if it doesn't exist:

```text
$ jetcarp pack publish --to my-store --feed penrose-finds --title "Penrose finds" --version-label 2026.11
publishing feed penrose-finds (version 1) into /home/ada/packs/my-store
3 patterns read, 3 new rows
…
published penrose-finds version 1 into /home/ada/packs/my-store
  manifest sha256:a968036b954f740867edcd9387c4d87c2ef39424903975cbea85f570154db964
  publisher SHA256:yNFIDrcM3AiT87AjIHMwLfCRBHMOW4KZm5X6pwQx1A8
  patterns       3 rows added, 2 parts
  …
  49117 bytes in all
```

Run it again later and it writes version 2 of the same feed, storing only what changed. The
feed's file in `refs/` names its newest version and is signed too.

**Choose what goes in:**

| Option | Effect |
| --- | --- |
| `--id ID` | Only these records: ids, id prefixes or names (repeat it) |
| `--query JSON` | Only the patterns a library query finds, such as `'{"class":"spaceship"}'` |
| `--search UUID` | Only these explore searches (repeat it) |
| `--no-searches`, `--no-ants`, `--no-quickstart`, `--no-media`, `--no-ratings` | Leave that kind out |

**Describe it:** `--title`, `--description` and `--version-label` (free text such as `2026.11`)
go into the manifest and the export's README. `--licence` takes an SPDX id; the default is
[CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/), which puts the data in the public
domain. `--created` fixes the version's time (RFC 3339, UTC): with the same library, key and time,
publishing gives the same version id, byte for byte.

**Withdraw a record:** `--retract pattern:ID=reason` lists it as retracted in this version and every
later one. Earlier versions stay as they were, since every file in them is fixed by its hash, but
anyone importing the new version sees the retraction and its reason:

```text
$ jetcarp pack publish --to my-store --feed penrose-finds --version-label 2026.12 \
    --retract 'pattern:3dec1a434a261b937448693adbeab960=duplicate of a known still life'
```

**Combine versions:** `--merge FOLDER` (or `FOLDER@sha256:…` for one version) brings another
store's records into this version. Each keeps its original publisher.

## Export a version

`jetcarp pack export` lays one version out for a single download:

```text
$ jetcarp pack export --from my-store --to penrose-finds-2026.11
exported sha256:a968036b… into /home/ada/packs/penrose-finds-2026.11: 24 files, 222559 bytes
```

Without `--from`, it publishes your library first, taking the same options as `pack publish`, and
exports the result. It keeps the store it made next to the export, in a hidden folder named after
it, for example `.penrose-finds-2026.11.store`. Use `--views` to add a `views.sql` for
[DuckDB](https://duckdb.org/) (see [below](#read-a-pack-without-jetcarp)).

The export's folder:

| Path | What it holds |
| --- | --- |
| `README.md`, `CITATION.cff`, `LICENSE` | For people and for citation managers, written from the manifest |
| `pack.json` | The manifest; its SHA-256 is the version id |
| `tables/<table>/…/part-NNNNNN.parquet` | The tables, split into parts by class and tiling group |
| `index/` | The hash of every part |
| `sigs/`, `keys/` | The publisher's signature and public key |
| `tilings/reference/` | Reference patches for aperiodic tilings, so their pins can be checked |

## Ratings and consent

Ratings are about people's taste, so a pack includes only the ratings of raters who have agreed to
a permanent public release. Record that agreement with `jetcarp pack rater`, using the name the
library's ratings give:

```text
$ jetcarp pack rater "Ada Example" --show-name
Ada Example                              rater-17935ea8ceb8a90a  consented 2026-11-12T02:14:37Z, named
```

- Without `--show-name`, a pack lists the rater only by a pseudonymous id such as
  `rater-17935ea8ceb8a90a`.
- `--notes` lets their notes go in too.
- `--orcid` adds an ORCID for a named rater.
- `--withdraw` takes the consent back: later packs leave that rater's ratings out. Versions already
  published can't be changed.
- A group rating, made by several people together, goes in only when every member has consented.

Your own curation (the names, tags, notes, star ratings and states you give patterns) is
different: it goes into your packs as your *annotations*, signed with your key.

## Read a pack without JETCARP

An export is plain Parquet and JSON, so any data tool can read it. With DuckDB, in the export's
folder:

```text
$ duckdb -init views.sql
D SELECT id, class, period, cells FROM patterns ORDER BY cells;
```

[Polars](https://pola.rs/) and pandas read the same files, with `read_parquet` on a table's folder.
JETCARP's source repository also holds an independent reader in Python (`tools/jetcarp_pack`),
written from the format description alone. It checks hashes, signatures and ids, at the
[conformance levels](../reference/pack-format.md#conformance-levels) the format defines.

## Share a pack

A store or an export is a folder of ordinary files, so any way of moving files works: a zip by
email, a static web host, a Git repository with Git LFS, or a data repository such as Zenodo or
Hugging Face. To publish a feed for others to follow, copy the whole store to a web host and give
people the address. Copy it again after each new version: files already there never change, so
only the new ones need uploading.

JETCARP 0.4 doesn't download packs itself: download the folder first, then import it. Fetching
feeds from an address and following them for new versions are planned for a later version.
