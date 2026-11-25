# Help test JETCARP

Every release is tested by hand on three computers first ([How JETCARP is tested](testing.md)).
Every other computer is new ground: another processor, graphics card, driver or system can show
a problem those three never will. The **field test** lets anyone test a release on their own
computer: one script, with nothing to install and no AI session, that runs the release's own
checks for a few minutes or up to an hour and writes **one report file**, for you to send back by
hand.

## What it promises

- **Nothing flashes.** Every test is a command-line run of `jetcarp`: no app window, no desktop
  background, nothing drawn on the screen at all. JETCARP's animations can flicker (see the
  [photosensitive seizure warning](../getting-started/photosensitivity.md)), so the field test shows
  none of them, and never answers the warning for you: nothing it runs asks for it.
- **Your own JETCARP is left alone.** The script downloads the release into a temporary folder and
  runs it from there, with JETCARP's settings, data and output folders inside that folder too. A
  JETCARP you have installed, its settings, its library and its pictures are not touched, and the
  Hardware Score it measures is not kept.
- **Nothing is sent.** JETCARP [makes no network connections](../getting-started/installing.md#no-telemetry),
  and the script only downloads: GitHub's list of releases, the release you chose with its
  `SHA256SUMS` (the archive is checked against it, as the installers do), and the tests' data from
  the JETCARP repository. The report stays on your computer until you send it.
- **It says what it is doing.** Each test has a heading, what it checks and the command it runs, a
  progress bar where the work has a count (scenes, soups, runs) and a spinner where it hasn't, with
  a bar for the whole run and the time left. Each result shows as it finishes: pass or fail, and
  its numbers.

## Running it

On Linux and macOS, in a terminal:

```bash
curl -LsSf https://raw.githubusercontent.com/jprodgers/jetcarp/main/testing/field/jetcarp-field-test.sh -o jetcarp-field-test.sh
sh jetcarp-field-test.sh
```

On Windows, in PowerShell (Windows PowerShell 5.1, which every Windows 10 and 11 has, or
PowerShell 7), save the script and run it:

```powershell
Invoke-WebRequest https://raw.githubusercontent.com/jprodgers/jetcarp/main/testing/field/jetcarp-field-test.ps1 -OutFile "$env:TEMP\jetcarp-field-test.ps1"
powershell -ExecutionPolicy Bypass -File "$env:TEMP\jetcarp-field-test.ps1"
```

(The script is saved first for the same reason the installer's is: Microsoft Defender can flag a
downloaded script piped straight into `iex`.)

It asks three things, each answered with Enter for the usual choice:

1. **Which release.** The newest, release candidates included, unless you pick another from the
   list, newest first, each marked as a release or a release candidate.
2. **How long.** Quick, Suite or Extended (below).
3. **At the end, keep it or not.** Remove everything the script added (the default), or keep this
   version, installed by JETCARP's own installer as [Installing JETCARP](../getting-started/installing.md#the-installers)
   describes (in place of an installer-made JETCARP that is there already; your settings and
   library stay).

Before measuring, it looks for anything that would skew the numbers: a JETCARP of your own that is
running, a busy processor, a laptop on battery. `jetcarp score` then names any busy programs itself
and asks whether to wait for them or measure anyway. Close what you can.

Ctrl-C stops it at any point: the running test is stopped, the temporary folder removed, and a
report of what had run is still written, marked as stopped.

The questions can be answered beforehand instead, for a run that asks nothing:

| Shell | PowerShell | Does |
| --- | --- | --- |
| `--release TAG` | `-Release TAG` | Test this release, such as `0.4.3-rc.2`, `v0.4.2` or `latest` |
| `--tier T` | `-Tier T` | `quick`, `suite` or `extended` |
| `--yes` | `-Yes` | Ask nothing: the newest release, Quick, and remove at the end, unless the other options say otherwise |
| `--keep` | `-Keep` | At the end, install the tested version with JETCARP's installer |
| `--remove` | `-Remove` | At the end, remove everything the script added |
| `--report PATH` | `-Report PATH` | Write the report here, rather than in the folder you ran it from (or your home folder) |

## The tiers

| Tier | Time | Runs |
| --- | --- | --- |
| **Quick** | a few minutes | What JETCARP sees on this computer (`jetcarp doctor`); which graphics adapters it runs on; anything that would skew the numbers; the [JETCARP Hardware Score](../guides/this-computer.md#the-benchmark-and-the-score) (`jetcarp score --no-save`, about a minute); 16 scenes on the processor and the GPU JETCARP picks |
| **Suite** | 10 to 20 minutes | Quick's first four, then all 371 scenes on the processor and on each GPU; two soup hunts with known results (5,000 Life soups on squares, 3,000 of B2/S34H on hexagons), on the processor and each GPU; a HashLife jump of 2<sup>30</sup> generations; *Fit to this computer* for a 1920 × 1080 screen, off screen (`jetcarp host tune --canvas 1920x1080 --dry-run`, with no background host running); and stepping speed, the generations a second of Life and of JACA on a 1024 × 1024 grid |
| **Extended** | 30 to 60 minutes | Suite, then 40 scenes for 5,000 generations each; 98 of the scenes again on every other graphics API the computer has (the same GPU through DirectX 12 or OpenGL, a software renderer); and a 15-minute [explore search](../guides/searching.md) with its memory read every 5 seconds, which stops itself if it passes a quarter of the computer's memory |

**The scenes** are the 371 each release candidate is held to on the test machines: every built-in
Quick Start scene of JETCARP 0.4.2, and seven sandpiles that drop grains. Each runs 300 generations
on the processor (`--cpu`) and on each GPU (`--gpu`), and every final
[state hash](../research/reproducibility.md) must equal the one recorded for that release: a GPU or
a driver that computes one cell differently fails. The hunts'
counts, the HashLife jump's population and the stepping runs' hashes must be the same everywhere
too. Only the speeds and the score depend on the computer.

**Older releases** skip what they can't run, and say so: 0.4.2 has no Hardware Score and no *Fit to
this computer*, and the scenes need 0.4.2-rc.1 or later. Each release is checked against the hashes
recorded for it (0.4.3 changed how they are written,
[#173](https://github.com/jprodgers/jetcarp/issues/173)).

How long each tier took on the main test machine, from the plan to the report:

| Tier | The desktop (Linux, 32 threads, Radeon 8060S), with 0.4.3-rc.2 |
| --- | --- |
| Quick | 1 minute |
| Suite | 2½ minutes |
| Extended | 19½ minutes, 15 of them the search |

That desktop is fast, and its one GPU is quick to start. A laptop takes longer, and each GPU adds
its share: the plan shown before it starts gives an estimate for a typical laptop, and the bar for
the whole run corrects it as the tests go.

## The report

At the end the script writes `jetcarp-field-report-<release>-<tier>-<date>.json` in the folder you
ran it from (or in your home folder, if that one can't be written), says where it is, and offers
to show you the whole file before you send it. It is JSON, with a short summary in words at the
top. It holds:

- the release, and the checksum of the archive tested;
- the computer: the system and its version, the processor, its cores and threads, the memory, and
  each graphics adapter with its driver and API, and whether JETCARP uses it (as
  [`jetcarp doctor`](../reference/cli.md#jetcarp-doctor) reports them, in full);
- each test's result, its numbers and how long it took: the score and its parts, every scene's
  hash on each device, the hunts' counts, the speeds, the search's memory over time;
- the output of anything that went wrong.

It doesn't hold your user name, your home folder, the computer's name or its network addresses:
the script replaces them (`[user]`, `~`, `[machine]`, `[ip]`) before it writes the file, and the
temporary folder's paths as `[scratch]`. It reads none of your files, and says nothing of other
programs but how busy the processor was and how many JETCARP programs were running.

**Send it by hand**, by e-mail or chat, to whoever asked you to test. A report can also go on a
[GitHub issue](https://github.com/jprodgers/jetcarp/issues) (which anyone can read).

## In CI

A workflow, *Field test*, runs both scripts the way a tester would (Quick, asking nothing) on
Linux, macOS and Windows, the PowerShell one under both 5.1 and 7, against the newest release once
a week and on demand, and keeps each report. Its runners have no real GPU, so it mostly checks the
scripts themselves and their fallbacks to the processor; it is advisory, never a check a change
must pass.

The scripts, their tests and their data are in the repository's
[`testing/field/`](https://github.com/jprodgers/jetcarp/tree/main/testing/field), whose README says
how to change them.
