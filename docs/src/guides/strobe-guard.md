# The strobe guard

JETCARP's animations can flash and flicker rapidly, and flashing can trigger seizures in people with
photosensitive epilepsy (see the [photosensitive seizure warning](../getting-started/photosensitivity.md)).
The strobe guard watches every frame JETCARP draws, and softens, slows or pauses a scene before it
flashes more than three times a second over a large part of the screen, or shows strong regular
stripes.

**The guard lowers the risk; it does not remove it.** It does not make JETCARP safe for someone with
a seizure disorder, and it does not certify that anything JETCARP shows or saves meets any guideline.
The photosensitive seizure warning still stands.

The guard is **on for everyone**. Someone who answered the warning *I have, or may have, a seizure
disorder* can't turn it off. Someone who answered *I don't* can turn it off in the app, through a
deliberate confirmation ([Turning it off](#turning-it-off)).

## What it measures

The guard measures what reaches the screen, never what a rule "should" do. A rule with B0 run at
twice the frame rate shows the same phase in every frame and does not flash at all; a slow rule with
a bright palette can. Each frame is reduced to tiles of 16 by 16 pixels, each with its mean
brightness and its mean "redness", worked out on the GPU where the frame is drawn there and on the
processor otherwise.

The definitions are those of [WCAG 2.3.1](https://www.w3.org/WAI/WCAG22/Understanding/three-flashes-or-below-threshold.html),
as the Trace Center's PEAT and EA's [IRIS](https://github.com/electronicarts/IRIS) apply them:

- **Brightness** is relative luminance: each colour channel decoded from sRGB to linear light, then
  `0.2126 R + 0.7152 G + 0.0722 B`, from 0 (black) to 1 (white).
- **A transition** is a change of brightness of at least 0.1 in one direction, where the darker of
  its two states is below 0.8. **A flash** is a pair of opposite transitions.
- **A red transition** is the same for saturated red (red at least 80% of red, green and blue
  together): a change of at least 20 in `(R - G - B) × 320`.
- **More than three flashes in any one second fail**: more than six transitions in a sliding
  window of one second. Two or three flashes a second is a warning, and a warning that lasts four
  seconds out of five fails too (IRIS's extended failure).

**The area rule.** Flashes are harmful when the flashing areas together cover more than a quarter
of any 10° field of vision. How many pixels that is depends on the screen and on how far away you
sit, which JETCARP cannot know, so it assumes a close, conservative case: 96 pixels an inch for each
step of the screen's scale factor, watched from 40 cm. On a screen at a scale of 1 the field is
265 pixels across, and a quarter of it is about 13,700 pixels: under 0.7% of a 1080p screen. A
closer view, or larger pixels, would make the field smaller; few people watch from closer. The
field slides over the screen a tile at a time. Alongside, the mean brightness of the whole screen
is tracked the way IRIS tracks a recording's frame, so anything IRIS would count on a recording of
the screen is counted too. A view smaller than the field (the grid in a small window, a preview)
counts against the whole field, so it can flash all over and pass.

A tile counts what changes its mean, so a checkerboard of squares smaller than a tile reversing in
step keeps every tile's mean: it is not a flash. It is caught as a stripe pattern instead.

**Stripes.** Regular high-contrast stripes can trigger seizures as flashes can
([ITU-R BT.1702](https://www.itu.int/rec/R-REC-BT.1702), the Harding guidance). The guard looks
for them four times a second, in a half-resolution copy of the frame, in windows of 16 by 16
samples at four scales, so stripes from 4 to about 128 pixels a pair are seen. A window has stripes
when its light and dark parts differ by at least 0.05 in brightness (the dark ones below 0.8); when
its rows or its columns cross from light to dark regularly, the same number of times in each line
and a period apart; and when it matches itself shifted by a period and is its own negative shifted
by half of one. Stripes at any angle, checkerboards and rows of dots all count; noise, random cells,
and cell outlines over dead cells do not. Stripes covering more than a quarter of the screen, with
more than five light-dark pairs across them, are a harmful pattern; harmful for half a second, they
fail, as IRIS counts them. The guidance allows still stripes over up to 40% of the screen; JETCARP,
like IRIS, uses a quarter for still and moving stripes alike. Stripes finer than 4 pixels a pair
blur into the half-resolution copy and are not measured; at the assumed distance they are more
than six cycles a degree.

## What it does

It acts in order, as far as it has to:

1. **Soften.** As soon as a quarter of a 10° field has made two transitions in a second (or the
   screen's mean has), the tiles that are changing, and those around them, are softened: each
   tile's brightness may change by at most 0.3 a second, so a flip becomes a fade, and what is
   shown makes at most three transitions a second whatever is drawn. A tile stays softened for a
   second and a half after it last changed. Tiles elsewhere are shown as drawn.
2. **Slow.** A scene still flashing after a second of softening is slowed, so that it can be seen
   again rather than as a wash: its speed is capped at what brings it to one flash a second, and
   lowered again every second and a half while it still flashes, down to half a generation a
   second. Slowed, it no longer flashes on its own, and the softening lets go.
3. **Pause.** When neither is enough (the scene still flashes at half a generation a second, so its
   flashing does not come from its generations), or more than three flashes a second reached the
   screen, the scene is paused, with a note saying why.

Strong stripes are **veiled** where they are found: blended towards their own mean brightness
until their contrast is 0.04, below what counts. The veil comes on and goes over about a second,
so it never flashes itself.

The guard changes only what is shown and how fast generations are shown. A world computes the same
generations whatever the guard does, and a scene's hash is the same.

## Where it works

| Where | Which switch |
| --- | --- |
| The app's view, full screen, and each output window | The app's |
| The world an AI acts on through [AI control](../tutorials/mcp.md) (the app's view) | The app's |
| The app's screensaver mode | The desktop's |
| The desktop background on Linux (the GNOME extension included), Windows and macOS | The desktop's |
| The screen savers: Windows' `.scr`, and macOS's `JETCARP.saver` | The desktop's |

The desktop background and the screen saver are drawn by the background host, which reads the
switch again every couple of seconds. macOS runs its screen saver in a sandbox that cannot read
JETCARP's settings, so `jetcarp host saver install` bakes the switch into the bundle as it is then;
install it again after changing the switch.

## Turning it off

The guard can be turned off only in the app, in **Settings → Safety**, and only by someone who
answered the warning *I don't have epilepsy or a seizure disorder*. For someone who answered *I have,
or may have, a seizure disorder*, the window says the guard is always on, and offers no switch.

The app and the desktop background have a switch each. **Turn off the strobe guard…** (and, for
the desktop background and the screen saver, **Turn off the strobe guard for the desktop…**) opens
a confirmation:

> **Turn off the strobe guard?**
>
> The guard slows or softens scenes that flash more than three times a second, or show strong
> flickering stripes. Without it, some scenes will flash rapidly at full speed.
>
> ☐ I don't have epilepsy or a seizure disorder, and I understand that rapid flashing can
> occasionally trigger a first seizure in someone with no history of seizures.
>
> **Keep the guard on** · Turn it off

<img src="../images/guides/strobe-guard-safety.webp" alt="Settings, Safety: the strobe guard's two switches, both on, each with its Turn off button; over them the confirmation Turn off the strobe guard?, its explanation, an unticked box beginning I don't have epilepsy or a seizure disorder, a highlighted Keep the guard on button, and a greyed-out Turn it off button" width="600">

The box starts unticked, *Turn it off* works only with it ticked, and *Keep the guard on* is the
default (Escape keeps it on too). The desktop background's confirmation adds a sentence: *The
desktop background and the screen saver can be seen by anyone in the room, who has not answered
this warning. One of them may have epilepsy, or not know that they do.* That is why its switch is
separate: turning the guard off in the app leaves it on for the desktop background and the screen
saver.

The choice is saved with the warning's answer, with its date and version
([Where the answer is kept](../getting-started/photosensitivity.md#where-the-answer-is-kept)).
**Turn the strobe guard back on** undoes it at any time. Answering the warning again starts with
both switches on.

Nothing else turns the guard off: not the command line, not a script, not the MCP tools, and so not
an AI. They can read the switches, and turn them back on:

```sh
jetcarp guard            # the switches, and the answer
jetcarp guard --on       # both back on (or --on app, --on desktop)
```

## What it tells you

While the guard acts, the app's status bar says so (*Strobe guard: softening flashes over 40% of
the view.*, *Strobe guard: slowed to 2 generations a second, because the scene flashed 4 or more
times a second.*); clicking it opens Settings → Safety, which shows what the guard is doing now,
the readings behind it, and what it did lately. With the guard off, the status bar says when a scene
flashes. The background host logs what the guard does (`jetcarp host status`), and each screen's
`stage.strobe_guard` says what it is doing there; the MCP tool `app_display` and `jetcarp host
status` report the switch as `strobe_guard`.

## What it costs

Measuring is one read of each frame on the GPU, in a compute pass. On an AMD Radeon 8060S it takes
0.02 ms a frame at 1920 × 1080 (0.1% of a 60 Hz frame) and 0.08 ms at 3840 × 2160; on a weak
integrated GPU, which reads memory perhaps a tenth as fast, it should stay near 1 to 2% of a frame
at 1080p. Softening costs more, 0.1 ms a frame at 1080p on the same GPU, but only while it acts.
The stripe check takes about 3 ms of one processor core for a 1080p frame, four times a second. On
a surface the guard cannot read in place, each frame is drawn into a texture and copied to the
screen.

## What it cannot do

- It sees only what JETCARP draws: not other programs, and not what your screen does with the
  picture (its brightness, its contrast, any motion smoothing).
- It assumes a viewing distance and a pixel size. Watching from closer than 40 cm, or on a screen
  with larger pixels than 96 an inch at its scale factor, makes a 10° field fewer pixels than it
  assumes.
- It acts on a frame or two of readings: a scene that starts flashing shows a few transitions
  before the softening starts. It starts at two, so even then what reaches the screen stays at
  three flashes a second or fewer.
- It cannot tell whether a pattern is provocative for you in particular. Patterns and flashes can
  be uncomfortable well below the thresholds.

How the guard is checked, without anyone watching flashing scenes, is described in
[How JETCARP is tested](../about/testing.md#the-strobe-guard).
