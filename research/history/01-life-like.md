# 01. Life-like rules (B/S notation): history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.

## Short history

- **Conway's Life, 1970.** John Horton Conway's Game of Life reached the public in Martin Gardner's
  "Mathematical Games" column in *Scientific American*, October 1970. The column's subtitle is "The
  fantastic combinations of John Conway's new solitaire game 'life'" [gardner1970life]. Gardner
  followed it up in February 1971 with a second column on cellular automata, self-reproduction and
  Life [gardner1971life]. LifeWiki says Conway was simplifying ideas from von Neumann's
  self-reproducing automaton (see file 12), and that Conway offered a $50 prize for a pattern that
  grows without limit. An MIT group led by Bill Gosper won it in November 1970 with the glider gun
  [lifewikind_conwayslife].
- **Universality.** Berlekamp, Conway and Guy's *Winning Ways for your Mathematical Plays* (Academic
  Press, 1982, two volumes; second edition A K Peters, 2001–2004, four volumes) gives Conway's
  sketch that Life can host a universal computer and constructor [berlekamp1982winning;
  lifewikind_winningways]. Paul Rendell later built an explicit Turing machine in Life
  [rendell2002turing].
- **Neighbourhoods.** Life uses the eight-cell neighbourhood named after Edward F. Moore, a pioneer
  of CA theory [lifewikind_moore]. Moore's paper "Machine models of self-reproduction" was given at
  the AMS symposium of April 1961 and printed in *Proc. Symp. Appl. Math.* 14 (1962)
  [moore1962machine]. The four-cell neighbourhood is named after John von Neumann, who used it in
  his 29-state automaton [lifewikind_vonneumann]. LifeWiki's convention adds a `V` suffix to a rule
  string for the von Neumann neighbourhood and an `H` suffix for hexagons [lifewikind_rulestring].
- **The Life-like rule space.** LifeWiki uses "Life-like" for two-state, outer-totalistic rules on the
  square grid with the Moore neighbourhood. A rule is fixed by its birth and survival counts (0–8), so
  there are 2^18 = 262,144 of them. They pair up under black/white reversal, and 2^9 = 512 are their
  own reversal [lifewikind_lifelikeca]. Packard and Wolfram's 1985 survey of two-dimensional CAs
  numbered outer-totalistic rules with integer codes and studied their classes
  [packard1985twodimensional]. Eppstein (2010) classified Life-like rules by whether patterns can
  escape a bounding box and whether they can die out [eppstein2010growth].
- **Notation.** Two orders are in use. Survival/birth (`23/3`) was the older and more common form, but
  LifeWiki now calls it disused. Birth/survival (`B3/S23`) is the form Golly and RLE files use
  [lifewikind_rulestring; lifewikind_lifelikeca]. Wójtowicz's MCell rule lexicon wrote the S/B form
  [wojtowicznd_rules]. We found no source that names who first wrote `B…/S…` or when. Golly (from
  2005) uses it, but that does not make Golly its origin.
- **Early variants.** 3-4 Life (B34/S34) was one of the first variants studied. LifeWiki cites
  Robert Wainwright's newsletter *Lifeline* vol. 4 (December 1971) [lifewikind_34life]. Later
  named variants, with credits checked in `named-rule-authors.md`:
  - **HighLife** (B36/S23): first considered by Nathan Thompson in 1994 and named by Conway. It has a
    small replicator [lifewikind_highlife].
  - **Day & Night** (B3678/S34678): explored by Nathan Thompson from 1997. It is its own black/white
    reversal [lifewikind_daynight].
  - **Seeds** (B2/S): first studied by Brian Silverman, and named by Wójtowicz in MCell
    [lifewikind_seeds].
  - **Replicator** (B1357/S1357) and **Fredkin** (B1357/S02468): every pattern replicates. These are
    the parity rules associated with Edward Fredkin. MCell credits Fredkin with the second.
  - **Life without Death** (B3/S012345678): Toffoli and Margolus's INKSPOT (1987)
    [toffoli1987cellular].
  - **Vote** and **Vote 4/5 (Anneal)**: Vichniac's majority and twisted-majority rules
    [vichniac1986cellular].
