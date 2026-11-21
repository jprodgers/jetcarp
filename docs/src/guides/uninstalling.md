# Uninstalling

JETCARP keeps almost everything in three folders, for settings, data and output (see
[Folders](../reference/folders.md)). Outside them it sets things up only when you ask it to: the
background host starting at login, the GNOME Shell extension, the Windows or macOS screen saver, and
a still picture as your desktop background. Removing JETCARP takes up to four steps, in this order:

1. undo what JETCARP set up, with `jetcarp uninstall`;
2. decide what to do with JETCARP's folders (the same command can delete them);
3. remove the program itself, the way you installed it;
4. remove JETCARP from any AI assistant you connected it to.

**`jetcarp uninstall` does not remove the program.** It undoes what JETCARP set up, keeps your
settings and data unless you ask it to delete them, and ends by naming the command that removes the
program. It needs the program to run, so run it first.

## 1. Undo what JETCARP set up

On its own, `jetcarp uninstall` shows what it would do, without changing anything:

```sh
jetcarp uninstall
```

It prints, for JETCARP installed with the shell installer:

```text
This undoes what JETCARP set up outside its folders. It never removes the program itself, and deletes its settings and data only when asked (--settings, --data).

Would undo (run again with --yes):
  stop the host starting at login

Kept the data folder /home/ada/.local/share/jetcarp (2.1 GB); add --data to delete it.
Kept the settings folder /home/ada/.config/jetcarp; add --settings to delete it.
Your pictures and exports in /home/ada/Pictures/JETCARP are never deleted.

Then, to remove the program itself (in /home/ada/.local/lib/jetcarp, installed with JETCARP's installer), run this:

    curl -LsSf https://github.com/jprodgers/jetcarp/releases/latest/download/jetcarp-installer.sh | sh -s -- --uninstall

If you connected JETCARP to an AI assistant, remove it there too: `claude mcp remove jetcarp` for Claude Code, or the "jetcarp" entry in Claude Desktop's claude_desktop_config.json.
```

