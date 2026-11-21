# Novelty audit 05: spaceships on non-square periodic tilings

Audit date: 2026-11-10.

## JETCARP data

Query (read-only) on the CART catalogue (`catalog.sqlite`):
`SELECT tiling_name, neighborhood, rule, period, dx, dy, speed, cells, count(*) FROM patterns WHERE class='spaceship' GROUP BY 1..8`.
There are 103 spaceship records, all with touching (vertex) neighbours:

| Tiling family | Ships (records) | Rules (default rule stored) |
| --- | --- | --- |
| 3.6.3.6 trihexagonal | 11 (p4 c/4: 5, 9, 10, 10, 10 cells; p12 c/4: 9–16 cells) | B3,8,9,10,11/S2,3,8,10,11,12. **Rule range of the p4 ships starts at B3/S23** (birth_min = 8, survive_min = 12). The p12 ships need B38 |
| 4.8.8 truncated square | 10 | B27/S25 (c/2, c/3) |
| 3.12.12 truncated hexagonal | 5 | B2,6,11,12/S2,6,7,11 |
| 3.4.6.4 rhombitrihexagonal | 2 (p42 c/14) | B3,10,12/S2,3,5,8,9,10 |
| 3.3.3.3.6 snub hexagonal | 1 (p27 c/27) | B3,7,11,12,17/S… |
| 3.3.3.4.4 elongated triangular | 1 (p8 c/4, 17 cells) | B4,6,9,10/S3,4,5,8,10,11 |
| 4.6.12 truncated trihexagonal | 1 (p4 c/4, 5 cells) | B2,6,8,9,10,12/S2,5,6,9,12 |
| Triangles ("Any triangle") | 1 (p2, c oblique, 2 cells) | B1,9,10,11/S0,10,11,12 |
| Isohedral IH46, IH54, IH62, IH72, IH84 | 9 | B3/S2378, B38/S238, B37/S238, B3,7,9,11/S29 |
| 2-uniform (k2-004, -006, -012, -013, -014, -017) | 11 | various |
| 3-uniform (k3-003, -009, -010, -013, -016, -019, -021, -022, -028, -029, -030, -032, -038, -039, -045, -053, -058) | 30 | various |
| Square (touch) | 20 | B36/S23, B3678/S23, B378/S238, etc. |

The ships named in the brief (from README "First results"):
- the 5-cell c/4 ship on 3.6.3.6 in B3/S23 (291 copies; also in B36/S23);
- a 12-cell period-8 c/4 ship on the snub square 3.3.4.3.4 in B3/S23 (119 copies), which is not in the current
  catalog's spaceship class;
- an 18-cell period-16 c/2 ship on 4.8.8 in B2/S34 (seen once), also not in the current catalog.

## Verdict

- **3.6.3.6 (kagome) in B3/S23 and snub square in B3/S23: Prior art found.** Guttesen (arXiv, Aug 2026)
  reports a period-4 small glider and a period-40 big glider on kagome, a period-276 glider gun, and a
  period-8 glider on the snub square, all in B3/S23 with corner-or-edge neighbours. JETCARP's 5-cell
  p4 ship and 12-cell p8 ship are almost certainly the same objects. That should be checked against the
  figures, because the paper gives no cell counts.
- **Hexagons, triangles, Cairo pentagons: Prior art found** for gliders in general (Bays 1994, 2005,
  2007; LifeWiki/forum hexagonal rules). JETCARP's non-square ships on these exact tilings were not
  compared object by object.
- **4.8.8, 3.12.12, 4.6.12, 3.3.3.3.6** in other rules: **nothing found.** Guttesen studied these
  tilings in B3/S23 only and found no gliders. The JETCARP ships are in other rules: B27/S25 and B2/S34
  on 4.8.8, B2,6,11,12/S2,6,7,11 on 3.12.12, and so on.
- **3.4.6.4, 3.3.3.4.4, isohedral types, 2- and 3-uniform tilings: nothing found after search.** We found
  no published Life-like spaceship on any k-uniform (k ≥ 2) tiling or on these isohedral types.