- **B0 rules and strobing.** In a rule with B0, every empty cell with no live neighbours is born, so
  the empty background turns on. Without S8 it flips back each generation, which is called
  strobing. Golly has supported B0 rules since version 0.2 (July 2005) [gollynd_changes]. A B0 rule
  with S8 is converted to an equivalent rule without B0 by inverting the neighbour counts. A B0 rule
  without S8 becomes two rules that alternate on even and odd generations, so no frame strobes
  [gollynd_quicklife]. Golly's credits name David Eppstein for the idea of emulating B0 rules
  [gollynd_credits].

## Key primary citations

- [gardner1970life] Gardner, M., "Mathematical Games: The fantastic combinations of John Conway's new
  solitaire game 'life'", *Scientific American* 223(4):120–123 (October 1970).
  DOI 10.1038/scientificamerican1070-120. Verified: Crossref record (title "Mathematical Games",
  vol. 223, issue 4, pp. 120–123, Oct 1970). The subtitle is given by LifeWiki and by Wikipedia's
  reference list.
- [gardner1971life] Gardner, M., "Mathematical Games" (second Life column: cellular automata,
  self-reproduction, the Garden of Eden and Life), *Scientific American* 224(2):112–117 (February
  1971). DOI 10.1038/scientificamerican0271-112. Verified: Crossref record. Crossref misspells the
  author as "Cardner", and the subtitle is not in the record.
- [berlekamp1982winning] Berlekamp, E. R., Conway, J. H. and Guy, R. K., *Winning Ways for your
  Mathematical Plays*, 2 vols, Academic Press, New York (1982); 2nd ed. A K Peters, 2001–2004, 4 vols
  (Life is the last chapter of vol. 4). Verified: edition and ISBN data from Wikipedia's article
  (vol. 2 ISBN 0-12-091152-3) and LifeWiki "Winning Ways"; the book itself was not seen.
- [moore1962machine] Moore, E. F., "Machine models of self-reproduction", *Proceedings of Symposia in
  Applied Mathematics* 14 (Mathematical Problems in the Biological Sciences), American Mathematical
  Society, pp. 17–33 (1962). Verified: by search only (Wikipedia, the IMSc library record for the
  volume, and the Wellcome Collection offprint record). Myhill (1963) cites pp. 17–34.
- [packard1985twodimensional] Packard, N. H. and Wolfram, S., "Two-dimensional cellular automata",
  *Journal of Statistical Physics* 38(5–6):901–946 (1985). DOI 10.1007/BF01010423. Verified: Crossref
  record.
- [eppstein2010growth] Eppstein, D., "Growth and decay in life-like cellular automata", in
  Adamatzky, A. (ed.), *Game of Life Cellular Automata*, Springer, pp. 71–98 (2010).
  DOI 10.1007/978-1-84996-217-9_6; arXiv:0911.2890. Verified: arXiv abstract page.
- [rendell2002turing] Rendell, P., "Turing universality of the Game of Life", in Adamatzky, A. (ed.),
  *Collision-Based Computing*, Springer, pp. 513–539 (2002). DOI 10.1007/978-1-4471-0129-1_18.
  Verified: Crossref record.
- [gollynd_quicklife] Golly Help, "QuickLife" (B0 emulation: inverted counts with Smax; alternating
  even and odd rules without it). https://golly.sourceforge.io/Help/Algorithms/QuickLife.html .
  Verified: fetched.
- [gollynd_credits] Golly Help, "Credits" (Rokicki and Trevorrow; Eppstein credited with the idea of
  emulating B0 rules). https://golly.sourceforge.io/Help/credits.html . Verified: fetched.
- [gollynd_changes] Golly Help, "Changes" (0.1, July 2005; B0 support from 0.2, July 2005).
  https://golly.sourceforge.io/Help/changes.html . Verified: fetched.
- LifeWiki pages, all read through the mirror conwaylife.njohnston.ca: "Life-like cellular automaton"
  [lifewikind_lifelikeca], "Rulestring" (last edited 30 Aug 2023) [lifewikind_rulestring], "Moore
  neighbourhood" (26 Aug 2024) [lifewikind_moore], "Von Neumann neighbourhood" (8 May 2026)
  [lifewikind_vonneumann], "Conway's Game of Life" (15 Dec 2025) [lifewikind_conwayslife], "Winning
  Ways" [lifewikind_winningways], "OCA:HighLife" [lifewikind_highlife], "OCA:Day & Night"
  [lifewikind_daynight], "OCA:Seeds" [lifewikind_seeds], "OCA:3-4 Life" [lifewikind_34life].

