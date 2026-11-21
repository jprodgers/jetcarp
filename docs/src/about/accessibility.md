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
  [WCAG 2.3.1](https://www.w3.org/WAI/WCAG22/Understanding/three-flashes-or-below-threshold.html)
  defines them) and for strong regular stripes (as ITU-R BT.1702 and the Harding guidance describe
  them). Before a scene crosses those limits it softens it, then slows it, then pauses it with a
  note saying why. The guard runs wherever JETCARP draws: the app, full screen, the desktop
  background, the screen savers, and the animations it exports, which it checks before saving. See
  [The strobe guard](../guides/strobe-guard.md).
- **Who can turn it off.** The guard is on for everyone. If you answered that you have, or may
  have, a seizure disorder, it can't be turned off. If you answered that you don't, you can turn it
  off in *Settings → Safety*, through a confirmation. The desktop background and the screen saver
  have a switch of their own and keep the guard on, because people walking past a screen never
  answered the warning. No command, script or AI client can turn it off.
- **What it does not do.** The guard lowers the risk; it does not remove it. It measures the screen
  under assumptions about its size and your distance from it, which the guard's page states. It
  is not a certificate of safety, and the warning stands.
- **Less motion.** You can pause any scene, run it slower, step it one generation at a time, or show
  it in a small window. The desktop background pauses on its own while windows cover it, on battery
  and while the processor is busy, and has a low-power pacing (`jetcarp host pacing low-power`).
  In this book, every animation is replaced by a still picture when your system asks for reduced
  motion.

## Without seeing the screen

- **Screen readers.** The app's menus, buttons, text fields and windows are offered to screen
  readers through AccessKit: UI Automation on Windows, the accessibility API on macOS, and AT-SPI
  on Linux. The world itself is a picture, and a screen reader can't describe it.
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

- **Size.** In the app, you can zoom the world in and out with the + and − keys or the scroll
  wheel. On the desktop background, the caption that names the rule and the tiling has a size of
  its own.
- **Colour.** JETCARP has 30 built-in palettes, light and dark, and anywhere a palette is accepted
  you can give your own colours instead ([Palettes and colouring](../guides/palettes.md)). The two
  palettes of JACA, JETCARP's two-sign automaton, are chosen to stay apart at every age.
- **Light and dark.** The app follows your system's light or dark setting on Windows and macOS.

## Using the keyboard

The common actions have keyboard shortcuts, which *Help → Keyboard shortcuts…* (F1) lists. Running,
pausing and stepping have single keys (Space to run or pause, S or → to step), and + and − zoom the
world; they work while no text box has the keyboard.

## Hearing

JETCARP makes no sound at present, so nothing in it depends on hearing. Sound may be added in the
future.

## Where JETCARP falls short

These are known, and each is tracked:

- **Colour alone.** Cells' states and ages are told apart by colour only, and no built-in palette
  has yet been chosen and checked for the common kinds of colour-vision deficiency
  ([#217](https://github.com/jprodgers/jetcarp/issues/217)).
- **Reduced motion in the app.** The app doesn't yet read your system's request for reduced motion;
  the strobe guard and the speed controls are what it offers instead
  ([#218](https://github.com/jprodgers/jetcarp/issues/218)).
- **Light and dark on Linux.** On Linux the app stays dark when the system is set to light
  ([#137](https://github.com/jprodgers/jetcarp/issues/137)).
- **The keyboard alone.** Drawing cells, selecting and moving patterns, and arranging the docked
  panes need a mouse or a touchpad, and the menus and windows have not been checked for use without
  one. Scripts and the command line can draw and edit worlds instead
  ([#219](https://github.com/jprodgers/jetcarp/issues/219)).
- **No high-contrast theme** for the app, beyond the light and dark ones
  ([#220](https://github.com/jprodgers/jetcarp/issues/220)).
- **Not yet tested** with screen readers, with keyboard-only use
  ([#219](https://github.com/jprodgers/jetcarp/issues/219)), or by disabled people. The checks so
  far are the program's own: its tests, the strobe guard's comparison with the IRIS flash and
  pattern analyser, and the hands-on testing of each release.

## How this was assessed

The maintainer assessed JETCARP against the [Web Content Accessibility Guidelines
2.2](https://www.w3.org/TR/WCAG22/) where they apply to a desktop program, with Claude Code, in
November 2026. No outside party has audited it. The strobe guard's detector is compared with
[IRIS](https://github.com/electronicarts/IRIS), Electronic Arts' open-source photosensitivity
analyser, on a corpus of scenes made to flash, scenes just under the limits and ordinary scenes
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