The outputs on this page come from Linux; on macOS and Windows the folders in them differ (see
[Folders](../reference/folders.md#the-three-folders)).

The command it names for removing the program is the one for how this copy of JETCARP was
installed, which it tells from where the program is: the file the installers leave in its folder,
Homebrew's `Cellar`, Scoop's `apps` folder, winget's packages, pipx's and uv's environments, any
other Python environment (pip), or cargo's `bin` folder. When it cannot tell, as with a release
archive or a program copied by hand, it lists every way instead (step 3 has each one).

To do it:

```sh
jetcarp uninstall --yes
```

It reports each step as *done* or *FAILED* with the reason. The steps it may list are:

- **stop the background host**;
- **put your own desktop picture back**, if the host left a still there (*Leave a still picture when
  it stops*, or `jetcarp host still on`) or will as it stops. JETCARP saved your picture when it
  first put a still up; if you have chosen another picture since, that one is left alone;
- **stop the host starting at login**. On macOS its job is unloaded from launchd too, which would
  otherwise keep it until you log out;
- **remove the GNOME Shell extension** (Linux): it is switched off and its folder deleted. Log out and
  back in once to finish;
- **remove the Windows screen saver** (`JETCARP.scr`), clearing the choice in Windows' screen saver
  settings if JETCARP was the one chosen;
- **remove the macOS screen saver** (`JETCARP.saver`), putting back the screen saver macOS had chosen
  before JETCARP was installed wherever JETCARP is still the chosen one (your desktop picture, shown
  as the screen saver, if nothing was recorded). If System Settings is open, it may keep listing
  JETCARP until you quit and reopen it;
- **forget the recorded host command**, a small file in the settings folder that GNOME's menu and the
  Windows screen saver use to start the host;
- **remove the host's lock file**, the empty `host.lock` in the runtime folder that keeps a second
  host from starting, once no host holds it;
- **remove the empty folder of job locks**, `library.sqlite-jobs` beside the pattern library. A
  paused job's lock in it keeps it, since the job will need it when it resumes.

Run it from your desktop session where you can, since putting your picture back and switching the
GNOME extension off both go through the desktop. From another desktop, a text console or over SSH
it still finds the GNOME extension's files and deletes them; GNOME Shell skips an extension on its
list whose files have gone.

## 2. JETCARP's folders

These options of `jetcarp uninstall` also delete JETCARP's folders:

| Option | Deletes |
| --- | --- |
| `--settings` | The settings folder: the app's and the host's settings, screensaver profiles, rotations, reseed presets you saved, the host's desktop pictures, and a kept MCP HTTP token, but only when everything in it is JETCARP's (below). A picture there that the desktop still shows is kept, and the report names it: on Windows the copy of your own picture that was put back, or a still if it could not be. **Your publisher key and rater book are kept too** (`pack/`: the key, its public half, `publisher.json` and `raters.json`), and the report says where: the key is your identity as the publisher of every pack you have signed, and the rater book holds your raters' names and their consent, and neither can be made again |
| `--publisher-key` | With `--settings`, the publisher key and rater book as well. Copy `pack/` out first if you might publish again. Without `--settings` it is refused |
| `--data` | The data folder: the pattern library with its media, jobs and Quick Start entries (the atlas and your own), and the exploratory search's databases, but only when everything in it is JETCARP's (below). **This cannot be undone** |

To undo everything and delete both folders, keeping your publisher key and rater book if you made
them:

```sh
jetcarp uninstall --yes --settings --data
```

The report then ends with where they are kept:

```text
Kept your publisher key and rater book, in /home/ada/.config/jetcarp/pack (publisher_ed25519, publisher_ed25519.pub, publisher.json, raters.json): the key is the identity behind every pack you signed, and the rater book holds your raters' names and their consent. Add --publisher-key to delete them too.
```

To delete those too, add `--publisher-key`.

On macOS the data folder is inside the settings folder. With both options, the data folder is
deleted first and then the settings folder; `--settings` alone deletes the settings and keeps the
data folder inside it.

`--data` and `--settings` delete their folders only when everything in each is JETCARP's. In the
data folder that is the library and the files SQLite keeps beside it, its folder of job locks, and
`media/`, `searches/` and `explore/` laid out as JETCARP lays them out (on Windows, also the
runtime files and the screen saver, which share the folder). In the settings folder it is the
settings files [Folders](../reference/folders.md#what-lives-in-each-folder) lists, the folders of
screensaver profiles, rotations, desktop stills, *Fit to this computer*'s pictures and kept MCP HTTP
tokens (`http-tokens/`), and `pack/`
with the publisher's files (and the data folder, where it is inside). A folder holding anything
else, such as a folder of documents that `JETCARP_DATA_DIR`, `JETCARP_CONFIG_DIR` or a data folder
chosen in setup names by mistake, is refused before anything is done, even with `--yes`: both
folders are checked first, and the uninstall names each folder it refuses, where its name came
from and what in it is not JETCARP's, and changes nothing. Correct the variable or the choice
(deleting `paths.json` in the settings folder brings the usual data folder back) and run it again;
or, if it really is JETCARP's folder and you want it gone, delete it yourself. Without the option,
the uninstall undoes the rest and keeps the folder. A library alone does not make a folder
JETCARP's, since JETCARP makes `library.sqlite` in whatever folder it is given. A folder that is a
link to another folder (a symbolic link, or a junction on Windows) is not looked into: the option
removes the link and leaves the folder it leads to exactly as it is, with what would otherwise be
kept, such as the picture the desktop shows or the data folder inside. A link inside either folder
is removed as a link too, never followed.

The output folder, with your pictures, wallpapers, script runs and sweeps, is never deleted. If you
want to keep your pattern library, copy `library.sqlite` (and its `media` folder) out of the data
folder first. If you moved the library elsewhere with `JETCARP_LIBRARY`, `--data` does not delete
it. Like every JETCARP command, `jetcarp uninstall` uses the folders that `JETCARP_CONFIG_DIR`,
`JETCARP_DATA_DIR` and `JETCARP_OUT` point to when they are set. To see which folders it uses, while
the program is still installed:

```sh
jetcarp paths
```

### What lives where

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

The Linux paths are the usual ones; they follow your XDG settings. A data folder chosen at setup is
wherever you chose. `jetcarp uninstall` removes the host's lock file (and the runtime folder, if
that leaves it empty); the port number and access token are removed as the app or host that wrote
them quits. It does not remove the macOS screen saver's log, which is small; delete it by hand if
you like.

To remove the GNOME extension by hand, switch it off in GNOME's *Extensions* app before deleting its
folder, or run this, which does both:

```sh
jetcarp host gnome remove
```

## 3. Remove the program

Remove the program last: `jetcarp uninstall` needs it to undo the rest. Then use what installed it
(see [Installing JETCARP](../getting-started/installing.md)); `jetcarp uninstall`'s last lines name
the command for your copy.

### The shell installer

On Linux and macOS:

```sh
curl -LsSf https://github.com/jprodgers/jetcarp/releases/latest/download/jetcarp-installer.sh | sh -s -- --uninstall
```

Or, with the installer already saved as `jetcarp-installer.sh`:

```sh
sh jetcarp-installer.sh --uninstall
```

It deletes `~/.local/lib/jetcarp` and the link `~/.local/bin/jetcarp`, and the folders it made for
them (such as `~/.local/lib`) if they are empty. If you installed with `--dir` or `--bin-dir`, add
the same options again.

Any release's installer removes any version's install, so the latest one, which the first command
fetches, removes an older JETCARP too: it looks for the `.jetcarp-installer` file it left in the
program's folder, removes the program, and stops before it would download anything.

### The PowerShell installer

On Windows, in PowerShell, save the installer:

```powershell
Invoke-WebRequest https://github.com/jprodgers/jetcarp/releases/latest/download/jetcarp-installer.ps1 -OutFile "$env:TEMP\jetcarp-installer.ps1"
```

and run it with `-Uninstall`:

```powershell
powershell -ExecutionPolicy Bypass -File "$env:TEMP\jetcarp-installer.ps1" -Uninstall
```

It deletes `%LOCALAPPDATA%\Programs\jetcarp`, takes it off your user `PATH` if it was there (an
install with `-NoModifyPath` never put it there), and deletes `%LOCALAPPDATA%\Programs` if it made it
and it is empty. If you installed with `-InstallDir`, add it
again. As with the shell installer, any release's installer removes any version's install.

Both installers say what they removed, and name the folders of your settings, data and pictures
that are still there. After `jetcarp uninstall --settings --data`, they are gone, and nothing is said
of them.

### pipx

```sh
pipx uninstall jetcarp
```

### uv

```sh
uv tool uninstall jetcarp
```

### pip

In the Python environment you installed it in:

```sh
pip uninstall jetcarp
```

### Homebrew

```sh
brew uninstall jetcarp
```

Then, if you no longer want JETCARP's tap:

```sh
brew untap jprodgers/jetcarp
```

### Scoop

```powershell
scoop uninstall jetcarp
```

Then, if you no longer want JETCARP's bucket:

```powershell
scoop bucket rm jetcarp
```

### cargo

```sh
cargo uninstall jetcarp
```

### A release archive, or a build from source

Delete the folder you unpacked, or the folder you built JETCARP in.

Each way removes the program with the files that came with it (on Windows `jetcarpw.exe`, the
windowless launcher; on macOS `libjetcarp_saver.dylib`; the starter pack) and nothing of yours: the
settings, data and output folders stay until you delete them (step 2's `--settings` and `--data`,
or by hand).

## 4. Remove JETCARP from your AI assistant

If you connected JETCARP to an AI assistant (see
[Driving JETCARP from an AI assistant](../tutorials/mcp.md)), the assistant keeps it on its list
after the program has gone, and fails to start it: Claude Code's `claude mcp list` shows JETCARP as
"Failed to connect". Remove it there too.

**Claude Code:**

```sh
claude mcp remove jetcarp
```

If you also added JETCARP attached to the app, as `jetcarp-app`:

```sh
claude mcp remove jetcarp-app
```

**Claude Desktop:** delete the `"jetcarp"` entry under `"mcpServers"` in its
`claude_desktop_config.json` (in `~/Library/Application Support/Claude/` on macOS,
`%APPDATA%\Claude\` on Windows), then restart Claude Desktop.

**Other clients:** remove the server whose command is `jetcarp` with the argument `mcp`.
