# Installing JETCARP

JETCARP is one program, `jetcarp`: with no subcommand it opens the app, and its subcommands are the
command line, the background host and the MCP server. Every way of installing it below gives the
same program. Pick the one that suits the tools you already use:

| Way | Systems | Starter pack | Updates with |
| --- | --- | --- | --- |
| [The installers](#the-installers) | Linux, macOS, Windows | yes | running the installer again |
| [pipx, uv or pip](#with-pipx-uv-or-pip) | Linux, macOS, Windows | yes | `pipx upgrade jetcarp`, `uv tool upgrade jetcarp` |
| [Homebrew](#with-homebrew) | macOS, Linux | yes | `brew update`, then `brew upgrade jetcarp` |
| [Scoop](#with-scoop) | Windows | yes | `scoop update jetcarp` |
| [A release archive](#from-a-release-archive) | Linux, macOS, Windows | yes | downloading the next one |
| [cargo](#with-cargo) | anywhere Rust runs | no | `cargo install jetcarp` again |

Testing a release candidate? See [Release candidates](#release-candidates) below. Once JETCARP is
installed, [open the app](#open-the-app) by running `jetcarp`, or from its icon in your
applications menu, Launchpad or the Start menu.

Release builds are made for Linux on x86_64 and ARM (aarch64), Windows on x86_64, and macOS on
Apple silicon and Intel. Linux on ARM and Intel Macs are built by the same release workflow but have
not been tried by hand. On anything else, [cargo](#with-cargo) builds JETCARP from source.

> **Photosensitive seizure warning.** JETCARP's animations can flash and flicker rapidly, and can
> trigger seizures in people with photosensitive epilepsy. Read the
> [warning](photosensitivity.md) first: the app shows it at its first launch, and `jetcarp setup`
> asks it, before anything animates on screen.

## The installers

On Linux and macOS, in a terminal:

```bash
curl -LsSf https://github.com/jprodgers/jetcarp/releases/latest/download/jetcarp-installer.sh | sh
```

On Windows, in PowerShell, save the installer and run it:

```powershell
Invoke-WebRequest https://github.com/jprodgers/jetcarp/releases/latest/download/jetcarp-installer.ps1 -OutFile "$env:TEMP\jetcarp-installer.ps1"
powershell -ExecutionPolicy Bypass -File "$env:TEMP\jetcarp-installer.ps1"
```

Then open a new PowerShell window and run `jetcarp`: the window you installed from can't see the new
`PATH` yet, so `jetcarp` isn't found there.

(The script is saved first because Microsoft Defender can flag the one-line form some installers
use, piping a downloaded script into `iex`, as a trojan; run as a file, the same script raises no
alert.)

Each installer downloads the release archive for your computer and the release's `SHA256SUMS`,
refuses the archive if its checksum does not match, and unpacks it into a folder of its own:

| | Linux and macOS | Windows |
| --- | --- | --- |
| The program's folder | `~/.local/lib/jetcarp` | `%LOCALAPPDATA%\Programs\jetcarp` |
| On your `PATH` as | a link, `~/.local/bin/jetcarp` | the folder, added to your user `PATH` |

Then the installer has the program put JETCARP among your apps, with its icon: in the applications
menu on Linux, in `~/Applications` as *JETCARP.app* on macOS, and in the Start menu on Windows
(`jetcarp launcher install`; see [Open the app](#open-the-app)). Nothing else is changed: no
administrator rights are needed, no system folders are touched, and the shell installer never edits
your shell's startup files. If `~/.local/bin` is not on your `PATH`, it says so and shows the line
to add.

The installers take options. With the shell installer, put them after `sh -s --` (or download the
script and run `sh jetcarp-installer.sh OPTIONS`); with PowerShell, put them after the script's
name (`powershell -ExecutionPolicy Bypass -File "$env:TEMP\jetcarp-installer.ps1" -NoPacks`), or set
the environment variable named with each.

| Shell | PowerShell | Variable | Does |
| --- | --- | --- | --- |
| `--version V` | `-Version V` | `JETCARP_VERSION` | Install release `V`, such as `0.4.0` or `0.4.0-rc.1`, instead of the installer's own |
| `--no-packs` | `-NoPacks` | `JETCARP_NO_PACKS=1` | Leave out the [starter pack](#the-starter-pack) |
| `--no-launcher` | `-NoLauncher` | `JETCARP_NO_LAUNCHER=1` | Leave JETCARP out of the applications menu, `~/Applications` or the Start menu |
| `--dir DIR` | `-InstallDir DIR` | `JETCARP_INSTALL_DIR` | Install into another folder |
| `--bin-dir DIR` | | `JETCARP_BIN_DIR` | Put the link in another folder |
| | `-NoModifyPath` | `JETCARP_NO_MODIFY_PATH=1` | Leave the user `PATH` alone |
| `--url URL` | `-DownloadUrl URL` | `JETCARP_DOWNLOAD_URL` | Take the release files from another address, or from a folder that holds them |
| `--gh` | `-UseGh` | `JETCARP_USE_GH=1` | Download with the [GitHub CLI](https://cli.github.com), signed in, which can read a private repository |
| `--uninstall` | `-Uninstall` | | Remove what the installer installed, its entry among your apps too |

An installer only ever replaces a folder it made itself (it leaves a `.jetcarp-installer` file
there), and refuses to replace a `jetcarp` on your `PATH` that something else put there, such as
pipx or cargo.

## With pipx, uv or pip

JETCARP's Python package contains the program and the starter pack, and no Python code. It installs
the program onto your `PATH` like any Python tool, with pipx:

```bash
pipx install jetcarp
```

or with uv:

```bash
uv tool install jetcarp
```

or with pip, inside a virtual environment:

```bash
pip install jetcarp
```

The wheels are built for the same systems as the release archives, for any Python 3.8 or newer;
there is no source package, so on other systems use [cargo](#with-cargo). The starter pack goes in
`share/jetcarp/packs` inside the Python environment, and on macOS the screen saver's code in
`lib/jetcarp`. To open JETCARP from your applications menu, Launchpad or the Start menu, leave
that box ticked in the setup window, or run `jetcarp launcher install` once (see
[Open the app](#open-the-app)).

## With Homebrew

On macOS or Linux:

```bash
brew tap jprodgers/jetcarp
brew install jetcarp
```

`brew install jprodgers/jetcarp/jetcarp` does both in one line. The formula installs the release
archive; nothing is compiled. The starter pack is in `$(brew --prefix)/share/jetcarp/packs`.
Homebrew cannot put anything in your home folder, so to open JETCARP from Launchpad and Spotlight
(as *JETCARP.app*) or from your applications menu, leave that box ticked in the setup window, or
run `jetcarp launcher install` once, as Homebrew's message after installing says (see
[Open the app](#open-the-app)). It starts Homebrew's `jetcarp` link, which an upgrade keeps.

To upgrade, run `brew update` before `brew upgrade jetcarp`. Homebrew looks at taps again only
once a day on its own, so for a day after you last used it, `brew upgrade` alone can miss a new
release. On macOS, if the desktop background starts at login, open the app or run
`jetcarp host start` once after upgrading, so that macOS accepts the new version at login (see
[Start at login](../guides/wallpapers.md#start-at-login)).

## With Scoop

On Windows:

```powershell
scoop bucket add jetcarp https://github.com/jprodgers/scoop-jetcarp
scoop install jetcarp
```

Scoop adds `jetcarp` to your `PATH` and a *JETCARP* entry, with JETCARP's icon, to the Start menu
(under *Scoop Apps*), which starts the app without a console window.

## From a release archive

Each release on [GitHub](https://github.com/jprodgers/jetcarp/releases) has an archive for each
system, such as `jetcarp-0.4.0-x86_64-unknown-linux-gnu.tar.gz` or
`jetcarp-0.4.0-x86_64-pc-windows-msvc.zip`. Unpack it anywhere and run `jetcarp` from the folder,
or put the folder on your `PATH`. The folder holds:

- `jetcarp` (`jetcarp.exe` on Windows), the program;
- on Windows `jetcarpw.exe`, which starts JETCARP without a console window, for shortcuts and the
  screen saver; keep it beside `jetcarp.exe`;
- on macOS `libjetcarp_saver.dylib`, the screen saver's code, which
  `jetcarp host saver install` uses; keep it beside `jetcarp`;
- `packs/`, the starter pack;
- `README.md`, the licences (`COPYING`, `UNLICENSE`, `LICENSE-MIT`), `THIRD_PARTY_NOTICES.md` and
  `THIRD_PARTY_LICENSES.html`, the notices of the libraries the program is built from.

## With cargo

With [Rust](https://rustup.rs) 1.95 or newer and a C compiler (for the SQLite and Zstandard
libraries JETCARP builds in), cargo builds JETCARP from source:

```bash
cargo install jetcarp
```

From a copy of the repository, `cargo install --path crates/jetcarp --profile dist` builds the same
way as the releases do: the `dist` profile leaves out debugging information, which would otherwise
make the program several hundred megabytes. cargo installs only the program, so there is no starter
pack (`jetcarp paths` says so); you can [import packs](../guides/packs.md) yourself. On Linux the
build needs no system libraries: windowing, Wayland, X11 and Vulkan are all loaded when the program
runs.

## Release candidates

Before a release `X.Y.Z`, its candidates (`X.Y.Z-rc.1`, `X.Y.Z-rc.2` and so on) are published on
GitHub as pre-releases, for testing. They are not on PyPI, crates.io, Homebrew or Scoop, and they
are never GitHub's "latest" release, so the installers above, which take the latest, do not install
them: name the one you want.

The [releases page](https://github.com/jprodgers/jetcarp/releases) lists the candidates, each
marked *Pre-release*. A candidate's tag is its version with a `v` in front, such as `v0.4.2-rc.4`,
and is in its page's title and address. Use the installer attached to it, putting the tag in place
of `vX.Y.Z-rc.N` in these lines:

```bash
curl -LsSf https://github.com/jprodgers/jetcarp/releases/download/vX.Y.Z-rc.N/jetcarp-installer.sh | sh
```

```powershell
Invoke-WebRequest https://github.com/jprodgers/jetcarp/releases/download/vX.Y.Z-rc.N/jetcarp-installer.ps1 -OutFile "$env:TEMP\jetcarp-installer.ps1"
powershell -ExecutionPolicy Bypass -File "$env:TEMP\jetcarp-installer.ps1"
```

Or run any installer with `--version X.Y.Z-rc.N` (`-Version X.Y.Z-rc.N` in PowerShell), the
version without the `v`. Installing the release itself later replaces the candidate.

## On each system

### What works on each system

JETCARP aims to do the same on every system. Where a system's own rules, or pieces it lacks, stop
a feature, JETCARP keeps to them rather than break something else to get round them, and the
tables below say what falls short and why. Each row links to the page with the details. Plain
differences of name, such as ⌘ for Ctrl on macOS, are left out.

#### Installing and builds

| | Linux | Windows | macOS | Why |
| --- | --- | --- | --- | --- |
| Release builds | x86_64 and ARM, with glibc 2.28 or newer (Debian 10, Ubuntu 20.04, Fedora 29 and later); none for musl systems such as Alpine | x86_64, on Windows 10 and 11; Windows on ARM runs it through its x64 emulation | Apple silicon and Intel, on macOS 11 or newer | None of JETCARP's test machines runs either: [cargo](#with-cargo) builds JETCARP on a musl system, and a native Windows on ARM build, tested on GitHub's ARM runners, is planned ([#322](https://github.com/jprodgers/jetcarp/issues/322)) |
| Signed ([Windows](#windows), [macOS](#macos)) | Linux makes no such check | Not yet: SmartScreen may stop a program from a zip that a browser downloaded | Not with an Apple Developer ID: macOS won't open an archive that a browser downloaded until you allow it | Signing costs money the project can't fund yet: Apple's Developer ID is $99 a year, and Microsoft's signing a monthly fee. The builds will be signed once it can. The installers and the package managers download without the browser's mark |
| [The installer](#the-installers) | One line in a terminal | Saved, then run; `jetcarp` then works in a new PowerShell window | One line in a terminal | Microsoft Defender can flag a downloaded script piped into `iex` as a trojan; the window you installed from can't see the new `PATH` |
| [The system's screen saver](../guides/wallpapers.md#the-systems-screen-saver) from a [source build](#with-cargo) | Nothing more to build: the GNOME extension is inside the program | `jetcarpw.exe` is built only with `--features windows-launcher` | `libjetcarp_saver.dylib` is built only with `-p jetcarp-saver` | Cargo can't build a program for one system only, so on Windows the windowless launcher is asked for with `cargo install jetcarp --features windows-launcher`; the Mac's saver is a library of its own |

#### The desktop background and the screen saver

| | Linux | Windows | macOS | Why |
| --- | --- | --- | --- | --- |
| [Live desktop background](../guides/wallpapers.md#the-live-desktop-background) | GNOME, with JETCARP's extension; on other desktops, a window instead | Yes, under the desktop icons | Yes, behind the desktop icons, on every Space | Under Wayland, how an app can be the desktop's background differs from desktop to desktop, and on GNOME it takes an extension; JETCARP is developed on GNOME, with three test machines, one for each system, so GNOME's is the only one written so far |
| [The GNOME extension](../guides/wallpapers.md#the-live-desktop-background) | GNOME Shell 50 and 51; a logout after installing, upgrading or removing it | — | — | GNOME loads an extension only on the versions of the Shell it names, and JETCARP's works inside the Shell's own code, which changes from version to version, so it names those it has been tried on. GNOME reads its extensions at login, and under Wayland the Shell can't restart without ending the session |
| [Live lock screen](../guides/wallpapers.md#the-live-desktop-background) | GNOME, through the extension; not other desktops | No; the screen saver does the job | No; the screen saver does the job | Windows and macOS let no app draw on the lock screen, but their screen savers can ask for the password as they end. Other desktops: as for the live background |
| [In the system's screen saver list](../guides/wallpapers.md#the-systems-screen-saver) | No: GNOME has none, and its lock screen's screensaver mode does the job; not other desktops | Yes, `JETCARP.scr` | Yes, `JETCARP.saver` | GNOME has no list of screen savers. Other desktops: as for the live background |
| [The host's menu](../guides/wallpapers.md#the-background-host) | GNOME, in the top bar, from the extension, which can start a stopped host too; none on other desktops | In the notification area, while the host runs | In the menu bar, while the host runs | GNOME Shell has no notification area, so GNOME's menu comes with the extension, which is part of the Shell and stays when the host stops; on Windows and macOS the menu is the host's own. The tray library JETCARP uses needs GTK on Linux, which JETCARP leaves out |
| [Still picture](../guides/wallpapers.md#still-picture) | GNOME, one picture for every monitor (the main monitor's); none on other desktops | One for each monitor | One for each monitor | GNOME has one background for every monitor. Other desktops: as for the live background |
| [Start the host at login](../guides/wallpapers.md#start-at-login) | GNOME, yes; on other desktops the entry is written, but the host shows nothing | Yes, after the login entries before it (97 seconds after sign-in, in testing) | Yes; after an upgrade, once the app or `jetcarp host start` has run | Started at login, the host opens no windows, and other desktops have no live background for it. Windows runs login entries one at a time and waits for each. launchd refuses at login a program changed since its entry was registered, and each build is signed ad hoc, so its signature changes with each version ([#99](https://github.com/jprodgers/jetcarp/issues/99)) |
| [The system's screen saver, after a change](../guides/wallpapers.md#the-systems-screen-saver) | — | At once: it reads JETCARP's settings each time it starts | Rebuilt within seconds while the app, the desktop background or an MCP server runs, else as the next command ends; a saver already showing keeps what it had until it next starts | macOS runs screen savers in a sandbox that can't read JETCARP's settings, so they are copied into the saver, with the seizure warning's answer and the strobe guard's setting |
| [Removing the screen saver](../guides/wallpapers.md#the-systems-screen-saver) | — | Close *Screen Saver Settings* first | System Settings may list it until it is reopened | Windows won't delete a `.scr` while the dialog's preview runs it; System Settings keeps the list it found when it opened |

#### Windows and monitors

| | Linux | Windows | macOS | Why |
| --- | --- | --- | --- | --- |
| [The app's window, at the next start](tour.md#a-world) | X11: its size and place; Wayland: its size only | Its size and place | Its size and place | Wayland gives an app no say in where its windows go, and doesn't tell it where they are: the desktop places them |
| [The host's screens in windows](../guides/wallpapers.md#the-live-desktop-background), after `jetcarp host wallpaper off` | X11: each on its monitor; Wayland: where the desktop puts them | Each on its monitor | Each on its monitor | As above |
| Monitors' places, for the app's outputs and the wallpaper export's [*Detected* layout](../guides/pictures.md#desktop-wallpapers) | X11: as the system says; Wayland: they can all read (0, 0), so the export lays them left to right, and the *Display* pane takes the outputs' places by hand | As the system says | As the system says | A Wayland desktop needn't tell an app where its monitors are |
| [The window's icon](#open-the-app) | X11: yes; Wayland: once `jetcarp launcher install` has run | Yes | Yes | A Wayland desktop takes a window's icon from the applications menu entry named after it |
| [Keeping the computer awake](../guides/wallpapers.md#screensaver-mode-in-the-app) | Through the desktop portal, `org.freedesktop.ScreenSaver` or GNOME's session manager; not on a desktop with none of them | Yes | Yes | Linux leaves it to the desktop, which may offer any of the three, or none |

#### Accessibility

| | Linux | Windows | macOS | Why |
| --- | --- | --- | --- | --- |
| [The menus](../guides/keyboard.md#what-the-keyboard-and-screen-readers-cant-do) | Yes; F10 opens them | Yes; F10 opens them | Not in the system's menu bar, so VoiceOver's VO+M can't reach them; F10 does | egui, the toolkit the app is drawn with, draws its menus inside the window |
| [The grid, to a screen reader](../guides/keyboard.md#with-a-screen-reader) | Yes | Narrator and NVDA may announce it as having "0 rows" ([#219](https://github.com/jprodgers/jetcarp/issues/219)) | Yes | AccessKit, which carries what the app shows to screen readers, gives Windows the grid with no count of its rows or columns |
| [The system's reduced motion, high contrast, light or dark](../guides/accessibility-settings.md) | Through the desktop portal, or GNOME's own settings; a desktop with neither leaves reduced motion off and the app dark | Yes | Yes | Such a desktop offers an app nothing to read |

#### The GPU

| | Linux | Windows | macOS | Why |
| --- | --- | --- | --- | --- |
| [The graphics API](../guides/gpu.md), with the same results as the processor, bit for bit | Vulkan | Vulkan or DirectX 12 | Metal | — |
| [A software renderer](../guides/gpu.md#which-adapter), when asked for | Mesa's llvmpipe or lavapipe | None: Windows' own, WARP, is never used | None | macOS has none. WARP crashed in the simulation's shaders; whether the cause is JETCARP's is being looked into ([#321](https://github.com/jprodgers/jetcarp/issues/321)) |

#### This computer

| | Linux | Windows | macOS | Why |
| --- | --- | --- | --- | --- |
| [GPU memory](../guides/this-computer.md#what-jetcarp-reads) | Read: AMD's from its driver, NVIDIA's from NVML, a discrete Intel GPU's on xe from the kernel's `dmem.capacity`, any other from its Vulkan driver; an integrated Intel GPU's estimated, since it shares the system's memory | Read, from DXGI | One pool with the processor's | AMD's driver writes it in `/sys/class/drm` and the kernel's dmem cgroup lists xe's, both readable by any program; NVIDIA's comes from its management library, i915's and nouveau's only through Vulkan; an integrated Intel GPU has no memory of its own to read |
| [The GPU's load, before it measures](../guides/this-computer.md#while-it-measures) | AMD's: yes; NVIDIA's: with its management library; Intel's and others': your own programs only | Yes | Yes | Intel's and other drivers' figure comes from `fdinfo`, which shows only the programs you may read (their drivers' own counters need administrator rights); NVIDIA's management library comes with its driver, though some distributions package it apart |
| [Held still while it measures](../guides/this-computer.md#while-it-measures) | The desktop background and GNOME's lock screen | The desktop background and the screen saver | The desktop background, not the screen saver | The Mac's screen saver can't read the runtime folder from its sandbox; it runs only while the Mac is idle |
| Freed memory given back to the system, by the [host](../guides/wallpapers.md#the-background-host) and by [explore searches](../guides/searching.md#watch-stop-carry-on) | By both | JETCARP doesn't ask for it | By both | glibc's and macOS's allocators keep what is freed inside their heaps until asked ([#69](https://github.com/jprodgers/jetcarp/issues/69), [#111](https://github.com/jprodgers/jetcarp/issues/111)); Windows' heap gives large blocks back by itself |

#### Stopping

| | Linux | Windows | macOS | Why |
| --- | --- | --- | --- | --- |
| [A command the system stops](../reference/folders.md#runtime-folder): `jetcarp search`, `jobs resume`, `script` and `mcp` | SIGTERM, SIGHUP and Ctrl-C stop it as Ctrl-C does, at its next checkpoint or step | Ctrl-C, closing its console, signing out, shutting down and `taskkill` without `/F` stop it the same way, within the 5 seconds Windows gives it | As on Linux | Windows ends a program 5 seconds after telling it to stop (unless that's changed): a search not at its checkpoint by then is ended as before, its job left marked running until `jetcarp jobs resume` carries it on ([#319](https://github.com/jprodgers/jetcarp/issues/319)) |
| The app and the host at logout or shutdown | They stop as their own *Quit* does, leaving nothing stale | The same, within the 5 seconds Windows gives them | As on Linux | As above ([#319](https://github.com/jprodgers/jetcarp/issues/319)) |

#### Files

| | Linux | Windows | macOS | Why |
| --- | --- | --- | --- | --- |
| [Paths](../reference/folders.md) JETCARP reads and writes | Shorter than 4,096 bytes | Shorter than 4,096 bytes | Shorter than 1,024 bytes | macOS's own limit |
| [The runtime folder](../reference/folders.md#runtime-folder), private to you | Mode 0700, its files 0600 | With your profile folder's permissions | Mode 0700, its files 0600 | Windows has no such modes: a new file takes its folder's permissions, and a profile folder's keep other users out |
| [The output folder](../reference/folders.md#the-three-folders) | `~/Pictures/JETCARP` | `JETCARP` in your Pictures folder, which is in OneDrive, and synced, when OneDrive backs up Pictures | `~/Pictures/JETCARP` | Windows then moves the Pictures folder into OneDrive; `JETCARP_OUT` keeps the output out of it |
| [The app's file dialogs](#linux) | The desktop portal's, else zenity's; with neither, none opens | Yes | Yes | Linux has no file dialog of its own: the desktop's portal shows one |

#### What has been tried by hand

| | Linux | Windows | macOS | Why |
| --- | --- | --- | --- | --- |
| [Before each release](../about/testing.md#release-candidates) | Fedora 45 with GNOME on Wayland, on x86_64 | Windows 11, on x86_64 | macOS 27, on Apple silicon | The three test machines |
| Not yet tried | ARM; other distributions; desktops other than GNOME; X11 | ARM; Windows 10 | Intel Macs; macOS before 27 | The test machines are the three above |
| [Graphics](../about/testing.md#what-this-testing-does-not-cover) | AMD's only, not NVIDIA's or Intel's | NVIDIA's and Intel's, not AMD's | Apple silicon's | The test machines' GPUs |
| [The desktop background](../guides/wallpapers.md#what-has-been-tested) | Several monitors only in a headless GNOME session; on GNOME 51, the lock screen, the top-bar menu and keeping the host out of the overview not yet | Yes, with one monitor and two | Yes, with one monitor and two | The Linux test machine has one monitor; the rest is still to be done |
| [Starting at a real login](../guides/wallpapers.md#what-has-been-tested) | Not yet | Yes, though not from a fresh boot | Yes | Still to be done |
| [A screen reader](../about/accessibility.md#where-jetcarp-falls-short) | Orca on GNOME, with 0.4.3-rc.1 | Narrator and NVDA: not yet | VoiceOver: not yet | Still to be done ([#219](https://github.com/jprodgers/jetcarp/issues/219)) |
| [The score and Fit's timings](../about/testing.md#the-test-machines-scores) | Measured | Not yet | Not yet | Wanted at 0.4.3's sittings |
| [In CI](../about/testing.md#continuous-integration) | GPU tests on lavapipe, a software renderer | GPU tests skipped | GPU tests on the runner's virtual GPU; the Intel build is built, and the release runs its program once, under Rosetta | CI's runners have no GPU of their own, and Windows' have only WARP; no runner is an Intel Mac |

For research (long searches, large libraries, deep folders of results), Linux is the system to
choose: JETCARP is developed on it and tested on it most, and its file work has been quickest there
in testing. A Mac is not recommended for serious research. macOS is the most restrictive of the
three systems: a path can't be longer than 1,024 bytes, its screen saver runs in a sandbox that
can't read JETCARP's settings, and it holds back a program that a browser downloaded unless a
registered Apple developer has signed it, which JETCARP's builds aren't. JETCARP keeps to macOS's
rules rather than working round them, and the Mac is the system JETCARP is tested on least. For
desktop art (live backgrounds, screen savers), use whichever system you already have; the tables
above and [the wallpapers table](../guides/wallpapers.md#what-works-on-which-system) say what each
one offers.

### Linux

The release builds run on Linux with glibc 2.28 or newer: Debian 10, Ubuntu 20.04, Fedora 29,
RHEL, Rocky and AlmaLinux 8, and anything newer. They are not built for musl systems such as
Alpine, where [cargo](#with-cargo) works instead.

The command line needs nothing beyond glibc. The app, the background host and the GPU simulation
load these libraries when they start, which a desktop system already has (without a usable GPU,
JETCARP runs on the processor; see [The GPU](../guides/gpu.md)):

- a Vulkan driver (`libvulkan.so.1` with Mesa's or your graphics card's driver), or failing that
  OpenGL ES through EGL (`libEGL.so.1`);
- for windows, `libwayland-client.so.0` on Wayland or `libX11.so.6`, `libX11-xcb.so.1`,
  `libxcb.so.1`, `libXcursor.so.1` and `libXi.so.6` on X11, and `libxkbcommon.so.0` (with
  `libxkbcommon-x11.so.0` on X11);
- for the app's file dialogs, `libdbus-1.so.3` and an XDG desktop portal, or failing those
  `zenity`; with none of them, no file dialog opens.

`jetcarp doctor` shows the graphics adapters it found; with none, simulation runs on the CPU. It
also shows what JETCARP read of the computer (processor, memory, GPU memory), its
[JETCARP Hardware Score](../guides/this-computer.md) once measured, and the limits set from them.

### macOS

The release builds run on macOS 11 (Big Sur) or newer. They are not signed with an Apple Developer
ID, so the first time you open one that your browser downloaded, macOS says it cannot check it for
malicious software and will not open it. Either remove the mark the browser left on the archive
before unpacking it:

```bash
xattr -d com.apple.quarantine jetcarp-0.4.0-aarch64-apple-darwin.tar.gz
```

or, after the warning, open *System Settings → Privacy & Security* and click *Open Anyway* beside
the message about `jetcarp`. The installers, Homebrew, pipx, uv and pip download without that mark,
so this applies only to an archive downloaded in a browser. The screen saver bundle that
`jetcarp host saver install` makes is signed on your Mac, ad hoc, as macOS requires, and so is
*JETCARP.app*, which `jetcarp launcher install` makes (the installer runs it for you); neither needs
a Developer ID, since neither was downloaded.

### Windows

The release builds run on 64-bit Windows 10 and 11, and on Windows on ARM through its x64
emulation. They are not yet signed, so Windows SmartScreen may stop a program from a zip that your
browser downloaded with *Windows protected your PC*. Click *More info*, then *Run anyway*; or, before
unpacking, open the zip's *Properties* and tick *Unblock*. The PowerShell installer and Scoop
download without the browser's mark, so this applies only to an archive downloaded in a browser.

`jetcarpw.exe`, beside `jetcarp.exe`, starts JETCARP without opening a console window; shortcuts, the
login entry and the screen saver use it. Both carry JETCARP's icon, which Explorer, the taskbar and
the Start menu show.

## The starter pack

Releases come with a starter pack: a folder of [packs](../guides/packs.md) (each a pack store, as
[Pack format](../reference/pack-format.md#the-store) lays out) that the
[first-run setup](first-run.md) imports into your library if you choose the Explorer, Researcher or
Everything profile. The starter pack itself is the pack whose feed or folder has `starter` in its
name, such as the feed `jetcarp-starter`; when no pack in the folder is named so, setup counts every
one of them as the starter pack. Everything imports every pack in the folder.

In the 0.4 releases the folder holds one pack, `jetcarp-starter` (version 2026.11, CC0-1.0, signed by Jimmie
Rodgers, about 1.5 MB): 79 Quick Start entries that Jimmie Rodgers picked, with the records they
show.

- 36 from the atlas of an [exploratory search](../research/search-methods.md#explore) over every
  tiling and rule family.
- 18 spaceships and 8 oscillators found by soup searches on Archimedean, k-uniform, isohedral and
  aperiodic tilings, with their pattern records.
- 8 ant and turmite highways, with their ant records.
- 7 sandpile identities: on the hat, the Spectre, both Penrose tilings, the snub square tiling, a
  3-uniform tiling and an isohedral one.
- 2 hat gliders: James Edmond's glider B, and the table JETCARP's evolutionary search found that
  runs exactly like it.

The 63 built-in examples picked with them (hand-made scenes, named rules, a tiling showcase and
JACA presets) are part of the program and need no pack. They are *featured*: the Quick Start
menu's *Featured* order mixes them in with the best entries, the pack's among them.

JETCARP looks for the starter pack's folder in one place, the first of:

1. `JETCARP_BUNDLED_PACKS`, if it is set;
2. `share/jetcarp/packs` beside the folder the program is in (`<program folder>/../share/jetcarp/packs`),
   when the program sits in a `bin` or `Scripts` folder, as Homebrew and the Python package put
   it;
3. `packs` in the program's folder, as in the release archives and the installers' folders.

The program's folder is the one the program really is in: a link to it on your `PATH`, as pipx, uv,
Homebrew and the installers make, is followed. `jetcarp paths` shows the folder it found as
`bundled:`, and `jetcarp doctor` its size, or where it looked when there is none. JETCARP reads the
folder and never writes to it. To import a pack from it yourself, run `jetcarp pack import` with the
pack's folder.

## Checking a download

Each release has a `SHA256SUMS` file listing the SHA-256 of every file in it. The installers check
the archive they download against it; to check a file you downloaded yourself, put `SHA256SUMS` in
the same folder and run, on Linux:

```bash
sha256sum --check --ignore-missing SHA256SUMS
```

on macOS, with your archive's name:

```bash
grep jetcarp-0.4.0-aarch64-apple-darwin.tar.gz SHA256SUMS | shasum -a 256 --check
```

or, in PowerShell, compare `(Get-FileHash jetcarp-0.4.0-x86_64-pc-windows-msvc.zip).Hash` with the
file's line in `SHA256SUMS` (the case of the letters does not matter).

`SHA256SUMS` shows that a download is complete and unchanged, but it comes from the same place as
the archive. To check where a file came from, each release file also has a provenance
attestation: a signed record, made by GitHub, of the repository, workflow and commit that built
it. The GitHub CLI checks it (the installers do not):

```bash
gh attestation verify jetcarp-0.4.0-x86_64-unknown-linux-gnu.tar.gz --repo jprodgers/jetcarp
```

## No telemetry

JETCARP makes no network connections. It does not check for updates, report crashes or collect
statistics of any kind; it never sends anything anywhere. The only network sockets it opens are on
your own computer, for the app's AI control and the background host (see
[`SECURITY.md`](https://github.com/jprodgers/jetcarp/blob/main/SECURITY.md)). Packs and other files
reach it only when you give them to it.

## Uninstalling

Run `jetcarp uninstall` first, while the program is still there, to undo what JETCARP set up outside
its folders; then remove the program the way you installed it. [Uninstalling](../guides/uninstalling.md)
has the details.

## Open the app

<img src="../images/jetcarp-icon.png" alt="JETCARP's icon for now: a glider, five black squares on a three-by-three grid, inside a white rounded square with a black outline" width="128">

With JETCARP installed, run `jetcarp` with no arguments in a terminal. This opens the app, on Linux,
macOS and Windows alike:

```bash
jetcarp
```

On Windows, open a new terminal after installing, so that it sees the new `PATH`. From a release
archive that is not on your `PATH`, run the program in its folder (`./jetcarp`, or `.\jetcarp.exe`
on Windows).

Or open it like any other app, from its icon. The installers put it among your apps for you, and
the [setup window](first-run.md) offers to; after any other way of installing (pipx, uv, pip,
Homebrew, cargo, a release archive) you can also run this once:

```bash
jetcarp launcher install
```

| | What it makes | Open JETCARP from |
| --- | --- | --- |
| Linux | `jetcarp.desktop` in `~/.local/share/applications`, and the icon at each size in `~/.local/share/icons/hicolor` (both follow `XDG_DATA_HOME`) | the applications menu: on GNOME, *Activities* and the app grid |
| macOS | *JETCARP.app* in `~/Applications`, signed ad hoc on your Mac | Launchpad, Spotlight or Finder; keep it in the Dock from there |
| Windows | `JETCARP.lnk` in your Start menu's folder (`%APPDATA%\Microsoft\Windows\Start Menu\Programs`), starting a copy of `jetcarpw.exe` in `%LOCALAPPDATA%\jetcarp\launcher`, so no console window opens | the Start menu |

Each keeps working when JETCARP is upgraded. On Linux and macOS it starts the program by a path an
upgrade keeps (Homebrew's `jetcarp` link, not the version's own folder); *JETCARP.app* holds a
small launcher that starts the program, not a copy of it. On Windows, Scoop's folders name the
version, so the Start menu entry starts a copy of the windowless launcher in a folder of
JETCARP's own instead, beside a record of the `jetcarp.exe` it starts. When the program the record
names has gone, as after `scoop update`, the launcher finds JETCARP on your `PATH`, where Scoop
keeps the current version, and that run puts its own path in the record. A record naming
a program that is still there is left alone, so a second copy of JETCARP, such as one you built
yourself, never takes the entry over: run `jetcarp launcher install` with the one you want the
entry to start, or again if you move the program by hand. Scoop adds a *JETCARP* entry
of its own as well. `jetcarp launcher status` says whether it is there, `jetcarp doctor` says so
too, `jetcarp launcher remove` takes it out (on Windows with the launcher's copy), and
`jetcarp uninstall` removes it with the rest of what JETCARP set up. A menu entry, app or shortcut
of the same name that JETCARP did not make is left alone.

The app's windows carry the icon too: in the title bar and taskbar on Windows and X11, and in the
Dock on macOS. On Wayland the desktop takes a window's icon from the menu entry named after it, so
there it needs `jetcarp launcher install`.

The first time, the app shows the [photosensitive seizure warning](photosensitivity.md), then asks a
few questions to set itself up: see [First run and setup](first-run.md).
