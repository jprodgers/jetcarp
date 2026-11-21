# 16. JACA (Jimmie's Annihilating Cellular Automata): history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.
Builds on novelty-01, which holds the full comparison and search log.

## Short history

- **Origin.** JACA is Jimmie Rodgers's idea, from November 2026. It was first committed to the CART
  repository on 8 November 2026 (commit 364b766, "JACA: ternary annihilating CA, signed reseeding,
  searched presets").
  - Cells are balanced ternary (−1, 0, +1), and each sign follows its own B/S rule, counting only
    neighbours of its own sign.
  - A live cell touching the opposite sign is destroyed. An optional blast radius also clears cells
    within N neighbourhood steps.
  - An empty cell that both signs would fill stays empty.
  - A one-sign world runs the plain B/S rule exactly.
  The preset collection (71 scenes on 24 tilings) came from a six-hour search with review by eye
  (README "## JACA").
- **Related earlier ideas.** These are the precedents for the ingredients, not for the combination
  (novelty-01):
  - **Two-player Life.** Levene and Roussos's p2life (2003) has black and white tokens. Opponents
    affect survival through a difference count, and ties at birth are settled by a coin flip
    [levene2003twoplayer].
  - **+1/−1/0 with cancelled births.** The CodinGame bot game "Game of Life or Death", described on
    the Unsupervision Blog (5 Nov 2025), encodes the players as 1 and −1. Each side ignores the
    other's cells, and a cell both sides would activate stays dead [unsupervision2025playable].
  - **Matter and antimatter Life.** KittyTac proposed it on the ConwayLife.com forums (1 Sep 2018):
    contact turns both cells into energy. wildmyron implemented it as the "Energetic" rule (8 Sep
    2018), which has photon states [kittytac2018rulerequest; lifewikind_energetic].
  - **Colourised Life.** Immigration, QuadLife and the like change only colours, not dynamics
    (LifeWiki "Colourised Life"; Silver's *Life Lexicon*).
  - **Physics.** A + B → 0 annihilation of diffusing particles [toussaint1983particle], with
    reaction–diffusion CAs treated in Chopard and Droz [chopard1998cellular].

## Key primary citations

- [levene2003twoplayer] Levene, M. and Roussos, G., "A two-player game of life", *International
  Journal of Modern Physics C* 14(2):195–201 (2003). DOI 10.1142/S0129183103004346;
  arXiv:cond-mat/0207679. Verified: arXiv page and rules page (novelty-01).
- [unsupervision2025playable] "Making Conway's Game of Life Playable", Unsupervision Blog, 5 Nov 2025
  (no author byline). https://blog.unsupervision.com/game-of-life-or-death/ . Verified: fetched
  (novelty-01).
- [kittytac2018rulerequest] KittyTac and wildmyron, posts in the "Rule request thread", ConwayLife.com
  forums, 1–9 Sep 2018, https://conwaylife.com/forums/viewtopic.php?p=63510 . Verified: read through
  the conwaylife.njohnston.ca mirror (novelty-01).
- [lifewikind_energetic] LifeWiki, "Rule:Energetic". https://conwaylife.com/wiki/Rule:Energetic .
  Verified: mirror (novelty-01).
- [toussaint1983particle] Toussaint, D. and Wilczek, F., "Particle–antiparticle annihilation in
  diffusive motion", *J. Chem. Phys.* 78(5):2642–2647 (1983). DOI 10.1063/1.445022. Verified: DOI
  landing page (novelty-01).
- [chopard1998cellular] Chopard, B. and Droz, M., *Cellular Automata Modeling of Physical Systems*,
  Cambridge University Press (1998). Verified: book page (novelty-01).

## Software that ran it before JETCARP

None. JACA was created in JETCARP (then CART). The closest earlier software is p2life (Java,
Birkbeck), the CodinGame "Game of Life or Death" arena, and Golly/LifeViewer running wildmyron's
Energetic rule table (novelty-01).

## People JETCARP should credit

Jimmie Rodgers (JACA). For related ideas: Mark Levene and George Roussos (p2life); KittyTac and
wildmyron (Energetic); the anonymous Unsupervision Blog author and CodinGame's "Game of Life or
Death"; Douglas Toussaint and Frank Wilczek (A + B → 0).

## How JETCARP's version relates

JACA is JETCARP's own family. Notation `JACA B…/S…` or `JACA +B…/S… -B…/S…` with optional
`blast <0-64>`. It runs on the CPU on every tiling. B0 rules are refused, and the annihilation glow is
drawn only and is not part of the state (README "## JACA" and "## Rule notation").

## Suggested wording (from novelty-01)

JACA's particular combination appears to be new: an independent B/S rule per sign, annihilation on
contact, a blast radius in neighbourhood steps, cancelled double births, an exact one-sign reduction,
and use on any tiling. Do not claim "first signed/ternary Life" or "first matter–antimatter Life".

## Unverified

- MCell's Weighted Life with negative weights, Golly's rule-table repository and the rest of the
  ConwayLife "Rule request thread" were not searched for two-species annihilating rules (novelty-01
  gaps).

## Search log

- Commit date: `git log -- crates/cart-core/src/jaca.rs` in the CART repository
  (2026-11-08 08:04 −0500, 364b766).
- All web sources: see novelty-01's search log (2026-11-10).
