# JETCARP novelty audit: summary

Audit date: 2026-11-10. Auditor: Claude Code (Opus 5.5), for Jimmie Rodgers. There is one file per
candidate in this folder, and each has full citations, quotes, a search log and its gaps.
Verdict scale: **Prior art found** / **Related work only** / **Nothing found after search**.

| # | Candidate | Verdict | Key citation(s) | File |
| --- | --- | --- | --- | --- |
| 1 | JACA (balanced ternary, own-sign B/S, contact annihilation, blast, cancelled births) | **Prior art found for each ingredient; the exact combination was not found** | "Making Conway's Game of Life Playable", Unsupervision Blog, 5 Nov 2025 (CodinGame "Game of Life or Death": cells 1/−1/0, each player counts only its own cells, double births cancel); KittyTac's matter/antimatter proposal and wildmyron's "Energetic" rule, ConwayLife.com Rule request thread, Sep 2018 (contact turns both cells into energy); Levene & Roussos, IJMPC 14:195 (2003) (p2life); Toussaint & Wilczek, J. Chem. Phys. 78:2642 (1983) (A + B → 0) | novelty-01-jaca.md |
| 2 | Colouring: age along palette stops, JACA per-sign palettes, annihilation glow, row gradients | **Prior art found** | LifeViewer ALIVE→ALIVERAMP over AGESTATES ("visual representation of cell longevity") and DEADRAMP history fade (Rowett); Dumont, ChipKIT Life, 4 Nov 2011 (age colours to show "which areas had been stable for a long time"); raywo MMM-GameOfLife PR #28, Oct 2026 (age palette, afterglow, "still lifes darken"). Row-depth colouring is common; **attribution to Wolfram not confirmed** | novelty-02-colour.md |
| 3a | HexW / HexW3 (two-parent hexagon rows) | **Prior art found** | Wolfram, *NKS* (2002) p. 885 note (j): cells "arranged like hexagons or staggered bricks", 16 rules (k = 2) and 19,683 (k = 3, 1,734 inequivalent); Kerekrety 2020 web tool (hexagons, 2 ancestors) | novelty-03-hex-elementary.md |
| 3b | HexT / HexT3 (flat-top, parents NW, N, NE) | **Related work only** | An order-2 staggered 1-D CA: NKS p. 437 and p. 1017 (second-order rules); Wolfram Language `CellularAutomaton` (half-integer ranges, order-s). The XOR-with-N subset is the reversible second-order form of the two-cell rules | novelty-03-hex-elementary.md |
| 4 | Layered rules (BFS distance shells as rows, any tiling) | **Nothing found after search** (related only) | Margenstern, arXiv:1004.1830 (1-D CAs embedded in hyperbolic tilings); Kerekrety 2020; polar ECA renderings. Name clash with Dalai, "Layered Cellular Automata", arXiv:2308.06370 | novelty-04-layered.md |
| 5a | Kagome 3.6.3.6 5-cell c/4 ship; snub square 12-cell p8 c/4 ship (B3/S23, touching) | **Prior art found** | Guttesen, "Game of Life on Archimedean Lattices: Glider Guns and Phase Dynamics", arXiv:2608.27622 (27 Aug 2026; accepted JCA): kagome p4 and p40 gliders, p276 gun; snub square p8 glider (corner-or-edge neighbours) | novelty-05-spaceships.md |
| 5b | Ships on triangles, hexagons, Cairo pentagons | **Prior art found** (gliders exist; not compared object by object) | Bays, Complex Systems 8(2) (1994), 15(3):245 (2005), JCA 2(4):345 (2007); ConwayLife "B2/S34H (Hexagonal Life)" thread, Jan 2016 (wildmyron's 2c/4 and c/5 ships) | novelty-05-spaceships.md |
| 5c | Ships on 4.8.8 (incl. 18-cell p16 in B2/S34), 3.12.12, 4.6.12, 3.4.6.4, 3.3.3.4.4, 3.3.3.3.6, isohedral IH46/54/62/72/84, 2- and 3-uniform tilings | **Nothing found after search** | Guttesen 2026 found no B3/S23 gliders on 4.8.8, 4.6.12, 3.12.12 or 3.3.3.3.6 (JETCARP's ships use other rules) | novelty-05-spaceships.md |
| 6 | Hat gliders from `cart evolve` (e.g. `Priority 4: 3\|n2>=1->2; 0\|n3>=2->0; 0\|n2>=1->3; 0\|n1>=1->2`) | **Prior art found** (method, glider class, corridor). The exact table is not in Edmond's records | Edmond, arXiv:2609.22579 (2026) and repo `GLIDERS.md`: hat gliders A/B/C share one corridor and have certificates of immortality. A rule-by-rule check finds this rule closest to Edmond's **hat glider B up to relabelling states** (561 of ~12k input cases differ). Also Trevorrow's HatLife worms (2023) and Gabbay's "Aperiodic Life" (2026) | novelty-06-hat-gliders.md |
| 7 | Sandpile identities and large piles on k-uniform, isohedral and aperiodic tilings | **Prior art found** for Penrose, hexagonal and 4.8.8 identities; **nothing found** for the hat, spectre, other k-uniform and isohedral tilings | Fersula, Noûs & Perrot, arXiv:2006.06254 (2020; identity on Penrose, triangular, hexagonal); Johnson, UW MSc thesis 2019 (hexagonal; 4.8.8 identity figure); Ostojic, Physica A 318:187 (2003) (single source) | novelty-07-sandpiles.md |
| 8 | Generalising CA families to arbitrary tilings with a neighbour graph | **Prior art found** for the idea and for many families; the breadth is the contribution | Ready (GollyGang; "polygonal and polyhedral meshes"); Trevorrow's HatLife (2023: Life-like, LtL-style, WireWorld on the hat); Edmond 2026 (Generations on hat/spectre); Langton's ant on hexagons (Wikipedia; Gale et al. 1995) and Penrose (github.com/ptiles/ant); Cai & Ghrist arXiv:1210.5232 (cyclic CA on networks). Nothing found for GH, cyclic, RPS, Hodgepodge, forest fire or turmites on the hat/spectre | novelty-08-generalising.md |
| 9 | Exploratory search: MAP-Elites over tiling × family × seed, logistic score fitted to human ratings | **Related work only** for the method; the tiling dimension appears new | Mouret & Clune arXiv:1504.04909 (2015); Faldor & Cully arXiv:2406.04235 (2024, MAP-Elites on Lenia); Reinke et al. ICLR 2020 (IMGEP); Kumar et al. arXiv:2412.17799 (ASAL); Picbreeder (CHI 2008); Christiano et al. arXiv:1706.03741 (preference models from human clip ratings) | novelty-09-explore.md |

## What can be claimed (careful wording)

- **Supportable, to our knowledge:** the JACA combination; layered (distance-shell) rules on any tiling;
  presenting and cataloguing the HexT/HexT3 family; spaceships on 4.8.8 (in non-Life rules),
  3.12.12, 4.6.12, 3.4.6.4, 3.3.3.4.4 and 3.3.3.3.6 (non-Life rules), isohedral and 2- and 3-uniform
  tilings; sandpile identities and large piles on the hat and spectre and across the k-uniform and
  isohedral catalogues; an exploratory search with tilings as a dimension; one engine running every
  family on every tiling.
- **Should not be claimed:** age colouring; hexagonal two-parent elementary rules; Life gliders on
  kagome or the snub square (credit Guttesen 2026 and say they were found independently); hat
  gliders as a new class (credit Edmond; likely the same mechanism); "first CA on aperiodic tilings";
  "first matter–antimatter Life".

## Confidence and main gaps

- **conwaylife.com (LifeWiki and forums) returns HTTP 403 to automated fetches.** Its pages were read
  through the mirror conwaylife.njohnston.ca, which works but may lag the live site. Forum coverage is
  the weakest link: hobbyists may have found ships on Archimedean tilings, or two-species annihilating
  rules, without papers.
- PDF text could not be extracted for several papers (Bays 2005, p2life after page 1, a Brown
  University PDF). The details there come from search-engine excerpts and are marked as such.
- Guttesen's figures were not compared cell for cell. The identity of JETCARP's kagome and
  snub-square ships with his rests on tiling, rule, neighbourhood and period. **Check before release.**
- The JETCARP hat rule and Edmond's glider B were not run side by side (no CPU jobs were allowed). Do
  this before claiming a new glider.
- Recent work (Aug–Oct 2026: Guttesen, Edmond v2, Gabbay's app, raywo PR, Edmond's certificates
  paper) shows that this area is moving fast. Re-run the key searches just before publication.
- The JACA search did not check MCell's Weighted Life with negative weights, Golly's rule-table
  repository, or the rest of the ConwayLife "Rule request thread".

## Reproducibility

Every file lists its queries (WebSearch, standard or extended) and fetched URLs, all dated 2026-11-10.
The rule comparison for candidate 6 was a short local Python script (not published with these notes),
run on a folder holding Edmond's `results/*.jsonl` (from github.com/jamesedmond/monotile-ca); the files were fetched from
raw.githubusercontent.com on 2026-11-10.
