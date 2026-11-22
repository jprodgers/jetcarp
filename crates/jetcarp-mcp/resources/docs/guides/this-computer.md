# This computer: the score and the limits

A desktop with 128 GB of memory can hold dozens of the largest grids; a 16 GB laptop holds a few.
Before 0.4.3 both had the same limits, fixed in the code. Now JETCARP knows two things about the
computer it runs on, and keeps them apart:

1. **What it holds**: memory, the processor and its cores, and each GPU's memory. JETCARP reads
   these from the system in a few milliseconds every time it starts, with no benchmark, and the
   limits that depend on memory come from them alone. A computer that was never measured still
   gets limits that fit it.
2. **What it does**: about a minute of JETCARP's own work, the **benchmark**. It gives the
   **JETCARP Hardware Score** and its parts. It runs at setup if you leave it ticked, and whenever
   you start it, never by itself.

**Limits only allow or refuse.** A world too big for this computer is refused with the reason, or
in the app built only when you ask; it is never made smaller or changed to fit. So anyone with a
big enough computer gets the same result from the same scene, as JETCARP promises everywhere else.

## What JETCARP reads

| | Linux | Windows | macOS |
| --- | --- | --- | --- |
| Memory, and how much is free | `/proc/meminfo` | `GlobalMemoryStatusEx` | `sysctl hw.memsize` and the virtual memory statistics |
| The processor and its cores | `/proc/cpuinfo` | the registry's processor name | `sysctl machdep.cpu.brand_string` |
| GPU memory | `mem_info_vram_total` and `mem_info_gtt_total` under `/sys/class/drm`, which AMD's driver writes (other drivers' GPUs are estimated) | DXGI's dedicated and shared memory | one pool shared with the processor |
| How busy the processor is, as it measures | `/proc/stat`, less its own time from `/proc/self/stat` | `GetSystemTimes`, less its own from `GetProcessTimes` | `host_statistics`, less its own from `getrusage` |
| How busy the GPU is, as it measures | AMD's `gpu_busy_percent`; NVIDIA's management library (NVML, `libnvidia-ml.so.1`, which comes with NVIDIA's driver, though some distributions package it on its own); for Intel's and other drivers' GPUs, DRM `fdinfo`: each program's GPU time, in `/proc/<pid>/fdinfo` | the `\GPU Engine(*)\Utilization Percentage` counters (PDH), which every vendor's driver gives and Task Manager shows | each `IOAccelerator`'s `PerformanceStatistics`, `Device Utilization %` (IOKit, what `ioreg -r -d 1 -c IOAccelerator` shows), with no administrator rights |

A GPU whose memory the system does not report gets its limit from its buffer limits and a cautious
share of the memory reserved for JETCARP, said as an estimate. On a computer whose firmware sets
part of the memory aside for an integrated GPU (a desktop with 128 GB might give 32 GB of it to the
GPU), the system's memory is what is left, and the GPU's is the part set aside, plus what the system
lets it borrow. On Apple silicon the processor and the GPU share one pool, and GPU worlds count
against it like everything else.

`jetcarp doctor` shows what it read, the score and the limits that matter most:

```text
This computer
  system:    linux x86_64
  processor: An 8-core processor (16 threads, 8 cores)
  memory:    32 GB (21 GB free now)
  GPU 1002:73df: 12 GB own, may borrow 16 GB (sysfs)
  JETCARP Hardware Score: not measured yet (`jetcarp score`)
  JETCARP in use now: jetcarp doctor 78.5 MB; 15.9 GB of the 16 GB reserved for JETCARP left
  reserved for JETCARP: 16 GB
  largest world: 8 GB
  an AI's share: small
  every limit: `jetcarp limits`
```

## The limits

Each limit is **your own value** if you set one, else **the machine's suggestion**, and never above
its **ceiling**.

**Memory comes first**, as two linked values: **Reserved for the System** and **Reserved for
JETCARP**. They add up to the computer's memory, so setting one sets the other. A smaller computer
leaves half its memory to the system; a bigger one reserves at most 16 GB for the system and gives
JETCARP the rest:

| Memory | Reserved for the System | Reserved for JETCARP |
| --- | --- | --- |
| 8 GB | 4 GB | 4 GB |
| 16 GB | 8 GB | 8 GB |
| 64 GB | 16 GB | 48 GB |
| 96 GB | 16 GB | 80 GB |
| 128 GB | 16 GB | 112 GB |

Someone with a big computer can give JETCARP nearly all of it; at least 512 MB stays on each side.

**JETCARP's part is one pool, for all of JETCARP**: the app, the desktop background, every command,
MCP servers, scripts and search jobs draw on it together, so a desktop with 128 GB that reserves
16 GB for the system gives all of JETCARP 112 GB, not 112 GB to each program. Every other memory
limit is a share of that pool, never larger than it: the desktop background may use all of it, less
what the rest of JETCARP holds, and the others are scaled from the values the code had fixed before
0.4.3 on a 16 GB computer, so a 16 GB computer keeps the limits it had:

| Limit (`jetcarp limits` name) | What it limits | On 16 GB | On 64 GB | Ceiling |
| --- | --- | --- | --- | --- |
| Largest world (`world`) | The memory one world may take | 4 GB (half of JETCARP's part) | 24 GB | all of JETCARP's part, and 4,294,967,295 cells |
| Largest world on the GPU (`gpu-world`) | Cells a world may have to step on the GPU; a larger one steps on the processor, with the same result | half the GPU's memory at 16 bytes a cell (for an integrated GPU with little of its own, with what it may borrow, within JETCARP's part), within its buffer limits | the same | 2,147,483,647 cells |
| Desktop background: memory (`host-memory`) | What the background host may use before it gives up its largest world | the pool, 8 GB, less what the rest of JETCARP holds | the pool, 48 GB, less the same | all of JETCARP's part |
| Desktop background: cells (`host-cells`) | Cells one of its worlds may have | 64 million | 384 million | 4,294,967,295 |
| HashLife: nodes (`hashlife-nodes`) | Blocks HashLife keeps before it clears out unused ones (about 45 bytes each) | 48 million | 288 million | 2,147,483,648 |
| Hunt caches (`hunt-cache`) | What a soup hunt's object caches may hold together | 1.5 GB | 9 GB | all of JETCARP's part |
| Media: pixels in one animation (`media-pixels`) | Width × height × frames of one catalog animation, held in memory | 300 million | 1.8 billion | JETCARP's part, at 4 bytes a pixel |
| Wallpaper export: fresh grid (`wallpaper-cells`) | Cells of a fresh grid the app's *Export wallpaper…* builds | 16 million | 96 million | 4,294,967,295 |
| Animation export: frames (`animation-frames`) | Frames of an animation exported from the app (each held until the end) | 2,000 | 12,000 | a million |
| An AI's share (`ai-share`) | How much of JETCARP's limits an AI's worlds may use | small | small | all of it |

**The ceilings are the code's own.** Cells are numbered with 32-bit integers in places (commands'
`[cell, state]` pairs, seeds, ants, layers, the batch stepper's neighbour lists), so a grid can have
at most 4,294,967,295 cells, the most such a number holds; 0.4.3 checked every such place before the
ceiling rose from 256 million. The GPU simulator stops at half that, and HashLife numbers its nodes
the same way. An aperiodic patch is held to its level (at most 8), as before.

**What a world needs** is worked out before anything is built, from its cells and its CAs. The table
was measured by counting every allocation while a world of about a million cells of each kind was
built, seeded and stepped (`tests/memory_per_cell.rs` in `jetcarp-core` checks it each time the tests
run):

| A cell of | Bytes |
| --- | --- |
| Life, Generations, WireWorld and every other rule with plain states | 8 |
| A layered rule | 12 |
| JACA | 9 |
| A sandpile | 16 |
| Lenia | 20 |
| An aperiodic patch, for its graph, on top of its CAs | 200 |

So 30,000 by 30,000 cells of Life take 6.7 GB, and a world of several CAs counts each of them.

### Sharing the pool

Each JETCARP program writes down the memory it holds (its resident memory, as the system counts
it) in the runtime folder, as `memory/<process id>.json`, when it starts and every 2 seconds after,
and removes it when it ends; a record not renewed for 10 seconds belongs to a program that has gone,
and no longer counts. (`jetcarp uninstall` and `jetcarp launcher`, which build nothing and are over
in a moment, write none, so that they leave no runtime folder behind.) From those records:

- **A new world must fit what is left of the pool**, as well as the largest-world limit: the pool,
  less what every other JETCARP program holds, less what this one holds beyond the world the new
  one replaces. `jetcarp run`, scripts, the MCP tools and the app all check this before anything is
  built.
- **The desktop background yields.** Its memory limit, and the largest grid it builds, are what the
  rest of JETCARP leaves it. When the app or a command takes the memory, the background gives up its
  largest world for the next item, as it does when it passes its own limit.
- `jetcarp limits`, `jetcarp doctor` and *Tools → This computer…* show who holds what:

```text
JETCARP in use now: the desktop background 1.2 GB, the app 640 MB, jetcarp limits 17 MB; 110 GB of
the 112 GB reserved for JETCARP left
```

**What is not enforced across programs.** A world is checked when it is built, not as it grows (a
sandpile's heights, HashLife's nodes and a hunt's caches grow within limits of their own, each a
share of the pool, but they are not reserved in advance). Two worlds started in two programs in the
same moment can each see the same free memory. Programs other than JETCARP, and copies of JETCARP
from before 0.4.3, are not counted: that is what *Reserved for the System* is for. A program that
cannot write to the runtime folder is not counted either, and the app asks before a world is built
over what is left, with *Build anyway*, as it does over any limit.

### Over a limit

The app asks first, with what the world needs, the limit and where the limit came from:

<img src="../images/guides/this-computer-over-limit.webp" alt="A dialog over a running world: this world needs 122 MB; this machine's limit is 100 MB (your own value), with Build anyway, Cancel and This computer… buttons" width="700">

**Build anyway** builds it, up to the code's ceilings; **Cancel** builds nothing. The command line,
scripts and the MCP tools refuse instead, until the limit is raised:

```text
Error: this world needs 31 GB; this machine's limit is 24 GB (half of the 48 GB reserved for
JETCARP, from its hardware: Tools → This computer…, or `jetcarp limits`)
Error: this world needs 1.9 GB, and 824 MB of the 8 GB reserved for JETCARP is left (the desktop
background 7 GB, jetcarp run 200 MB): close something, or reserve more for JETCARP (Tools → This
computer…, or `jetcarp limits`)
```

### An AI's share

The MCP tools (and the scripts they run) work within **an AI's share** of JETCARP's limits. It is
**small** by default: worlds of up to 4 million cells, as before 0.4.3, on any computer. Raise it,
up to **all of it**, when an AI should check on a massive grid; the MCP server takes a new share
when it next starts (`jetcarp mcp`, or the app for its AI control). The tools' descriptions are
written from the live values, and `reference {"what": "machine"}` shows an AI this computer, its
score, its limits and its own. An AI can read the score; it can never run the benchmark.

## The benchmark and the score

The benchmark is JETCARP's own work on fixed scenes and seeds, about a minute in all:

- **Life soups on five non-square tilings**: the hat, the Spectre, Penrose's rhombs, the 3.4.6.4
  tiling and the 2-uniform tiling k2-017. Each is a batch of 256 soups, each a random start in the
  middle third of a world of about 2,048 cells, run 200 generations, the way explore searches run
  them: on every core, and on the GPU JETCARP would use. These make the score.
- **One core**: a square and a hexagonal Life world, in cell-generations a second.
- **A hunt**: Life soups on the square grid, run to their census, as soup hunts run them.
- **The GPU crossover**: the smallest square Life world the GPU steps faster than every core.
- **Memory in use per cell**: what the process grew by while a large world of each main kind was
  built and stepped (the limits use the allocation table above, which counts every buffer).
- **The disk**: nested folders and small files made and removed in the data folder.

**Every workload checks its answer.** Its final states must give the digest stored with the
benchmark, on the processor and the GPU alike, since JETCARP's results are bit-identical everywhere.
So the score is also a self-test: a device that gives a wrong answer is not used for that workload,
and the score says so. Nothing animates and nothing is drawn but a progress bar, and nothing is
sent anywhere.

**The JETCARP Hardware Score is its own unit.** Score v1 is the geometric mean of the five soup
rates, in soups a second, each the fastest device that gave the right answer. No computer is pinned
to a value: whatever number a computer gets is its number, and another is simply some multiple of
it. The parts are kept and shown too, because the limits and the defaults that depend on time come
from the parts, not from the one number: with no size from before,
[Fit to this computer](wallpapers.md#fit-to-this-computer) starts its search at the size the
one-core rate (on the processor) or the GPU's soup rate (on the GPU) says the scene's work fits in,
and it counts a step on the GPU at the GPU's speed over the processor's.

### While it measures

The score times JETCARP's own work, so anything else keeping the computer busy lowers it, and
JETCARP's own running worlds most of all. So before measuring, setup, *This computer* and
`jetcarp score` ask you to close other heavy programs: games, video, builds and other simulations.

**JETCARP holds its own work still while it measures**, and carries on as it was after:

| What | While it measures |
| --- | --- |
| The app's world, screensaver mode's too | Paused; it runs again after, if it was running |
| The desktop background, GNOME's lock screen and Windows' screen saver | Held still: `jetcarp host status` says *JETCARP is measuring this computer*, and a pause by hand stays as it was |
| Searches and jobs, in any program | Each waits between two of its steps, in the program running it, then carries on |

The measuring says so in the runtime folder, as `measuring/<process id>.json`, renewed every 2
seconds and removed when it ends. A record not renewed for 10 seconds is from a measuring that has
gone, so a crash holds nothing still for long. Each program that holds still says so in its record
in the [memory pool](#sharing-the-pool).

**What would still lower the score is said first:**

- **A JETCARP program that can't hold still**: one that steps worlds of its own (`jetcarp run`, a
  script, a picture being rendered) or another measuring. MCP servers are not named, since they
  work only while an AI's tool call runs, and the load check sees one that is busy. Neither is
  macOS's screen saver: its sandbox can't read the runtime folder, and it runs only while the Mac
  is idle.
- **A processor busy with something else**: other programs at 15% or more of it, as the system
  counts its busy time, less the measuring program's own. An idle desktop keeps a few percent busy;
  a game, a video call, a build or another simulation keeps a quarter or more. The parts on every
  core lose about the share others take, so at 15% they read about 15% low.
- **A busy GPU**: 25% or more of its time, on Windows, macOS and Linux, whatever its make
  ([What JETCARP reads](#what-jetcarp-reads) says from where). The figure is the busiest GPU's
  busiest drawing or compute engine: a game or another simulation keeps it far above, while a
  video decoded or a copy runs on engines of its own and is not counted. It counts the desktop's
  own drawing, so its threshold is higher. Windows' counters and `fdinfo` leave the measuring
  program's own drawing out; AMD's, NVIDIA's and macOS's figures are the whole GPU's.
  - `fdinfo` sees only the programs you may read, so another user's (a second login's game) are
    not seen; the i915 and xe drivers' own counters would see them, but need administrator rights.
  - Where the GPU's load can't be read (an NVIDIA GPU on Linux without its management library, or
    a system that gives no figure), the result says *GPU load not read*, and why. That never
    stops the measuring, nor marks it as taken under load.

The load is looked at once JETCARP has held still, for up to five seconds while it stays over a
threshold (a job's step under way ends first). Then the app's *This computer* window and setup's
progress page say what they found and wait, measuring as soon as it has gone, with **Measure
anyway**, and **Stop** or **Skip measuring**. `jetcarp score` asks in a terminal whether to wait,
measure anyway or stop; with its output going to a file or another program, or with `--json`, it
never asks: it says what it found and measures anyway, so scripts carry on.

**A score measured anyway is marked as taken under load**, in the result, in `machine.json` and in
the report, beside the score. Its `conditions` say other programs' share of the processor (`cpu`,
0 to 1) and the GPU's busy figure (`gpu`) as the measuring started, where the GPU's figure came
from (`gpu_load_source`: `pdh` on Windows, `iokit` on macOS, `amdgpu`, `nvml` or `fdinfo` on Linux)
or why it wasn't read (`gpu_not_read`), the JETCARP programs that held still (`paused`) and those
that couldn't (`not_paused`), and `under_load`. Such a score is likely
lower than the computer's. **The limits never come from the score**: they come from the hardware
and your own values, so a score taken under load leaves them as they were. Fit to this computer's
first guesses, which do come from the score's parts, skip a score taken under load, as they skip a
short run, until the computer is measured again with nothing else busy.

The benchmark is **versioned**. A JETCARP that changes the workloads raises the version, and an
older score is then shown as from an older benchmark, with a suggestion to measure again. A computer
that changed (another processor, memory, GPU or system, or a new firmware split of its memory) is
noticed when the app starts, and the status line suggests measuring again, once. Neither runs the
benchmark by itself.

```text
$ jetcarp score --report report
Close other heavy programs first: games, video, builds and other simulations lower the score.
JETCARP holds its own work still while it measures (the app's worlds, the desktop background,
searches and jobs, the screen saver) and carries on after.

Held still while it measures: the desktop background.
Measuring this computer (about a minute): JETCARP's own work on fixed scenes, each answer checked.
Nothing is drawn or sent anywhere.
JETCARP Hardware Score: 1,234 (score v1, 2026-11-21 14:05, JETCARP 0.4.3)
  Life soups on the hat                    processor (16 threads)     402 soups/s
  ...
  taken with the computer quiet (other programs 3% of the processor, the GPU 1% busy, from AMD's gpu_busy_percent)
Kept in ~/.config/jetcarp/machine.json
Report: report.md
Report: report.json
```

With something in the way, it says so first, and in a terminal asks:

```text
Running too, and can't be paused for the measuring: jetcarp run. Wait for it to end, or measure
anyway.
The processor is 42% busy with other programs: close what keeps it busy, or measure anyway.
A score measured anyway is marked as taken under load.
Wait for them (w), measure anyway (m), or stop (s)? [w]
```

`--report FILE` writes the computer, the parts, the score, how it was taken and the limits in
Markdown and JSON, for comparing computers; `--quick` runs each workload once, for its answers
alone, in a few seconds; `--no-gpu` leaves the GPU out; `--no-save` keeps nothing. In a terminal
the progress is one line, redrawn in place; to a file or a pipe each step is a line of its own,
with no escape codes.

## Tools → This computer…

<img src="../images/guides/this-computer-window.webp" alt="The This computer window: an example 8-core, 32 GB machine with a 12 GB GPU, not measured yet, the request to close other heavy programs above Measure this computer, what JETCARP's programs hold of its pool now, and its limits, with 12 GB reserved for the system as the person's own value" width="800">

The window shows the hardware, the score with its parts, the date of the last run and how it was
taken, **Measure this computer** (then **Run again**) with the request to close other heavy programs
above it, **Save a report…**, and the limits. While something would lower the score, it says what
and waits ([While it measures](#while-it-measures)). Each limit shows what
applies now, the machine's suggestion (with how it was worked out when you hover over it), your own
value, and its ceiling. Type a value of your own (memory as `24GB` or `512MB`, counts as `64000000`
or `64M`) and leave the field; **Back to the suggestion** puts one back, **All back to the
suggestions** every one. An AI's share is a slider, from small to all of it.

## On the command line

```text
$ jetcarp limits
Limits on this machine (32 GB of memory)
  Reserved for the System          16 GB                  reserved-system (half of its 32 GB)
  Reserved for JETCARP             16 GB                  reserved-jetcarp (the rest of its 32 GB)
  Largest world                    8 GB                   world (half of the 16 GB reserved for JETCARP)
  Largest world on the GPU         403 million cells      gpu-world (half of the GPU's 12 GB at 16 bytes a cell, within its buffer limits)
  ...
$ jetcarp limits set reserved-system 12GB
$ jetcarp limits set ai-share 25%
$ jetcarp limits reset ai-share
$ jetcarp limits reset
```

Your own values are kept in `limits.json` in the [settings folder](../reference/folders.md), and
only they are: measuring again never undoes a choice. The app and a running desktop background read
a change at once; another program started before it, at its next start. *Reserved for JETCARP* is kept as the system's
part, so a computer whose memory changes keeps what the system was given:

```json
{
  "version": 1,
  "own": {
    "reserved-system": 12884901888,
    "ai-share": 0.25
  }
}
```

The score and its parts are kept in `machine.json` beside it, with the hardware it was measured on
and how it was taken (`conditions`).
`host.json`'s `max_cells` and `max_memory_mb` give way to `host-cells` and `host-memory`, unless
they were edited by hand, when the edited value still applies. `JETCARP_HUNT_CACHE_MB` still sets
the hunt caches over everything.