- **Square-grid ships:** presumed known. Catagolue (https://catagolue.hatsya.com) censuses these rules,
  e.g. /census/b3678s23 has 10 official symmetries and an `ikpx2_stdin` spaceship-search census. No
  claim should be made for square ships.

## Prior work (verified)

1. **Guttesen, H. S., "Game of Life on Archimedean Lattices: Glider Guns and Phase Dynamics"**,
   arXiv:2608.27622 (v1 27 Aug 2026, v2 14 Sep 2026; nlin.CG; the comments say "accepted for
   publication in the Journal of Cellular Automata"). https://arxiv.org/abs/2608.27622 .
   B3/S23 on six composite Archimedean lattices with the "Moore neighborhood (corner- or edges-sharing
   plaquettes)". Results:
   - Kagome: "small glider, g, of period 4" and a "big C2 symmetric glider, G, of period 40". "The
     glider gun comprises four interacting bouncers and stably emits a small glider every 276th
     generation." Gliders and puffers "arise frequently from random initial configurations".
   - Snub square: "Snub square lattice GoL glider of period 8".
   - 4.8.8, 4.6.12, 3.12.12 ("Star") and 3.3.3.3.6 ("maple leaf"): still lifes and oscillators only, with
     no gliders in B3/S23.
   - 3.4.6.4 and 3.3.3.4.4 were not studied.
   The paper does not claim to be first to find the gliders; its claim is a "novel glider gun".
   **For JETCARP:** the 3.6.3.6 B3/S23 5-cell c/4 ship and the snub-square p8 ship should be credited to
   (or at least compared with) Guttesen 2026. JETCARP found them independently, from soups.
2. **Bays, C., "Cellular Automata in the Triangular Tessellation"**, Complex Systems 8(2) (1994)
   (https://www.complex-systems.com/?p=2198 ). Defines "lifelike" (LFR) and "Game of Life" (GL)
   rules, the latter requiring a glider that turns up from random starts. A 2005 follow-up says this
   paper "introduced several games of life in the triangular tessellation".
3. **Bays, C., "A Note on the Game of Life in Hexagonal and Pentagonal Tessellations"**, Complex Systems
   15(3):245–252 (2005), DOI 10.25088/ComplexSystems.15.3.245
   (https://www.complex-systems.com/abstracts/v15_i03_a04/ ). Search-engine excerpts of the full text,
   which we could not read directly (the PDF did not extract), give a hexagonal glider in rule 2,4,5/3
   (Bays's survival/birth order, i.e. B3/S245) of period 7, another in 3/2,4,5 (B245/S3) of period 10,
   and a pentagonal (Cairo) glider in 2,3/3,4,6 (B346/S23) of period 48 moving 6 units. The pentagonal
   one is confirmed independently by Kerekrety (below).
4. **Bays, C., "The Discovery of Glider Guns in a Game of Life for the Triangular Tessellation"**,
   Journal of Cellular Automata 2(4):345–350 (2007)
   (https://www.oldcitypublishing.com/journals/jca-home/jca-issue-contents/jca-volume-2-number-4-2007/jca-2-4-p-345-350/ ).
   "This note introduces and briefly discusses another GL rule that supports 'glider guns' … in the
   triangular tessellation".
5. **Hexagonal B2/S34 (B2/S34H)**: ConwayLife.com forum thread "B2/S34H (Hexagonal Life)", started by
   drc on 18 Jan 2016 (https://conwaylife.com/forums/viewtopic.php?p=26870 ; read through the mirror
   conwaylife.njohnston.ca). Dean Hickerson (20 Jan 2016): "I never found any spaceships or puffers,
   but I'd bet that some exist". wildmyron (20 Jan 2016): "spaceships do exist". He posted 2c/4 and c/5
   ships found with a modified gfind. Also LifeWiki "OCA:B2o/S2m34H" (Paul Callahan's December 1997
   non-totalistic hexagonal rule with a common 2c/4 ship; from a search summary only).
6. **Kerekrety, M., "That game of life is hard to play"**, 2020, https://matejker.github.io/game-of-life/ .
   Life-like rules on hexagons (3,5/2), Cairo pentagons (2,3/3,4,6, with the "shrimp" glider of period 48
   that "moves 6 units up"), rhombille (very few gliders) and sphinx tilings.
7. **Code Golf Stack Exchange, "Implement the game of life on anything but a regular grid"**, question
   35827 (2014, by "Calvin's Hobbies"; it has answers on rhombille, Penrose and other tilings, and a 2026
   answer cited by Guttesen). The page could not be fetched (blocked); it is known only through Guttesen's
   reference [22] and a search summary.
8. **Aperiodic (for context; see novelty-06):** Hill, M., Stepney, S. and Wan, F., "Penrose Life: Ash and
   Oscillators", ECAL 2005, LNCS 3630, pp. 471–480, DOI 10.1007/11553090_48. Owens, N. and Stepney, S.,
   "The Game of Life Rules on Penrose Tilings: still life and oscillators", in Adamatzky (ed.) *Game of Life
   Cellular Automata*, Springer 2010, ch. 18, pp. 331–378 ("We have found no propagating features
   analogous to gliders"). Goucher, A. P., "Gliders in Cellular Automata on Penrose Tilings", JCA
   7(5–6):385–392 (2012) ("the first glider capable of navigating an aperiodic tiling", four-state
   outer-totalistic). Bailey, D. A. and Lindsey, K. A., "A Game of Life on Penrose tilings", arXiv:1708.09301
   (2017).

## Per tiling family: is any glider documented?

| Tiling | Documented glider in any Life-like rule? | Source |
| --- | --- | --- |
| Triangles | Yes | Bays 1994, 2007 |
| Hexagons | Yes (B3/S245, B245/S3; B2/S34H 2c/4 and c/5) | Bays 2005; ConwayLife forum 2016 |
| Cairo pentagons | Yes (B346/S23, p48) | Bays 2005; Kerekrety 2020 |
| 3.6.3.6 kagome | **Yes, B3/S23 p4 and p40, plus a gun** | Guttesen 2026 |
| 3.3.4.3.4 snub square | **Yes, B3/S23 p8** | Guttesen 2026 |
| 4.8.8 | None found (B3/S23: none, per Guttesen) | — |
| 3.12.12, 4.6.12, 3.3.3.3.6 | None found (B3/S23: none, per Guttesen) | — |
| 3.4.6.4, 3.3.3.4.4 | None found | — |
| 2- and 3-uniform tilings | None found | — |
| Isohedral IH46/54/62/72/84 | None found (other isohedral types: rhombille has "very little of gliders", Kerekrety) | — |

## Suggested credit wording

> Gliders on non-square tilings go back to Bays (triangles 1994; hexagons and Cairo pentagons 2005;
> triangular glider guns 2007). For Life (B3/S23) itself, Guttesen (2026) reported gliders on the
> kagome (3.6.3.6) and snub-square lattices and a kagome glider gun. JETCARP's soup searches found the
> same small kagome and snub-square ships independently. To our knowledge, the spaceships JETCARP
> catalogues on 4.8.8, 3.12.12, 4.6.12, 3.4.6.4, 3.3.3.4.4, 3.3.3.3.6, several isohedral tilings, and
> 2- and 3-uniform tilings (in rules other than those studied by Guttesen) have not been reported before.

Before release, compare JETCARP's kagome 5-cell ship and snub-square 12-cell ship with Guttesen's
Figs. 4 and 5. Also compare the kagome 9- and 10-cell p4 ships (also in range of B3/S23), which
Guttesen does not show.

## Search log (2026-11-10)

- WebSearch: `Carter Bays "A Note on the Game of Life in Hexagonal and Pentagonal Tessellations" Complex Systems 2005 glider`
- WebSearch: `hexagonal Life B2/S34 glider spaceship "B2/S34H" LifeWiki hexagonal neighbourhood`
- WebSearch: `"B2/S34H" hexagonal life glider conwaylife forum thread`
- WebSearch: `Carter Bays "Cellular Automata in the Triangular Tessellation" Complex Systems 1994 glider rule`
- WebSearch: `Game of Life Archimedean tilings gliders spaceships kagome trihexagonal 4.8.8 snub square cellular automata` (this one found Guttesen)
- WebSearch (conwaylife.com, conwaylife.njohnston.ca): `snub square tiling life rule spaceship OR glider conwaylife`; `trihexagonal OR kagome OR "truncated square" OR "4.8.8" OR octagon tiling life-like rule glider spaceship`; `"Archimedean" tilings cellular automata Life forum thread rule spaceships "semiregular"` (none relevant)
- WebSearch: `cellular automata "k-uniform" OR "semi-regular" OR "Archimedean" tilings Life-like rules gliders paper thesis …`
- WebSearch: `"truncated square tiling" OR "octagonal tiling" OR "4.8.8" "Game of Life" OR "life-like" cellular automaton glider`
- WebSearch: `"Implement the game of life on anything but a regular grid" codegolf glider kagome snub square Peter Taylor`
- WebSearch: Hill/Stepney/Wan; Goucher JCA 2012; Trevorrow HatLife.
- Fetched: arxiv.org/abs and /html of 2608.27622 (twice), arxiv.org/abs and /html v2 of 2609.22579,
  complex-systems.com (both Bays abstracts), oldcitypublishing.com (Bays 2007, Goucher 2012), the Bays
  2005 PDF (no text could be extracted), conwaylife.njohnston.ca Hexagonal_neighbourhood and forum
  p=26870, matejker.github.io/game-of-life, York pages (Owens and Stepney), arxiv 1708.09301,
  catagolue.hatsya.com/census/b3678s23.

## Gaps / unverified leads

- Cell counts in Guttesen's figures were not available as text. Identity with JETCARP's ships is
  inferred from tiling, rule, neighbourhood and period.
- Bays's rule/period details for hexagons and pentagons come from search excerpts, not the PDF.
- Bays, "The Game of Life in Non-square Environments", in Adamatzky (ed.) *Game of Life Cellular
  Automata* (Springer 2010), was not checked. It may cover more tessellations.
- The ConwayLife forum's "Other Cellular Automata" board was not searched exhaustively. Threads on
  Archimedean tilings may exist that the search engines do not index. LifeViewer's and Golly's support
  for non-square grids (e.g. Golly rule tables with custom neighbourhoods, and Ready meshes) means
  hobbyists could have found ships without publishing them.
- A Brown University PDF (cs.brown.edu …/donahuehenry.pdf) turned up for "4.8.8 Game of Life" but its
  text could not be read.
- The 18-cell p16 c/2 4.8.8 ship in B2/S34 and the snub-square 18- and 21-cell ships are not in the
  current catalog. Re-verify them before they are cited.
