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
- **Searches survive interruption.** Every kind of job, stopped and resumed (on the same runner or
  another), must end with exactly the results of one that ran through.
- **Exact verification.** Every object a hunt keeps is run again alone and compared cell by cell.
- **The front ends.** The command line's JSON output, the operations the front ends share, the MCP
  server's protocol, its folder rules (paths outside its folders, symbolic links, read-only folders)
  and its limits, the scripting API, packs (signed, unsigned, tampered and truncated), and the app,
  which a test harness drives offscreen, clicking widgets by their text.
- **The book.** The command-line and MCP reference pages are generated from the program and checked
  against it, and the copy of the book built into the program must match the book.

About 35 more tests are ignored by default and run only when asked (`-- --ignored`): most draw the
book's pictures of the app, and the rest time the program, run very large worlds, or need a desktop
session.

Tests never write to your own settings, data or pictures: they run in scratch folders, and CI fails
if a test touches the default ones.

## Continuous integration

GitHub Actions runs on every push to `main` and every pull request. A release is prepared on a
branch with a pull request open against `main`, so every push to it is tested; a tag runs the
release workflow.

- **Build and test** the whole workspace on Linux (x86_64), Windows (x86_64) and macOS (Apple
  silicon). The runners have no GPU of their own, so on Linux the GPU tests run on Mesa's software
  renderer (lavapipe), on macOS on the runner's virtual GPU, and on Windows they are skipped (JETCARP
  never uses Windows' software renderer, WARP). On Linux the tests run once more with the default
  folders, failing if any test writes to them.
- **Build only** for macOS on Intel and Linux on ARM, which nobody tests by hand yet.
- **Checks:** formatting (`cargo fmt --check`), the third-party notices are current, the workflows
  and packaging scripts pass `actionlint`, every crate packages and builds on its own (a crates.io
  dry run), and the generated reference pages match the program.
- **The book** builds, its internal links all resolve, and its scripting reference matches the
  program.
- **A release** is built only from a tag whose version agrees with the program and the changelog,
  and only after the tests pass.

## Release candidates

Before a release, JETCARP goes through release candidates: `0.4.2-rc.1`, `rc.2`, and so on. Each is
a public pre-release, built by the same workflow as a release, which the package managers (Homebrew,
Scoop, PyPI, crates.io) do not pick up. Each candidate is tested on three machines, with GPUs from
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

**Phase 1, unattended**, on every machine:

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
- **An overnight run** of the live desktop background, eight hours, for leaks and stalls, when a
  machine can be left idle that long.

**Phase 2, by hand**, in sittings of an hour or two at each machine:

- **Searching:** an exploratory search on the GPU, replays, the CPU confirming the GPU's finds,
  rating clips on the rating page in two browsers, fitting a score to the ratings, packs.
- **The app:** every menu item, pane and tool window, popped out and back, AI control from an MCP
  client, light and dark, the scripting examples, exports, and big worlds on each GPU.
- **The desktop background:** rotations, *Fit to this computer*, pauses for full-screen windows,
  battery and busy processors, sleep, lock and login, and the background host killed and restarted.
- **MCP and robustness:** every tool, bad input of every kind (124 malformed calls in one sitting),
  paths outside the allowed folders, read-only folders, damaged and tampered packs, two programs
  using one library, and the largest worlds the limits allow.
- **A newcomer's first hour and the upgrade from the previous release**, each in a separate user
  account, and **two monitors**.

**R, a check of every fix.** Each candidate's plan lists every problem fixed since the last one,
with a step that shows the fix working, run on every machine.

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
- **Fixes come with tests.** Where a test can be written, it is, and it is checked to fail without
  the fix. Some fixes can only be checked on a particular system (how macOS treats a hidden window,
  for example); those are checked by hand there, in the next candidate's R.
- **The book is updated with each change**, and every candidate gets a full audit of the book before
  its tag: every page that mentions something changed, the app's pictures, and the changelog.
- **A candidate is tagged** only when CI passes on all three systems, and only on Jimmie's go.
- **Security problems** are reported and fixed privately, as
  [SECURITY.md](https://github.com/jprodgers/jetcarp/blob/main/SECURITY.md) says, and described in
  public only once fixed.

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

A record of one cycle, as of its fourth release candidate (November 2026):

| Stage | Issues filed | Found by |
|---|---|---|
| Development, before the first candidate | 12 (#2–#13) | reading, using and porting the code, an audit of the GPU code, a long search, everyday use |
| `rc.1`'s testing | 32 (#15–#46) | the three machines, CI (3), the book audit (3) |
| `rc.2`'s testing | 31 (#47–#78, less #70), and one security problem, handled privately | the three machines, a full pass |
| `rc.3`'s testing | 12 (#80–#91) | the three machines |

Of the 88 issues filed in the cycle (#2–#91), 78 were fixed for 0.4.2 (those found in `rc.3`'s
testing are checked in `rc.4`'s) and 10 were moved to 0.4.3. Two came from outside: a user's report
(#1, from 0.4.1) and a question on the ConwayLife forum (#70). Every issue says where it was found,
and the [changelog](changelog.md) lists the fixes with their issues.

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
  sessions worked in, less the records of a security problem handled privately (withheld until it
  is described publicly) and a few personal details. Its `WITHHELD.md` lists what is left out.
- **Expert review.** The check by people who know the field best, the ConwayLife community, is still
  to come.
