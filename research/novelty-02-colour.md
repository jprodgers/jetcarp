# Novelty audit 02: colouring (age colouring, JACA per-sign palettes, annihilation glow)

Audit date: 2026-11-10.

## What JETCARP does

- (a) Age colouring (`render.rs`, README "## Palettes and backgrounds"): a live cell's colour follows
  log(1 + age) along a palette's colour stops. New cells take the first stop and long-settled
  structures (still lifes) reach the last stop, so settled regions stand out from active ones. There
  are 20 built-in palettes, each with its own background and stage colour.
- (b) JACA: +1 and -1 cells have their own palettes, each coloured by age. Annihilated cells glow and
  fade over a few generations. The glow is drawn only and is not part of the state.
- (c) Row gradients for row rules ("By row (top to bottom): Wolfram row gradients").

## Verdict: Prior art found for (a) and for the fading glow; (b)'s per-sign palettes are a trivial extension; (c) is common practice (Wolfram itself not confirmed)

## Closest prior work (verified)

1. **LifeViewer** (Chris Rowett), LifeWiki page "LifeViewer" (mirror conwaylife.njohnston.ca/wiki/LifeViewer,
   last edited 24 Feb 2025) and "LifeViewer/Scripts" (last edited 19 Feb 2026); source
   https://github.com/rowett/lifeviewer (GPL-3.0; README: "Colour themes with cell history and
   longevity"). In two-state themes, live cells "fade from the ALIVE colour to the ALIVERAMP colour"
   over AGESTATES generations (default 63), which "provides a visual representation of cell
   longevity". Dead cells fade from DEAD to DEADRAMP over HISTORYSTATES generations, "a visual
   representation of cell history". Script commands: `COLOR ALIVERAMP R G B`, `AGESTATES <0..63>`,
   `HISTORYSTATES <0..63>`.
   *Overlap:* age colouring in which long-lived cells, such as still lifes, end at the final colour, plus
   fading of dead cells. This is the standard viewer for the ConwayLife community and is prior art for
   (a) and for fading after death.
   *Difference:* LifeViewer has a two-colour ramp over a linear age that saturates at 63 generations.
   JETCARP uses multi-stop palettes with a logarithmic age mapping on any tiling. These are design
   choices, not new ideas.

2. **Nathan Dumont, "ChipKIT: Conway's Game of Life on VGA, now with more colours"**, blog post, last
   updated 4 Nov 2011, https://nathandumont.com/blog/chipkit-conways-game-of-life-on-vga-now-with-mo .
   Age colouring was added to show "where the areas of most rapid change were, and which areas had been
   stable for a long time". Live cells "fade from green to blue representing just born to oldest alive".
   Dead cells run through "shades of red fading to orange which displays most recently dead to longest
   inactive".
   *Overlap:* the same motivation as JETCARP's, telling settled regions from active ones, stated in 2011.

3. **raywo, MMM-GameOfLife, pull request #28 "Color cells by age and let dead cells fade out"**, opened
   18 Oct 2026, merged 19 Oct 2026, https://github.com/raywo/MMM-GameOfLife/pull/28 . "`agePalette`
   blends from `aliveColorCode` to `oldAliveColorCode` over `agingGenerations` generations".
   "afterglow lets dead cells fade out over that many generations". "still lifes darken to the old
   color, active regions stay bright and leave a fading trail."
   *Overlap:* age colouring with still lifes at the old colour, and a fading afterglow on dead cells.
   It is contemporaneous with JETCARP (October 2026), so it shows independent arrival at the same idea.

