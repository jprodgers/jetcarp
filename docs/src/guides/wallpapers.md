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

Not tested yet: starting at a real login on any of the three (the entries are written and removed
correctly, but no one has logged in with them), a fresh boot on Windows, Linux desktops other than
GNOME, Intel Macs, and any other versions of these systems.

## Screensaver mode in the app

*View → Screensaver mode* (F12), or *Start the screensaver* in the *Screensaver* pane, hides every
control and runs the screensaver; Esc leaves it. It runs full screen on the app's monitor, or on the
monitors chosen in the *Display* pane.

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
are shared: the desktop background runs the same ones, and they are edited here.

The app can also start straight into screensaver mode, for example from a desktop shortcut:

```text
jetcarp --screensaver --rotation "Wolfram series" --quit-on-exit
jetcarp --screensaver --profile Life --monitors 0,1 --span --cell-px 3 --caption
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

```text
$ jetcarp host start --rotation "Wolfram series"
running, pid 676359, up 0.0s, 90 MB, GPU AMD Radeon 8060S Graphics (RADV STRIX_HALO)
screen 0: Wolfram series · W0 (1 of 256) · generation 4 · 57600 cells of 4 px · window
caption: off
$ jetcarp host next
running, pid 676359, up 3.0s, 90 MB, GPU AMD Radeon 8060S Graphics (RADV STRIX_HALO)
screen 0: Wolfram series · W1 (2 of 256) · starting · window
caption: off
$ jetcarp host stop
stopped
```

`status` (and every command that changes something) says what each screen shows and where (a window,
or the desktop background on a monitor or on every monitor), the monitors, whether the caption is
on, and, while it is paused, why: by hand, on battery, the processor is busy, or held by the desktop
(windows cover it, or the screen is locked or off). Just after `next` the new item is *starting*
until its world is built.

| Command | Effect |
| --- | --- |
| `jetcarp host start [--rotation NAME] [--headless]` | Start the host; it keeps running after the command ends. `--rotation` is for this run only; `--headless` runs it with no window or menu |
| `jetcarp host stop`, `status`, `pause`, `resume` | Quit it, say what it shows, freeze it, carry on |
| `jetcarp host next`, `previous` `[--screen N]` | The next or previous item, on every screen or one |
| `jetcarp host rotation NAME [--screen N]` | Choose a screen's rotation |
| `jetcarp host fps N` | The most frames per second it draws (1 to 240; default 30) |
| `jetcarp host caption on\|off` | Show what is running in a corner, with `--corner`, `--size`, `--generation on\|off` and `--seconds` |
| `jetcarp host battery on\|off` | Pause while the computer runs on battery (default on) |
| `jetcarp host tune [--cpu-limit P] [--gpu-limit P] [--fps N]` | Fit the desktop background to this computer (see [below](#fit-to-this-computer)) |
| `jetcarp host scene [--screen N] [--save FILE]` | The scene a screen shows, its grid's size included, as a scene file |
| `jetcarp host busy PERCENT` | Pause while the whole processor stays at least this busy, as during a long search (default 80; 0 never). It carries on once the load is 20 points lower |
| `jetcarp host wallpaper`, `still`, `lock`, `gnome`, `saver`, `login` | See the sections below |

The settings are saved in `host.json` in the settings folder. The app and the command line both write
it and tell a running host to read it again, so a setting changed while the host is stopped applies
the next time it starts (*saved; the host is not running*). Besides pausing on battery and when busy,
the host also pauses by itself while its desktop background is completely hidden behind other windows
and while the screen is locked and dark. `jetcarp host status --json` shows what it is holding for,
under `held`.

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

## The Desktop background pane

*View → Desktop background settings…* opens the *Desktop background* pane, which holds everything
about the host. The *Display* and *Screensaver* panes link to it.

<img src="../images/tutorials/wallpaper-desktop-pane.webp" alt="The Desktop background pane: Start; Show on the desktop; Rotation A classic a day on Every monitor, Add a screen, and a link to the Screensaver pane for rotations and profiles; Leave a still picture when it stops; On the lock screen; the GNOME extension's state with Install; Frames per second, GPU simulation Automatic, Caption, Pause on battery, Pause while the processor is busier than 80 %, Start at login; Performance: the GPU, Processor limit 10 %, GPU limit 25 %, Smooth at the monitor's rate, Fit to this computer…, and Tune again by itself when the monitors change" width="700">

- **The host:** *Start* or *Stop*, *Pause*, ◀ ▶, and what each screen shows (its cell size too, and
  whether tuning chose it), or why it is paused.
- **Where:** *Show on the desktop*, and for each screen its rotation and its monitor (*Every monitor*,
  or one of them). *Add a screen* puts another rotation on another monitor. Rotations and profiles are
  edited in the *Screensaver* pane, which the link beside them opens.
- **The desktop:** *Leave a still picture when it stops*, and on GNOME *On the lock screen*, how long
  the screen stays lit while locked, and the extension's state with an *Install* button.
- **How it runs:** *Frames per second*, *GPU simulation* (the host's own choice) and the *Shared
  setting* it can follow (see [The GPU](gpu.md#the-setting)), the *Caption*, *Pause on battery*,
  *Pause while the processor is busier than* a share, and *Start at login*.
- **Performance:** the GPU it uses, the limits, and *Fit to this computer…* (see below).

The settings are the host's own `host.json`, as before; the pane writes them and tells a running host.

## Fit to this computer

A live wallpaper should stay out of the way. *Fit to this computer…* in the pane's *Performance* group,
or `jetcarp host tune`, finds for each scene of the desktop background the most cells it can show
while JETCARP stays within three limits:

- **Processor limit:** JETCARP's own processor time, as a share of the whole machine with every core
  counted (default 10%).
- **GPU limit:** JETCARP's own share of the graphics card's time (default 25%).
- **Smooth at:** the frame rate it must keep (default the monitor's refresh rate, or 60, and never more
  than the host's *Frames per second*, which is 30 unless you change it).

The rules, palettes and colouring stay as they are: only the cell size changes, and with it the number
of cells. It never makes a scene's cells smaller than the scene's own size.

```text
$ jetcarp host tune --cpu-limit 1 --gpu-limit 3
tuning 7 scenes on 1 screen for 30 frames a second, on AMD Radeon 8060S Graphics (RADV STRIX_HALO)
screen 0, scene 1 of 7: profile Life at 3 px
  3 px, 409,440 cells: processor 1.1%, GPU 2.8%, frames 5.9 ms (95th percentile), stepped on the GPU — over: processor 1.1% (over 0.8%: the 1.0% limit, less 20%), GPU 2.8% (over 2.4%: the 3.0% limit, less 20%)
