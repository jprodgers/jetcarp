# First run and setup

To open the app, run `jetcarp` with no arguments in a terminal, on any system, or open JETCARP from
its icon in your applications menu, Launchpad or the Start menu, where the installers put it
(`jetcarp launcher install` puts it there after any other way of installing; see
[Installing](installing.md#open-the-app)).

The first time you open the app, before anything else, it shows the
[photosensitive seizure warning](photosensitivity.md), which asks whether you have epilepsy or a
seizure disorder. It shows once; setup comes after it.

Then JETCARP asks a few questions in a small *Set up JETCARP* window: what you would like it for,
where its data should go, which packs to import and whether to use it on your desktop. Your answers choose a *setup profile*. Setup then measures your computer (about a
minute, unless you untick it), and imports packs and searches for Quick Start entries of your own,
as background jobs, while you use the app. Nothing is downloaded: everything comes with JETCARP or
stays on your computer.

You can skip all of it. **Later** closes the window and asks again next time; the *Blank slate*
profile sets nothing up and never asks again. **Help → Setup…** opens the window whenever you like,
and `jetcarp setup` asks the same questions in a terminal.

## The profiles

| Profile | Data folder | Packs | Quick Start build | Desktop |
| --- | --- | --- | --- | --- |
| Desktop art | the usual one | none | none | offers the live wallpaper and the screen saver |
| Explorer | the usual one | the starter pack | 10 minutes (you can leave it out) | – |
| Researcher | one you choose | the starter pack and packs you choose | 30 minutes | – |
| Everything | one you choose | every pack that came with JETCARP and packs you choose | 60 minutes | offers the live wallpaper and the screen saver |
| Blank slate | the usual one | none | none | – |

Every profile keeps the built-in examples in the Quick Start menu.

- **Desktop art** is for moving pictures on your desktop. It imports and builds nothing, and offers
  the live wallpaper and the screen saver (see [The desktop offers](#the-desktop-offers)).
- **Explorer** imports the [starter pack](installing.md#the-starter-pack), whose patterns go into
  your library and whose scenes go into the Quick Start, and runs a ten-minute *Quick Start build*
  (see below).
- **Researcher** is for hunting and studying patterns. Libraries and searches grow large, so it asks
  where the data should go. It imports the starter pack and any packs you add from your own folders,
  and runs a 30-minute build.
- **Everything** imports every pack that came with JETCARP and any you add, runs an hour's build, and
  makes the desktop offers.
- **Blank slate** sets nothing up. The built-in examples are all there, and you can do everything
  later by hand.

**The Quick Start build** is an [exploratory search](../guides/searching.md) over every tiling and
rule family, on your GPU when you have one, for the minutes you choose. When the minutes are spent,
it adds its best runs to the Quick Start as an atlas, picked per category, rule family and tiling
(as `jetcarp quickstart publish` does). Each computer's build searches from a starting point of its
own, so yours finds things no one else's has. The search ends on time: when the minutes are spent,
runs still going on the processor are given up, and the GPU's last batches are sized to end with
them (one that would run on is given up too). Adding the atlas to the Quick Start then takes a few
seconds more (up to a minute on a slow computer), because each entry's run is replayed to where it
is most interesting. On a computer without a usable GPU it runs on the processor and finds less in
the time. It leaves two of the computer's cores free, so the computer stays usable meanwhile, and
its progress moves every couple of seconds.

The window shows roughly how much disk space and time each profile takes on your computer. The
estimate counts the packs and about 8 MB for each minute of the build; its time is the build's
minutes, half a minute to add its atlas, and the imports.

## What the window asks

**Welcome.** What JETCARP is, in two sentences, and a reminder that you can change your answers
later.

<img src="../images/getting-started/setup-1-welcome.png" alt="The Set up JETCARP window's first page: Welcome to JETCARP, with Later and Next buttons" width="600">

**What for.** The five profiles, each with a line saying what it does and what it takes. Pick one
and press **Next**. The pages after it depend on the profile: the steps along the top show which.

<img src="../images/getting-started/setup-2-profile.png" alt="The five profiles, Desktop art, Explorer, Researcher, Everything and Blank slate, each with a line and its cost; Explorer is chosen" width="600">

**Data folder** (Researcher and Everything). Where the library, its pictures and your searches go,
with the free space on that disk. Type a folder or press **Choose…**. The folder must be empty, or
already hold a JETCARP data folder; setup never moves a library. **Default** goes back to the usual
folder. The choice is kept in `paths.json` in JETCARP's settings folder (see
[Where things go](#where-things-go)).

<img src="../images/getting-started/setup-3-data.png" alt="The data folder page: a folder on another disk typed in, Choose and Default buttons, and the free space on that disk" width="600">

**Packs** (Explorer, Researcher and Everything). The packs that came with JETCARP, with what is in
each, ticked to import. A pack already in your library is shown as such and not imported again.
Researcher and Everything also have **Add packs from a folder…**, for packs you were given or
downloaded: choose a pack store, an export, or a folder holding several. **Remove** leaves a folder
out again. Each pack says who signed it, with the start of their key (hover for the whole key). A
pack from your own folders whose key no pack in your library was signed with is marked **New key:
never seen in this library**: worth a look if you expected it from someone whose packs you already
have ([Packs](../guides/packs.md#check-a-pack) says more). Setup imports only packs their publisher
signed.

<img src="../images/getting-started/setup-4-packs.png" alt="The packs page for Explorer: the starter pack ticked, with its patterns, Quick Start entries and size, and who signed it" width="600">

<img src="../images/getting-started/setup-4-packs-researcher.png" alt="The packs page for Researcher: the starter pack, signed by Jimmie Rodgers, and a folder added with Add packs from a folder, holding a pack signed by Ada Example and marked New key: never seen in this library" width="600">

**Desktop** (Desktop art and Everything). The desktop offers, each off until you tick it.

<img src="../images/getting-started/setup-5-desktop.png" alt="The desktop page: Show a live wallpaper on the desktop ticked, Start the live wallpaper when you log in, and Use JETCARP as the screen saver greyed out on Linux" width="600">

**This computer** (every profile). Setup measures your computer first, unless you untick **Measure
this computer first**: about a minute of JETCARP's own work, with nothing drawn but a progress bar,
for the [JETCARP Hardware Score](../guides/this-computer.md) and limits that fit the computer. It
runs before the jobs start, so they do not slow it. Without it, the limits come from the computer's
memory alone, and **Tools → This computer…** measures it whenever you like.

<img src="../images/getting-started/setup-5b-measure.webp" alt="The This computer page: Measure this computer first, ticked, and what the limits are without measuring: 8 GB reserved for JETCARP, worlds of up to 4 GB" width="600">

**Summary.** What setup will do: the data folder, the packs, whether it measures the computer first,
and the Quick Start build, which you can switch off (Explorer) or lengthen and shorten, from 1
minute to 600. It also shows whether the
build runs on the GPU, and roughly what it all takes. **Start** begins; for a profile with nothing to
import or build the button says **Finish**.

**Your apps** (every profile), when JETCARP isn't among them yet: **Add JETCARP to the Start menu**
on Windows, **Add JETCARP to Launchpad** on macOS (as *JETCARP.app* in your Applications folder) or
**Add JETCARP to the apps menu** on Linux, ticked, so that JETCARP opens from its icon like your
other apps. It does what `jetcarp launcher install` does (see
[Open the app](installing.md#open-the-app)), and the next page says how it went. The installers
have done it already, so after them the box isn't there.

<img src="../images/getting-started/setup-6-summary.png" alt="The summary for Explorer: the data folder, the starter pack, measuring the computer first, Add JETCARP to the apps menu, and a ten-minute search on the GPU, about 83 MB and 11 minutes" width="600">

**Setting up.** Each job with its progress; each import says who signed the pack and what your
library knew of their key, with a new key marked again below it. They run in the background, so **Continue in the
background** closes the window and they carry on; **Tools → Hunts…** shows them too, and the new
entries appear in the Quick Start as they arrive. A job that was cut off, because the app closed or
the computer restarted, has a **Resume** button here and in the Hunts window. When everything is
done the button says **Close**.

<img src="../images/getting-started/setup-7-progress.png" alt="Setting up for Explorer: the starter pack imported, and the Quick Start build running, with Continue in the background" width="600">

<img src="../images/getting-started/setup-8-done.png" alt="Explorer: all set; the starter pack imported and the Quick Start build done, with the number of atlas entries it published" width="600">

The pictures here were made with the starter pack and a one-minute build, so the numbers on your
computer will differ.

## The desktop offers

Desktop art and Everything offer three things. Each changes your desktop, so none is done unless
you tick it:

- **Show a live wallpaper on the desktop** turns the wallpaper on and starts the background host,
  as `jetcarp host wallpaper on` and `jetcarp host start` do. On GNOME it installs JETCARP's GNOME
  Shell extension too, which GNOME loads only at login: setup ends by saying so, and until you log
  out and back in once the scene shows in a window whose title says the same.
- **Start the live wallpaper when you log in**, as `jetcarp host login on` does.
- **Use JETCARP as the screen saver** (Windows and macOS), as `jetcarp host saver install` does. It
  opens the system's screen saver settings, where you choose JETCARP.

[Wallpapers, screensavers and the background host](../guides/wallpapers.md) explains each, and
`jetcarp uninstall` undoes them all.

## From the command line

`jetcarp setup` does the same in a terminal. It shows the
[photosensitive seizure warning](photosensitivity.md) first, unless it was answered before. With no
`--profile` it asks the questions there; with `--profile NAME --yes` it asks nothing, for scripts,
and the first time it needs the warning answered with `--seizure-warning`. Either way it says what
it will do, then does it, with the jobs' progress on a line below. Here with a one-minute build:

```sh
jetcarp setup --profile explorer --yes --quickstart-minutes 1 --seizure-warning no-disorder
```

It prints:

```text
saved your answer to the photosensitive seizure warning: no epilepsy or seizure disorder

setting up JETCARP as Explorer:
  1. record the profile Explorer in /home/ada/.config/jetcarp/setup.json
  2. import JETCARP starter pack (26 patterns, 79 Quick Start entries, 8 ant runs, 1.5 MB)
  3. search for 1 minute on the GPU (AMD Radeon 8060S Graphics (RADV STRIX_HALO)), then add its best runs to the Quick Start
and add JETCARP to the apps menu (`jetcarp launcher install`)
pack import job 1 in /home/ada/.local/share/jetcarp/library.sqlite
explore search job 2 in /home/ada/.local/share/jetcarp/library.sqlite
GPU: AMD Radeon 8060S Graphics (RADV STRIX_HALO) (radv), Vulkan
pack import 1: JETCARP starter pack imported: 26 patterns new, 0 merged, 79 Quick Start entries, 8 ant runs (done)
Quick Start build (explore 2): 873 atlas entries published (873 new, 0 updated, 0 removed); 9,980 runs (115/s), 2,994 elites in 1,664 niches (122 of them confirmed), 13 of 1,359 tilings; AMD Radeon 8060S Graphics (RADV STRIX_HALO) (radv), Vulkan (done)
recorded Explorer in /home/ada/.config/jetcarp/setup.json
added JETCARP to the applications menu (/home/ada/.local/share/applications/jetcarp.desktop), with its icon in /home/ada/.local/share/icons/hicolor
Open the app with `jetcarp`; `jetcarp setup` again changes your mind.
```

The outputs on this page come from Linux; on macOS and Windows the folders in them differ (see
[Folders](../reference/folders.md#the-three-folders)).

| Option | Does |
| --- | --- |
| `--profile NAME` | `desktop-art`, `explorer`, `researcher`, `everything` or `blank-slate` |
| `--data-dir DIR` | Keep JETCARP's data in this folder from now on (empty, or already a JETCARP data folder) |
| `--quickstart-minutes N` | The build's minutes instead of the profile's; `0` for none |
| `--packs DIR` | Import the packs in this folder too (repeatable) |
| `--yes` | Ask nothing. The desktop offers are only printed, as the commands that do them; JETCARP is added to the system's apps, if it isn't there yet, as the app's setup does by default |
| `--no-launcher` | Leave JETCARP out of the Start menu, Launchpad or the apps menu (asked in a terminal otherwise, when it isn't there yet) |
| `--seizure-warning ANSWER` | Answer the [photosensitive seizure warning](photosensitivity.md) without being asked: `no-disorder` (you don't have epilepsy or a seizure disorder) or `at-risk` (you have, or may have, one, and go on at your own risk). `--yes` needs it the first time; with `--yes` and no `--profile`, only the answer is saved |
| `--no-gpu` | Build on the processor even when there is a GPU |
| `--status` | Only show where things stand (below) |

`jetcarp setup --status` shows whether setup has run and with which profile, the folders and the
free space, the packs that came with JETCARP, whether the build can use a GPU, the library's counts
(its Quick Start entries by where they came from), setup's jobs, and what each profile would take
here. After the setup above:

```sh
jetcarp setup --status
```

It prints:

```text
setup: Explorer on 2026-11-12 11:57 UTC (JETCARP 0.4.0)
  settings: /home/ada/.config/jetcarp
  data:     /home/ada/.local/share/jetcarp, 7.4 TB free
  library:  /home/ada/.local/share/jetcarp/library.sqlite
  output:   /home/ada/Pictures/JETCARP
  bundled:  /home/ada/.local/lib/jetcarp/packs
packs that came with JETCARP:
  JETCARP starter pack (jetcarp-starter/jetcarp-starter): 26 patterns, 79 Quick Start entries, 8 ant runs, 1.5 MB; the starter pack, imported
GPU: AMD Radeon 8060S Graphics (RADV STRIX_HALO) (a Quick Start build runs on it)
library: 26 patterns, 8 ant runs, 24 jobs; Quick Start entries: 364 built in, 79 from packs, 873 from searches
setup's jobs:
  pack import 1: JETCARP starter pack imported: 26 patterns new, 0 merged, 79 Quick Start entries, 8 ant runs (done)
  Quick Start build (explore 2): 873 atlas entries published (873 new, 0 updated, 0 removed); … (done)
profiles:
  desktop-art  Desktop art: Moving pictures for your desktop: … (no extra disk space or time)
  explorer     Explorer: The built-in examples and the starter pack's patterns, and a ten-minute search … (about 80 MB and 11 minutes)
  …
```

`jetcarp quickstart build --minutes N` runs a Quick Start build by itself, at any time.

## Changing your mind

Run setup again, from **Help → Setup…** or `jetcarp setup`, and choose another profile. Setup only
ever adds: a pack already in your library is skipped, a new build adds an atlas of its own beside
the earlier ones, and nothing is deleted. A build that was cut off is carried on, not started again.
To undo parts by hand:

- hide or remove Quick Start entries from a card's menu in the app, or with
  `jetcarp library quickstart` (see [Building your own Quick Start](../tutorials/building-quick-start.md));
- remove a build's search and its runs with `jetcarp jobs remove JOB --delete-results` (its atlas
  entries stay in the Quick Start until you hide or remove them);
- turn the desktop offers off with `jetcarp host`, or all at once with `jetcarp uninstall`;
- go back to the usual data folder with **Default** on the data folder page, `--data-dir default`,
  or by deleting `paths.json` from the settings folder.

Setup never moves a library from one data folder to another. To move yours, quit JETCARP, move the
data folder's contents to the new folder yourself, then choose the new folder.

## Where things go

- **The choice** is recorded in `setup.json` in the settings folder: the profile, the date, the
  JETCARP version, the data folder, and the jobs setup started. Its being there is what tells the
  app that setup has run.
- **A data folder you chose** is kept in `paths.json` in the settings folder, and every part of
  JETCARP (the app, the command line, the background host, the MCP server) uses it from then on.
  The `JETCARP_DATA_DIR` variable still wins over it, and `JETCARP_LIBRARY` still names the library
  (see [Folders and environment variables](../reference/folders.md)).
- **Imported packs** go into the library, as `jetcarp pack import` puts them (see
  [Packs](../guides/packs.md)).
- **The Quick Start build** is an explore job in the library, with its runs in its own folder,
  `searches/<uuid>/` in the data folder, and its atlas in the library's Quick Start table. Like any
  other job it is listed by `jetcarp jobs list` and in the Hunts window, labelled *Quick Start
  build*.

An AI assistant connected through [MCP](../tutorials/mcp.md) can read where things stand
(`help` with the topic `setup`), but cannot run setup: that choice is yours.
