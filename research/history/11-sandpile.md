# 11. Abelian sandpile: history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.

## Short history

- **Bak, Tang and Wiesenfeld, 1987.** The sandpile model comes from statistical physics. Per Bak,
  Chao Tang and Kurt Wiesenfeld proposed it in *Physical Review Letters* in 1987 as an example of
  "self-organized criticality". In their model, grains are dropped at random sites of a grid. A site
  holding as many grains as it has neighbours topples and sends one grain to each neighbour, and
  avalanche sizes follow a power law [bak1987selforganized]. They developed the idea at length in
  *Physical Review A* the next year [bak1988selforganized].
- **Dhar, 1990: the abelian property.** Deepak Dhar showed that the final stable state does not
  depend on the order of topplings. Adding grains therefore defines commuting operators, and the
  recurrent configurations form an abelian group [dhar1990selforganized]. Levine and Propp credit the
  name "sandpile group" to Dhar [levine2010sandpile]. Majumdar and Dhar later linked the model to
  spanning trees and the q → 0 Potts model [majumdar1992equivalence].
- **The same model under other names.** Levine and Propp note that sandpile dynamics "have been
  invented numerous times", as chip-firing, the probabilistic abacus and the dollar game
  [levine2010sandpile]. In combinatorics the main early reference is Björner, Lovász and Shor's
  "Chip-firing games on graphs" (1991) [bjorner1991chipfiring]. The group has other names as well:
  group of components, Jacobian group, critical group [levine2010sandpile].
- **The identity element, Creutz 1991.** Because the recurrent states form a group, it has an
  identity, and on a square grid that identity has an intricate fractal structure. Michael Creutz
  drew attention to it in "Abelian sandpiles" (*Computers in Physics*, 1991) [creutz1991abelian].
  Caracciolo, Paoletti and Sportiello begin their 2008 paper "Since the work of Creutz" and call this
  configuration the Creutz identity [caracciolo2008explicit]. Le Borgne and Rossin studied the
  identity combinatorially [leborgne2002identity]. Levine and Propp's Figure 2 shows it on a
  523 × 523 grid [levine2010sandpile].
- **Single-source piles.** Dropping N grains on one site gives a large, scale-invariant pattern.
  Ostojic studied these on Z² [ostojic2003patterns], and Pegden and Smart proved that the rescaled
  piles converge [pegden2013convergence]. Levine and Propp's Figure 1 shows a pile of 10^6 chips
  [levine2010sandpile].
- **Off the square grid.** See novelty-07. Identities have been computed on Penrose, triangular and
  hexagonal grids [fersula2020sandpile] and on the hexagonal and octagon–square grids (Johnson 2019
  thesis).

## Key primary citations

- [bak1987selforganized] Bak, P., Tang, C. and Wiesenfeld, K., "Self-organized criticality: An
  explanation of the 1/f noise", *Physical Review Letters* 59(4):381–384 (1987).
  DOI 10.1103/PhysRevLett.59.381. Verified: Crossref record.
- [bak1988selforganized] Bak, P., Tang, C. and Wiesenfeld, K., "Self-organized criticality",
  *Physical Review A* 38(1):364–374 (1988). DOI 10.1103/PhysRevA.38.364. Verified: Crossref record.
- [dhar1990selforganized] Dhar, D., "Self-organized critical state of sandpile automaton models",
  *Physical Review Letters* 64(14):1613–1616 (1990). DOI 10.1103/PhysRevLett.64.1613. Verified:
  Crossref record.
- [creutz1991abelian] Creutz, M., "Abelian sandpiles", *Computers in Physics* 5(2):198–203 (1991).
  DOI 10.1063/1.168408. Verified: Crossref record. Its priority for the identity is stated by
  Caracciolo et al. 2008. Creutz also has a short paper with the same title in *Nucl. Phys. B Proc.
  Suppl.* 20:758–761 (1991), DOI 10.1016/0920-5632(91)91016-D (Crossref).
- [bjorner1991chipfiring] Björner, A., Lovász, L. and Shor, P. W., "Chip-firing games on graphs",
  *European Journal of Combinatorics* 12(4):283–291 (1991). DOI 10.1016/S0195-6698(13)80111-4.
  Verified: Crossref record.
- [majumdar1992equivalence] Majumdar, S. N. and Dhar, D., "Equivalence between the Abelian sandpile
  model and the q→0 limit of the Potts model", *Physica A* 185(1–4):129–145 (1992).
  DOI 10.1016/0378-4371(92)90447-X. Verified: Crossref record.
