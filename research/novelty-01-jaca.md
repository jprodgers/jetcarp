# Novelty audit 01: JACA (balanced-ternary annihilating Life-like rules)

Audit date: 2026-11-10. Auditor: Claude Code (Opus 5.5) for Jimmie Rodgers.

## What JETCARP does (from README "## JACA" and `crates/cart-core/src/jaca.rs`)

- Cells are -1, 0, +1. Each sign has its own B/S rule and counts only same-sign neighbours.
- A live cell with any opposite-sign neighbour (in the CA's neighbourhood) is a "contact" cell and is
  empty next generation; optional `blast N` also empties everything within N neighbourhood steps of a
  contact cell.
- An empty cell that both signs would fill stays empty (cancelled double birth).
- A one-sign world runs exactly the plain B/S rule. Runs on every tiling (lattice, k-uniform,
  isohedral, Penrose, hat, spectre). B0 rules refused.

## Verdict: Prior art found for the main ingredients; the exact combination appears undocumented

Each ingredient has been published before. Most important, a 2025 two-player Life uses the same
+1/-1/0 encoding, own-colour counting and cancelled double births. A 2018 ConwayLife rule thread also
turned matter–antimatter contact into "energy". We found no published rule that combines all of these:
a separate B/S rule per sign, same-sign counting, annihilation of every live cell that touches the other
sign, a blast radius in neighbourhood steps, cancelled double births, an exact reduction to the plain
rule when only one sign is present, and use on arbitrary or aperiodic tilings.

## Closest prior work (verified)

1. **"Making Conway's Game of Life Playable"**, Unsupervision Blog, 5 Nov 2025 (no author byline on the
   page). https://blog.unsupervision.com/game-of-life-or-death/ . The rules are those of the CodinGame
   multiplayer bot game "Game of Life or Death",
   https://www.codingame.com/multiplayer/bot-programming/game-of-life-or-death (that page needs
   JavaScript, so its rules could not be read directly).
   Quotes: "Player 1's cells are `1`, Player 2's are `-1`, and dead cells are `0`." "each player treats
   the opponent's cells as dead." "Each player updates their own grid independently, and then the results
   are added together." "If both players try to activate the same cell, it stays dead."
   *Overlap with JACA:* the same balanced-ternary encoding, counting only one's own sign, and cancelled
   double births. One sign alone runs plain Life.
   *Differences:* touching opposite-sign cells are NOT destroyed. The opponent's cells are simply
   ignored, and only a coincidence on the same cell cancels. The page mentions no blast radius and no
   separate rule per side; it is Life (B3/S23) on squares as a game. JACA's main dynamic, annihilation
   fronts between territories, comes from contact annihilation, which this rule does not have.

