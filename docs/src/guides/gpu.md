# The GPU

JETCARP runs on the GPU wherever it can, and treats the processor as the fallback. The results are
the same either way, bit for bit: a scene gives the same world on any GPU, from any vendor, and on
the processor. Only the speed differs.

## What runs on the GPU

- **Worlds** of Life-like (B0 rules too), Generations, WireWorld, cyclic, rock-paper-scissors,
  Greenberg–Hastings, hodgepodge, JACA, Larger than Life, forest fire, sandpile, Spectre-glider and
  priority rules, on lattices and on aperiodic patches, and Margolus block rules and isotropic rules
  on the square grid. Other rules step on the processor, and a world that mixes them moves between the two as it
  needs.
- **Explore searches**: the rules a GPU can run go there in batches, isotropic rules among them;
  the others (turmites, Lenia, sandpiles and the like) run on the processor beside them.
- **Hunts**: the start of each soup.
- **Large sandpiles** on periodic tilings.

## When

The GPU setting is automatic unless you change it. Automatic means:

| What | On the GPU |
| --- | --- |
| Windows, wallpapers and screen savers (a set speed) | Worlds of 16,000 cells or more |
| `jetcarp run`, `jetcarp wallpaper`, scripts, the MCP server, the app at maximum speed | Worlds with a million neighbour reads a generation or more (about 125,000 cells of Life on squares), and any run of known length whose reads in all reach 100 million (1,500 generations of a 120,000-cell world, for example), since opening the GPU then pays for itself |
| Explore searches and hunts | Always |
| Sandpiles | From 100,000 grains |

Below those sizes the processor is about as quick, and the GPU need not wake. The thresholds come
from timings on a Radeon 8060S; `examples/bench.rs` in `jetcarp-gpu` measures them again.

## The setting

The shared setting lives in `gpu.json` in the [settings folder](../reference/folders.md), and the
app, the background host, the command line and the MCP server all read it:

```json
{"gpu": "auto", "gpu_adapter": "UHD"}
```

- `gpu` is `auto` (the default), `on` (every world the rules allow, however small) or `off`.
- `gpu_adapter`, which is optional, picks the adapter: `integrated`, `discrete`, `software`, or part
  of an adapter's name.

*Shared setting* under *GPU simulation* in the *Desktop background* pane writes its mode, and *Run →
GPU adapter* in the app its adapter: *Automatic* (the best one for each use), *Discrete*,
*Integrated*, or one of the adapters this computer has, by name. On a laptop with two GPUs, this is
how to move JETCARP from one to the other. The app picks its adapter when it starts, so it moves at
its next start; the command line, the MCP server and the host follow at theirs.