4. **Golly / LifeHistory** (history envelope rather than age): the Golly rule file `LifeHistory.table`
   (e.g. Debian's golly 2.1 source, https://sources.debian.org/src/golly/2.1-1/Rules/LifeHistory.table )
   has state 2 as the envelope, "state=2 if cell was ever ON". It was based on Dave Greene's 3-state
   HistoricalLife (April 2001, per LifeWiki, which was not fetched directly). This marks where cells
   have been, which is related to the glow and trails.

5. **Generations dying states** (MCell family, Golly's Generations algorithm,
   https://golly.sourceforge.io/Help/Algorithms/Generations.html , "an extra history component that
   allows cells to have up to 256 states"; it refers to "Mirek Wojtowicz's MCell website"). Dying cells
   that are drawn in a gradient of colours are a long-standing visual convention. Golly's help page does
   not itself describe the gradient, so this is related only.

6. **Row gradients (c):** colouring an elementary CA by row depth is common. One verified example is the
   Tiny Tapeout SKY 26b project "833 IEEE Rule 30 Cellular Automaton VGA Display" (Naveen Kumaar S),
   https://tinytapeout.com/chips/ttsky26b/833 : "colour changes with row depth". **We could not confirm
   that Wolfram himself (NKS or Demonstrations) colours rows by time.** NKS pictures are mostly
   black/white or coloured by state. Call this practice common, and do not attribute it to Wolfram unless
   a specific figure is found.

## How JETCARP differs

- Age colouring on any tiling (periodic, k-uniform, isohedral, aperiodic), with multi-stop palettes,
  a log age scale, and palette-specific backgrounds chosen so every stop contrasts. This is a design
  and implementation contribution, not a new idea.
- JACA: two independent age palettes (one per sign) plus a glow that marks only annihilation, not
  ordinary death. We found no prior viewer that marks annihilations specifically. That is because
  annihilation rules are rare, so this is a small, rule-specific detail; it should not be presented
  as a contribution.
- The "rainbow fronts" in JACA (README) are an emergent visual effect of age colouring applied to rules
  whose territories freeze while fronts stay active. They are worth showing, but they are an
  observation, not a technique.

## Suggested credit wording

> Cells are coloured by age, as in LifeViewer's ALIVE→ALIVERAMP themes (Rowett) and earlier hobby
> programs (e.g. Dumont 2011), so long-settled structures stand apart from active regions. JETCARP
> uses multi-stop palettes on a log age scale. JACA colours each sign with its own palette and briefly
> marks annihilated cells with a fading glow, like the dead-cell fading in LifeViewer's history themes.

No novelty claim recommended for colouring.

## Search log (2026-11-10)

- WebSearch: `LifeViewer ALIVERAMP AGESTATES colour cells by how long alive longer-lived cells`
- WebSearch: `Mirek's Cellebration MCell "trails" colors cell age option Life display`
- WebSearch: `Golly LifeHistory rule history envelope states "LifeHistory" Golly help cells that have ever been alive`
- WebSearch: `Game of Life colour cells by age program history "color by age" cellular automata viewer still lifes old cells different colour`
- WebSearch: `elementary cellular automaton colored by generation row gradient rainbow time step coloring rule 30 picture`
- WebSearch (demonstrations.wolfram.com): `Wolfram Demonstrations elementary cellular automaton "color" each step different colour rows colored by time`
- Fetched: conwaylife.njohnston.ca/wiki/LifeViewer, …/LifeViewer/Scripts, github.com/rowett/lifeviewer,
  nathandumont.com ChipKIT post, github.com/raywo/MMM-GameOfLife/pull/28,
  golly.sourceforge.io/Help/Algorithms/Generations.html, tinytapeout.com/chips/ttsky26b/833.

## Gaps / unverified leads

- When LifeViewer first shipped ALIVERAMP is not established (the page cites 2019 forum posts).
- MCell's display options (trails, colour by age) were not confirmed. The MCell help files were not
  reachable.
- Older (1980s–1990s) Life programs that coloured by age (e.g. CelLab, Life32, Xlife) were not
  checked. Age colouring is probably much older than 2011.
- The Ready (GollyGang) and Golly "Cell Age" shader claims from search snippets were not verified.
