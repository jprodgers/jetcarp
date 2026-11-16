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

A screen whose world can't keep up with its scene's speed says what it reaches instead, as in
`30 fps, 3.90 of 30 generations a second` (`measured_speed` in `--json`).

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
the processor is busy, and while the desktop holds it (windows cover it, or the screen is locked or
off). Its `note:` lines are the host's recent messages, such
as the GPU it took. Just after `next` the new item is *starting* until its world is built.

A host started before the [photosensitive seizure warning](../getting-started/photosensitivity.md)
was answered (at login after an upgrade, say) holds every screen at its first picture, and `status`
says so on a line of its own: `waiting: the photosensitive seizure warning has not been
accepted (…)`. The scenes start by themselves, within a few seconds, once it has been answered in
the app or with `jetcarp setup`. Meanwhile `start`, `wallpaper on`, `login on` and `saver install`
are refused, with the warning.

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
| `jetcarp host wallpaper`, `still`, `lock`, `gnome`, `saver`, `login` | See the sections below |

The settings are saved in `host.json` in the settings folder. The app and the command line both write
it and tell a running host to read it again, so a setting changed while the host is stopped applies
the next time it starts (*saved; the host is not running*). When the host reads it again, a screen
whose rotation, or a profile in it, has been saved since starts that rotation afresh, as saved.
Should the host's memory pass `max_memory_mb` in `host.json` (6,144 MB by default), it gives up the
biggest scene it shows for the next item, with a note; on Linux it first gives back the memory it
has freed but kept.
Besides pausing on battery and when busy, the host also pauses by itself while its desktop background
is completely hidden behind other windows, while the screen is locked, and while the display is
asleep (on macOS, whether locked or not). `jetcarp host
status --json` gives `paused` (paused by hand, or nothing in the rotation can be shown; *Resume*
carries on) with `paused_why`, and apart from it what the host is holding for, which passes by
itself: `held` (`battery`, `busy`, `covered` or `dark`) with `held_why`, the same in words.

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

<img src="../images/tutorials/wallpaper-desktop-pane.webp" alt="The Desktop background pane, not running: Start; Show on the desktop; Rotation A classic a day on Every monitor, with Edit, New and Show just this scene under it; Add a screen; a link to the Screensaver pane, where profiles are made; Leave a still picture when it stops; On the lock screen; the GNOME extension's state with Install; Frames per second 30, GPU simulation Follow the shared setting (Automatic) and the Shared setting Automatic, Caption, Pause on battery, Pause while the processor is busier than 80 %, Start at login; Performance: the GPU, Show every generation (chosen) or Low power, Up to 60 frames a second, and a line saying each scene shows at its own cell size, which its profile or its rotation sets" width="700">

- **The host:** *Start* or *Stop*, *Pause*, ◀ ▶, and what each screen shows (its cell size too),
  why it is paused (*Paused: by hand.*), and what holds it, which passes
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
  second or the monitor's refresh rate; see [Frame rate and speed](#frame-rate-and-speed)). Each
  scene shows at its own cell size (see [Cell sizes](#cell-sizes)).

The settings are the host's own `host.json`, as before; the pane writes them and tells a running host.

## Cell sizes

How small a scene's cells can be depends on the screen and the computer. On a lower-resolution
laptop panel, Life on squares at one pixel a cell is still easy to see; on a Retina screen a
one-pixel cell is too small to see; and the more cells, the harder the processor and the GPU work.
JETCARP 0.4.2 shows each scene of the desktop background at its own cell size, as 0.4.1 did. To
change it:

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

*Fit to this computer*, which showed each scene at a ladder of sizes, measured each and recommended
the finest within limits on the processor and the GPU, was tried in 0.4.2's release candidates. A
fit took from 45 seconds to 12 minutes, so it is held back, and a faster one is planned for 0.4.3
([#113](https://github.com/jprodgers/jetcarp/issues/113)). Sizes a release candidate's Fit saved stay
in `host.json`, unused.

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
