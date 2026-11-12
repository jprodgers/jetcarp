# Installing JETCARP

JETCARP is one program, `jetcarp`: with no subcommand it opens the app, and its subcommands are the
command line, the background host and the MCP server. Every way of installing it below gives the
same program. Pick the one that suits the tools you already use:

| Way | Systems | Starter pack | Updates with |
| --- | --- | --- | --- |
| [The installers](#the-installers) | Linux, macOS, Windows | yes | running the installer again |
| [pipx, uv or pip](#with-pipx-uv-or-pip) | Linux, macOS, Windows | yes | `pipx upgrade jetcarp`, `uv tool upgrade jetcarp` |
| [Homebrew](#with-homebrew) | macOS, Linux | yes | `brew upgrade jetcarp` |
| [Scoop](#with-scoop) | Windows | yes | `scoop update jetcarp` |
| [A release archive](#from-a-release-archive) | Linux, macOS, Windows | yes | downloading the next one |
| [cargo](#with-cargo) | anywhere Rust runs | no | `cargo install jetcarp` again |

Release builds are made for Linux on x86_64 and ARM (aarch64), Windows on x86_64, and macOS on
Apple silicon and Intel. Linux on ARM and Intel Macs are built by the same release workflow but have
not been tried by hand. On anything else, [cargo](#with-cargo) builds JETCARP from source.

<div class="checked">

**Before JETCARP's repository is public.** The commands on this page are written as they will be
for the 0.4.0 release. The release files, PyPI, crates.io, the Homebrew tap and the Scoop bucket
appear with it; until then the links here do not work. Release candidates such as `0.4.0-rc.1` are
published on GitHub only, for testing; see [Release candidates](#release-candidates).

</div>

## The installers

On Linux and macOS, in a terminal:

```bash
curl -LsSf https://github.com/jprodgers/jetcarp/releases/latest/download/jetcarp-installer.sh | sh
```

On Windows, in PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/jprodgers/jetcarp/releases/latest/download/jetcarp-installer.ps1 | iex"
```

Each installer downloads the release archive for your computer and the release's `SHA256SUMS`,
refuses the archive if its checksum does not match, and unpacks it into a folder of its own:

| | Linux and macOS | Windows |
| --- | --- | --- |
| The program's folder | `~/.local/lib/jetcarp` | `%LOCALAPPDATA%\Programs\jetcarp` |
| On your `PATH` as | a link, `~/.local/bin/jetcarp` | the folder, added to your user `PATH` |

Nothing else is changed: no administrator rights are needed, no system folders are touched, and the
shell installer never edits your shell's startup files. If `~/.local/bin` is not on your `PATH`, it
says so and shows the line to add. On Windows, open a new terminal afterwards so that it sees the
new `PATH`.

The installers take options. With the shell installer, put them after `sh -s --` (or download the
script and run `sh jetcarp-installer.sh OPTIONS`); with PowerShell, download
`jetcarp-installer.ps1` and run it with the options, or set the environment variable named with
each before `irm | iex`.

| Shell | PowerShell | Variable | Does |
| --- | --- | --- | --- |
| `--version V` | `-Version V` | `JETCARP_VERSION` | Install release `V`, such as `0.4.0` or `0.4.0-rc.1`, instead of the installer's own |
| `--no-packs` | `-NoPacks` | `JETCARP_NO_PACKS=1` | Leave out the [starter pack](#the-starter-pack) |
| `--dir DIR` | `-InstallDir DIR` | `JETCARP_INSTALL_DIR` | Install into another folder |
| `--bin-dir DIR` | | `JETCARP_BIN_DIR` | Put the link in another folder |
| | `-NoModifyPath` | `JETCARP_NO_MODIFY_PATH=1` | Leave the user `PATH` alone |
| `--url URL` | `-DownloadUrl URL` | `JETCARP_DOWNLOAD_URL` | Take the release files from another address, or from a folder that holds them |
| `--gh` | `-UseGh` | `JETCARP_USE_GH=1` | Download with the [GitHub CLI](https://cli.github.com), signed in, which can read a private repository |
| `--uninstall` | `-Uninstall` | | Remove what the installer installed |

An installer only ever replaces a folder it made itself (it leaves a `.jetcarp-installer` file
there), and refuses to replace a `jetcarp` on your `PATH` that something else put there, such as
pipx or cargo.

## With pipx, uv or pip

JETCARP's Python package contains the program and the starter pack, and no Python code. It installs
the program onto your `PATH` like any Python tool:

```bash
pipx install jetcarp
# or
uv tool install jetcarp
# or, inside a virtual environment
pip install jetcarp
```

The wheels are built for the same systems as the release archives, for any Python 3.8 or newer;
there is no source package, so on other systems use [cargo](#with-cargo). The starter pack goes in
`share/jetcarp/packs` inside the Python environment, and on macOS the screen saver's code in
`lib/jetcarp`.

## With Homebrew

On macOS or Linux:

```bash
brew tap jprodgers/jetcarp
brew install jetcarp
```

`brew install jprodgers/jetcarp/jetcarp` does both in one line. The formula installs the release
archive; nothing is compiled. The starter pack is in `$(brew --prefix)/share/jetcarp/packs`.

## With Scoop

On Windows:

```powershell
scoop bucket add jetcarp https://github.com/jprodgers/scoop-jetcarp
scoop install jetcarp
```

Scoop adds `jetcarp` to your `PATH` and a *JETCARP* entry to the Start menu, which starts the app
without a console window.

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

Before a release, its candidates (`0.4.0-rc.1`, `0.4.0-rc.2` and so on) are published on GitHub as
pre-releases, for testing. They are not on PyPI, crates.io, Homebrew or Scoop, and they are never
GitHub's "latest" release, so name the one you want. Use the installer attached to it:

```bash
curl -LsSf https://github.com/jprodgers/jetcarp/releases/download/v0.4.0-rc.1/jetcarp-installer.sh | sh
```

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/jprodgers/jetcarp/releases/download/v0.4.0-rc.1/jetcarp-installer.ps1 | iex"
```

or any installer with `--version 0.4.0-rc.1` (`-Version 0.4.0-rc.1` in PowerShell). Installing
the release itself later replaces the candidate.

## On each system

### Linux

The release builds run on Linux with glibc 2.28 or newer: Debian 10, Ubuntu 20.04, Fedora 29,
RHEL, Rocky and AlmaLinux 8, and anything newer. They are not built for musl systems such as
Alpine, where [cargo](#with-cargo) works instead.

The command line needs nothing beyond glibc. The app, the background host and the GPU simulation
load these libraries when they start, which a desktop system already has:

- a Vulkan driver (`libvulkan.so.1` with Mesa's or your graphics card's driver), or failing that
  OpenGL ES through EGL (`libEGL.so.1`);
- for windows, `libwayland-client.so.0` on Wayland or `libX11.so.6`, `libX11-xcb.so.1`,
  `libxcb.so.1`, `libXcursor.so.1` and `libXi.so.6` on X11, and `libxkbcommon.so.0` (with
  `libxkbcommon-x11.so.0` on X11);
- for the app's file dialogs, `libdbus-1.so.3` and an XDG desktop portal.

`jetcarp doctor` shows the graphics adapters it found; with none, simulation runs on the CPU.

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
`jetcarp host saver install` makes is signed on your Mac, ad hoc, as macOS requires.

### Windows

The release builds run on 64-bit Windows 10 and 11, and on Windows on ARM through its x64
emulation. They are not yet signed, so Windows SmartScreen may stop a program from a zip that your
browser downloaded with *Windows protected your PC*. Click *More info*, then *Run anyway*; or, before
unpacking, open the zip's *Properties* and tick *Unblock*. The PowerShell installer and Scoop
download without the browser's mark, so this applies only to an archive downloaded in a browser.

`jetcarpw.exe`, beside `jetcarp.exe`, starts JETCARP without opening a console window; shortcuts, the
login entry and the screen saver use it.

## The starter pack

Releases come with a starter pack: a folder of [packs](../guides/packs.md) (each a pack store, as
[Pack format](../reference/pack-format.md#the-store) lays out) that the
[first-run setup](first-run.md) imports into your library if you choose the Explorer, Researcher or
Everything profile. The starter pack itself is the pack whose feed or folder has `starter` in its
name, such as the feed `jetcarp-starter`; when no pack in the folder is named so, setup counts every
one of them as the starter pack. Everything imports every pack in the folder.

In 0.4.0 the folder holds one pack, `jetcarp-starter` (version 2026.11, CC0-1.0, signed by Jimmie
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
the same folder and run

```bash
sha256sum --check --ignore-missing SHA256SUMS                      # Linux
grep jetcarp-0.4.0-aarch64-apple-darwin.tar.gz SHA256SUMS | shasum -a 256 --check   # macOS
```

or, in PowerShell, compare `(Get-FileHash jetcarp-0.4.0-x86_64-pc-windows-msvc.zip).Hash` with the
file's line in `SHA256SUMS` (the case of the letters does not matter). Once the repository is
public, each release file also has a signed record of the workflow run that built it, which the
GitHub CLI checks:

```bash
gh attestation verify jetcarp-0.4.0-x86_64-unknown-linux-gnu.tar.gz --repo jprodgers/jetcarp
```

## No telemetry

JETCARP makes no network connections. It does not check for updates, report crashes or collect
statistics of any kind; it never sends anything anywhere. The only sockets it opens are on your own
computer, for the app's AI control and the background host (see
[`SECURITY.md`](https://github.com/jprodgers/jetcarp/blob/main/SECURITY.md)). Packs and other files
reach it only when you give them to it.

## Uninstalling

Run `jetcarp uninstall` first, while the program is still there, to undo what JETCARP set up outside
its folders; then remove the program the way you installed it. [Uninstalling](../guides/uninstalling.md)
has the details.