2. **KittyTac's "matter/antimatter" proposal and wildmyron's "Energetic" rule**, ConwayLife.com forums,
   "Rule request thread", posts of 1–9 Sep 2018 (e.g. https://conwaylife.com/forums/viewtopic.php?p=63510 );
   rule page LifeWiki "Rule:Energetic" https://conwaylife.com/wiki/Rule:Energetic .
   (conwaylife.com returns HTTP 403 to automated fetches. Both pages were read through the mirror
   conwaylife.njohnston.ca, which serves the same content.)
   KittyTac (1 Sep 2018) proposed two Life states for which contact between a state-1 and a state-2 cell
   turns "both cells" into "energy", and the energy then decays into an "8-directional burst of
   particles". wildmyron's "Energetic v1" (8 Sep 2018) implemented it. In v1 a cell is born only with
   three same-state neighbours and no opposite-state or energy neighbours. The LifeWiki page describes
   "A rule with two totalistic Life states combined with energy particles" that decay into eight photons.
   *Overlap:* two Life species that annihilate on contact. **This is the closest prior art for contact
   annihilation.**
   *Differences:* annihilation produces energy and photon states that are part of the CA's state and
   can create cells. JACA's glow is drawing only. Energetic also has one fixed Life rule rather than a
   B/S rule per sign, no blast radius in neighbourhood steps, no cancelled-birth rule (mixed births were
   left undefined in v1), and runs on the Moore square grid only.

3. **Levene, M. and Roussos, G., "A Two-Player Game of Life"**, International Journal of Modern Physics C
   14(2):195–201 (2003), DOI 10.1142/S0129183103004346; arXiv:cond-mat/0207679 (v4, 23 Sep 2002).
   Rules page: https://titan.dcs.bbk.ac.uk/~gr/software/p2life/p2life.php . p2life has black and white
   tokens. White is born with "exactly three white neighbours and the number of black neighbours is
   different from three". When there are three of each, "an unbiased coin determines whether a white or
   black token is born". Survival depends on "the difference between the number of white and black
   neighbours".
   *Differences:* opponents weaken survival through a difference count. Nothing is annihilated on
   contact, and double births are settled at random rather than cancelled.

4. **Immigration and QuadLife (colourised Life)**: Silver, S. A., *The Life Lexicon*, entries
   "Immigration" and "QuadLife" (https://conwaylife.com/ref/lexicon/lex_i.htm#immigration ,
   https://conwaylife.com/ref/lexicon/lex_q.htm#quadlife ), as cited and quoted by Wikipedia, "Conway's
   Game of Life", Variations section (fetched 2026-11-10): "Whenever a new cell is born, it takes on the on
   state that is the majority in the three cells that gave it birth … Except for the variation among on
   cells, both of these variations act identically to the Game of Life." LifeWiki "Colourised Life"
   (mirror page, last edited 7 Nov 2024): "If the colours of the live cells are ignored, these rules behave
   identically to Life". Niemiec's 8-colour Life: "Two complementary cells cancel out" (this affects colour
   inheritance only).
   *Differences:* colour never changes which cells live or die. In JACA the sign changes the dynamics.

5. **LifeWiki "Rule:Positronic"** (Yoel Matveyev, 2020; mirror page last edited 12 Jul 2020): "A 5-state
   rule … a symmetrical version of Fireworld". It has "positively" and "negatively" charged worlds,
   matter and antimatter, which "behave identically and may interact with each other". The page does
   not say they annihilate. *Related only.*

6. **Physics background, A + B → 0 annihilation:** Toussaint, D. and Wilczek, F., "Particle–antiparticle
   annihilation in diffusive motion", J. Chem. Phys. 78(5):2642–2647 (1983), DOI 10.1063/1.445022 (DOI
   resolves to that title at pubs.aip.org). Chopard, B. and Droz, M., *Cellular Automata Modeling of
   Physical Systems*, Cambridge University Press (1998) (book page
   https://cui.unige.ch/~chopard/CA/Book/book.html , with chapters on reaction–diffusion processes).
   These are diffusing particles, not Life-like birth/survival rules. They are the physics origin of
   "opposite species annihilate on contact", so JACA's naming and framing should mention them.

7. Related only: Yang, X.-S., "Characterization of Multispecies Living Ecosystems With Cellular Automata",
   Artificial Life VIII, MIT Press, pp. 138–141 (2002), arXiv:1003.5288 (a predator–prey food chain on
   Life, which reduces to Life for one species). Tapani Raiko's "Game of Lives" (Palm, v1.3, 27 Mar 2003,
   https://users.ics.aalto.fi/praiko/lives ) is a colour-blind Life count with majority colour, as in
   Immigration.

## How JETCARP differs (defensible claims)

- Annihilation of every live cell that touches the other sign, as a rule step, combined with an
  independent B/S rule per sign that counts only same-sign neighbours. Energetic (2018) has contact
  annihilation, but its products are energy and photon states and it has a single rule.
- A blast radius measured in neighbourhood steps of the CA's own neighbourhood, valid on any tiling.
- Cancelled double births together with contact annihilation (the 2025 blog has cancellation, but no
  contact annihilation).
- A guaranteed exact reduction to the plain B/S rule when only one sign is present (the 2025 blog
  also has this property for Life).
- The same rule runs on arbitrary periodic, k-uniform, isohedral and aperiodic tilings, with a
  search-derived preset set (71 scenes on 24 tilings).

## Suggested credit wording

> JACA combines ideas with earlier precedents: two-player Life with own-colour counting (Levene and
> Roussos 2003; the 2025 "Game of Life or Death", which also uses a +1/-1/0 encoding and cancels births
> on the same cell), and matter–antimatter Life whose touching cells turn into energy (KittyTac's
> proposal and wildmyron's Energetic rule, ConwayLife.com, 2018), with A + B → 0 annihilation from
> physics (Toussaint and Wilczek 1983). As far as we know, JACA's particular combination is new: an
> independent B/S rule per sign, annihilation of every live cell touching the other sign, a blast
> radius in neighbourhood steps, cancelled double births, and an exact reduction to the plain rule for
> one sign, on arbitrary and aperiodic tilings.

Do not claim "first signed/ternary Life" or "first matter–antimatter Life".

## Search log (2026-11-10)

WebSearch queries (standard or extended mode):
- `"Game of Life" antimatter cells annihilate cellular automaton`
- `two-species Life-like cellular automaton opposite species annihilate on contact positive negative cells`
- `Immigration QuadLife multistate rule colours birth majority` (site: conwaylife.com)
- `"Colourised Life" LifeWiki rules list Immigration QuadLife "Rainbow" LifeHistory …`
- `two-player Game of Life variant colours cells of opposing player destroy each other rules`
- `p2life Levene Roussos rules "white" "black" survives difference neighbours …`
- `"balanced ternary" cellular automaton -1 0 +1 Game of Life` (nothing relevant)
- `"matter" "antimatter" cellular automaton Life-like rule particles antiparticles annihilate golly`
- `"Positronic" rule LifeWiki Fireworld matter antimatter five states`
- `conwaylife forum rule two colours annihilate when they touch "Energetic" rule …` (site: conwaylife.com)
- `"Niemiec's Life" colourised eight states complementary colours cancel newborn colour`
- `Toussaint Wilczek 1983 "Particle-antiparticle annihilation in diffusive motion" …`
- `Chopard Droz cellular automata model reaction A+B→0 annihilation lattice gas …`
- `multi-species Game of Life cellular automaton each species own birth survival rule …`
- `github "game of life" "antimatter" OR "anti-cells" OR "antilife" cells annihilate …` (nothing relevant)

Pages fetched: blog.unsupervision.com/game-of-life-or-death (twice), codingame.com (JavaScript only),
arxiv.org/abs/cond-mat/0207679, the p2life PDF (text could be extracted only from page 1),
titan.dcs.bbk.ac.uk p2life rules, en.wikipedia.org "Conway's Game of Life",
conwaylife.njohnston.ca/wiki/QuadLife (Colourised Life), Rule:Positronic, Rule:Energetic, and
forums/viewtopic.php?p=63510, users.ics.aalto.fi/praiko/lives, doi.org/10.1063/1.445022,
cui.unige.ch Chopard–Droz book page, arxiv.org/abs/1003.5288, arxiv.org/abs/2406.13383.
Blocked: conwaylife.com (HTTP 403 to WebFetch), web.archive.org, scilit.com.

## Gaps and unverified leads

- MCell / Mirek's Cellebration rule families (Weighted Life with negative weights, "Rules table") were
  not checked in detail. A Weighted Life rule with negative weights for "enemy" neighbours would be
  related; it is not confirmed.
- Golly's Rule Table Repository (golly.sourceforge.io) was not searched rule by rule for two-species
  annihilation rules.
- The full ConwayLife "Rule request thread" may hold later variants of matter/antimatter (only the
  September 2018 posts were read). The forum's "Other cellular automata" board may hold further
  two-species annihilation rules.
- Wolfram Demonstrations / NKS "competition" CAs were not searched.
