# Photosensitive seizure warning

**JETCARP's animations can flash and flicker rapidly, and can trigger seizures in people with
photosensitive epilepsy.**

Some rules change most of the screen every generation. A rule with B0, for example, turns the whole
field over at each step. Run fast, full screen or as the desktop background, such a rule can flicker
many times a second.

Flashing like this can also trigger a seizure, occasionally, in someone with no history of seizures.

If you feel dizzy, or notice altered vision, eye or muscle twitching, disorientation or any
involuntary movement, stop at once, look away from the screen, and see a doctor.

## If you have, or may have, a seizure disorder

**If you have, or may have, epilepsy or a seizure disorder, don't use JETCARP.**

JETCARP can flash rapidly, and some of its rules flicker the whole screen many times a second. It
cannot yet detect or limit flashing, so it cannot protect you from it.

An automatic limiter is planned ([#97](https://github.com/jprodgers/jetcarp/issues/97)). If you
continue, JETCARP saves your answer, so that the limiter can switch itself on for you when it comes.
Until then, the safe choice is not to use JETCARP.

## Where JETCARP asks

JETCARP shows this warning before it animates anything on screen. The first time you open the app,
it fills the window, before [setup](first-run.md) and before anything else runs. If you used
JETCARP before this warning existed, it shows the next time you open the app, even though setup has
run. To go on, you confirm that you don't have epilepsy or a seizure disorder:

<img src="../images/getting-started/photosensitivity-1-warning.webp" alt="The app's first screen: the heading Photosensitive seizure warning, the key sentence in bold, the rest of the warning, the question Do you have epilepsy or a seizure disorder?, and two buttons: I don't have epilepsy or a seizure disorder, and I have, or may have, a seizure disorder" width="600">

The other answer leads to the advice above. Its main button is **Quit JETCARP**; a smaller one,
**Continue at my own risk**, goes on:

<img src="../images/getting-started/photosensitivity-2-at-risk.webp" alt="The advice: the heading Please don't use JETCARP, the sentence If you have, or may have, epilepsy or a seizure disorder, don't use JETCARP in bold, a large Quit JETCARP button and a small Continue at my own risk button" width="600">

Either answer that goes on is saved, and the warning is not shown again. Quitting saves nothing, so
the warning shows at the next launch.

`jetcarp setup` asks the same in a terminal, before its own questions. Until one of them has been
answered, nothing else starts anything on screen:

| What | Before the warning has been answered |
| --- | --- |
| `jetcarp host start`, `host wallpaper on`, `host login on`, `host saver install` | Refused, with the warning |
| The background host started at login, or by `jetcarp host serve` | Runs, but holds every screen still; `jetcarp host status` says why, and the scenes start by themselves once the warning has been answered |
| The system's screen saver (Windows), `jetcarp host serve --saver` | Refuses to start, with the warning (on Windows in a message box) |
| The app's screensaver mode and full-screen outputs | Wait for the app's own warning |
| The MCP tool `host_control` | Refuses the actions that start or show scenes (`resume`, `next`, `previous`, `rotation`, `wallpaper`, `login` and `tune` when turning on), with the warning |

What only reads or writes files is not held back: `jetcarp host status`, `jetcarp doctor`, pictures,
GIF and WebP animations, videos and scene files. They don't animate anything on your screen, but
an animation you make can flash like the scene it shows when you play it.

A macOS screen saver installed before this warning existed keeps running as it was built, since
macOS gives it no way to read JETCARP's settings; `jetcarp host saver install` builds it again,
once the warning has been answered.

## Answering from the command line

`jetcarp setup` in a terminal shows the warning and asks. Where there is no terminal to ask in, or
with `--yes`, it needs the answer given as `--seizure-warning`, unless it was answered before:

| Answer | Means |
| --- | --- |
| `--seizure-warning no-disorder` | You don't have epilepsy or a seizure disorder |
| `--seizure-warning at-risk` | You have, or may have, a seizure disorder, and go on at your own risk, against the advice above |

With no `--profile`, only the answer is saved:

```text
$ jetcarp setup --yes --seizure-warning no-disorder
saved your answer to the photosensitive seizure warning: no epilepsy or seizure disorder
```

## Where the answer is kept

The answer is `photosensitivity.json` in the [settings folder](../reference/folders.md), with the
date and the version of JETCARP that saved it:

```json
{
  "version": 1,
  "answer": "no_disorder",
  "date": "2026-11-16T12:00:00Z",
  "jetcarp": "0.4.2",
  "via": "app"
}
```

`answer` is `no_disorder` or `at_risk`, and `via` is `app` or `setup`. The app, the command line,
the background host and the MCP server all read this file. Deleting it, or the whole settings
folder, brings the warning back. The planned limiter will read `at_risk` to switch itself on.

## Lowering the risk

Fast, large changes of brightness are what can trigger a seizure. Guidance on flashing content, such
as [WCAG 2.3.1](https://www.w3.org/WAI/WCAG22/Understanding/three-flashes-or-below-threshold.html),
allows no more than three flashes a second over a large part of the screen. A B0 rule run at 30
generations a second flips the whole field 30 times a second.

Slower speeds, smaller windows, low-power pacing (`jetcarp host pacing low-power`) and rules
without B0 all flash less. None of this makes JETCARP safe for someone with a seizure disorder.
