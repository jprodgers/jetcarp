# Accessibility

JETCARP is meant for anyone who wants to explore cellular automata. This page says what it does to
be usable by people with disabilities, where it falls short, and how to tell us when it gets in your
way. It was written for JETCARP 0.4.3 (November 2026) and is checked again with every release.

A cellular automaton is something you watch change, so much of JETCARP is visual and in motion. Two
things follow from that, and they shape this page: some of what JETCARP draws can be dangerous to
watch, and everything it shows should also be reachable some other way.

## Flashing and motion

JETCARP's animations can flash and flicker rapidly, and can trigger seizures in people with
photosensitive epilepsy. This is the part of JETCARP's accessibility we take most seriously.

- **A warning before anything moves.** Before JETCARP shows its first scene, it asks whether you
  have, or may have, a seizure disorder, and keeps your answer. If you do, it advises you strongly
  not to use JETCARP and offers to quit first. The desktop background and the screen saver wait for
  the same answer, however they are started: from the app, the command line, at login or by an AI
  client. See [Photosensitive seizure warning](../getting-started/photosensitivity.md).
- **The strobe guard.** JETCARP watches every frame it draws for flashing and red flashing (as
  WCAG 2.3.1 defines them: [W3C 2024](../research/bibliography.md#w3c2024wcag22);
  [W3C 2025](../research/bibliography.md#w3c2025understanding)) and for strong regular stripes (as
  ITU-R BT.1702 and Ofcom's broadcasting guidance describe them:
  [ITU-R 2023](../research/bibliography.md#itu2023bt1702);
  [Ofcom 2017](../research/bibliography.md#ofcom2017flashing)). Before a scene crosses those limits
  it softens it, then slows it, then pauses it with a note saying why. The guard runs wherever
  JETCARP draws: the app, full screen, the desktop background, the screen savers, and the
  animations it exports, which it checks before saving. See
  [The strobe guard](../guides/strobe-guard.md).
- **Stricter for someone at risk.** If you answered that you have, or may have, a seizure
  disorder, the guard also dims any pattern of similar parts, regular or not, as EA's IRIS
  analyser counts one ([Electronic Arts 2023](../research/bibliography.md#ea2023iris)), so soups,
  waves and mazes are often shown dimmed for you. See
  [Stricter for someone at risk](../guides/strobe-guard.md#stricter-for-someone-at-risk).
- **Who can turn it off.** The guard is on for everyone. If you answered that you have, or may
  have, a seizure disorder, it can't be turned off. If you answered that you don't, you can turn it
  off in *Settings → Safety*, through a confirmation. The desktop background and the screen saver
  have a switch of their own and keep the guard on, because people walking past a screen never
  answered the warning. No command, script or AI client can turn it off.
- **What it does not do.** The guard lowers the risk; it does not remove it. It measures the screen
  under assumptions about its size and your distance from it, which the guard's page states. It
  is not a certificate of safety, and the warning stands.
- **Less motion.** The app follows your system's request for reduced motion: the Quick Start's
  cards and the small animations in other windows show stills until you choose one, a scene opens
  paused with a note saying why, and the interface's own animations stop. *Settings →
  Accessibility* turns this on or off whatever the system says
  ([Accessibility settings](../guides/accessibility-settings.md#reduced-motion)). You can also
  pause any scene, run it slower, step it one generation at a time, or show it in a small window.
  The desktop background pauses on its own while windows cover it, on battery
  and while the processor is busy, and has a low-power pacing (`jetcarp host pacing low-power`).
  In this book, every animation is replaced by a still picture when your system asks for reduced
  motion.

## Without seeing the screen

- **Screen readers.** The app's controls are offered to screen readers through AccessKit: UI
  Automation on Windows, the accessibility API on macOS, and AT-SPI on Linux. Every control has a
  role and a name, the custom-drawn ones too (the Quick Start's cards, filters, swatches, stars and
  pictures), and a button drawn as a symbol is named by what it does ("Hide the CAs pane", not
  "×"). The status line is a live region: a scene the strobe guard pauses, a hunt that finishes,
  fails or pauses, and a scene that can't open are read out as they happen. The world itself is a
  picture: a screen reader hears the cell under the keyboard's cursor ("Cell 1234, column 5, row 7:
  alive"), not the whole grid. See
  [With a screen reader](../guides/keyboard.md#with-a-screen-reader).
- **Everything has a text form.** JETCARP is one program with four ways in: the app, the command
  line, Rhai scripts and an MCP server. The command line and scripts can do most of what the app
  does, and answer in text: a world's status, its population and hash, the objects in it, the
  patterns in the library and the results of every search, with `--json` where you want data.
  Through MCP, an AI assistant can read a world's cells and objects (`world_read`) and the library
  (`library_query`) and describe them to you. Every command is in the
  [command-line reference](../reference/cli.md).
- **This book** gives every picture a text description, and every picture of a world a caption
  naming the scene it was made from.

## Seeing the screen differently

- **Letters easier to tell apart.** The app's words are in Atkinson Hyperlegible Next, and its
  monospaced text (rules, scripts, numbers in tables) in Atkinson Hyperlegible Mono: fonts whose
  letters are easier to tell apart, designed by the Braille Institute for low-vision readers. So
  is this book, and so are the pages JETCARP writes for a browser. Someone who reads more easily
  in another font can choose any font installed on their computer, or a font file, in *Settings →
  Accessibility*, and egui's original fonts are there too; the caption on the desktop background
  follows the choice ([Accessibility settings](../guides/accessibility-settings.md#font)).
- **Size.** In the app, you can zoom the world in and out with the + and − keys or the scroll
  wheel. On the desktop background, the caption that names the rule and the tiling has a size of
  its own.
- **Colour.** JETCARP has 75 built-in palettes, light and dark, and anywhere a palette is accepted
  you can give your own colours instead ([Palettes and colouring](../guides/palettes.md)). 49 of
  them pass JETCARP's colour-vision check: their colours stay clearly apart for protanopia,
  deuteranopia, tritanopia and their milder forms, as a standard model simulates them. They
  include ramps of many looks, palettes for light backgrounds, closed wheels for cyclic rules, and
  seven pairs for the two signs of JACA, JETCARP's two-sign automaton. *Settings → Accessibility*
  makes Lantern, and Cobalt and Saffron for JACA, the defaults for scenes that don't choose their
  own. The [palettes reference](../reference/palettes.md#colour-vision-deficiency) gives every
  built-in palette's result.
- **High contrast.** The app has a high-contrast theme, dark and light: text at a contrast of 7:1
  or more, thicker outlines, and a thicker ring round whatever has the keyboard. It follows Windows'
  contrast themes, macOS's *Increase contrast* and GNOME's *High Contrast*, and *Settings →
  Accessibility* turns it on or off whatever the system says
  ([Accessibility settings](../guides/accessibility-settings.md#high-contrast)).
- **Light and dark.** The app follows your system's light or dark setting, and *Settings →
  Accessibility* chooses either whatever it says.

## Using the keyboard

The app can be used from the keyboard alone, as far as the toolkit it's built with allows
([Using the app from the keyboard](../guides/keyboard.md)).

- **Moving about.** Tab and Shift+Tab reach every control in every window, in the order the window
  shows them, and scroll each into view. F6 and Shift+F6 move between the menu bar, each group of
  panes, the grid, the status line and each floating window. F10 opens the menus. Esc closes the
  tool window that has the keyboard, and dialogs open with their main button ready for Enter.
- **A ring in every theme** shows which control has the keyboard: two points wide in the standard
  themes, three in high contrast, at a contrast of at least 3:1 against every background.
- **The grid.** With the keyboard on the grid, the arrow keys move a cursor from cell to cell on any
  tiling. Space draws and Delete erases. Shift and the arrows select, and Ctrl and the arrows
  move what's selected by the tiling's own steps, so a pattern keeps its shape. Space also pastes a
  pattern chosen from the library at the cursor.
- **Shortcuts.** Running, pausing and stepping have single keys (Space to run or pause, S or → to
  step), and + and − zoom the world. S, + and − work unless a text box has the keyboard; Space
  and → press or move the control that has it, so press Esc first.
  *Help → Keyboard shortcuts…* (F1) lists every key.

## Hearing

JETCARP makes no sound at present, so nothing in it depends on hearing. Sound may be added in the
future.

## Where JETCARP falls short

These are known, and each is tracked:

- **Colour alone.** Cells' states and ages are told apart by colour only. The palettes chosen for
  colour-vision deficiency are checked by simulation, not yet by people with those deficiencies,
  and 26 of the built-in palettes don't pass the check
  ([#217](https://github.com/jprodgers/jetcarp/issues/217)).
- **Dragging has no keyboard path.** Moving tabs between groups of panes, resizing panes, editing
  a tile's shape and placing an output need a mouse or a touchpad; the *Window* menu places panes
  instead, and scripts and the command line can draw and edit worlds
  ([What the keyboard and screen readers can't do](../guides/keyboard.md#what-the-keyboard-and-screen-readers-cant-do)).
- **Limits of the toolkit.** The app draws its menus inside its window, so on macOS VoiceOver's
  menu-bar key (VO+M) can't reach them; F10 does. A window popped out on its own is a separate
  system window, which F6 can't move to. Scroll areas don't scroll with Page Up and Page Down.
  Only the cursor's cell of the grid is offered to a screen reader, and on Windows, Narrator and
  NVDA may announce the grid as having "0 rows"
  ([#219](https://github.com/jprodgers/jetcarp/issues/219)).
- **Characters no font on the computer has.** The fonts that come with the app draw Latin, Greek
  and Cyrillic, arrows and common symbols, but not Chinese, Japanese or Korean, nor the macOS key
  symbols (⌘, ⌥). The app draws those in an installed font that has them (macOS's own fonts have
  the key symbols); with none installed, they are drawn as boxes. The caption on the desktop
  background has no such last resort: a rotation named in Chinese, Japanese or Korean is drawn
  there with boxes unless the font chosen in *Settings → Accessibility* has them.
- **Not yet tested by hand** with screen readers (Narrator, NVDA, VoiceOver and Orca) or with
  keyboard-only use, though the program's tests drive the whole app with key presses and read
  what it offers screen readers ([#219](https://github.com/jprodgers/jetcarp/issues/219)); nor by
  disabled people. Reduced motion
  and the high-contrast theme are checked by the program's tests; reading each system's settings is
  checked by hand on Windows, macOS and GNOME as part of each release's testing
  ([#218](https://github.com/jprodgers/jetcarp/issues/218),
  [#220](https://github.com/jprodgers/jetcarp/issues/220)). The checks so
  far are the program's own: its tests, the strobe guard's comparison with the IRIS flash and
  pattern analyser, and the hands-on testing of each release.

## How this was assessed

The maintainer assessed JETCARP against the Web Content Accessibility Guidelines 2.2
([W3C 2024](../research/bibliography.md#w3c2024wcag22)) where they apply to a desktop program,
with Claude Code, in November 2026. No outside party has audited it. The strobe guard's detector
is compared with IRIS, Electronic Arts' open-source photosensitivity analyser
([Electronic Arts 2023](../research/bibliography.md#ea2023iris)), on a corpus of scenes made to
flash, scenes just under the limits and ordinary scenes
([How JETCARP is tested](testing.md)).

## Tell us

If something in JETCARP or this book is hard or impossible for you to use, please say so: it is a
bug, and it is treated as one.

- [Open an issue](https://github.com/jprodgers/jetcarp/issues/new/choose) and say what you were
  trying to do, what got in the way, and what you use (a screen reader, magnification, the keyboard
  alone, and so on).
- Or ask in [Discussions](https://github.com/jprodgers/jetcarp/discussions), or write to
  jimmie@jimmieprodgers.com if you'd rather not post in public.

JETCARP is maintained by one person, so a reply can take a few days.
