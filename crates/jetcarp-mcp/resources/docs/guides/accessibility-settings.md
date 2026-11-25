# Accessibility settings

*Settings → Accessibility* holds the app's settings for reduced motion, the high-contrast theme,
light or dark, the fonts, and the palettes that scenes are drawn in when they don't choose their
own. Reduced
motion and high contrast follow your system's settings unless you choose otherwise here. The app
reads the system's settings when it starts and every couple of seconds while it runs, so a change
you make in your system's settings shows within a moment, without a restart. The app can be used
from the keyboard alone and with a screen reader: see
[Using the app from the keyboard](keyboard.md).

<img src="../images/guides/accessibility-window.webp" alt="The Accessibility window: Reduced motion, with Follow the system chosen; Theme, with Follow the system, High contrast and Standard, and Light or dark; Font, with Atkinson Hyperlegible Next chosen for the font and Atkinson Hyperlegible Mono for the monospace font; and Palettes for scenes that name none, with a button that uses the palettes chosen for colour-vision deficiency and a palette list for most rules, JACA's +1 cells and JACA's −1 cells" width="520">

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
  Pause*, runs it as usual. When the [strobe guard](strobe-guard.md) has a note too, the guard's
  comes first and this one follows it on the same line; with the status bar off, the two show over
  the grid's top left, this one below the guard's.
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
<div><img src="../images/guides/accessibility-high-contrast-dark.webp" alt="The app in the dark high-contrast theme: white text on black panes and menus, white outlines round every button and field, the Accessibility window open with its Font section, and a JACA world on the Cairo tiling in blue and orange" width="480"></div>
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
  keyboard, and round a field you are typing in. (The standard themes have a ring too, two points
  of blue; see [Using the app from the keyboard](keyboard.md#moving-about).)

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
*Follow the system* (the default), *Dark* or *Light*. The app follows the system's light or dark
choice: on Windows and macOS, and on Linux through the desktop's settings portal
(`org.freedesktop.appearance color-scheme`, or GNOME's own `org.gnome.desktop.interface
color-scheme` where there is no portal). GNOME's *Light* style says no preference there, and the
app takes it as light; a desktop it can't read at all leaves the app dark.

Under a Windows contrast theme, Windows says the style is light whatever the theme's colours (it
sets `AppsUseLightTheme` to 1), so there the app takes the form from the theme itself: whether its
window colour (`GetSysColor(COLOR_WINDOW)`) is dark or light. *Night sky*, *Aquatic* and *Dusk*
give the dark form, *Desert* the light one. A choice of *Dark* or *Light* here still wins.

## Font

The app's words are in **Atkinson Hyperlegible Next**, and its monospaced text (rules, scripts,
numbers in tables) in **Atkinson Hyperlegible Mono**: fonts whose letters are easier to tell
apart, designed by the Braille Institute for low-vision readers. Headings and window titles are
in its bold. So is this book, and so are the pages JETCARP writes for a browser (the clip-rating
page and the website export's page) and the notices of the libraries it is built from
(`THIRD_PARTY_LICENSES.html`), which carry the fonts inside them.

Two lists choose the fonts, **Font** for the app's words and **Monospace font** for its
monospaced text. Each offers:

- **Atkinson Hyperlegible Next** (or **Mono**), JETCARP's, the default;
- **Ubuntu Light** (or **Hack**), egui's original fonts, as JETCARP drew before 0.4.3;
- **every font installed on this computer**, by its family name. The app looks for them when the
  window first opens, in each system's usual folders: `/usr/share/fonts`, `/usr/local/share/fonts`,
  `~/.local/share/fonts` and `~/.fonts` on Linux; `C:\Windows\Fonts` and
  `%LOCALAPPDATA%\Microsoft\Windows\Fonts` on Windows; `/System/Library/Fonts`, `/Library/Fonts`
  and `~/Library/Fonts` on macOS. It lists the fonts that draw basic Latin letters and digits, and
  draws each family in its upright face nearest regular weight. Someone who reads more easily in
  another font chooses it here once it is installed;
- **Choose a font file…**, for a TrueType or OpenType file (`.ttf`, `.otf`, `.ttc`) that isn't
  installed.

The choice applies at once, and is saved with the other settings (`font` and `mono_font` in the
`accessibility` section of `app.json`, the file's path among them). Whatever font is chosen,
egui's own fonts stay behind it: a character the font lacks is drawn in Ubuntu Light (Greek,
Cyrillic and more Latin letters), then egui's icons, then Hack (arrows, boxes and dots).

A character that none of those has is drawn, as a last resort, in a font installed on the computer
that has it: the macOS key symbols (⌘, ⌥, ⌃, ⌫, ⌦) in the keyboard window's macOS column and the
macOS menus, which macOS's own fonts have, and Chinese, Japanese and Korean, wherever a font for
them is installed. The app reads which characters each installed font has (from the folders
above) when it starts, on a thread of its own so starting isn't held up; until it has, which takes
a fraction of a second, such a character is a box. Of the fonts that have a character, it uses
the upright regular face in the smallest file, so the same fonts give the same answer every time.
Without a font that has it, a character is still drawn as a box.

**The caption on the desktop background follows the Font setting** (its first list): the desktop
background reads it from `app.json` when it starts and again every couple of seconds, so a font
chosen in the app shows on the desktop without a restart. It is drawn with egui's own font reader
and rasteriser (skrifa and vello_cpu), as the app's words are, its letters sized and spaced as they
were before 0.4.3. Behind the chosen font, the caption
has Ubuntu Light, as the app has (with egui's original choice it is drawn in Ubuntu Light alone),
and the same last resort: a character neither has, such as a rotation named in Chinese, is drawn
in an installed font that has it. The desktop background reads which characters the installed
fonts have when it starts, on a thread of its own so no frame waits; until it has, such a
character is a box, and the caption is drawn again once it has.
If the chosen font can't be loaded there (a file since removed), the caption is drawn in
Atkinson Hyperlegible Next and the desktop background's log says why. The Windows screen saver
reads the setting when it starts.

**The macOS screen saver has its own copy of the font.** macOS runs it in a sandbox that
can't read JETCARP's settings, nor be counted on to read your font folders, so installing it
(`jetcarp host saver install`, or *Install* in the *Screensaver* pane) copies the chosen font file
into the saver, as it copies the rotation; Atkinson Hyperlegible Next and egui's original fonts are
in the saver already. Installing also copies in the installed fonts the last resort would use for
the characters its caption can show (the rotation's name, the rules' names and notations, the
grids' names) that its fonts lack. When the font changes, JETCARP builds the saver again with the
new one ([The system's screen saver](wallpapers.md#the-systems-screen-saver)). A font that can't be
read then is left out and the saver's caption is in Atkinson Hyperlegible Next, and the install
(or the note when it is built again) says why; so does the saver's log if its copy is later missing or broken. The font file is
copied whole: a few hundred kilobytes for most fonts, but a collection of Chinese, Japanese and
Korean faces can be tens of megabytes.

A file that isn't a font JETCARP can read, or a font with no basic Latin letters and digits, is
refused with a sentence saying so, and the setting stays as it was. If a font chosen earlier has
since been removed, the app goes back to Atkinson Hyperlegible when it starts, and says why on the
status line.

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

- The Quick Start's card pictures keep the palettes their entries name; the palettes chosen here
  apply when an entry is opened.
- What the keyboard and screen readers can't do yet is listed in
  [Using the app from the keyboard](keyboard.md#what-the-keyboard-and-screen-readers-cant-do).
