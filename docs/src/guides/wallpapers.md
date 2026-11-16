# Wallpapers, screensavers and the background host

JETCARP can keep cellular automata running on your screen in four ways:

- **Screensaver mode in the app** fills the screen (or several) with a scene until you press Esc.
  It runs while the app is open.
- **The background host** is a separate program, `jetcarp host`, that keeps scenes running with no app
  window: in a window of its own, or as the live desktop background behind your windows. It can start
  at login.
- **The system's screen saver.** On Windows and macOS, JETCARP can be added to the system's own list
  of screen savers. GNOME has no such list; JETCARP's lock-screen mode does the job there.
- **Still wallpapers**, pictures cut to fit your monitors, are covered in
  [Pictures and animations](pictures.md#desktop-wallpapers).

What any of these shows is a [screensaver profile](#screensaver-profiles), a [rotation](rotations.md)
(a list of items that change over time), a scene file or a Quick Start entry. For a first, step-by-step
setup, see [Your first live wallpaper](../tutorials/first-wallpaper.md).

> **Photosensitive seizure warning.** Full screen, as the desktop background or as a screen saver,
> some rules flicker the whole screen many times a second, which can trigger seizures in people with
> photosensitive epilepsy. None of these starts until the
> [warning](../getting-started/photosensitivity.md) has been answered, in the app or with
> `jetcarp setup`: `jetcarp host start`, `host wallpaper on`, `host login on` and
> `host saver install` refuse, a host started at login holds its screens still, and the screen
> saver does not start.

## What works on which system

| | Linux with GNOME | Other Linux desktops | Windows | macOS |
| --- | --- | --- | --- | --- |
| Screensaver mode in the app | Yes | Yes | Yes | Yes |
| Background host in a window | Yes | Yes | Yes | Yes |
| Live desktop background | With JETCARP's GNOME extension (GNOME Shell 50 or 51) | No: the host shows a window instead | Yes, under the desktop icons | Yes, behind the desktop icons, on every Space |
| Live lock screen | Yes, through the extension | No | No | No |
| In the system's screen saver list | No (use the lock screen instead) | No | Yes (`JETCARP.scr`) | Yes (`JETCARP.saver`) |
| Menu for the host | In the top bar, from the extension | None | In the notification area | In the menu bar |
| Still picture as the desktop background | Yes | No | Yes | Yes |
| Start the host at login | Yes | Yes (an autostart entry) | Yes | Yes |

None of this needs administrator rights: everything JETCARP sets up lives in your own folders and
settings, and [Uninstalling](uninstalling.md) undoes it.

## What has been tested

These features were tested by hand in November 2026 on three machines, on the code of JETCARP's
predecessor CART just before it took its present name. The renamed program has not yet been tested
on these machines again, except as the Fedora 45 line says.

- **Fedora 44, GNOME 50.6 on Wayland, one monitor:** the live desktop background, the top-bar menu,
  and the lock screen in both modes, including unlocking with a real password and stopping the host by
  force while locked (GNOME's own background came back and unlocking still worked). Several monitors
  have only been tried in a headless GNOME session. Since those tests, the part of the extension that
  keeps the host's windows out of the overview and the window switcher has been rewritten, and the new
  version has not yet been tested in a real GNOME session.
- **Fedora 45, GNOME 51.1 on Wayland, one monitor, with JETCARP 0.4.1's host and the extension as
  fixed for [#2](https://github.com/jprodgers/jetcarp/issues/2):** the live desktop background runs
  on the desktop. Pausing under a full screen app has been tried only in a headless GNOME 51.1
  session. The lock screen, the top-bar menu, and keeping the host's windows out of the overview
  and the window switcher have not been checked on GNOME 51 yet.
- **Windows 11 Home (version 26H2) on a laptop with two graphics adapters, with and without a second
  monitor:** the live desktop background under the icons (including after Explorer restarts and after
  a change of display scaling), the screen saver (install, preview, starting after the idle time,
  removal), the still picture, sleep, and the pauses on battery and when busy.
- **macOS 27.0.1 on an M2 Max MacBook Pro, with and without an external monitor:** the live desktop
  background, the screen saver (preview, starting after the idle time, two screens, sleep while it
  runs, removal), sleep with the lid closed, the still picture on one screen, and the pauses on battery
  and when busy.

Starting at a real login has been tried only on the Windows laptop, with JETCARP 0.4.2-rc.2: after
signing out and in, the host started with its saved rotation, 97 seconds after sign-in, once the
login entries before it had had their turn. JETCARP's launcher then kept Windows waiting on it too,
which held up the entries after it ([#78](https://github.com/jprodgers/jetcarp/issues/78)); the
launcher now starts the host and exits at once, which has not been tried at a real login yet. On
the Mac, with 0.4.2-rc.3 put in place of 0.4.1, the first login after the upgrade did not start the
host ([#99](https://github.com/jprodgers/jetcarp/issues/99), fixed since, which has not been tried
at a real login yet); once launchd had started the new program by hand, the next login did.

Not tested yet: starting at a real login on Linux (the entry is written and removed correctly, but
no one has logged in with it), a fresh boot on Windows, Linux desktops other than
GNOME, Intel Macs, and any other versions of these systems.

## Screensaver mode in the app

*View → Screensaver mode* (F12), or *Start the screensaver* in the *Screensaver* pane, hides every
control and runs the screensaver; Esc leaves it. It runs full screen on the app's monitor, or on the
monitors chosen in the *Display* pane. Like everything else in the app, it waits for the
[photosensitive seizure warning](../getting-started/photosensitivity.md) to be answered; `jetcarp
--screensaver` shows the warning first, then the screensaver.

*View → Screensaver settings…* opens the *Screensaver* pane:

- **Show** chooses what it runs: *One profile*, *A rotation*, *The current world*, *A scene file* or
  *A Quick Start entry* (such as `rule:B3/S23`).
- **While it runs** has the caption (*Show what is running (the rule and the grid)*, its corner, size,
  whether it includes the generation count, and how long it stays up after each new item; press I
  while the screensaver runs to show or hide it), *Keep the computer awake while it runs*, *Leave on
  any key or mouse movement (else Esc only)*, *Hide floating and popped-out windows*, *Start AI
  control*, and *Hide the pointer after* a delay.
- **System screen saver runs** (Windows and macOS) chooses the rotation of the system's own screen
  saver, with an *Install* button (see [below](#the-systems-screen-saver)).
- **Profiles** lists and edits the screensaver profiles (see [below](#screensaver-profiles)).

The background host, which runs the desktop background, has a pane of its own (see
[below](#the-desktop-background-pane)); a link under *While it runs* opens it. Profiles and rotations
are shared: the desktop background runs the same ones. Profiles are edited here, and rotations here
or in the *Desktop background* pane.

The app can also start straight into screensaver mode, for example from a desktop shortcut. Each of
these does so:

```sh
jetcarp --screensaver --rotation "Wolfram series" --quit-on-exit
```

```sh
jetcarp --screensaver --profile Life --monitors 0,1 --span --cell-px 3 --caption
```

```sh
jetcarp --screensaver --quickstart rule:B3/S23 --keep-awake
```

`--scene FILE` shows a scene, `--mirror` shows the same view on every monitor instead of one canvas
across them (`--span`, the default), and `--quit-on-exit` closes the app when the screensaver ends.
`jetcarp --help` lists every option. A running app with control switched on (*Tools → AI control…* or
`jetcarp --control`) can be steered from a terminal with `jetcarp app screensaver on --profile NAME`,
`jetcarp app next`, `jetcarp app profiles` and the other `jetcarp app` commands.

## Screensaver profiles

A *profile* is a named, complete setup for the screensaver: what to show (a scene kept in the profile,
a scene file or a Quick Start entry), and optionally a palette and colouring for every automaton, reseed
policies that replace the scene's own, the cell size in pixels, whether to rebuild the tiling to fill
the screen, the speed, the GPU simulation, and how many minutes it gets in a rotation. JETCARP has
eleven built in: Life, HighLife, Day & Night, Hexagonal Life, Brian's Brain, Star Wars, Triangles,
Snub square Life, Penrose waves, Cyclic spirals and Hodgepodge.

To make your own, set up a world as you like it and press *Save current world as profile*, or select
a profile and change it in the editor below the list. The easiest way to tune one is to press *Try in
this window*, adjust it with the *Reseed* pane, the palettes and the *Run* menu, and then press *Update
from the current world*. *Tune for this rule* runs small copies of the profile's world with each of the
reseed presets and scores how lively each keeps it. Editing a built-in profile saves your own copy
under the same name; *Revert to built-in* goes back.

Profiles are JSON files, one each, in the `screensavers` folder of the settings folder (see
[Folders](../reference/folders.md)). *Export…* saves one to share, *Import…* adds files, and *Reload*
reads them again after editing them by hand.

## The background host

The host runs as its own program, so its scenes keep going when the app closes, and only one runs at a
time. It shows one or more *screens*, each running a [rotation](rotations.md) either in a window or
as the desktop background, on a chosen monitor or across all of them.

**Start and stop it** from the *Desktop background* pane (*Start*, *Stop*, *Pause*, and ◀ ▶ for the
previous and next item), from its menu, or from the command line:

```sh
jetcarp host start --rotation "Wolfram series"
```

It prints:

```text
running, pid 863629, up 0.0s, 81 MB, GPU AMD Radeon 8060S Graphics (RADV STRIX_HALO)
screen 0: Wolfram series · W0 (1 of 256) · generation 5 · 57600 cells of 4 px · 60 fps, 60 generations a second · window · simulation: GPU (AMD Radeon 8060S Graphics (RADV STRIX_HALO) (radv), Vulkan)
caption: off
pacing: every generation (at least 30 fps, up to 60 fps, or the monitor's refresh rate if lower)
note: GPU: AMD Radeon 8060S Graphics (RADV STRIX_HALO) (radv), Vulkan; GPU simulation auto (automatic)
```

Move on to the next item:

```sh
jetcarp host next
```

It prints:

```text
running, pid 863629, up 3.0s, 81 MB, GPU AMD Radeon 8060S Graphics (RADV STRIX_HALO)
screen 0: Wolfram series · W1 (2 of 256) · starting · window · simulation: GPU (AMD Radeon 8060S Graphics (RADV STRIX_HALO) (radv), Vulkan)
caption: off
pacing: every generation (at least 30 fps, up to 60 fps, or the monitor's refresh rate if lower)
note: GPU: AMD Radeon 8060S Graphics (RADV STRIX_HALO) (radv), Vulkan; GPU simulation auto (automatic)
```

And stop it:

```sh
jetcarp host stop
```

It prints:

```text
stopped
```

`status` (and every command that changes something) says what each screen shows and where (a window,
or the desktop background on a monitor or on every monitor), how fast it draws and its scene runs
(see [below](#frame-rate-and-speed)), what its world steps on now (`simulation: GPU (…)` with the
adapter, or `simulation: CPU:` and why, such as `GPU simulation off, from the desktop background's
own setting`, or a world too small to be worth the GPU), the monitors, whether the caption is on,
how the screens are paced, and, while it is paused by hand, `paused: by hand`. A hold passes by
itself, so it is told apart from a pause: `held: on battery (resumes by itself)`, and likewise while
the processor is busy, while the desktop holds it (windows cover it, or the screen is locked or off)
and during a sweep of *Fit to this computer*. Its `note:` lines are the host's recent messages, such
as the GPU it took. Just after `next` the new item is *starting* until its world is built.

A host started before the [photosensitive seizure warning](../getting-started/photosensitivity.md)
was answered (at login after an upgrade, say) holds every screen at its first picture, and `status`
says so on a line of its own: `waiting: the photosensitive seizure warning has not been
accepted (…)`. The scenes start by themselves, within a few seconds, once it has been answered in
the app or with `jetcarp setup`. Meanwhile `start`, `wallpaper on`, `login on`, `saver install` and
*Fit to this computer* on screen are refused, with the warning.

| Command | Effect |
| --- | --- |
| `jetcarp host start [--rotation NAME] [--headless]` | Start the host; it keeps running after the command ends. `--rotation` is for this run only; `--headless` runs it with no window or menu. Refused until the photosensitive seizure warning has been answered |
| `jetcarp host stop`, `status`, `pause`, `resume` | Quit it, say what it shows, freeze it, carry on |
| `jetcarp host next`, `previous` `[--screen N]` | The next or previous item, on every screen or one |
| `jetcarp host rotation NAME [--screen N]` | Choose a screen's rotation |
| `jetcarp host fps N` | The frame rate: every screen draws at least this often (1 to 240; default 30). See [below](#frame-rate-and-speed) |
| `jetcarp host max-fps N\|refresh` | The most frames a second a screen showing every generation draws at (1 to 240; default 60), or `refresh` for its monitor's refresh rate however high. See [below](#frame-rate-and-speed) |
| `jetcarp host pacing every-generation\|low-power` | Whether a faster scene draws faster, showing every generation (the default), or every screen draws at the frame rate and no scene runs faster (see [below](#frame-rate-and-speed)) |
| `jetcarp host caption on\|off` | Show what is running in a corner, with `--corner`, `--size`, `--generation on\|off` and `--seconds` |
| `jetcarp host battery on\|off` | Pause while the computer runs on battery (default on) |
| `jetcarp host tune [--cpu-limit P] [--gpu-limit P] [--fps N]` | Fit the desktop background to this computer (see [below](#fit-to-this-computer)) |
| `jetcarp host scene [--screen N] [--save FILE]` | The scene a screen shows, its grid's size included, as a scene file |
| `jetcarp host busy PERCENT` | Pause while the whole processor stays at least this busy, as during a long search (default 80; 0 never). It carries on once the load is 20 points lower |
| `jetcarp host wallpaper`, `still`, `lock`, `gnome`, `saver`, `login` | See the sections below |

The settings are saved in `host.json` in the settings folder. The app and the command line both write
it and tell a running host to read it again, so a setting changed while the host is stopped applies
the next time it starts (*saved; the host is not running*). When the host reads it again, a screen
whose rotation, or a profile in it, has been saved since starts that rotation afresh, as saved.
Should the host's memory pass `max_memory_mb` in `host.json` (6,144 MB by default), it gives up the
biggest scene it shows for the next item, with a note; on Linux it first gives back the memory it
has freed but kept, as it also does after each sweep of *Fit to this computer*.
Besides pausing on battery and when busy, the host also pauses by itself while its desktop background
is completely hidden behind other windows, while the screen is locked, and while the display is
asleep (on macOS, whether locked or not). `jetcarp host
status --json` gives `paused` (paused by hand, or nothing in the rotation can be shown; *Resume*
carries on) with `paused_why`, and apart from it what the host is holding for, which passes by
itself: `held` (`battery`, `busy`, `covered`, `dark` or `tuning`) with `held_why`, the same in
words.

The host, the lock screen and the screen savers draw on the adapter driving the screen (on a laptop
with two GPUs, the integrated one, so the discrete one can sleep), and step a world on it once it has
16,000 cells, as the [GPU setting](gpu.md) says. *GPU simulation* in the *Desktop background* pane is
the host's own choice: *Follow the shared setting* (the default), *On* or *Off*, whatever the shared
setting says. *Shared setting*, under it, is the one the app, the command line and the screen savers
follow too (*Automatic*, *On* or *Off*). The host's log names the adapter it took.

On Windows and macOS the host has an icon in the notification area or the menu bar, and on GNOME the
extension adds one to the top bar. Its menu names what is showing and has *Next*, *Previous*,
*Paused*, *Show on the desktop*, *Rotation*, *Open JETCARP*, *Start at login* and *Quit* (on GNOME,
also *Start JETCARP host* while it is stopped).

## Frame rate and speed

Each scene runs at its own speed, in generations a second (a profile's *speed*, or a rotation's).
By default every generation is shown: each screen draws as fast as its scene steps, at least the
frame rate (`jetcarp host fps`, 30 unless you change it) and at most 60 frames a second (or the
monitor's refresh rate, if lower). Life at 60 generations a second draws 60 frames a second, one
generation each. A scene slower than the frame rate, or one running as fast as it can, draws at the
frame rate; one faster than 60 a second draws at 60, several generations a frame.
`jetcarp host max-fps` changes the most: `jetcarp host max-fps refresh` lets a screen draw at its
monitor's full refresh rate, 144 frames a second on some laptops, for a faster scene drawn faster
and a GPU that works harder (the pane's *Up to* choice is the same). With several screens, each draws at its
own rate, so a scene of 60 a second on one monitor does not make a scene of 30 on another draw
twice as often. A frame that comes late (a busy computer) still steps one generation: the scene
falls a little behind its speed rather than skip one.

*Low power* (`jetcarp host pacing low-power`, or the *Desktop background* pane's *Performance*
group) draws every screen at the frame rate instead, and caps each scene's speed there, so no frame
steps more than one generation: Life at 60 generations a second runs at 30, at 30 frames a second,
for half the drawing. A scene that runs as fast as it can runs at the frame rate too.

Drawing more slowly than a scene steps would show only every second or third generation, and an
oscillator whose period divides that would seem to stand still or blink slowly: Life's blinkers did
before 0.4.2. `jetcarp host status` gives each screen's frame rate and speed (`frame_rate` and
`speed` in `--json`, and `frames`, the frames each screen has drawn), and its *pacing* line the
choice and its rates: `pacing: every generation (at least 30 fps, up to 60 fps, or the monitor's
refresh rate if lower)`, or `pacing: low power, 30 fps (each scene at most 30 generations a
second)` (`pacing`, `fps` and `max_fps` in `--json`; `max_fps` is `null` with `max-fps refresh`).

## The Desktop background pane

*View → Desktop background settings…* opens the *Desktop background* pane, which holds everything
about the host. The *Display* and *Screensaver* panes link to it.

<img src="../images/tutorials/wallpaper-desktop-pane.webp" alt="The Desktop background pane, not running: Start; Show on the desktop; Rotation A classic a day on Every monitor, with Edit, New and Show just this scene under it; Add a screen; a link to the Screensaver pane, where profiles are made; Leave a still picture when it stops; On the lock screen; the GNOME extension's state with Install; Frames per second 30, GPU simulation Follow the shared setting (Automatic) and the Shared setting Automatic, Caption, Pause on battery, Pause while the processor is busier than 80 %, Start at login; Performance: the GPU, Show every generation (chosen) or Low power, Up to 60 frames a second, Processor limit 10 %, GPU limit 25 %, Smooth at the monitor's rate, Fit to this computer…, and Measure again by itself when the monitors change" width="700">

- **The host:** *Start* or *Stop*, *Pause*, ◀ ▶, and what each screen shows (its cell size too, and
  whether tuning chose it), why it is paused (*Paused: by hand.*), and what holds it, which passes
  by itself (*Held: the processor is busy (resumes by itself).*).
- **Where:** *Show on the desktop*, and for each screen its rotation and its monitor (*Every monitor*,
  or one of them). *Add a screen* puts another rotation on another monitor. Under each screen's
  rotation:
  - *Show just this scene* shows the scene open in the app on that screen, and nothing else. It
    saves the scene as a profile named after it (as *Save current world as profile* does) and a
    rotation of just that profile, of the same name, and chooses that rotation for the screen; a
    note under the buttons says what it saved and where. Pressed again for the same scene, it
    updates that profile from the world and keeps the rotation, rather than adding more. (A
    built-in profile, or a rotation of that name that shows something else, is left alone, and the
    new ones take the next free name: *Life 2*.)
  - *Edit* and *New* open the [rotation editor](rotations.md#make-a-rotation-in-the-app) under the
    screen. *Save* there chooses the rotation for that screen and leaves the screensaver's rotation
    as it was.

  Profiles are made and edited in the *Screensaver* pane, which the link below opens.
- **The desktop:** *Leave a still picture when it stops*, and on GNOME *On the lock screen*, how long
  the screen stays lit while locked, and the extension's state with an *Install* button.
- **How it runs:** *Frames per second*, *GPU simulation* (the host's own choice) and the *Shared
  setting* it can follow (see [The GPU](gpu.md#the-setting)), the *Caption*, *Pause on battery*,
  *Pause while the processor is busier than* a share, and *Start at login*.
- **Performance:** the GPU it uses, *Show every generation* or *Low power*, and *Up to* (60 frames a
  second or the monitor's refresh rate; see [Frame rate and speed](#frame-rate-and-speed)), the
  limits, *Fit to this computer…*, and the cards it leaves (see below).

The settings are the host's own `host.json`, as before; the pane writes them and tells a running host.

## Fit to this computer

How small a wallpaper's cells can be depends on the screen and the computer. On a lower-resolution
laptop panel, Life on squares at one pixel a cell is still easy to see; on a Retina screen, a
one-pixel cell is too small to see, and four million of them crawl; an aperiodic patch wants tiles
small but not tiny. So rather than guess, *Fit to this computer…* in the pane's *Performance* group,
or `jetcarp host tune`, shows you: it sweeps each scene of the desktop background through a ladder
of sizes, a couple of seconds each, on the desktop itself, measures each one, and then lets you
choose from a row of cards.

<img src="../images/guides/desktop-pane-cards.webp" alt="The Performance group after a sweep: Show every generation (chosen) or Low power, Up to 60 frames a second, the limits (processor 10%, GPU 25%, smooth at the monitor's rate), Fit to this computer…, and a row of cards for each scene. Life and a forest fire at 1, 2, 3, 4 and 6 pixels a cell, each card a picture of the screen at that size with its cells, processor, GPU and frame rate, all within the limits and 1 px recommended; Langton's ant, a placed pattern, at the same sizes drawn at 60 frames a second, 1 px over the limits and its own 4 px recommended" width="700">

**The sizes.** On a lattice the ladder is 1, 2, 3, 4, 6, 8, 12, 16, 24, 32, 48 and 64 pixels a cell,
in the screen's own pixels (on a screen of two pixels a point, 2 px is one point), and the scene's own
size. It runs from the finest that keeps the world within 4 million cells (and within the GPU's
buffers) to the coarsest sensible, 60 cells across the screen's shorter side. On an aperiodic tiling
it is the patch's levels, each fitted to the screen, whatever size that makes its tiles, and the
scene's own. The rules, palettes and colouring stay as they are: only the number of cells changes.

**The sweep** shows the coarsest size first and works finer, each size for two seconds, measured over
the last one and a half, each scene at the frame rate it draws at (see
[Frame rate and speed](#frame-rate-and-speed)). On the host it runs on the real screen (the other
screens pause meanwhile); move your windows aside to watch. It stops going finer once a size is
more than twice over a limit: anything finer would only crawl. Several scenes take a minute or two.

**The limits** decide which cards are marked *within the limits*:

- **Processor limit:** JETCARP's own processor time, as a share of the whole machine with every core
  counted (default 10%).
- **GPU limit:** JETCARP's own share of the graphics card's time (default 25%).
- **Smooth at:** the frame rate it must keep (default the monitor's refresh rate, or 60, and never more
  than the rate the scene draws at: Life at 60 generations a second must keep 60 frames a second, a
  scene of 30 the host's *Frames per second*, which is 30 unless you change it).

A size is within them when the processor and GPU stay under 80% of their limits, 95% of frames take
under 90% of the frame's time, and the scene keeps its speed. With several screens on the desktop,
each gets an equal share. Sizes over the limits are shown too, marked, and you can still choose them.

**The cards.** Each card has a picture of the screen at that size, shown at the screen's own pixels so
a one-pixel cell looks as it will; the size and the number of cells; the processor, GPU (*unknown*
where it could not be measured, see *What it measures* below) and frame rate; and whether it is
within the limits. *Recommended* marks the finest size within them, and each scene shows at its
recommended size unless you click another card. A scene that is a pattern placed on an empty field
(a glider gun, an ant from one cell) keeps its own size unless you choose another. *Use this size
for every scene on this tiling* gives the card's size to the screen's other scenes on the same
tiling, and *Use the recommended size for the rest* settles every scene you have not chosen. *Use
the scenes' own sizes* forgets the sweep.

```sh
jetcarp host tune --cpu-limit 1 --gpu-limit 3
```

It prints:

```text
sweeping 3 scenes on 1 screen at 30 frames a second, on AMD Radeon 8060S Graphics (RADV STRIX_HALO)
screen 0, scene 1 of 3: profile Life at 24 px (size 1 of up to 9)
  24 px, 6,360 cells: processor 0.1%, GPU 0.8%, 30 fps, frames 2.3 ms, on the processor — within the limits
...
  4 px, 230,400 cells: processor 0.8%, GPU 1.8%, 30 fps, frames 4.8 ms, on the GPU — within the limits
  3 px, 409,440 cells: processor 1.2%, GPU 2.5%, 30 fps, frames 4.5 ms, on the GPU, its own size — over: processor 1.2% (over 0.8%: the 1.0% limit, less 20%), GPU 2.5% (over 2.4%: the 3.0% limit, less 20%)
...
screen 0: profile Life: 9 sizes; recommended 4 px; shows at 4 px, 230,400 cells
...
screen 0: profile Penrose waves: 3 sizes; recommended level 5 (7.3 px); shows at level 5 (7.3 px), 35,000 cells
swept 3 scene(s) (GPU time from the system's count of JETCARP's GPU time); saved to ~/.config/jetcarp/host.json
```

**What it measures.** The processor time is JETCARP's own, on every thread. The GPU time is the
system's own count of JETCARP's GPU time on Linux; elsewhere it comes from the GPU's timestamp
queries, in the host's window and off screen alike, where the adapter has them. On Windows, an
adapter without them (such as an Intel UHD) is measured by Windows' own GPU engine counters for
JETCARP's process, the figures Task Manager shows, and held to the GPU limit like the others.
Failing those, the GPU share is not measured: timing each frame from its submission until the GPU is
done gives only an upper bound, which counts the time the work waits in the driver's queue (on
Windows it read about 90% for a scene the GPU spent 10% of its time on). The cards then show the
share as *GPU unknown*, each size's line from `jetcarp host tune` has `GPU not measured`, and the
pane and the sweep's last line say "GPU share not measured on this adapter (it has no GPU
timestamps): the GPU limit isn't applied"; the frame times are still held to the frame rate. With
`--json`, such a card has `"gpu_unmeasured": true` and no `gpu_percent`, and the last line has the
same words as `gpu_note`. The summary says which clock it used (with `--json`, `gpu_timing`:
`kernel`, `timestamps`, `counters` for Windows' engine counters, or `submission`). A world small
enough that the [GPU setting](gpu.md) leaves it on the processor is tried there, and held to the
processor limit like any other. On battery a sweep started by hand says that a computer on battery
may measure slower than plugged in.

**With the host stopped**, `jetcarp host tune` (and the pane) sweep off screen in a program of their own,
at each screen's size, drawing and stepping the way the host does, and the cards are the same.

**What is kept.** Each scene's cards and the size it shows at are saved in `host.json` as each scene's
sweep ends, with whether you chose it, and the pictures in the `tune` folder beside it. The cards
show only the scenes of the rotation each screen shows now: a size you chose for a scene of another
rotation stays saved, and is kept while it fits should that scene come back, but gets no row. A
screen given another rotation shows its scenes at their own sizes until it is swept again; on a
screen of another size, a lattice scene keeps its number of cells and a patch its level. With
*Measure again by itself when the monitors change* (on by default), the host sweeps again once a
swept screen's size changes and the monitors have stayed put for 15 seconds (not on battery, while
the processor is busy, or while paused by hand). A size you chose is kept if it is still within the
limits there; if not, the recommended one shows, with a note in the pane and in `jetcarp host
status`.

**A tuned scene is still exactly reproducible**, since a scene's grid size is part of it. The host's
status gives each screen's cell size and grid; `jetcarp host scene --save FILE` saves the scene a
screen shows, at the size it was built, and the still picture the host leaves carries that scene
inside it, as every picture JETCARP writes does.

| Option | Effect |
| --- | --- |
| `--cpu-limit P` | JETCARP's processor time, in percent of the whole machine (1 to 100; saved; default 10) |
| `--gpu-limit P` | JETCARP's share of the GPU's time, in percent (1 to 100; saved; default 25) |
| `--fps N` | The frame rate to keep (saved; default the monitor's refresh rate, or 60, at most the rate each scene draws at) |
| `--pick WHICH` | The size each scene is left at: `recommended` (the finest within the limits), `finest` (the finest shown), `own`, or a cell size in pixels. `own` and a size in pixels are placed whether or not the sweep showed them, so a pick undoes a sweep that stopped early (on an aperiodic patch, a size in pixels takes the nearest level shown). Without it, a size chosen before is kept while it fits, else the recommended one |
| `--crops DIR` | Save each size's picture here as a PNG |
| `--screen N` | Only this screen |
| `--canvas WxH` | The screen's size, when no host is running to say (else the size it was swept for before, else its monitor's) |
| `--trial-seconds S` | How long each size shows (default 2; the last three quarters are measured) |
| `--dry-run` | Measure and report, but save nothing |
| `--stop` | Stop the host's sweep (the scenes done stay saved) |
| `--json` | Each size's numbers, and each scene's cards, as lines of JSON |

Scripts and AI assistants reach it through the host: the MCP tool `host_control` takes
`{"action": "tune", "cpu_limit": 10, "pick": "recommended"}` (and `"on": false` to stop it), and its
status shows the progress under `tuning`; `{"action": "scene"}` gives a screen's scene.

## The live desktop background

Tick *Show on the desktop* in the *Desktop background* pane, or run `jetcarp host wallpaper on`. To
put different rotations on different monitors, set up one screen per monitor:

```sh
jetcarp host wallpaper on --screen 0 --monitor 0
jetcarp host wallpaper on --screen 1 --monitor 1 --rotation "Classics, hourly"
```

`jetcarp host status` lists the monitors by number. A screen given no monitor fills every monitor no
other screen claims, and `jetcarp host wallpaper off` turns the screens back into ordinary windows
(1280 by 720, each on its monitor) that you can move, resize or minimise.

**GNOME** needs JETCARP's GNOME Shell extension, *JETCARP live desktop*, which shows the host's
windows as the background of the desktop, the overview and the lock screen, and adds the top-bar menu.
It supports GNOME Shell 50 and 51.

```sh
jetcarp host gnome install
```

copies it into place and switches it on; log out and back in once to load it. Then

```sh
jetcarp host gnome status
```

says *The GNOME extension is running.*, and `jetcarp host gnome remove` removes it.

Upgrading JETCARP upgrades the extension: the host, when it starts, and any `jetcarp host` command
bring an installed extension up to the program's version, and leave it switched off if you switched it
off. GNOME reads extensions at login, so log out and back in once after upgrading; until then
`jetcarp host gnome status` says so: *The GNOME extension was updated to JETCARP 0.4.2's, but GNOME
still has JETCARP 0.4.1's loaded. Log out and back in once to load the new one.* An extension you
switched off reads *The GNOME extension is not running: it is switched off.*

The *Desktop background* pane shows the same status, with an *Install* button. Without the extension
the host shows a plain window and says why, so nothing is left covering the desktop. The extension
only places copies of the host's picture in GNOME's background; it takes no part in unlocking, and if
the host stops, GNOME's own background comes back.

On the lock screen (`jetcarp host lock`, or *On the lock screen* in the app) the choices are `off`
(GNOME's own background), `live` (the default: the scene, blurred behind the clock, with GNOME fading
the screen to black as usual) and `screensaver` (the scene stays sharp and lit until someone reaches
for the password box). In screensaver mode the screen and the computer stay awake while locked;
`--awake MINUTES` (default 30, 0 for never) lets the screen go dark after that long.

**Windows**: the host puts a window of its own under the desktop icons, one per monitor, with no
extension needed. **macOS**: one borderless window per monitor at the desktop's level, behind the
icons, on every Space and ignoring the mouse. The host lives in the menu bar: it has no Dock icon, even
while it shows a scene in a window.
**Other Linux desktops** are not supported yet: the host shows a window instead.

**A still picture.** With `jetcarp host still on` (*Leave a still picture when it stops*), the host
sets a picture of its scene as the desktop's own background when it stops, and while the live one
cannot be shown, through the system's own setting. It is the host that sets it, so a host that
crashes or is killed leaves your own picture, not the still. On Windows and macOS each monitor gets a picture of
what it showed; GNOME has one background for every monitor, so there it is the primary monitor's.
Your own picture (each monitor's) is remembered and put back when the still or the live background
is turned off, unless you have chosen another picture in the meantime. It is off by default.

## The system's screen saver

`jetcarp host saver install` adds JETCARP to the system's list of screen savers and opens the system's
screen saver settings, with JETCARP to choose. `--rotation NAME` chooses what it runs (otherwise the
first screen's rotation); in the app, the same choice is *System screen saver runs* in the
*Screensaver* pane, with an *Install* button.
`jetcarp host saver status` says whether it is installed, and `remove` takes it out.

Until the [photosensitive seizure warning](../getting-started/photosensitivity.md) has been
answered, `install` refuses, and on Windows a screen saver installed before shows nothing but the
warning, in a message box. A macOS screen saver installed before the warning existed keeps running
as it was built until it is installed again.

- **Windows.** A copy of JETCARP's windowless launcher is made at `%LOCALAPPDATA%\JETCARP\JETCARP.scr`
  and selected in *Screen Saver Settings*, as right-clicking a `.scr` file and choosing *Install*
  would. It reads JETCARP's settings each time it starts, so changes apply at once, and its *Settings*
  button opens the JETCARP app. It needs `jetcarpw.exe`, the windowless launcher, beside
  `jetcarp.exe`; a build from source makes it with
  `cargo build --release -p jetcarp --features windows-launcher`. Removing it also clears the
  selection if JETCARP was the chosen screen saver. Close *Screen Saver Settings* first (`install`
  opens it): while the dialog's preview runs the `.scr`, Windows won't let it be deleted, and
  `remove` says so and changes nothing.
- **macOS.** macOS runs screen savers in a sandbox that cannot read JETCARP's settings, so installing
  builds `~/Library/Screen Savers/JETCARP.saver` with the rotation and every item's scene copied
  inside it, signs it for this machine, and opens System Settings where screen savers are chosen
  (*System Settings, Wallpaper, Screen Saver…* on macOS 27). Install it again after changing the
  rotation. It needs `libjetcarp_saver.dylib` beside the `jetcarp` program; a build from source makes
  it with `cargo build --release -p jetcarp-saver`. macOS prepares the saver's full-screen views ahead
  of time, as soon as it is chosen and after each run; they hold their first picture, and the scene
  starts moving only once the screen saver has really started. The preview in System Settings always
  moves. `remove` deletes the bundle and puts back the screen saver macOS had chosen before JETCARP
  was installed, on every display and Space that still chooses JETCARP; with nothing recorded (an
  install by 0.4.0-rc.1), they show your desktop picture as the screen saver instead. If System
  Settings is open, it may keep listing JETCARP until you quit and reopen it.
- **GNOME** has no list of screen savers. Use the lock screen's screensaver mode instead:
  `jetcarp host lock screensaver --awake 30`.

## Start at login

`jetcarp host login on` (or *Start at login*, in the *Desktop background* pane or the host's menu)
makes the host start when
you log in; `jetcarp host login off` stops that. Turning it on waits for the
[photosensitive seizure warning](../getting-started/photosensitivity.md) to be answered. It uses each system's own per-user mechanism:

| System | What is written |
| --- | --- |
| Linux | `~/.config/autostart/jetcarp-host.desktop` |
| macOS | `~/Library/LaunchAgents/com.jimmieprodgers.jetcarp.host.plist`, registered with launchd at once, which starts the host unless one is running |
| Windows | The value `JETCARP` under `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run` |

The entry runs `jetcarp host serve --login` (on Windows through `jetcarpw.exe` when it is there, so
no console window opens) at the path the program has now. Windows runs these entries one at a time
and waits for each, so at login `jetcarpw.exe` starts the host and exits at once, rather than
waiting for it as it does from a shortcut. If you move the program, turn the setting off and on
again. A host already running is left alone, except on macOS one that launchd started from the
entry, which `host login on` restarts.

On macOS, launchd refuses at login a program that has changed since the entry was registered, and
each Homebrew build is signed on its own, so after an upgrade the desktop background would not come
back at your next login ([#99](https://github.com/jprodgers/jetcarp/issues/99)). JETCARP registers
the entry again for the new version the first time the app opens, `jetcarp host start` runs or
`jetcarp host wallpaper on` runs; a host that launchd started from the old version is stopped and
the new version started in its place. So after upgrading, open the app or run `jetcarp host start`
once before you next log in.

Started at login, the host shows only the screens placed on the desktop, and opens no windows: a
screen in a window (after `jetcarp host wallpaper off`, say) waits unseen, and with no desktop screens
the host just waits in the notification area or the menu bar (on GNOME, the top bar's menu). This
holds for the whole run; `jetcarp host stop` and `jetcarp host start` start a host that opens windows
as usual. `jetcarp host status` lists a waiting screen as *not shown (started at login)*.

More: [Rotations](rotations.md), [Reseeding](reseeding.md), [Folders](../reference/folders.md) and
[Uninstalling](uninstalling.md).
