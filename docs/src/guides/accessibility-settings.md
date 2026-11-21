# Accessibility settings

*Settings → Accessibility* holds the app's settings for reduced motion, the high-contrast theme,
light or dark, and the palettes that scenes are drawn in when they don't choose their own. Reduced
motion and high contrast follow your system's settings unless you choose otherwise here. The app
reads the system's settings when it starts and every couple of seconds while it runs, so a change
you make in your system's settings shows within a moment, without a restart.

<img src="../images/guides/accessibility-window.webp" alt="The Accessibility window: Reduced motion, with Follow the system chosen; Theme, with Follow the system, High contrast and Standard, and Light or dark; and Palettes for scenes that name none, with a button that uses the palettes chosen for colour-vision deficiency and a palette list for most rules, JACA's +1 cells and JACA's −1 cells" width="520">

The settings are saved with the app's other settings (the `accessibility` section of `app.json`
in the [settings folder](../reference/folders.md)). They are separate from the strobe guard, which
is in *Settings → Safety*: the guard acts on what is drawn, whatever these settings say (see
[The strobe guard](strobe-guard.md)).

## Reduced motion

Each system has a setting that asks apps for less motion. The app reads:

| System | Setting | Where to find it |
| --- | --- | --- |
| Windows | *Show animations in Windows* (`SPI_GETCLIENTAREAANIMATION`); off asks for reduced motion | *Settings → Accessibility → Visual effects → Animation effects* |
| macOS | *Reduce motion* (`accessibilityDisplayShouldReduceMotion`) | *System Settings → Accessibility* (under *Display*, or *Motion* in later versions) |
| Linux (GNOME) | `org.gnome.desktop.interface enable-animations`; off asks for reduced motion | *Settings → Accessibility → Seeing → Reduce Animation* |

On Linux the app asks the desktop portal, which other desktops answer too
(`org.freedesktop.appearance reduced-motion`), and asks `gsettings` where there is no portal. Where
nothing can be read, the app takes it that reduced motion is not asked for.

When reduced motion is on:

- **The Quick Start's cards show stills.** Each card shows its entry's picture, still, and *Live*
  starts off. Tick *Live* to run the cards anyway; the app keeps your choice until the setting
  changes.
- **The small animations in other windows show stills until you choose one.** The finds in the
  [Hunts dashboard](searching.md#in-the-app) and an ant run's animation in *Ants and turmites* show
  their first frame, with a small ▶ in the corner. Click one (or press Enter while it has the
  keyboard) to play it; click it again to still it.
- **A scene opens paused,** with a note in the status line: "Paused because your system asks for
  reduced motion. Space runs it; Settings → Accessibility changes this." Space, or *Run → Run /
  Pause*, runs it as usual.
- **The interface's own animations are off:** windows and menus appear at once, collapsing headers
  open without sliding, scrolling jumps rather than glides, and spinners and progress bars stand
  still.

A screensaver profile or rotation you start still runs: you asked for it. The desktop background
and the screen savers have their own settings (see
[Wallpapers, screensavers and the background host](wallpapers.md)).

**Follow the system** (the default) does what the system says. **Reduce motion** turns it on and
**Don't** turns it off, whatever the system says.

## High contrast

The high-contrast theme draws the app's controls in black and white, in a dark or a light form:

<figure class="pair">
<div><img src="../images/guides/accessibility-high-contrast-dark.webp" alt="The app in the dark high-contrast theme: white text on black panes and menus, white outlines round every button and field, and a JACA world on the Cairo tiling in blue and orange" width="480"></div>
<div><img src="../images/guides/accessibility-high-contrast-light.webp" alt="The same app in the light high-contrast theme: black text on white panes and menus, black outlines round every button and field" width="480"></div>
<figcaption>The high-contrast theme, dark and light, with the Accessibility window open over a JACA
world drawn in Cobalt and Saffron, the palettes chosen for colour-vision deficiency. Scene
<code>docs/scenes/guides/accessibility-jaca.json</code>, 120 generations in.</figcaption>
</figure>

- **Text** is at a contrast of 7:1 or more against whatever it is drawn on: panes, windows, menus,
  buttons, fields and striped rows ([WCAG 2.2](https://www.w3.org/TR/WCAG22/#contrast-enhanced),
  level AAA). That includes secondary text, which the standard theme draws faint, and the app's
  coloured words (errors, notes, *Valid tile*), which keep their kind of colour but are made as
  legible as the text around them. Their meaning is in the words too, never in the colour alone.
- **Outlines** round buttons, fields and windows, separators, and the boxes and rails of check
  boxes and sliders are at 3:1 or more
  ([WCAG 2.2](https://www.w3.org/TR/WCAG22/#non-text-contrast)), and thicker than the standard
  theme's.
- **The keyboard focus** is a thick gold ring (navy in the light form) round whatever has the
  keyboard, and round a field you are typing in.

A test in the app's code computes the contrast of every text and outline colour the theme uses
against every background it is drawn on, and fails under those limits. What the theme doesn't
change: the world itself, which is drawn in its palettes; the pictures in the Quick Start's cards
and other windows, which keep their own dark frames; and the photosensitive seizure warning, which
keeps its own high-contrast look.

The app follows your system's high-contrast setting:

| System | Setting |
| --- | --- |
| Windows | The contrast themes (*Settings → Accessibility → Contrast themes*), read as `SPI_GETHIGHCONTRAST` |
| macOS | *Increase contrast* (*System Settings → Accessibility → Display*), read as `accessibilityDisplayShouldIncreaseContrast` |
| Linux (GNOME) | *High Contrast* (*Settings → Accessibility → Seeing*), `org.gnome.desktop.a11y.interface high-contrast`, or the portal's `org.freedesktop.appearance contrast` |

**High contrast** turns the theme on and **Standard** off, whatever the system says.

**Light or dark** chooses the form, for the standard theme and the high-contrast one alike:
*Follow the system* (the default), *Dark* or *Light*. On Windows and macOS the app follows the
system's light or dark choice; on Linux it stays dark unless you choose *Light* here
([#137](https://github.com/jprodgers/jetcarp/issues/137)).

## Palettes for scenes that name none

A scene can name each automaton's palette. When it doesn't, the app draws it in these: one palette
for most rules (JETCARP's own default is Ember), and two for JACA's +1 and −1 cells (Tide and Koi).
**Use the palettes chosen for colour-vision deficiency** sets all three to the palettes chosen to
stay apart for protanopia, deuteranopia, tritanopia and their milder forms: Lantern, and Cobalt
and Saffron for JACA (see
[Palettes for colour-vision deficiency](palettes.md#palettes-for-colour-vision-deficiency)). Or
choose any built-in palette for each. The window says how many built-in palettes pass JETCARP's
colour-vision check, and names JACA's pairs that stay apart.

The choice applies to the scenes you open after making it: Quick Start entries, scene files, the
library's patterns and AI control's worlds. A scene that names its palettes keeps them, and a scene
you save names the palettes it was drawn in, so it looks the same wherever it is opened. Outside
the app, `--palette` on the commands that draw pictures does the same job.

## Not yet covered

- The app hasn't yet been checked from start to end with the keyboard alone or with a screen
  reader ([#219](https://github.com/jprodgers/jetcarp/issues/219)).
- The Quick Start's card pictures keep the palettes their entries name; the palettes chosen here
  apply when an entry is opened.
