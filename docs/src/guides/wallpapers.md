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
| Live desktop background | With JETCARP's GNOME extension (GNOME Shell 50) | No: the host shows a window instead | Yes, under the desktop icons | Yes, behind the desktop icons, on every Space |
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
on these machines again.

- **Fedora 44, GNOME 50.6 on Wayland, one monitor:** the live desktop background, the top-bar menu,
  and the lock screen in both modes, including unlocking with a real password and stopping the host by
  force while locked (GNOME's own background came back and unlocking still worked). Several monitors
  have only been tried in a headless GNOME session. Since those tests, the part of the extension that
  keeps the host's windows out of the overview and the window switcher has been rewritten, and the new
  version has not yet been tested in a real GNOME session.
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
- **Background host** controls the host (see [below](#the-background-host)).
- **Profiles** lists and edits the screensaver profiles (see [below](#screensaver-profiles)).

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

**Start and stop it** from the *Background host* section of the *Screensaver* pane (*Start*, *Stop*,
*Pause*, and ◀ ▶ for the previous and next item), from its menu, or from the command line:

```text
$ jetcarp host start --rotation "Wolfram series"
running, pid 676359, up 0.0s, 90 MB, GPU AMD Radeon 8060S Graphics (RADV STRIX_HALO)
screen 0: Wolfram series · W0 (1 of 256) · generation 4 · 57600 cells
$ jetcarp host next
running, pid 676359, up 3.0s, 90 MB, GPU AMD Radeon 8060S Graphics (RADV STRIX_HALO)
screen 0: Wolfram series · W1 (2 of 256) · generation 198 · 57600 cells
$ jetcarp host stop
stopped
```

| Command | Effect |
| --- | --- |
| `jetcarp host start [--rotation NAME] [--headless]` | Start the host; it keeps running after the command ends. `--rotation` is for this run only; `--headless` runs it with no window or menu |
| `jetcarp host stop`, `status`, `pause`, `resume` | Quit it, say what it shows, freeze it, carry on |
| `jetcarp host next`, `previous` `[--screen N]` | The next or previous item, on every screen or one |
| `jetcarp host rotation NAME [--screen N]` | Choose a screen's rotation |
| `jetcarp host fps N` | The most frames per second it draws (1 to 240; default 30) |
| `jetcarp host caption on\|off` | Show what is running in a corner, with `--corner`, `--size`, `--generation on\|off` and `--seconds` |
| `jetcarp host battery on\|off` | Pause while the computer runs on battery (default on) |
| `jetcarp host busy PERCENT` | Pause while the whole processor stays at least this busy, as during a long search (default 80; 0 never). It carries on once the load is 20 points lower |
| `jetcarp host wallpaper`, `still`, `lock`, `gnome`, `saver`, `login` | See the sections below |

The settings are saved in `host.json` in the settings folder. The app and the command line both write
it and tell a running host to read it again, so a setting changed while the host is stopped applies
the next time it starts (*saved; the host is not running*). Besides pausing on battery and when busy,
the host also pauses by itself while its desktop background is completely hidden behind other windows
and while the screen is locked and dark. `jetcarp host status --json` shows what it is holding for,
under `held`.

On Windows and macOS the host has an icon in the notification area or the menu bar, and on GNOME the
extension adds one to the top bar. Its menu names what is showing and has *Next*, *Previous*,
*Paused*, *Show on the desktop*, *Rotation*, *Open JETCARP*, *Start at login* and *Quit* (on GNOME,
also *Start JETCARP host* while it is stopped).

## The live desktop background

Tick *Show on the desktop* in the *Background host* section, or run `jetcarp host wallpaper on`. To
put different rotations on different monitors, set up one screen per monitor:

```text
jetcarp host wallpaper on --screen 0 --monitor 0
jetcarp host wallpaper on --screen 1 --monitor 1 --rotation "Classics, hourly"
```

`jetcarp host status` lists the monitors by number. A screen given no monitor fills every monitor no
other screen claims, and `jetcarp host wallpaper off` turns the screens back into windows.

**GNOME** needs JETCARP's GNOME Shell extension, *JETCARP live desktop*, which shows the host's
windows as the background of the desktop, the overview and the lock screen, and adds the top-bar menu.
It supports GNOME Shell 50.

```text
jetcarp host gnome install    # copy it into place and switch it on; log out and back in once
jetcarp host gnome status     # "The GNOME extension is running."
jetcarp host gnome remove
```

The *Background host* section shows the same status, with an *Install* button. Without the extension
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
icons, on every Space and ignoring the mouse; the host has no Dock icon while it shows only wallpaper.
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
first screen's rotation); in the app, the same choice is *Screen saver runs*, with an *Install* button.
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
  it with `cargo build --release -p jetcarp-saver`.
- **GNOME** has no list of screen savers. Use the lock screen's screensaver mode instead:
  `jetcarp host lock screensaver --awake 30`.

## Start at login

`jetcarp host login on` (or *Start at login*, in the app or the host's menu) makes the host start when
you log in; `jetcarp host login off` stops that. It uses each system's own per-user mechanism:

| System | What is written |
| --- | --- |
| Linux | `~/.config/autostart/jetcarp-host.desktop` |
| macOS | `~/Library/LaunchAgents/com.jimmieprodgers.jetcarp.host.plist` (it takes effect at the next login) |
| Windows | The value `JETCARP` under `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run` |

The entry runs `jetcarp host serve` (on Windows through `jetcarpw.exe` when it is there, so no console
window opens) at the path the program has now. If you move the program, turn the setting off and on
again. A host already running is left alone.

More: [Rotations](rotations.md), [Reseeding](reseeding.md), [Folders](../reference/folders.md) and
[Uninstalling](uninstalling.md).