## Software that ran it before JETCARP

- **Golly** (Andrew Trevorrow and Tom Rokicki, first release 0.1 in July 2005; open source; the
  QuickLife and HashLife engines, B0 emulation from 0.2) [gollynd_credits; gollynd_changes].
  HashLife itself is Bill Gosper's algorithm.
- **MCell / Mirek's Cellebration** (Mirek Wójtowicz; Windows, from 1999), whose rules lexicon
  documents most named Life-like rules [wojtowicznd_rules]. See file 02.
- **CelLab** (Rucker and Walker, Autodesk, 1989) and the CAM-6 hardware of Toffoli and Margolus
  [rucker1989cellab; toffoli1987cellular].
- **LifeViewer** (Chris Rowett), the in-browser viewer on LifeWiki and the forums [rowett2019lifeviewer].
- **Catagolue / apgsearch** (Adam P. Goucher), the soup census, which also covers other rules
  (cited on the HighLife page [lifewikind_highlife]).
- Unverified here: Life32 (Johan Bontes), Xlife.

## People JETCARP should credit

John Horton Conway (Life); Martin Gardner (publication); Edward F. Moore and John von Neumann
(neighbourhoods); Berlekamp, Conway and Guy (universality); Norman Packard and Stephen Wolfram (the
2-D rule space); David Eppstein (Life-like classification; idea of B0 emulation); Nathan Thompson
(HighLife, Day & Night); Brian Silverman (Seeds); Edward Fredkin (replicator rules); Mirek Wójtowicz
(MCell and its rule lexicon); Andrew Trevorrow and Tom Rokicki (Golly); the LifeWiki editors (named
rule lists). Further rule-by-rule credits are in `named-rule-authors.md`.

## How JETCARP's version relates

JETCARP runs any B/S rule with touching or edge (`V`) neighbours on every tiling. Commas separate
counts of 10 or more, since tiles on some tilings have more than 8 neighbours, and `H` marks a rule
whose usual grid is hexagons. B0 rules run as alternating complementary rules, "as other CA programs
do", so the background does not strobe. This is the Golly/Eppstein method and is checked against
brute force (README, "What Phase 2 adds"). The named catalogue comes from LifeWiki's lists.

## Unverified

- Who first wrote rules as `B…/S…`. No fetched source names the person or the date.
- The year Conway devised Life (1969 or 1970), and the roles of Cambridge colleagues. Neither was
  stated in the sources fetched.
- The exact chapter number and title of the Life chapter in the 1982 first edition of *Winning Ways*
  (ch. 25 "What is Life?" per a later edition's listing, seen only in search results).
- Moore 1962 page range (17–33 or 17–34). The paper itself was not seen.
- Life32 and Xlife dates and authors.

## Search log

- Crossref API: 10.1038/scientificamerican1070-120, 10.1038/scientificamerican0271-112,
  10.1007/BF01010423, 10.1007/978-1-4471-0129-1_18.
- Fetched: conwaylife.njohnston.ca/wiki/{Life-like_cellular_automaton, Rulestring,
  Moore_neighbourhood, Von_Neumann_neighbourhood, Black/white_reversal, Conway%27s_Game_of_Life,
  Winning_Ways, OCA:HighLife}; en.wikipedia.org/wiki/{Life-like_cellular_automaton,
  Winning_Ways_for_Your_Mathematical_Plays} (used as pointers); golly.sourceforge.io (home page,
  Help/credits.html, Help/changes.html, Help/Algorithms/QuickLife.html); arxiv.org/abs/0911.2890.
- WebSearch: `Edward F. Moore "Machine models of self-reproduction" Proceedings of Symposia in Applied
  Mathematics 14 1962`; `Berlekamp Conway Guy "Winning Ways …" 1982 Academic Press volume 2 …`;
  `Golly 1.0 released 2005 Andrew Trevorrow Tom Rokicki …`.