**The app and the background host each have a choice of their own,** over the shared setting:
*Follow the shared setting* (the default), *On* or *Off*. So one setting serves both, and either can
be set apart, for example the GPU on for the wallpaper and off for the app. The app's is *Run → GPU
simulation* (`gpu` in `app.json`; also `jetcarp app display --gpu follow|on|off` and the MCP
`app_display` tool); the host's is *GPU simulation* in the *Desktop background* pane (`gpu` in
`host.json`, which older files held as `true`, now read as follow, or `false`, now off). Either
choice, and the shared setting, take effect at once in a running host, and `jetcarp host status`
says what each screen steps on now, as `jetcarp doctor` names the choice: `simulation: GPU (…)`
with the adapter, or `simulation: CPU: GPU simulation off, from the desktop background's own
setting` (with `--json`, each screen's `simulation`: `on`, `adapter` and `why`).

`JETCARP_GPU` overrides all of it while it is set. It takes `off`, `auto` or `on` (or `0` and `1`),
or which adapter to use automatically: `integrated`, `discrete`, `software`, or part of a name. On a
laptop with an Intel UHD and an RTX 3050, `JETCARP_GPU=UHD` uses the Intel one (and leaves on or off
to the rest). When the adapter asked for isn't there, JETCARP looks twice more, a quarter of a
second apart (a laptop's discrete GPU waking from sleep can be missing from the first look), then
uses the best one. Wherever the adapter is named it says so, `gpu (Intel(R) UHD Graphics …, Dx12; no
RTX adapter was found, so the best one)`, and the command line also says it once on the error
stream: `note: GPU: no RTX adapter was found, so the best one`.

A command's own choice comes next: `--gpu` uses the GPU, `--cpu` only the processor (`jetcarp run`,
`jetcarp wallpaper`, `jetcarp search hunt|explore`, `jetcarp pile`, `jetcarp jobs resume`, `jetcarp
jobs refit` and `rescore`). The operations take the same as `gpu: true` or `false`; left out, the
setting decides. An explore search or a pile made with `--gpu` or `--cpu` keeps that choice for its
later runs.

In all, first to last: `JETCARP_GPU` (when it names off, auto or on), a command's `--gpu` or `--cpu`,
the app's or host's own choice, the shared setting, and automatic. A run or search given `--gpu`
while `JETCARP_GPU=off` runs on the processor and says why: its log has `the GPU was asked for, but
JETCARP_GPU=off: running on the processor`, and a run's summary `backend: cpu (32 threads); the GPU
was asked for, but JETCARP_GPU=off`. An adapter that failed its self-test is never used. `jetcarp
doctor` says what the app and the host each do, and why:

```text
GPU: auto (automatic)
  setting: /home/ada/.config/jetcarp/gpu.json (not saved)
  the app: auto, from automatic (its own choice: follow)
  the desktop background: off, from the desktop background's own setting (its own choice: off)
```

## Which adapter

Each part of JETCARP asks for an adapter in the same way, for one of two purposes:

- **Computing** (runs, searches, scripts) takes the fastest: a discrete GPU before an integrated
  one.
- **Displays** (the app's window, wallpapers, screen savers, the lock screen) take the adapter
  driving the screen, the integrated one first. On a laptop with two GPUs a wallpaper then does not
  keep the discrete one awake.

An adapter must be decent to simulate on: a discrete or integrated GPU, with compute shaders, at
least 9 storage buffers per shader stage, storage bindings of at least 128 MiB, and a passed
self-test. Software adapters (llvmpipe, lavapipe, SwiftShader) are slow and used only when asked
for, with `software` or by name, and so is an adapter the system calls neither discrete nor
integrated (such as an OpenGL one), by name. With no decent adapter, everything runs on the
processor, and windows still draw on whatever adapter can.

Windows' software renderer, WARP, is not supported, and never used to simulate (windows still draw
on it). The WARP that comes with Windows crashes while it compiles the world stepper's kernel (and
the soup hunt's), taking the program down with it before a self-test could turn it down. The fault
is in WARP's own shader compiler, which fails on a workgroup barrier after an early return or in an
`if` that ends the shader. The newer WARP Microsoft publishes for developers runs the world stepper
and explore batches bit for bit with the processor, and JETCARP's Windows CI runs the GPU tests on
it; it takes minutes to compile each of the soup hunt's kernels, though, and gets Generations rules
wrong in them, so the hunter never runs on WARP
([#321](https://github.com/jprodgers/jetcarp/issues/321)). WARP won't be supported until Windows
Update brings a WARP that compiles JETCARP's shaders, and even then only if supporting it breaks
nothing else; the kernels won't be reshaped to suit a WARP that can't.

JETCARP looks for adapters through every graphics interface wgpu has: Vulkan, Metal, DirectX 12
and OpenGL (OpenGL ES on Linux, where it draws when there is no Vulkan). On Windows it leaves
OpenGL out: a graphics card there is reached through DirectX 12 or Vulkan, which come first
anyway, and looking through OpenGL loads the card's OpenGL driver into every program that opens
the GPU. It looks there too when `JETCARP_GPU` or the saved setting names an adapter, which may be
one only OpenGL reaches (an old card with no DirectX 12 or Vulkan driver); the same card through
DirectX 12 or Vulkan is still taken first. wgpu's own `WGPU_BACKEND` overrides all of this
(`WGPU_BACKEND=gl` for OpenGL alone).

Worlds too large for an adapter's buffers, or over this computer's `gpu-world` limit (half the
GPU's memory at 16 bytes a cell, where the system reports it), step on the processor, with the
same result; `jetcarp doctor` shows the buffer limits, and `jetcarp limits` the rest (see
[This computer](this-computer.md)). The benchmark behind the JETCARP Hardware Score runs its soups
on the GPU too, checks every answer against the stored ones, and measures the smallest world the GPU
steps faster than every core.

## The self-test

The first time JETCARP uses an adapter, it steps one rule of every family the GPU runs, and a few
kinds random draws may miss (B0 rules, JACA with a blast, isotropic rules with B0 and Generations,
Larger than Life), on a square lattice, a lattice of several tile types and a Spectre patch, and
runs those the explore batches take as explore batches. It compares every hash with the
processor's. This takes about a tenth of a second (more on
a software adapter). The result is kept in `gpu-checks.json` in the settings folder, for that
adapter, driver and JETCARP version, so it runs again only after an update; on a fresh install,
JETCARP makes the settings folder for it. An adapter whose results differ is never used, and
`jetcarp doctor` says why.

`jetcarp doctor` gives the result with its time, and *kept from before* when it was read from
`gpu-checks.json` rather than run just now. It tests only the adapters the setting could choose:
with `JETCARP_GPU=llvmpipe`, not another software or OpenGL adapter. With the GPU simulation off for
both the app and the desktop background, doctor tests no adapter, and says under each `not used, and
not tested: the GPU simulation is off`.

## Checking

`jetcarp doctor` shows the setting, where it came from, and for each purpose the adapter, why it was
chosen, its limits and the self-test:

```text
GPU: auto (automatic)
  setting: /home/ada/.config/jetcarp/gpu.json (not saved)
  display: AMD Radeon 8060S Graphics (RADV STRIX_HALO) (radv), Vulkan (integrated)
      why: the adapter driving the screen (automatic)
      limits: 1864135 storage buffers per stage, 2 GiB storage bindings, 2 GiB buffers
      self-test: passed (141 ms, kept from before): 66 world rules and 46 explore batches matched the CPU
      the app and the desktop background simulate on it
  compute: AMD Radeon 8060S Graphics (RADV STRIX_HALO) (radv), Vulkan (integrated)
      why: the fastest adapter (automatic)
      ...
      runs, searches and scripts simulate on it
```

The app and the desktop background step their worlds on the adapter they draw with, the display
one; runs, searches and scripts use the compute one. On a laptop with two GPUs these can differ: an
Intel UHD driving the screen and an RTX for computing.

Its *Environment* list, at the end, shows `JETCARP_GPU` and wgpu's own variables, such as
`WGPU_BACKEND`, when they are set, since they change which adapters it finds.

`jetcarp doctor --json` gives the same as `gpu_policy`. A run's summary says where it ran, such as
`backend: gpu (AMD Radeon 8060S Graphics …)` or `cpu (32 threads)`.
