# How JETCARP is tested

This page sets out how JETCARP is tested before a release: the automated tests, the
continuous-integration runs, the release candidates tested by hand on real hardware, how each
problem found is handled, and who does what. It ends with a record of one release cycle and with
what this testing does not cover. [How JETCARP was made](how-made.md) says how the program was
written, and [Reproducibility](../research/reproducibility.md) describes the tests that pin its
behaviour in detail.

## Automated tests

JETCARP has over a thousand automated tests, beside the code (`#[cfg(test)]`) and in each crate's
`tests/` folder. `cargo test --workspace --release` runs them (see
[Contributing](contributing.md#building-and-testing)). They cover:

- **Pinned behaviour.** The cell polygons and neighbour lists of every tiling, the state of every
  built-in Quick Start entry after 48 generations, and one fixed scene for each rule family. A change
  that alters what an old scene replays to fails these tests
  ([Reproducibility](../research/reproducibility.md#pinned-geometry-and-dynamics)).
- **The CPU and the GPU agree.** Wherever a world can run on the GPU, the tests require the same
  state hash, generation, status and command log as the CPU, over every family the GPU runs, every
  kind of edge, and runs from 1 to 180 generations; hunts with and without the GPU must leave the same
  catalogue. A GPU that cannot run something must fall back to the CPU, never run it differently.
- **Searches survive interruption.** Every kind of job but the exploratory search, stopped and
  resumed (on the same runner or another), must end with exactly the results of one that ran through;
  the exploratory search continues from its saved archive instead
  ([what is not guaranteed yet](../research/reproducibility.md#what-is-not-guaranteed-yet)).
- **Exact verification.** Every object a hunt keeps is run again alone and compared cell by cell.
- **The front ends.** The command line's JSON output, the operations the front ends share, the MCP
  server's protocol, its folder rules (paths outside its folders, symbolic links, read-only folders;
  on Windows directory junctions and symbolic links too) and its limits, that nothing listens for AI
  control while it is switched off, the scripting API, packs (signed, unsigned, tampered and truncated), and the app,
  which a test harness drives offscreen, clicking widgets by their text.
- **The book.** The command-line and MCP reference pages are generated from the program and checked
  against it, and the copy of the book built into the program must match the book.

About 40 more tests are ignored by default and run only when asked (`-- --ignored`): most draw the
book's pictures of the app, and the rest time the program, run very large worlds, or need a desktop
session.

Tests never write to your own settings, data or pictures: they run in scratch folders, and CI fails
if a test touches the default ones.

## Continuous integration

GitHub Actions runs on every push to `main` and every pull request. A release is prepared on a
branch with a pull request open against `main`, so every push to it is tested; a tag runs the
release workflow. The workflows name each action they use by its commit, with its version beside it,
so a tag that moves cannot change what runs. Every job has a time limit, about twice its usual length or more, so a
job that hangs is stopped rather than left running for hours.

- **Build and test** the whole workspace on Linux (x86_64), Windows (x86_64) and macOS (Apple
  silicon). The runners have no GPU of their own, so on Linux the GPU tests run on Mesa's software
  renderer (lavapipe), on macOS on the runner's virtual GPU, and on Windows they are skipped (JETCARP
  never uses Windows' software renderer, WARP). On Linux the tests run once more with the default
  folders, failing if any test writes to them. On Windows each of the folder rules' link cases must
  make its junction or symbolic link and run; the log names each one.
- **The installers** are each given a release folder of the test's own: an intact archive installs;
  an archive with one byte changed, and one that `SHA256SUMS` does not list, are refused with
  nothing installed (the shell installer on Linux, the PowerShell one on Windows).
- **Build only** for macOS on Intel and Linux on ARM, which nobody tests by hand yet, and the whole
  workspace with the oldest Rust it says it supports (1.95), so that the stated minimum stays true.
- **Checks:** formatting (`cargo fmt --check`), the third-party notices are current, the workflows
  and packaging scripts pass `actionlint`, every crate packages and builds on its own (a crates.io
  dry run), and the generated reference pages match the program.
- **The dependencies,** with [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) (`deny.toml`),
  on every push to `main`, every pull request and once a week, since an advisory can be published
  without anything in JETCARP changing. The check fails on a known vulnerability or unsoundness in
  any dependency (the [RustSec](https://rustsec.org/) database), on any HTTP, TLS, DNS, WebSocket
  or peer-to-peer client or server crate that `deny.toml` does not allow, so that no networking
  comes in unplanned while [`SECURITY.md`](https://github.com/jprodgers/jetcarp/blob/main/SECURITY.md)
  says JETCARP makes no network connections, and on any crate from somewhere other than crates.io. A notice that a crate
  is no longer maintained is shown as a warning, and never fails it.
- **The book** builds, its internal links all resolve, and its scripting reference matches the
  program.
- **A release** is built only from a tag whose version agrees with the program and the changelog,
  and only after the tests pass.

## Release candidates

Since 0.4.2, a release goes through release candidates first: `0.4.2-rc.1`, `rc.2`, and so on.
Each is a public pre-release, built by the same workflow as a release, which the package managers
(Homebrew, Scoop, PyPI, crates.io) do not pick up. Each candidate is tested on three machines, with GPUs from
four vendors (as of 0.4.2):

| | Framework Desktop (the main machine) | Acer Nitro V15 laptop | MacBook Pro |
|---|---|---|---|
| Processor | AMD Ryzen AI Max+ 395, 16 cores, 32 threads | Intel Core i5-13420H, 8 cores, 12 threads | Apple M2 Max, 12 cores |
| Memory | 128 GB unified, split by hand: 96 GB for the system and 32 GB for the GPU (changeable with a reboot) | 15.7 GB | 64 GB unified (shared with the GPU) |
| Graphics | AMD Radeon 8060S (integrated), Vulkan (Mesa 26.2.5, RADV); Mesa's llvmpipe, a software renderer | NVIDIA GeForce RTX 3050 6GB Laptop GPU (driver 546.18) and Intel UHD Graphics (driver 31.0.101.4146), each on Vulkan and DirectX 12; an Optimus laptop, the UHD driving the screen | Apple M2 Max, 38-core GPU, Metal |
| System | Fedora Linux 45, Linux 7.3, GNOME Shell 51.1 on Wayland | Windows 11 Home 26H2 (build 26300) | macOS 27.2.1 |
| Screen | AOC Q3277, 2560 × 1440 at 60 Hz (moved to a laptop for the two-monitor sitting) | 1920 × 1080, built in | the built-in Retina panel |

The test plan for a candidate is written down before it is tested: a document listing each step,
what to run, and what counts as a pass. It is carried out in three parts.

**Phase 1, unattended**, on each machine:

- **Parity on every GPU.** 371 scenes (every built-in Quick Start scene, 364, and 7 sandpiles that
  drop grains) run for 300 generations on the CPU and on each adapter and graphics API the machine
  has: seven in all, from the Radeon and llvmpipe to the RTX and the UHD on both Vulkan and
  DirectX 12, to the M2 Max on Metal. Every final hash must match the one recorded on the Framework's CPU. Then 40 scenes chosen
  across every family and kind of tiling run for 5,000 generations each, so that slow drift shows.
- **The whole test suite on real GPUs**, built from a clean clone of the candidate's tag, on each
  machine's own adapters rather than CI's software ones.
- **A long search.** A four-hour exploratory search, killed hard after an hour, resumed, then packed
  and imported into a fresh library. Its memory is logged every few minutes, the killed run must be
  reported as stopped at once, the resume must keep the search's original time budget, and the
  imported search must be the same as the original.
- **A deterministic hunt** that must give the same numbers on every machine (in 0.4.2: 1,783
  distinct objects among 760,892, on all three).
- **The hunt against Catagolue** (from 0.4.3). `docs/tools/check-catagolue.py` runs a fixed hunt
  of 150,000 soups of Life and holds the shares of its twelve commonest objects against a copy of
  the top of Catagolue's census kept in the repository. It fails when one lies more than 4 standard
  errors from Catagolue's share, or the sum of their squares passes 32.9: an outside check of how
  the hunt runs soups, cuts out ships and separates and names objects, which the project's own
  tests cannot give (see [Soup hunt](../research/search-methods.md#soup-hunt)). JETCARP 0.4.2
  fails it.
- **An overnight run** of the live desktop background, eight hours, for leaks and stalls, when a
  machine can be left idle that long.

**Phase 2, by hand**, in sittings of an hour or two at each machine:

- **Searching:** an exploratory search on the GPU, replays, the CPU confirming the GPU's finds,
  rating clips on the rating page in two browsers, fitting a score to the ratings, packs.
- **The app:** every menu item, pane and tool window, popped out and back, AI control from an MCP
  client, light and dark, the scripting examples, exports, and big worlds on each GPU.
- **The desktop background:** rotations, cell sizes, pauses for full-screen windows,
  battery and busy processors, sleep, lock and login, and the background host killed and restarted.
- **MCP and robustness:** every tool, bad input of every kind (124 malformed calls in one sitting),
  paths outside the allowed folders, read-only folders, damaged and tampered packs, two programs
  using one library, and the largest worlds the limits allow.
- **A newcomer's first hour and the upgrade from the previous release**, each in a separate user
  account, and **two monitors**.

**R, a check of every fix.** Each candidate's plan lists every problem fixed since the last one,
with a step that shows the fix working, run on the machines it applies to.

**A pre-check before the tag** (since `rc.4`): the suite and both parity sets run on every machine's
adapters from the release branch before the candidate is tagged, with the scripted checks of what
the candidate changed, so a candidate starts from a branch already known to pass.

**A narrow candidate** is tested to fit what it changes. `rc.5` carried one fix that couldn't wait
(#131: on macOS, the app's window stopped drawing after ⌘M) and seven small ones, none of them in the
background host or the simulation. So the Acer's and the Mac's overnight runs on `rc.4` counted for it
(the Framework's ran on `rc.5`). Its sittings ran on
`rc.4` or `rc.5`, and every machine ran parity, the suite and each fix's check again on `rc.5`.

**What each candidate actually got.** The lists above are what a candidate's plan draws on, not what
every candidate had on every machine. In the 0.4.2 cycle the eight-hour overnight run happened four
times (the Mac on a branch build, the Acer and the Mac on `rc.4`, the Framework on `rc.5`), none on
`rc.1` to `rc.3`. The four-hour search ran on `rc.3` on all three machines (`rc.2`'s plan asked for
two hours). No candidate had every sitting on every machine: the sittings were spread across the
candidates. In `rc.4`'s check of every fix, 12 of the 22 rows were for all three machines and 9 for
one. Each candidate's plan, with every machine's results, is in
[jetcarp-testing](https://github.com/jprodgers/jetcarp-testing).

The unattended steps, and the commands within the sittings, are run by a Claude Code session on each
machine, working from the written plan. It records every step with its evidence (commands, outputs,
timings, screenshots where they matter) and asks Jimmie, at the machine, whenever a step needs eyes
or hands: whether a picture looks right, whether a page is clumsy, whether a shortcut does what it
says. Results are committed as they come in, and a main session reads them, files the problems and
fixes them. The plans, logs and results are published in
[jetcarp-testing](https://github.com/jprodgers/jetcarp-testing).

## How a problem is handled

- **Every problem becomes a GitHub issue**, however small, saying where it was found (the
  candidate, the machine, the step) with the evidence. The issues for 0.4.2 are on the
  [tracker](https://github.com/jprodgers/jetcarp/issues).
- **Triage.** A fix that fits goes into the next candidate; anything that would change stored data,
  or is too large to test properly in time, goes to the next version. GitHub milestones track which.
  A feature that misses what it is for is held back: in `rc.3`'s testing, Fit to this computer took
  from 45 seconds to 12 minutes to size the desktop background for a machine, against Jimmie's target
  of 10 to 30 seconds, so 0.4.2 ships without it and a faster design is planned for 0.4.3 (#112,
  #113).
- **Fixes come with tests.** Where a test can be written, it is. Some fixes can only be checked on a
  particular system (how macOS treats a hidden window, for example); those are checked by hand
  there, in the next candidate's R. In the 0.4.2 cycle, 74 of the 102 closed issues have a test
  change in a commit that cites them; 14 fixes have neither a test nor a stated reason, and that a
  test failed without its fix is written down for five. From 0.4.3, each issue is closed with a note
  naming its fix, its test and that the test failed before the fix, or why there is no test.
- **The book is updated with each change**, and every candidate gets a full audit of the book before
  its tag: every page that mentions something changed, the app's pictures, and the changelog.
- **A candidate is tagged** only on a commit whose CI has passed on all three systems, and only on
  Jimmie's go. Since 0.4.3 the release workflow holds to the first half itself: it waits for CI on
  the tagged commit and stops unless it passed. And `main` takes changes only through pull requests
  whose CI checks have passed. In the 0.4.2 cycle, `rc.1` was tagged before its CI run had finished,
  and that run failed (#15); `rc.3` and `rc.5` were tagged while their commit's run was still going,
  and both passed.
- **Security problems** are reported privately, as
  [SECURITY.md](https://github.com/jprodgers/jetcarp/blob/main/SECURITY.md) says. In the 0.4.2 cycle
  what stayed private was the problem: no public issue, its test records withheld and its advisory
  drafted privately until the release. The fixes went onto the public release branch, so their
  commits were public before 0.4.2 came out, while 0.4.1 was the current release. From 0.4.3, a fix
  for a problem not yet made public is made in its advisory's private fork on GitHub and merged when
  the release is made.

A step that turns out to have been set up wrong is run again, and the result recorded as the second
run's, with a note. A step that fails in a way that may be chance (a hang in one run of thirty) is
repeated until the evidence either way is clear, and the issue says how strong that evidence is.

## Who does what

- **Jimmie** decided what needed checking and how thoroughly: every operating system and GPU vendor,
  the upgrade path, a newcomer's first hour, full passes rather than quick ones, an issue for every
  problem. Jimmie approved each test plan, ran the hands-on sittings on each machine, judged the
  results by eye, and decided which problems held a release back. Jimmie wrote no test code.
- **Claude**, in Claude Code, drafted the plans' detailed steps from Jimmie's requests, wrote all of
  the test code and the test scripts, ran the unattended parts, read the results, filed the issues
  and wrote the fixes.

Many problems were found only because a person was using the program: a rotation that could not be
saved because its Save button was out of sight, blinkers that looked frozen on the desktop, a round of
clips that was "lots of static", a menu that closed before a number could be typed into it, an app
that stopped when its window was hidden. Others were found only by the scripted checks: a memory
reading on macOS that counted memory the system could take back, a resumed search that ran past its
budget, a pack that silently lost its runs. Neither would have been enough alone.

## The 0.4.2 cycle in numbers

A record of one cycle, from the first issue to the release, after five candidates (November 2026):

| Stage | Issues filed | Found by |
|---|---|---|
| Development, before the first candidate | 12 (#2–#13) | reading, using and porting the code, an audit of the GPU code, a long search, everyday use |
| `rc.1`'s testing | 32 (#15–#46) | the Acer (23), the Framework (3), CI (3), the book audit (3) |
| `rc.2`'s testing | 31 (#47–#78, less #70), and one security problem, handled privately | the three machines, a full pass |
| `rc.3`'s testing | 37 (#80–#116) | the three machines (30: 4 in Phase 1, 25 in the Mac's sittings, 1 in the Acer's), writing the book (1), a user's report and one issue split from it (#114, #115), Jimmie's decisions (4: the seizure warning #96 and its roadmap item #97; holding Fit back, #112 and #113) |
| `rc.4`'s pre-check, on the branch | 6 (#117–#122) | the Acer (3: #117, #121, #122), the book audit (#118), a Dependabot alert (#119), CI and the Acer (#120) |
| `rc.4`'s testing | 15 (#123–#137), and a second security problem, handled privately | the Mac (8), the Framework (4), the Acer (2), looking into #119 (#123) |
| `rc.5`'s testing | 7 (#138–#144) | the Framework (3: the suite run with the GPU taken away, #138; a newcomer's first hour, #139, #140), the Mac (2), the Acer (2) |

Of the 141 issues filed in the cycle (#2–#144), 101 were fixed or done for 0.4.2 (each checked on the
next candidate), 37 were moved to 0.4.3 (the strobe guard, #97, first meant for later, among them),
one was answered in part (#114, below), and two were closed with no change: a Dependabot alert for a
crate that no build compiles (#119), and a rule applied only when its box loses focus, which is by
design (#132). One came from outside: a user's report on the MCP server (#114), answered with a fix in
0.4.2 and an issue for 0.4.3 (#115); the same user's report on 0.4.1 (#1) came before the cycle. A
question on the ConwayLife forum became #70, filed by the maintainer. Every issue says where it was
found, and the [changelog](changelog.md) lists the fixes to problems that 0.4.1 had, with their
issues.

## What guards each security promise

[`SECURITY.md`](https://github.com/jprodgers/jetcarp/blob/main/SECURITY.md) says what JETCARP does
and does not do. Each of its promises is held up by automated tests, by a check in CI, by the
hands-on rows of each release candidate's plan (its part S, from 0.4.3), or by how the code is
built. The next audit checks this table against the code.

| # | Promise | Automated tests and CI checks | Hands-on (part S) |
|---|---|---|---|
| 1 | It makes no network connections; the only network sockets it opens are local, on `127.0.0.1` | The Dependencies workflow (`deny.toml`): no HTTP, TLS, DNS, WebSocket or peer-to-peer crate in the dependency tree that `deny.toml` does not allow, so networking comes only on purpose. `the_listener_is_on_loopback_only` (`jetcarp-mcp/tests/link.rs`). By construction: the workspace's one listening socket is bound to `127.0.0.1` (`Listener` in `jetcarp-mcp/src/link.rs`) | |
| 2 | Nothing in a pack is run; every hash and signature is checked before anything is stored | `jetcarp-pack/tests/hostile.rs`: oversized and deep documents, unknown versions and features, altered objects, names that would leave the store, too many parts, decompression and decoding bombs, records that do not hash to their ids, unsigned and badly signed versions, links out of an export or a store | S-P1 (the same tests on each machine), S-P2 (a pack signed by a key the machine has not seen) |
| 3 | A scene holds no code | By construction: a scene's `script` is a list of steps and the conditions between them, which is data (`jetcarp-core/src/script.rs`) | |
| 4 | Rhai scripts run only when you run them | By construction: a script runs from `jetcarp script`, the app's script window, the MCP server's `run_script` tool, or as a rotation's script while that rotation plays ([Rotations](../guides/rotations.md#a-rotations-script)); nothing else starts the engine | |
| 5 | Scripts write only inside the output folder, the folder given with `--out`, or (through MCP) the allowed folders | `writes_stay_inside_the_output_root` (`jetcarp-script/tests/script.rs`); `jetcarp-mcp/tests/folders.rs`, whose link cases run with symbolic links on every system and junctions on Windows | S-F1 to S-F10 |
| 6 | The app takes commands only while AI control is switched on | `nothing_listens_while_ai_control_is_off` (`jetcarp-app/src/app/ai_control.rs`) | S-L1 |
| 7 | Each listener makes a fresh 128-bit token from the system's random source; a wrong token is refused; the comparison takes the same time whichever character is wrong | `tokens_are_128_random_bits`, `each_listener_makes_its_own_token_and_records_it_privately`, `a_wrong_token_is_refused_and_the_right_one_works` (`jetcarp-mcp/tests/link.rs`). By construction: `constant_time_eq` (`jetcarp-mcp/src/link.rs`) compares every byte, with no early exit | S-L3, S-L4 (from a second user account) |
| 8 | The token file is readable by this user only (folder 0700, file 0600 on Linux and macOS), and is not left behind if it cannot be kept private | `a_loose_folder_and_file_are_made_private`, `each_listener_makes_its_own_token_and_records_it_privately` (Linux and macOS) | S-L2 (on Windows, the folder's permissions with `icacls`) |
| 9 | An AI client's tools read and write only in the output folder, the library's folder and the `--allow` folders | `files_stay_inside_the_folder_rules` (`jetcarp-mcp/tests/protocol.rs`), `jetcarp-mcp/tests/folders.rs`, `an_export_s_scratch_store_stays_inside_the_folder_rules` (`jetcarp-api/tests/reference.rs`) | S-F1 to S-F10 |
| 10 | An AI client cannot make the pack-signing key or record anyone's consent to publish their ratings | `tools_list_is_well_formed_and_from_the_operations` (`jetcarp-mcp/tests/protocol.rs`) checks that no tool does either | |
| 11 | What a script may use through MCP is limited | `run_script_with_limits_and_progress`, `a_grid_over_the_cell_limit_is_refused` (`jetcarp-mcp/tests/protocol.rs`) | S-S1 |
| 12 | The installers check the archive they download against `SHA256SUMS`; every release file has a provenance attestation | `packaging/test-install-sh.sh` (Linux) and `packaging/test-install-ps1.ps1` (Windows), in CI: an archive with one byte changed, and one `SHA256SUMS` does not list, are refused with nothing installed | S-I1 (the same with each system's installer), S-I2 (`gh attestation verify` on each file of the candidate), S-I3 and S-I4 (`jetcarp uninstall` with linked folders) |

The crates that take in other people's data (`jetcarp-mcp`, `jetcarp-pack`, `jetcarp-script`,
`jetcarp-store` and `jetcarp-gpu`) forbid unsafe code (`#![forbid(unsafe_code)]`), so the compiler
keeps them free of it.

## What this testing does not cover

- **Hardware and systems.** Three machines: no Linux on NVIDIA or Intel graphics, no Windows on AMD,
  no Mac with an Intel processor, and on Linux only GNOME on Wayland has been tried for the desktop
  background. CI's runners have no real GPU. A driver that miscomputes on hardware we have not tried
  could still give a different answer; `jetcarp verify` checks any run on the CPU.
- **One person at the keyboard.** The hands-on sittings are Jimmie's: one person's eyes and habits,
  on one set of machines.
- **The same author for the code and the tests.** The plans' detailed steps and all the test code
  were written by the AI that wrote the program, so they can share its blind spots. The checks on
  that are the hands-on sittings, the checks that compare one way of computing with another (each
  GPU with the CPU, each machine with the others), and reports from outside. None of these is an
  independent implementation: a mistake made the same way on the CPU and the GPU would pass.
- **The AI's reading of results** can be wrong. Its test sessions have misread or mis-set-up steps:
  one ran a read-only-folder check against a folder it had itself left writable, and the step had to
  be rerun. Results are kept with their evidence so such mistakes can be found.
- **Withheld records.** The test plans, logs and results are public, in
  [jetcarp-testing](https://github.com/jprodgers/jetcarp-testing): a copy of the repository the test
  sessions worked in, less a few personal details. The records of a security problem not yet fixed
  in a release are left out until that release (the two problems of the 0.4.2 cycle are in it now).
  Its `WITHHELD.md` lists what is left out.
- **Expert review.** The check by people who know the field best, the ConwayLife community, is still
  to come.
