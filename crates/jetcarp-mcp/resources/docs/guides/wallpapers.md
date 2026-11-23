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
  on the desktop. With 0.4.2's release candidates, in headless GNOME 51.1 sessions only: pausing
  under a full screen app, two monitors, a monitor taken away and brought back, and eight hours
  through a rotation's scenes. The lock screen, the top-bar menu, and keeping the host's windows out
  of the overview and the window switcher have not been checked on GNOME 51 yet.
- **Windows 11 Home (version 26H2) on a laptop with two graphics adapters, with and without a second
  monitor:** the live desktop background under the icons (including after Explorer restarts and after
  a change of display scaling), the screen saver (install, preview, starting after the idle time,
  removal), the still picture, sleep, and the pauses on battery and when busy.
- **macOS 27.0.1 on an M2 Max MacBook Pro, with and without an external monitor:** the live desktop
  background, the screen saver (preview, starting after the idle time, two screens, sleep while it
  runs, removal), sleep with the lid closed, the still picture on one screen, and the pauses on battery
  and when busy.

Starting at a real login has been tried on the Windows laptop and the Mac:
- **Windows, with 0.4.2-rc.2:** after signing out and in, the host started with its saved rotation,
  97 seconds after sign-in, once the login entries before it had had their turn. JETCARP's launcher
  then kept Windows waiting on it too, which held up the entries after it
  ([#78](https://github.com/jprodgers/jetcarp/issues/78)); the launcher now starts the host and
  exits at once.
- **Windows, with 0.4.2-rc.5,** in a new account, put in place of 0.4.1 installed with Scoop: the
  host came back at sign-in as the new version. It held until the photosensitive seizure warning
  was answered in the app, then ran with the saved settings.
- **The Mac:** with 0.4.2-rc.3 put in place of 0.4.1, the first login after the upgrade did not
  start the host ([#99](https://github.com/jprodgers/jetcarp/issues/99)). With that fixed, upgrading
  from rc.3 to rc.4 restarted the host as rc.4 at once, and it came back as rc.4 at the next login.

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

A screen whose world falls more than a tenth short of its scene's speed says what it reaches, as in
`30 fps, 3.90 of 30 generations a second`, and a scene that runs as fast as it can says how fast
that is: `30 fps, as fast as it can (412 generations a second)` (`measured_speed` in `--json`).

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

On Linux and macOS, SIGTERM (what `kill` sends, launchd at logout on the Mac, and a Linux session
as it ends), SIGHUP and Ctrl-C stop the host the same way; on Windows, signing out, shutting down
and `taskkill` without `/F` do, and Ctrl-C or closing the console of a host started in one
(`jetcarp host serve`). So a logout stops it cleanly: its port and token file and its memory
record leave the [runtime folder](../reference/folders.md#runtime-folder), and the
[still picture](#still-picture), if you asked for one, is left on the desktop. A second signal
ends it at once.

`status` (and every command that changes something) says what each screen shows and where (a window,
or the desktop background on a monitor or on every monitor), how fast it draws and its scene runs
(see [below](#frame-rate-and-speed)), what its world steps on now (`simulation: GPU (…)` with the
adapter, or `simulation: CPU:` and why, such as `GPU simulation off, from the desktop background's
own setting`, or a world too small to be worth the GPU), the monitors, whether the caption is on,
how the screens are paced, and, while it is paused by hand, `paused: by hand`. A hold passes by
itself, so it is told apart from a pause: `held: on battery (resumes by itself)`, and likewise while
the processor is busy, while the desktop holds it (windows cover it, or the screen is locked or
off), during *Fit to this computer*, and while JETCARP measures this computer
([While it measures](this-computer.md#while-it-measures)). Its `note:` lines are the host's recent messages, such
as the GPU it took. Just after `next` the new item is *starting* until its world is built. A host
that has not opened its GPU yet (started at login, with nothing shown so far) says `GPU not opened
yet`, not `none`. A screen kept for a monitor that isn't connected moves to the main monitor if
nothing else shows there; otherwise it waits, neither stepping nor drawing, and `status` says
`(not connected: waiting for it, neither stepping nor drawing)` until the monitor comes back.

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
| `jetcarp host scene [--screen N] [--save FILE]` | The scene a screen shows, its grid's size included, as a scene file |
| `jetcarp host busy PERCENT` | Pause while the whole processor stays at least this busy, as during a long search (default 80; 0 never). It carries on once the load is 20 points lower |
| `jetcarp host tune [--cpu-limit P] [--gpu-limit P] [--fps N]` | Fit the desktop background to this computer, in a few seconds (see [below](#fit-to-this-computer)) |
| `jetcarp host wallpaper`, `still`, `lock`, `gnome`, `saver`, `login` | See the sections below |

The settings are saved in `host.json` in the settings folder. The app and the command line both write
it and tell a running host to read it again, so a setting changed while the host is stopped applies
the next time it starts (*saved; the host is not running*). When the host reads it again, a screen
whose rotation, or a profile in it, has been saved since starts that rotation afresh, as saved.
The host shares JETCARP's one pool of memory, the part reserved for JETCARP, with the app and every
other JETCARP program (see [This computer](this-computer.md#sharing-the-pool)). Should its memory
pass what the rest of JETCARP leaves it (or its own `host-memory` limit, or a `max_memory_mb` edited
by hand in `host.json`, if lower), it gives up the biggest scene it shows for the next item, with a
note; on Linux and macOS it first gives back the memory it has freed but kept, as it also does after
each *Fit to this computer*. Its worlds may have at most
the computer's `host-cells` (64 million cells on a 16 GB computer), and no more than fit in what is
left of the pool. On macOS its memory is its physical footprint, the figure Activity Monitor
shows.
Besides pausing on battery and when busy, the host also pauses by itself while its desktop background
is completely hidden behind other windows, while the screen is locked, and while the display is
asleep. On macOS it holds whenever a wallpaper is wanted and none of it can be seen: the session
locked, every display asleep (locked or not), or no wallpaper window there at all. `jetcarp host
status --json` gives `paused` (paused by hand, or nothing in the rotation can be shown; *Resume*
carries on) with `paused_why`, and apart from it what the host is holding for: `held` (`battery`,
`busy`, `covered`, `dark`, `tuning` or `measuring`, which pass by themselves, and `warning` until the
photosensitive seizure warning has been answered) with `held_why`, the same in words.

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

<img src="../images/tutorials/wallpaper-desktop-pane.webp" alt="The Desktop background pane, not running: Start; Show on the desktop; Rotation A classic a day on Every monitor, with Edit, New and Show just this scene under it; Add a screen; a link to the Screensaver pane, where profiles are made; Leave a still picture when it stops; On the lock screen; the GNOME extension's state with Install; Frames per second 30, GPU simulation Follow the shared setting (Automatic) and the Shared setting Automatic, Caption, Pause on battery, Pause while the processor is busier than 80 %, Start at login; Performance: the GPU, Show every generation (chosen) or Low power, Up to 60 frames a second, Processor limit 10 %, GPU limit 25 %, Smooth at the monitor's rate, a line saying the limits are for the whole desktop, each screen getting a share by its pixel area, Fit to this computer…, Measure again by itself when a monitor changes, and Not fitted: each scene shows at its own cell size; the status line notes the strobe guard softening the open scene's flashes" width="700">

- **The host:** *Start* or *Stop*, *Pause*, ◀ ▶, and what each screen shows (its cell size too, and
  whether *Fit to this computer* chose it), why it is paused (*Paused: by hand.*), and what holds it,
  which passes
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
  limits, *Fit to this computer…*, and the cards it leaves (see
  [Fit to this computer](#fit-to-this-computer)).

The settings are the host's own `host.json`, as before; the pane writes them and tells a running host.

## Cell sizes

How small a scene's cells can be depends on the screen and the computer. On a lower-resolution
laptop panel, Life on squares at one pixel a cell is still easy to see; on a Retina screen a
one-pixel cell is too small to see; and the more cells, the harder the processor and the GPU work.
Each scene of the desktop background shows at its own cell size until
[Fit to this computer](#fit-to-this-computer) finds one for this computer. To change a scene's own
size:

- **A profile** has its own: *Cells*, in the *Screensaver* pane's profile editor (see
  [Screensaver profiles](#screensaver-profiles)), with *Rebuild to fill* to rebuild a periodic
  tiling to cover the screen at that size. It holds wherever the profile shows.
- **A rotation** sets one in the [rotation editor](rotations.md#make-a-rotation-in-the-app) (*Edit*
  under a screen's rotation in the *Desktop background* pane): *Settings for every item*, then *Cell
  size*, for every item that is not a stored profile (4 unless set), and *Settings for this item*,
  then *Cell size*, for one item, a profile included.
- **By hand**, a rotation file's `look` takes `cell_px`, for the whole rotation or for one item (see
  [Rotation files](rotations.md#rotation-files)), and a profile's file has `cell_px` too.

A cell size is in points, 1 to 64: on a screen of two pixels a point, 1 is two pixels. On a periodic
tiling rebuilt to fill the screen (as profiles and rotations do unless told not to), it sets how many
cells cover the screen. An aperiodic patch (Penrose, the hat, the Spectre) keeps its scene's level,
and so its number of tiles: they show at the cell size, or larger where the patch would not fill
the screen at that size. A running host shows a rotation or profile saved again at once.

Finer cells cost more processor and GPU time, so if the desktop background works the computer too
hard, give its scenes bigger cells, or choose *Low power* (see
[Frame rate and speed](#frame-rate-and-speed)).

The host's status gives each screen's cell size and grid (`cell_px` and `grid` with `--json`).
`jetcarp host scene --save FILE` saves the scene a screen shows, at the size it was built, so it
replays exactly, and the still picture the host leaves carries that scene inside it, as every
picture JETCARP writes does.

## Fit to this computer

*Fit to this computer…*, in the pane's *Performance* group, or `jetcarp host tune`, finds how fine
each screen's scenes can be on this computer, in a few seconds, and lets you choose.

<img src="../images/guides/desktop-pane-cards.webp" alt="The Desktop background pane after a fit of Until it settles on a 2560 by 1440 screen. In its Performance group: Show every generation (chosen) or Low power, Up to 60 frames a second, Processor limit 10 %, GPU limit 25 %, Smooth at the monitor's rate, a line saying the limits are for the whole desktop, with several screens each getting a share by its pixel area, Fit to this computer…, Use the scenes' own sizes, Measure again by itself when a monitor changes, and Use the recommended size for the rest. Below, Life (Quick Start rule:B3/S23), measured as the most demanding scene that steps on the GPU, has four cards, each a picture of the screen at that size: 1 px, 3,686,400 cells, processor 12%, GPU 21%, over the limits; 2 px, 921,600 cells, processor 3.3%, GPU 6.4%, within the limits and Recommended, outlined as the size it shows at; 3 px, its own size, and 6 px, both within the limits. Under the cards, a line saying the other scenes take their sizes from the scene measured on their device, by their cost, at most twice its cells unless you choose, and the other two scenes, B36/S23 and B368/S245, each at 2 px, recommended, in a menu of sizes, with the same work a cell as the measured scene" width="700">

**One scene on each device.** A fit measures the most demanding scene of each screen's rotation
that steps on the GPU, and the most demanding that steps on the processor: the one with the most
work a cell, from its tiling's neighbours, its rules (and their passes a generation), its speed and
its drawing, with the GPU's steps counted at the GPU's speed. A rotation whose scenes all step on
one device has one scene measured; one that mixes them (Life on the GPU beside the Wolfram series'
rows on the processor, say) has two, each found by its own binary search, one after the other. A
rule series, such as the Wolfram series, is one scene, measured by its first rule. A scene that is a
pattern placed on an empty field (a glider gun, an ant from one cell), or that is not rebuilt to fill
the screen, keeps its own size, and is not one of those measured.

**A few sizes.** The measured scene is shown at a size for a second (the first quarter to warm up,
the rest measured), and a binary search chooses the next: finer if it was within the limits,
coarser if not. A fit's first size warms up half a second longer, so a fit made just after the host
started, with its shaders and buffers still new, measures as later ones do. Four or five sizes
settle it, six at most. The first size tried is the one you chose before, else the one recommended
last time, else a guess from this computer's [JETCARP Hardware Score](this-computer.md) (its speed
on one core, and its GPU's), else the middle of the ladder. A good first guess settles it in two
sizes: that one, and the one next to it.

**Every screen at once.** With several screens on the desktop, every screen's measured scene shows
at once, each at its own size, so a fit of two screens takes little longer than a fit of one. On
the computer JETCARP is developed on, a first fit took 3.2 s on one 2560 × 1440 screen and 5.4 s on
that screen beside a 1920 × 1080 one at 120 Hz, and a fit repeated 2.0 s and 3.1 s. A rotation
mixing devices takes about twice as long the first time, its two scenes measured one after the
other: 9.6 s on the one screen, and 2.8 s repeated (see [Testing](../about/testing.md)).

**The size found serves the whole rotation.** Every other scene takes its size from the scene
measured on its own device, by their work a cell: a scene with half the work a cell may have twice
the cells, so it goes finer; one with the same work shows at about the same size. How many cells
that is comes from where the measured scene's loads cross the limits: between the finest size
within them and the next finer one over them, or, where nothing finer was over them, its finest
size scaled by how far under the limits it was. Should no scene on a scene's device have been
measured (its measured scene could not be built, say), it is sized from the other device's by the
estimate alone, so it goes no finer than that scene's own size allows, and its line says so.

**Twice the measured scene's cells at most, unless you choose.** Where the measured scene had room
to spare at its finest size (a Penrose patch has only three levels on a 2560 × 1440 screen), the
estimate of how many of its cells would reach the limits goes no further than twice the cells it
was measured at: past that, it is a guess. So a scene with the same work a cell goes no finer than
twice the measured scene's cells, and one with half the work four times. A scene held there says so
where its size is shown: in the pane, "held to twice the measured scene's cells: choose a finer size
to go further", and in `jetcarp host tune`, "held to twice the measured scene's cells; choose a
finer size to go further". Choosing a finer size, from its menu in the pane or a finer card for the
measured scene, goes further, and so does `--pick` with `finest`, `own` or a cell size; `--pick
recommended` keeps to the limit.

**The sizes.** On a lattice the ladder is 1, 2, 3, 4, 6, 8, 12, 16, 24, 32, 48 and 64 pixels a cell,
in the screen's own pixels (on a screen of two pixels a point, 2 px is one point), and the scene's own
size. It runs from the finest that keeps the world within 4 million cells (and within the GPU's
buffers) to the coarsest sensible, 60 cells across the screen's shorter side. On an aperiodic tiling
it is the patch's levels, each fitted to the screen, whatever size that makes its tiles. The rules,
palettes and colouring stay as they are: only the number of cells changes.

**The limits** decide which sizes are *within the limits*:

- **Processor limit:** JETCARP's own processor time, as a share of the whole machine with every core
  counted (default 10%).
- **GPU limit:** JETCARP's own share of the graphics card's time (default 25%).
- **Smooth at:** the frame rate it must keep (default the monitor's refresh rate, or 60, and never more
  than the rate the scene draws at: Life at 60 generations a second must keep 60 frames a second, a
  scene of 30 the host's *Frames per second*, which is 30 unless you change it).

The limits are for the whole desktop: with several screens, each gets a share by its pixel area. A
2560 × 1440 screen beside a 1920 × 1080 one has 64% of the area, so 6.4% of a 10% processor limit,
and the other 3.6%; together they stay within the 10%. The pane says how they are shared once a fit
has measured more than one screen, and so does the first line of `jetcarp host tune`.

A size is within the limits when the processor and GPU stay under 80% of the screen's share, 95% of
frames take under 90% of the frame's time, and the scene keeps its speed (to within a generation).
A frame's time is its work, stepping and drawing it and handing it to the GPU, not the wait for the
screen to take it: on a monitor beside another of another refresh rate, that wait can follow the
other monitor, whatever the size. Sizes over the limits are shown too, marked, and you can still
choose them.

**At the limit.** A measure varies a little from one fit to the next. A size within a tenth of a
limit, either side, is marked *at the limit* on its card. A fit repeated with nothing changed keeps
the size it recommended last time while that stays within a tenth of the limits, and a finer size
takes its place only when clearly within them; the other scenes keep their sizes the same way. So the
same fit recommends the same sizes.

**The cards.** The measured scene has a row of cards, one for each size tried: a picture of the
screen at that size, shown at the screen's own pixels so a one-pixel cell looks as it will; the size
and the number of cells; the processor, GPU (*unknown* where it could not be measured, see *What it
measures* below) and frame rate; and whether it is within the limits. The row scrolls sideways, with
a trackpad's sideways swipe or with shift and the mouse wheel, and opens at the card the scene shows
at. *Recommended* marks the finest size within the limits, and each scene shows at its recommended
size unless you choose another. Click a card to choose it: the rest of the rotation follows it.
A rotation that mixes devices has a row for each measured scene, each saying which device it
steps on. Below the cards, each other scene has its size, a menu of the sizes it can take, and how
its work a cell compares with that of the scene measured on its device. *Use this size for every
scene on this tiling* gives a chosen card's size to the screen's other scenes on the same tiling,
*Use the recommended size for the rest* settles every scene you have not chosen, and *Use the
scenes' own sizes* forgets the fit.

```sh
jetcarp host tune
```

It prints (here, off screen, on one 2560 × 1440 screen):

```text
fitting 7 scenes on 1 screen at 30 to 60 frames a second, on AMD Radeon 8060S Graphics (RADV STRIX_HALO); limits: screen 0 (2560x1440): processor 10%, GPU 25%; screen 0 measures profile Penrose waves, the most demanding of its 7 scenes
screen 0: profile Penrose waves at level 5 (size 1 of up to 3)
  level 5 (7.3 px), 35,000 cells: processor 0.2%, GPU 1.6%, 29 fps, frames 1.7 ms, on the GPU — within the limits
screen 0: profile Penrose waves at level 6 (size 2 of up to 3)
  level 6 (2.6 px), 280,000 cells: processor 0.7%, GPU 2.7%, 29 fps, frames 2.2 ms, on the GPU, its own size — within the limits
screen 0: profile Life: sized from the measured scene, at 0.96 times its work a cell; shows at 3 px (held to twice the measured scene's cells; choose a finer size to go further)
screen 0: Quick Start langtons-ant: a placed pattern, at its own size unless chosen; shows at 4 px
...
screen 0: profile Penrose waves: measured on the GPU, 2 sizes; recommended level 6 (2.6 px); shows at level 6 (2.6 px), 280,000 cells
screen 0: profile Hodgepodge: sized from the measured scene, at 0.96 times its work a cell; shows at 3 px (held to twice the measured scene's cells; choose a finer size to go further)
fitted 7 scene(s) in 3.1 s, 2 sizes tried (GPU time from the system's count of JETCARP's GPU time); saved to ~/.config/jetcarp/host.json
```

Here the Penrose patch had room to spare at its finest level, so Life and Hodgepodge, with about
the same work a cell, are held at 3 px; `--pick 2`, or choosing 2 px for them in the pane, shows
them finer. A rotation mixing devices says which scene it measures on each in its first line:
`screen 0 measures B3/S23 on the GPU and W30 on the processor, the most demanding of its 3 scenes
on each`.

With two screens, the first line also says how the limits are shared: `the 10% processor and 25%
GPU limits are one for the whole desktop, shared between the screens by their pixel area: screen 0
(2560x1440, 64% of the area): processor 6.4%, GPU 16%; screen 1 (1920x1080, 36% of the area):
processor 3.6%, GPU 9.0%`. With `--json`, each line is a JSON object; the first has each screen's
share and the scenes it measures (`measured`) in `shares`, and the last how long the fit took
(`seconds`) and how many sizes it tried (`trials`).

**Choosing without measuring.** `jetcarp host tune --pick own` (or `recommended`, `finest`, or a cell
size in pixels such as `--pick 4`) places sizes from the last fit while it is still current: found
by this JETCARP, for each screen's rotation and every one of its scenes, on the same monitor (its
size, pixels per point and refresh rate), within the same limits and on the same GPU. Nothing is
measured, and it takes a moment. Otherwise it fits first; `--fresh` fits again anyway.

**What it measures.** The processor time is JETCARP's own, on every thread. With several screens
measured at once, the process's processor and GPU time is shared between them by how long each
one's frames took. The GPU time is the system's own count of JETCARP's GPU time on Linux and on
macOS (on the Mac, the task's GPU time that Activity Monitor shows), used once a fit has seen it
move; elsewhere it comes from the GPU's timestamp queries, where the adapter has them, off screen. On
Windows, an adapter without them (such as an Intel UHD) is measured by Windows' own GPU engine
counters for JETCARP's process, the figures Task Manager shows, and held to the GPU limit like the
others. Failing those, the GPU share is not measured: timing each frame from its submission until
the GPU is done gives only an upper bound, which counts the time the work waits in the driver's
queue (on Windows it read about 90% for a scene the GPU spent 10% of its time on). The cards then
show the share as *GPU unknown*, each size's line from `jetcarp host tune` has `GPU not measured`, and
the pane and the fit's last line say "GPU share not measured on this adapter (it has no GPU
timestamps): the GPU limit isn't applied"; the frame times are still held to the frame rate. With
`--json`, such a card has `"gpu_unmeasured": true` and no `gpu_percent`, and the last line has the
same words as `gpu_note`. The last line says which clock it used (with `--json`, `gpu_timing`:
`kernel`, `timestamps`, `counters` for Windows' engine counters, or `submission`). A world small
enough that the [GPU setting](gpu.md) leaves it on the processor is tried there, and held to the
processor limit like any other. On battery a fit started by hand says that a computer on battery
may measure slower than plugged in.

**With the host stopped**, `jetcarp host tune` (and the pane) fit off screen in a program of their
own, at each screen's size, drawing and stepping the way the host does, and the cards are the same.

**What is kept.** Each screen's sizes are saved in `host.json` when the fit ends, with its share of
the limits, its monitor and whether you chose each size, and the pictures in the `tune` folder
beside it; a fit stopped part way saves nothing. The cards show only the scenes of the rotation each
screen shows now: a size you chose for a scene of another rotation stays saved, and is kept while it
fits should that scene come back, but gets no row. A screen given another rotation shows its scenes
at their own sizes until it is fitted again; on a screen of another size, a lattice scene keeps its
number of cells and a patch its level. With *Measure again by itself when a monitor changes* (on by
default), the host fits again only the screens whose monitor changed (its size, pixels per point or
refresh rate), once the monitors have stayed put for 15 seconds (not on battery, while the processor
is busy, or while paused by hand); the other screens keep their fit, each still held to its share of
the whole desktop. A size you chose is kept if it is still within the limits there; if not, the
recommended one shows, with a note in the pane and in `jetcarp host status`.

**A fitted scene is still exactly reproducible**, since a scene's grid size is part of it. The host's
status gives each screen's cell size and grid; `jetcarp host scene --save FILE` saves the scene a
screen shows, at the size it was built, and the still picture the host leaves carries that scene
inside it, as every picture JETCARP writes does.

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

<a id="still-picture"></a>**A still picture.** With `jetcarp host still on` (*Leave a still picture when it stops*), the host
sets a picture of its scene as the desktop's own background when it stops (a logout stops it too),
and while the live one cannot be shown, through the system's own setting. It is the host that sets
it, so a host that crashes or is killed outright (`kill -9`) leaves your own picture, not the still. On Windows and macOS each monitor gets a picture of
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
warning, in a message box. A macOS screen saver installed before the warning existed is built again,
with the strobe guard on, the first time this version of JETCARP runs (see below).

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
  inside it, and the caption's font (a font file chosen in *Settings → Accessibility* is copied in
  whole; see [Font](accessibility-settings.md#font)), signs it for this machine, and opens System
  Settings where screen savers are chosen (*System Settings, Wallpaper, Screen Saver…* on
  macOS 27). It needs `libjetcarp_saver.dylib` beside the `jetcarp` program; a build from source
  makes it with `cargo build --release -p jetcarp-saver`.

  **It is kept up to date.** When something the bundle holds changes (the rotation or a scene in
  it, the caption or its font, the strobe guard's setting, the answer to the seizure warning, or the
  program itself, after an upgrade), JETCARP builds the bundle again as installing does: in a fresh
  folder, signed, then put in the old one's place, so no file macOS may be using is changed where it
  is. There is no need to install it again. The app and the desktop background check every couple
  of seconds while they run, an MCP server too, and the command line once as each command ends, so
  a change made any of those ways reaches the saver, and so does one made in the settings files by
  hand, the next time one of them runs. The check looks only at the size and time of each file the
  bundle is baked from; it is baked again only after a change, and built again only if it would come
  out different, on a thread of its own in the app and the desktop background. A Quick Start entry
  the rotation shows is looked up again in the library whenever the bundle is built, but a change to
  the entry alone isn't watched for. A screen saver already showing keeps what it had until it next
  starts, and System Settings keeps the one its preview loaded until it is quit and opened again. If a rebuild fails, the old bundle stays as it was, and the app (under *Install* in the
  *Screensaver* pane, and on the status line), `jetcarp host saver status` and `jetcarp doctor` say
  why; it is tried again at the next change, or install it again. What the bundle was built from is
  recorded in `screen-saver-state.json` in the settings folder.

  macOS prepares the saver's full-screen views ahead of time, as soon as it is chosen and after each
  run; they hold their first picture, and the scene starts moving only once the screen saver has
  really started. The preview in System Settings always moves. `remove` deletes the bundle and puts back the screen saver macOS had chosen before JETCARP
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
again. The entry carries no environment variables, so the host it starts uses the default folders:
run with `JETCARP_*` folders of your own, `host login on` names the variables it won't pass on. A
host already running is left alone, except on macOS one that launchd started from the
entry, which `host login on` restarts. Turning it off removes the entry, and on macOS also unloads
it from launchd, unless the host it started is still running: that host is left running.

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

At logout the system stops the host: on the Mac launchd sends it SIGTERM, a Linux session ends
its programs the same way, and Windows tells every program's windows that the session is ending.
It stops as `jetcarp host stop` stops it ([above](#the-background-host)), so the runtime folder
holds nothing stale at your next login. Windows gives it 5 seconds (unless changed) and then
ends it.

More: [Rotations](rotations.md), [Reseeding](reseeding.md), [Folders](../reference/folders.md) and
[Uninstalling](uninstalling.md).
