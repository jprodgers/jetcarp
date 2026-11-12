# Uninstalling

JETCARP keeps almost everything in three folders, for settings, data and output (see
[Folders](../reference/folders.md)). Outside them it sets things up only when you ask it to: the
background host starting at login, the GNOME Shell extension, the Windows or macOS screen saver, and
a still picture as your desktop background. Removing JETCARP takes three steps: undo those with
`jetcarp uninstall`, decide what to do with the folders, and remove the program itself.

## 1. Undo what JETCARP set up

`jetcarp uninstall`, on its own, shows what it would do without changing anything:

```text
$ jetcarp uninstall
Would undo (run again with --yes):
  stop the host starting at login

Kept the data folder /home/ada/.local/share/jetcarp (2.1 GB); add --data to delete it.
Your pictures and exports in /home/ada/Pictures/JETCARP are never deleted.

To remove the program itself, use what installed it: the installer again with `--uninstall`, `pipx uninstall jetcarp`, `uv tool uninstall jetcarp`, `pip uninstall jetcarp`, `brew uninstall jetcarp`, `scoop uninstall jetcarp`, `cargo uninstall jetcarp`, or delete its folder.
```

`jetcarp uninstall --yes` does it, and reports each step as *done* or *FAILED* with the reason. The
steps it may list are:

- **stop the background host**;
- **put your own desktop picture back**, if the host left a still there (*Leave a still picture when
  it stops*, or `jetcarp host still on`) or will as it stops. JETCARP saved your picture when it
  first put a still up; if you have chosen another picture since, that one is left alone;
- **stop the host starting at login**;
- **remove the GNOME Shell extension** (Linux): it is switched off and its folder deleted. Log out and
  back in once to finish;
- **remove the Windows screen saver** (`JETCARP.scr`), clearing the choice in Windows' screen saver
  settings if JETCARP was the one chosen;
- **remove the macOS screen saver** (`JETCARP.saver`);
- **forget the recorded host command**, a small file in the settings folder that GNOME's menu and the
  Windows screen saver use to start the host.

Run it from your desktop session where you can, since putting your picture back and switching the
GNOME extension off both go through the desktop. From another desktop, a text console or over SSH
it still finds the GNOME extension's files and deletes them; GNOME Shell skips an extension on its
list whose files have gone.

Two options also delete JETCARP's folders:

| Option | Deletes |
| --- | --- |
| `--settings` | The settings folder: the app's and the host's settings, screensaver profiles, rotations, reseed presets you saved, and the host's desktop pictures. A picture there that the desktop still shows is kept, and the report names it: on Windows the copy of your own picture that was put back, or a still if it could not be |
| `--data` | The data folder: the pattern library with its media, jobs and Quick Start entries (the atlas and your own), and the exploratory search's databases. **This cannot be undone** |

The output folder, with your pictures, wallpapers, script runs and sweeps, is never deleted. If you
want to keep your pattern library, copy `library.sqlite` (and its `media` folder) out of the data
folder first; `jetcarp doctor` shows where it is. If you moved the library elsewhere with
`JETCARP_LIBRARY`, `--data` does not delete it. Like every JETCARP command, `jetcarp uninstall` uses
the folders that `JETCARP_CONFIG_DIR`, `JETCARP_DATA_DIR` and `JETCARP_OUT` point to when they are
set; `jetcarp paths` shows which.

## 2. What lives where

If you would rather remove things by hand, or check that nothing is left, this is everything JETCARP
writes outside the program's own folder. `~` is your home folder.

| What | Linux | macOS | Windows |
| --- | --- | --- | --- |
| Settings folder | `~/.config/jetcarp` | `~/Library/Application Support/jetcarp` | `%APPDATA%\jetcarp` |
| Data folder | `~/.local/share/jetcarp` | `~/Library/Application Support/jetcarp/data` | `%LOCALAPPDATA%\jetcarp` |
| Output folder (kept) | `~/Pictures/JETCARP` | `~/Pictures/JETCARP` | your Pictures folder, `\JETCARP` |
| Runtime files (a port number, an access token and a lock file) | `$XDG_RUNTIME_DIR/jetcarp`, else `~/.cache/jetcarp` | `~/.cache/jetcarp` | inside the data folder |
| Start at login | `~/.config/autostart/jetcarp-host.desktop` | `~/Library/LaunchAgents/com.jimmieprodgers.jetcarp.host.plist` | the value `JETCARP` under `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run` |
| GNOME Shell extension | `~/.local/share/gnome-shell/extensions/jetcarp@jimmieprodgers.com`, and its entry in GNOME's list of enabled extensions | | |
| Screen saver | | `~/Library/Screen Savers/JETCARP.saver` | `%LOCALAPPDATA%\JETCARP\JETCARP.scr`, and the `SCRNSAVE.EXE` value under `HKEY_CURRENT_USER\Control Panel\Desktop` when it is the chosen one |
| Screen saver log | | `jetcarp-saver.log` in the screen saver's sandbox (`~/Library/Containers/com.apple.ScreenSaver.Engine.legacyScreenSaver/Data/tmp/` on macOS 27) | |

The Linux paths are the usual ones; they follow your XDG settings. `jetcarp uninstall` does not remove
the runtime files or the macOS screen saver's log, which are small; delete them by hand if you like.
To remove the GNOME extension by hand, switch it off in GNOME's *Extensions* app before deleting its
folder, or run `jetcarp host gnome remove`, which does both.

## 3. Remove the program

Remove the program last: `jetcarp uninstall` needs it to undo the rest. Then use what installed it
(see [Installing JETCARP](../getting-started/installing.md)):

| Installed with | Remove with |
| --- | --- |
| The shell installer | `curl -LsSf https://github.com/jprodgers/jetcarp/releases/latest/download/jetcarp-installer.sh \| sh -s -- --uninstall`, or `sh jetcarp-installer.sh --uninstall`: it deletes `~/.local/lib/jetcarp` and the link `~/.local/bin/jetcarp` |
| The PowerShell installer | the installer again with `-Uninstall`: it deletes `%LOCALAPPDATA%\Programs\jetcarp` and takes it off your user `PATH` |
| pipx, uv, pip | `pipx uninstall jetcarp`, `uv tool uninstall jetcarp`, `pip uninstall jetcarp` |
| Homebrew | `brew uninstall jetcarp` (and `brew untap jprodgers/jetcarp`) |
| Scoop | `scoop uninstall jetcarp` (and `scoop bucket rm jetcarp`) |
| cargo | `cargo uninstall jetcarp` |
| A release archive | delete the folder you unpacked |

Each removes the program with the files that came with it (on Windows `jetcarpw.exe`, the
windowless launcher; on macOS `libjetcarp_saver.dylib`; the starter pack) and nothing of yours: the
settings, data and output folders stay until you delete them (step 1's `--settings` and `--data`,
or by hand). If you built JETCARP from source, delete the folder you built it in.