- [leborgne2002identity] Le Borgne, Y. and Rossin, D., "On the identity of the sandpile group",
  *Discrete Mathematics* 256(3):775–790 (2002). DOI 10.1016/S0012-365X(02)00347-3. Verified:
  Crossref record.
- [ostojic2003patterns] Ostojic, S., "Patterns formed by addition of grains to only one site of an
  abelian sandpile", *Physica A* 318(1–2):187–199 (2003). DOI 10.1016/S0378-4371(02)01426-7.
  Verified: Crossref record.
- [caracciolo2008explicit] Caracciolo, S., Paoletti, G. and Sportiello, A., "Explicit
  characterization of the identity configuration in an Abelian sandpile model", *J. Phys. A* 41:495003
  (2008). DOI 10.1088/1751-8113/41/49/495003; arXiv:0809.3416. Verified: arXiv abstract page and
  Crossref record.
- [levine2010sandpile] Levine, L. and Propp, J., "What is … a sandpile?", *Notices of the AMS*
  57(8):976–979 (September 2010). PDF https://faculty.uml.edu/jpropp/whatisasandpile.pdf . Verified:
  PDF text extracted locally (title, authors, "September 2010 Notices of the AMS"). Volume, issue and
  pages come from citing papers in search results.
- [pegden2013convergence] Pegden, W. and Smart, C. K., "Convergence of the Abelian sandpile", *Duke
  Mathematical Journal* 162(4):627–642 (2013). DOI 10.1215/00127094-2079677. Verified: Crossref
  record.
- [fersula2020sandpile] Fersula, J., Noûs, C. and Perrot, K., "Sandpile toppling on Penrose tilings:
  identity and isotropic dynamics", arXiv:2006.06254 (2020). Verified: novelty-07.

## Software that ran it before JETCARP

The model has been reimplemented countless times since 1987, and we found no canonical program.
Documented computations off the square grid are Fersula et al. (Penrose, triangular and hexagonal) and
Johnson 2019 (hexagonal, 4.8.8) (novelty-07). Golly, CelLab and MCell sandpile rules were not checked.

## People JETCARP should credit

Per Bak, Chao Tang and Kurt Wiesenfeld (the model); Deepak Dhar (abelian property and the sandpile
group); Michael Creutz (the identity pictures); Anders Björner, László Lovász and Peter Shor
(chip-firing); Lionel Levine and James Propp (survey); Wesley Pegden and Charles Smart (scaling
limit); Fersula, Noûs and Perrot (Penrose identities).

## How JETCARP's version relates

JETCARP runs the abelian sandpile on every tiling. A site topples when it holds as many grains as
its tile has neighbours. It computes the identity as stab(2δ − stab(2δ)) with sink (dead) edges and
tests stab(I + I) = I on squares, hexagons, Cairo pentagons and the Spectre (README "What Phase 1
adds"). Large single-source piles grow by binary doubling up to 10^8 grains (README "## Large
sandpiles").

## Unverified

- That Creutz's 1991 *Computers in Physics* paper contains the identity pictures. This rests on
  Caracciolo et al.'s statement and on search excerpts from a 2018 PNAS paper (PMC page blocked), not
  on reading Creutz.
- Engel's "probabilistic abacus" (1975) and Spencer's chip-firing (1986) as the earliest
  appearances. These are named by Levine and Propp, but the papers were not fetched.
- The formula stab(2δ − stab(2δ)) for the identity is standard, but no source for it was fetched
  in this pass.

## Search log

- Crossref API (curl): 10.1103/PhysRevLett.59.381, 10.1103/PhysRevA.38.364,
  10.1103/PhysRevLett.64.1613, 10.1016/S0195-6698(13)80111-4, 10.1215/00127094-2079677,
  10.1016/0378-4371(92)90447-X; bibliographic queries for Creutz "Abelian sandpiles", Le Borgne and
  Rossin, Levine and Propp, Ostojic, Caracciolo et al.
- Fetched: arxiv.org/abs/0809.3416; faculty.uml.edu/jpropp/whatisasandpile.pdf (text extracted with a
  local script); pmc.ncbi.nlm.nih.gov/articles/PMC6386721 (blocked by a browser
  check).
- WebSearch: `Creutz 1991 "Abelian sandpiles" identity configuration square lattice picture Le Borgne
  Rossin …`; `Levine Propp "What is a sandpile?" Notices of the AMS 2010 57(8) 976`.
