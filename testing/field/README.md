# JETCARP's field test

Scripts that anyone can run to test a JETCARP release on their own computer, with no AI session
and nothing to install, and that write one report file to send back by hand. The book's page *Help
test JETCARP* (`docs/src/about/field-testing.md`) is the one for testers; this is the one for
changing them.

| File | What it is |
| --- | --- |
| `jetcarp-field-test.sh` | Linux and macOS: POSIX sh, with awk, tar, ps and curl or wget |
| `jetcarp-field-test.ps1` | Windows: Windows PowerShell 5.1 and PowerShell 7 |
| `tests.json` | The tiers and the tests: what each runs, how its result is read, its time, the releases it applies to |
| `parity/` | The parity scenes and the hashes they must reach, per range of releases |
| `speed/` | The two scenes the stepping-speed test runs |
| `make-parity-data.py` | Makes `parity/` from a release cycle's parity set |

Run from this folder (a clone of the repository), the scripts read the data beside them; run on
their own (downloaded alone, as the book says), they download it from `main`. `--data DIR|URL`
(`-Data` in PowerShell) points them elsewhere. `FIELD_TEST_KEEP_WORK=1` keeps the temporary
folder at the end, for looking into a run.

## What the scripts promise, and how

- **Nothing on screen.** Every test is a subcommand of `jetcarp` (never the app, never the
  background host), and every command in `tests.json` is checked to need no seizure warning and
  open no window: `host tune` is given `--canvas` and finds no host (the runtime folder is the
  test's own), so it measures off screen. On Linux the scripts also clear `DISPLAY` and
  `WAYLAND_DISPLAY` for every run; on Windows each run starts with no window and the script
  checks that none opens one (`platform.windows_seen` in the report).
- **Nothing of the tester's touched.** The release is unpacked into a temporary folder, and
  `JETCARP_CONFIG_DIR`, `JETCARP_DATA_DIR`, `JETCARP_OUT`, `JETCARP_RUNTIME_DIR` and
  `JETCARP_LIBRARY` point inside it; any `JETCARP_*` and `WGPU_*` variables already set are
  cleared for the run and put back at the end. `jetcarp score` runs with `--no-save`.
- **Nothing sent.** The scripts only download: GitHub's list of releases (one unauthenticated
  API call a page), the release's archive and `SHA256SUMS`, and this data. The archive is
  checked against `SHA256SUMS` as the installers do.
- **A report with nothing that identifies the computer.** The temporary folder, the home folder,
  the user name, the computer's name and its network addresses are replaced (`[scratch]`, `~`,
  `[user]`, `[machine]`, `[ip]`) before the file is written.

## Changing the tests

Each entry of `tests` in `tests.json` has:

- `id`, `name`, `about` (shown as it runs), and `tiers` (`quick`, `suite`, `extended`);
- `kind`, how the scripts read its result: `doctor`, `adapters`, `busy`, `score`, `parity`,
  `check`, `fit`, `speed` or `explore`; a new kind needs code in both scripts, while a new test
  of a known kind needs only an entry here;
- `args`, the command line after `jetcarp`, with `{work}` (a folder of the run's own),
  `{scene:N}` (parity scene N), `{minutes}` and `{max_rss_gb}` filled in;
- `from` (and `to`), the releases with what it needs; a release outside them skips it and says why;
- `seconds`, its time on a typical laptop, and `per_adapter`, the time each GPU adds.

The top-level `script` is the oldest script version that knows everything in the file (each
script's `FT_VERSION` or `$FtVersion`). A test of a new kind, or a new key a kind reads, raises
it in the same change that raises both scripts' versions; an older copy then stops and asks the
tester to download the script again, rather than misreading a test it doesn't know.

The kinds' own keys:

- `parity`: `set` (`short`, the 371 scenes at `gens` 300, or `long`, the 40 at 5,000), `pick`
  (`all`, `every K`, `A-B`, `N`, separated by commas), `cpu` (run on the processor too), and
  `adapters`: `chosen` (the GPU JETCARP picks), `devices` (each GPU, through the API JETCARP picks
  for it) or `others` (every other adapter and API the doctor lists that JETCARP will run on).
- `check`: a command that prints JSON (`--json`). `devices` (`cpu`, `gpu`) runs it once with
  `--cpu` and once with `--gpu` on each adapter of `adapters`; `same` lists the paths whose values
  must be the same on every run, `expect` the known values per range of releases, `say` the path
  shown for each run, and `progress` the word of its "N/M word" progress lines.
- `speed`: `scenes`, each a `name`, a `scene` file and `gens`.
- `explore`: `minutes`, and `max_rss_share`, the share of the computer's memory past which the
  search stops itself.
- `score`: `report`, the JSON file `--report` writes, and `attach`, to run it on the terminal so
  that its own question (wait for busy programs, or measure anyway) is asked.

Known results come from a run on each release they name: a hunt with fixed soups gives the same
counts on every machine, the processor and every GPU, so a release that changes them (0.4.3 did,
for Life's exploded soups) gets an entry of its own.

## The parity data, and updating it

`parity/scenes.tsv` holds every built-in Quick Start scene of JETCARP 0.4.2, one a line (`n`, its
Quick Start id, the scene as compact JSON), and `expected-<from>.tsv` and `long-<from>.tsv` the
hash each must reach after 300 and 5,000 generations. They come from the release-candidate test
records' parity set, `v0.4.3/parity/` (`scenes/`, `expected.csv`, `long/expected.csv`), published
with the release's records in jetcarp-testing. `tests.json`'s `parity.sets` says which releases
each pair of files is for:

| Files | Releases | Hash |
| --- | --- | --- |
| `expected-0.4.3-rc.1.tsv`, `long-0.4.3-rc.1.tsv` | 0.4.3-rc.1 to 0.4.3-rc.2 | state hash version 2, 64 hexadecimal digits |
| `expected-0.4.2-rc.1.tsv`, `long-0.4.2-rc.1.tsv` | 0.4.2-rc.1 to 0.4.2 | state hash version 1, 16 hexadecimal digits |

The 0.4.3 set is 0.4.2's scenes with their hashes in version 2 (#173), and the B0 and Larger than
Life rows as #238 changed them; the 0.4.2 set is the `hash_v1` column of the same files, which
0.4.2's own set equals row for row. Each is the set the cycle's candidates were held to on the
three test machines, and the scripts here reproduce it on the Framework (Radeon 8060S, llvmpipe):
0.4.3-rc.2, 0.4.3-rc.1 and 0.4.2 each match all 371 scenes and the 40 long ones, on the processor
and the Radeon, against their own set.

Each set ends (`to`) at the newest release it has been checked with. A later release isn't
checked yet: its testers see "expected hashes aren't checked yet", and the scenes run anyway with
the processor and each GPU compared, so a rule a new candidate changed on purpose never shows as
a failure on someone's computer. When a candidate is tagged, run the Suite against it on the
Framework; if every scene matches, move the set's `to` on to it.

When a release changes what a scene replays to:

1. update the cycle's parity set (as each candidate's plan does), then
2. `python3 testing/field/make-parity-data.py PATH/TO/v0.X.Y/parity --from 0.X.Y-rc.N` writes
   `expected-0.X.Y-rc.N.tsv` and `long-0.X.Y-rc.N.tsv` (`--column hash_v1` for a version 1 column;
   `--scenes` rewrites `scenes.tsv`, for a new set of scenes);
3. add the set to `parity.sets` with its `from`, and give the set before it a `to`.

The scenes stay 0.4.2's, so that every release is held to the same ones.