screen 0, scene 1 of 7: profile Life at 24 px
  24 px, 6,360 cells: processor 0.1%, GPU 1.0%, frames 3.0 ms (95th percentile), stepped on the processor — fits
...
screen 0: profile Life: 4 px (its own: 3 px), 230,400 cells; processor 0.7%, GPU 2.0%, frames 4.9 ms
...
tuned 7 scene(s) (GPU time from the system's count of JETCARP's GPU time); saved to ~/.config/jetcarp/host.json
```

**How it works.** Each scene of each screen on the desktop (every screen, if none is) runs for a few
seconds at a time, at that screen's size, the way the host runs it: built and stepped by the same code,
drawn off screen, at the host's frame rate. The tuner tries the scene's own size first, and if that
fits, keeps it. Otherwise it tries the coarsest sensible size (60 cells across the screen's shorter
side, or 64 pixels a cell) and then halves the gap until it finds the smallest size that fits. A size
fits when the processor and GPU stay under 80% of their limits and 95% of frames take under 90% of the
frame's time, and the scene keeps its speed. With several screens on the desktop, each gets an equal
share of the limits. A scene that cannot fit even at the coarsest size gets the coarsest, with a note
saying which limit it passed; a scene whose size the scene itself sets (an aperiodic patch, or one not
rebuilt to fill the screen) is measured and left as it is. Tuning takes a few seconds a scene, more
for a scene that does not fit at its own size.

**What it measures.** The processor time is JETCARP's own: the tuner runs as a program of its own, and
counts its processor time on every thread. The GPU time is the system's own count of JETCARP's GPU
time on Linux; elsewhere it comes from the GPU's timestamp queries around each frame's step and
drawing, or, on a GPU without them, from handing each frame's work to the GPU until it is done (an
upper bound). The summary says which. A world small enough that the [GPU setting](gpu.md) leaves it
on the processor is tried there, and held to the processor limit like any other.

**While it runs**, a running host pauses (*tuning the desktop background for this computer*, in its
status), so the two do not measure each other, and it shows the new sizes as soon as they are saved.
*Cancel* stops it and saves nothing. On battery the host pauses anyway, and a tune started by hand says
that a computer on battery may measure slower than plugged in.

**The sizes found** are saved in `host.json`, with the rotation and the screen size they were found for,
and the pane lists them: each scene's size and cells, and the processor, GPU and frame times measured.
A screen given another rotation shows its scenes at their own sizes until it is tuned again; on a
screen of another size, a tuned scene keeps its number of cells. *Use the scenes' own sizes* forgets
them. With *Tune again by itself when the monitors change* (on by default), the host tunes again,
once the monitors have stayed as they are for 15 seconds, when a tuned screen's size changes: not on
battery, while the processor is busy, or while paused by hand.

**A tuned scene is still exactly reproducible**, since a scene's grid size is part of it. The host's
status gives each screen's cell size and grid; `jetcarp host scene --save FILE` saves the scene a
screen shows, at the size it was built, and the still picture the host leaves carries that scene
inside it, as every picture JETCARP writes does.

| Option | Effect |
| --- | --- |
| `--cpu-limit P` | JETCARP's processor time, in percent of the whole machine (1 to 100; saved; default 10) |
| `--gpu-limit P` | JETCARP's share of the GPU's time, in percent (1 to 100; saved; default 25) |
| `--fps N` | The frame rate to keep (saved; default the monitor's refresh rate, or 60, at most the host's) |
| `--screen N` | Only this screen |
| `--canvas WxH` | The screen's size, when no host is running to say (else the size it was tuned for before, else its monitor's) |
| `--trial-seconds S` | How long each trial measures (default 3) |
| `--dry-run` | Measure and report, but save nothing |
| `--json` | Each step as a line of JSON |

Scripts and AI assistants reach it through the host: the MCP tool `host_control` takes
`{"action": "tune", "cpu_limit": 10, "gpu_limit": 25}` (and `"on": false` to stop it), and its status
shows the progress under `tuning`; `{"action": "scene"}` gives a screen's scene.

## The live desktop background

Tick *Show on the desktop* in the *Desktop background* pane, or run `jetcarp host wallpaper on`. To
put different rotations on different monitors, set up one screen per monitor:

```text
jetcarp host wallpaper on --screen 0 --monitor 0
jetcarp host wallpaper on --screen 1 --monitor 1 --rotation "Classics, hourly"
```

`jetcarp host status` lists the monitors by number. A screen given no monitor fills every monitor no
other screen claims, and `jetcarp host wallpaper off` turns the screens back into ordinary windows
(1280 by 720, each on its monitor) that you can move, resize or minimise.

**GNOME** needs JETCARP's GNOME Shell extension, *JETCARP live desktop*, which shows the host's
windows as the background of the desktop, the overview and the lock screen, and adds the top-bar menu.
It supports GNOME Shell 50 and 51.

```text
jetcarp host gnome install    # copy it into place and switch it on; log out and back in once
jetcarp host gnome status     # "The GNOME extension is running."
jetcarp host gnome remove
```

Upgrading JETCARP upgrades the extension: the host, when it starts, and any `jetcarp host` command
bring an installed extension up to the program's version, and leave it switched off if you switched it
off. GNOME reads extensions at login, so log out and back in once after upgrading; until then
`jetcarp host gnome status` says that GNOME is still running the older version.

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
cannot be shown, through the system's own setting. On Windows and macOS each monitor gets a picture of
what it showed; GNOME has one background for every monitor, so there it is the primary monitor's.
Your own picture (each monitor's) is remembered and put back when the still or the live background
is turned off, unless you have chosen another picture in the meantime. It is off by default.

## The system's screen saver

`jetcarp host saver install` adds JETCARP to the system's list of screen savers and opens the system's
screen saver settings, with JETCARP to choose. `--rotation NAME` chooses what it runs (otherwise the
first screen's rotation); in the app, the same choice is *System screen saver runs* in the
*Screensaver* pane, with an *Install* button.
`jetcarp host saver status` says whether it is installed, and `remove` takes it out.

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
you log in; `jetcarp host login off` stops that. It uses each system's own per-user mechanism:

| System | What is written |
| --- | --- |
| Linux | `~/.config/autostart/jetcarp-host.desktop` |
| macOS | `~/Library/LaunchAgents/com.jimmieprodgers.jetcarp.host.plist` (it takes effect at the next login) |
| Windows | The value `JETCARP` under `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run` |

The entry runs `jetcarp host serve --login` (on Windows through `jetcarpw.exe` when it is there, so no
console window opens) at the path the program has now. If you move the program, turn the setting off
and on again. A host already running is left alone.

Started at login, the host shows only the screens placed on the desktop, and opens no windows: a
screen in a window (after `jetcarp host wallpaper off`, say) waits unseen, and with no desktop screens
the host just waits in the notification area or the menu bar (on GNOME, the top bar's menu). This
holds for the whole run; `jetcarp host stop` and `jetcarp host start` start a host that opens windows
as usual. `jetcarp host status` lists a waiting screen as *not shown (started at login)*.

More: [Rotations](rotations.md), [Reseeding](reseeding.md), [Folders](../reference/folders.md) and
[Uninstalling](uninstalling.md).
