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

| | Linux | Windows | macOS |
| --- | --- | --- | --- |
| Release builds | x86_64 and ARM, with glibc 2.28 or newer (Debian 10, Ubuntu 20.04, Fedora 29 and later) | x86_64, on Windows 10 and 11 (and Windows on ARM through its x64 emulation) | Apple silicon and Intel, on macOS 11 or newer |
| Tried by hand before each release ([how](../about/testing.md#release-candidates)) | Fedora 45 with GNOME, on x86_64 | Windows 11, on x86_64 | macOS 27, on Apple silicon |
| [The GPU](../guides/gpu.md), with the same results as the processor, bit for bit | Vulkan | Vulkan or DirectX 12 | Metal |
| [Live desktop background](../guides/wallpapers.md#what-works-on-which-system) | On GNOME, with JETCARP's extension (GNOME Shell 50 or 51), and its lock screen too; on other desktops, a window | Yes, and in the system's screen saver list | Yes, and in the system's screen saver list |
| Paths JETCARP reads and writes | Shorter than 4,096 bytes | Shorter than 4,096 bytes | Shorter than 1,024 bytes, the system's own limit |

For research (long searches, large libraries, deep folders of results), Linux is the system to
choose: JETCARP is developed on it and tested on it most, and its file work has been quickest there
in testing. For desktop art (live backgrounds, screen savers), use whichever system you already
have; [the wallpapers table](../guides/wallpapers.md#what-works-on-which-system) shows what each one
offers.

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
- for the app's file dialogs, `libdbus-1.so.3` and an XDG desktop portal.

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

<img src="../images/jetcarp-icon.png" alt="JETCARP's icon: a black line drawing of a koi carp with jet engines, curled inside a white rounded square with a black outline" width="128">

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
