# The GPU

JETCARP runs on the GPU wherever it can, and treats the processor as the fallback. The results are
the same either way, bit for bit: a scene gives the same world on any GPU, from any vendor, and on
the processor. Only the speed differs.

## What runs on the GPU

- **Worlds** of Life-like, Generations, WireWorld, cyclic, rock-paper-scissors, Greenberg–Hastings,
  hodgepodge, Spectre-glider and priority rules, on lattices and on aperiodic patches. Other rules
  step on the processor, and a world that mixes them moves between the two as it needs.
- **Explore searches**: the rules a GPU can run go there in batches.
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

*Shared setting* under *GPU simulation* in the *Desktop background* pane writes it.

**The app and the background host each have a choice of their own,** over the shared setting:
*Follow the shared setting* (the default), *On* or *Off*. So one setting serves both, and either can
be set apart, for example the GPU on for the wallpaper and off for the app. The app's is *Run → GPU
simulation* (`gpu` in `app.json`; also `jetcarp app display --gpu follow|on|off` and the MCP
`app_display` tool); the host's is *GPU simulation* in the *Desktop background* pane (`gpu` in
`host.json`, which older files held as `true`, now read as follow, or `false`, now off).

`JETCARP_GPU` overrides all of it while it is set. It takes `off`, `auto` or `on` (or `0` and `1`),
or which adapter to use automatically: `integrated`, `discrete`, `software`, or part of a name. On a
laptop with an Intel UHD and an RTX 3050, `JETCARP_GPU=UHD` uses the Intel one (and leaves on or off
to the rest).

A command's own choice comes next: `--gpu` uses the GPU, `--cpu` only the processor (`jetcarp run`,
`jetcarp wallpaper`, `jetcarp search hunt|explore`, `jetcarp pile`, `jetcarp jobs resume`, `jetcarp
jobs refit` and `rescore`). The operations take the same as `gpu: true` or `false`; left out, the
setting decides. An explore search or a pile made with `--gpu` or `--cpu` keeps that choice for its
later runs.

In all, first to last: `JETCARP_GPU` (when it names off, auto or on), a command's `--gpu` or `--cpu`,
the app's or host's own choice, the shared setting, and automatic. An adapter that failed its
self-test is never used. `jetcarp doctor` says what the app and the host each do, and why:

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
for, with `software` or by name. Windows' software renderer (WARP) is never used. With no decent
adapter, everything runs on the processor, and windows still draw on whatever adapter can.

Worlds too large for an adapter's buffers step on the processor; `jetcarp doctor` shows the limits.

## The self-test

The first time JETCARP uses an adapter, it steps one rule of every family the GPU runs, on a square
lattice, a lattice of several tile types and a Spectre patch, and runs the same rules as explore
batches. It compares every hash with the processor's. This takes about a tenth of a second (more on
a software adapter). The result is kept in `gpu-checks.json` in the settings folder, for that
adapter, driver and JETCARP version, so it runs again only after an update. An adapter whose results
differ is never used, and `jetcarp doctor` says why.

## Checking

`jetcarp doctor` shows the setting, where it came from, and for each purpose the adapter, why it was
chosen, its limits and the self-test:

```text
GPU: auto (automatic)
  setting: /home/ada/.config/jetcarp/gpu.json (not saved)
  display: AMD Radeon 8060S Graphics (RADV STRIX_HALO) (radv), Vulkan (integrated)
      why: the adapter driving the screen (automatic)
      limits: 1864135 storage buffers per stage, 2 GiB storage bindings, 2 GiB buffers
      self-test: passed (101 ms, kept from before): 53 world rules and 33 explore batches matched the CPU
      the GPU simulation uses it
  compute: AMD Radeon 8060S Graphics (RADV STRIX_HALO) (radv), Vulkan (integrated)
      why: the fastest adapter (automatic)
      ...
```

`jetcarp doctor --json` gives the same as `gpu_policy`. A run's summary says where it ran, such as
`backend: gpu (AMD Radeon 8060S Graphics …)` or `cpu (32 threads)`.
