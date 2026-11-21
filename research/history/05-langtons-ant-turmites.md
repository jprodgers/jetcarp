# 05. Langton's ant and turmites: history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.

## Short history

- **Langton's "vants" (1986).** Christopher G. Langton described single-state two-dimensional
  Turing machines, which he called *vants* (virtual ants), in "Studying artificial life with
  cellular automata", Physica D 22 (1986) [langton1986studying]. Greg Turk's 2025 review says that
  sections 9 to 11 of that paper are about Turing machines moving on a square grid, including the
  RL vant, which turns right on a cell it has visited an even number of times and left on an odd
  number [turk2025vants]. Turk also credits Langton with coining the term "vant" and, as far as he
  knows, with being the first to study such machines.
- **Earlier relatives.** Turk notes that Paterson and Conway's "worms" (graph-paper experiments,
  studied exhaustively by Beeler in an MIT AI Lab memo of 1973 and popularised by Gardner in his
  November 1973 Mathematical Games column [gardner1973worms]) are a related idea: the first eight
  steps of Beeler's first square-grid worm trace the same path as the RL vant [turk2025vants].
- **Turk and Dewdney: "tur-mites" (1989).** Greg Turk, without knowing of Langton's work, rediscovered
  the RL ant and sent several 2-D Turing machines to A. K. Dewdney. Dewdney presented them in his
  Computer Recreations column, "Two-dimensional Turing machines and tur-mites make tracks on a
  plane", Scientific American, September 1989 [dewdney1989computer; turk2025vants]. Dewdney coined
  the name *tur-mite* (now "turmite"). The column also described a two-state machine that draws a
  spiral and the bilaterally symmetric RRLL ant [turk2025vants]. In Turk's usage (2025), a *vant* has
  one internal state, and a *turmite* has two or more.
- **Multi-colour ants and the L/R strings (1993–1995).** David Gale's "Mathematical
  Entertainments" columns in *The Mathematical Intelligencer* took up the ant [gale1993industrious]
  and reported James Propp's and others' work on generalised ants whose turn sequence is a string of
  Ls and Rs ("Further ant-ics", 1994 [propp1994further]). Gale, Propp, Sutherland and Troubetzkoy,
  "Further travels with my ant" (1995), identified a class of ants whose patterns are at times
  bilaterally symmetric and proved this with Truchet tiles [gale1995further]. These columns were
  collected in Gale's book *Tracking the Automatic Ant* (Springer, 1998) [gale1998tracking]. Ian
  Stewart's article "The Ultimate in Anty-Particles" (Scientific American,
  July 1994) brought the ant to a wide audience [stewart1994ultimate].
- **Physics: Lorentz lattice gases.** Independently, physicists studied the same dynamics as a
  particle scattered by "flipping" rotators on lattice sites. Bunimovich and Troubetzkoy (1992)
  proved recurrence properties of these Lorentz lattice-gas automata
  [bunimovich1992recurrence]. Their result is the source usually cited (in Wikipedia and MathWorld)
  for the theorem that the ant's trajectory is always unbounded, which is also called the
  Cohen–Kong theorem. Kong and Cohen studied the triangular-lattice version in 1991
  [kong1991diffusion], and Wang and Cohen compared honeycomb and quasi-lattices with the square and
  triangular lattices in 1995 [wang1995diffusion]. Their 1995 abstract describes *fixed* rotators,
  so it is a relative of the ant rather than the ant itself.
- **The highway.** From an empty grid the RL ant behaves irregularly for about 10,000 steps and then
  builds a "highway" that repeats every 104 steps, moving two cells diagonally per period. Wikipedia
  and Hutton's Golly wiki page give the onset as step 9,977 [hutton2015twodimensional]. A primary
  source for the exact onset was not read (see Unverified). JETCARP's test reproduces 9,977 and
  period 104.
- **Computation.** Gajardo, Moreira and Goles (2002) showed that the trajectory of a single ant can
  compute any boolean circuit, so predicting the ant is P-hard. They also showed that the ant is
  universal [gajardo2002complexity]. Tsukiji and Hagiwara (2011) proved PSPACE-hardness for
  time-reversible generalised ants on square and hexagonal grids [tsukiji2011recognizing].
- **Hexagonal ants.** On hexagons an ant can make six turns. Wikipedia writes them N, R1, R2, U,
  L2, L1 (no source given). Tsukiji and Hagiwara (2011) treat the ant on a hexagonal grid
  [tsukiji2011recognizing]. Hutton's Golly page (TwoDimensionalTuringMachines) covers relative
  turmites on hexagonal and triangular grids, including busy-beaver searches, and gives Allen H.
  Brady credit for first studying the triangular case [hutton2015twodimensional]. **Who first ran a
  multi-colour ant on hexagons was not established.**
- **Turmites as a research tool (2025).** Turk's paper "Vants and Turmites", Artificial Life 31(4)
  (2025), publishes for the first time much of his work from around 1989. It covers four-direction
  turns (forward, back, left, right), multistate turmites (self-similar growth, epidemics), a
  universal turmite and a self-reproducing turmite. It also mentions Rudy Rucker's 1993 software lab
  that evolved turmites ("boppers") [turk2025vants].

## Key primary citations

- [langton1986studying] Christopher G. Langton, "Studying artificial life with cellular automata",
  Physica D: Nonlinear Phenomena 22(1–3):120–149 (Oct 1986). DOI 10.1016/0167-2789(86)90237-X.
  Verified: Crossref record for the DOI (the ScienceDirect and Deep Blue pages return 403). The
  content (sections 9–11 on vants) is known from Turk 2025 only.
- [dewdney1989computer] A. K. Dewdney, "Computer Recreations: Two-dimensional Turing machines and
  tur-mites make tracks on a plane", Scientific American 261(3):180–183 (Sep 1989). DOI
  10.1038/scientificamerican0989-180. Verified: Crossref gives Dewdney, "Computer Recreations",
  261(3):180–183, Sept 1989. The subtitle comes from Turk 2025's reference list and a search summary
  of the Scientific American archive.
- [turk2025vants] Greg Turk, "Vants and Turmites", Artificial Life 31(4):401–434 (2025). DOI
  10.1162/ARTL.a.9. Verified: Crossref and the author's PDF
  (faculty.cc.gatech.edu/~turk/turmites/vants_and_turmites_turk_2025.pdf), whose sections 1–2 were
  read. Code and video: https://github.com/gturk1/turmites (not opened).
- [gale1995further] David Gale, James Propp, Scott Sutherland and Serge Troubetzkoy, "Further travels
  with my ant", The Mathematical Intelligencer 17(3):48–56 (1995), Mathematical Entertainments column.
  DOI 10.1007/BF03024370. arXiv:math/9501233. Verified: arXiv abstract page and Crossref.
- [propp1994further] Jim Propp, "Further ant-ics", The Mathematical Intelligencer 16(1):37–44 (1994),
  in Gale's Mathematical Entertainments column. DOI 10.1007/BF03026614. Verified: Crossref (author
  Jim Propp, column title only). The title "Further ant-ics" is from Turk 2025 and MathWorld, which
  credit it to Gale (Turk) or to Gale and Propp (MathWorld).
- [gale1993industrious] David Gale, "The industrious ant", The Mathematical Intelligencer 15(2):54–58
  (1993), Mathematical Entertainments column. DOI 10.1007/BF03024194. Verified: Crossref (column title,
  volume and pages; no author recorded). The title and author are from Turk 2025 and MathWorld.
- [gale1998tracking] David Gale, *Tracking the Automatic Ant and Other Mathematical Explorations*,
  Springer, New York (1998). DOI 10.1007/978-1-4612-2192-0. Verified: Crossref. Per arXiv, it reprints
  Gale et al. 1995 as chapter 18.
- [bunimovich1992recurrence] L. A. Bunimovich and S. E. Troubetzkoy, "Recurrence properties of
  Lorentz lattice gas cellular automata", Journal of Statistical Physics 67(1–2):289–302 (1992). DOI
  10.1007/BF01049035. Verified: Crossref.
- [gajardo2002complexity] A. Gajardo, A. Moreira and E. Goles, "Complexity of Langton's ant",
  Discrete Applied Mathematics 117(1–3):41–50 (2002). DOI 10.1016/S0166-218X(00)00334-6.
  arXiv:nlin/0306022. Verified: Crossref and the arXiv abstract page.
- [stewart1994ultimate] Ian Stewart, "The Ultimate in Anty-Particles", Scientific American
  271(1):104–107 (July 1994). DOI 10.1038/scientificamerican0794-104. Verified: Crossref.

Secondary or related (verified):

- [kong1991diffusion] X. P. Kong and E. G. D. Cohen, "Diffusion and propagation in triangular Lorentz
  lattice gas cellular automata", J. Stat. Phys. 62(3–4):737–757 (1991). DOI 10.1007/BF01017981.
  Verified: Crossref.
- [wang1995diffusion] F. Wang and E. G. D. Cohen, "Diffusion in Lorentz lattice gas cellular automata:
  the honeycomb and quasi-lattices compared with the square and triangular lattices", J. Stat. Phys.
  81(1–2):467–495 (1995). DOI 10.1007/BF02179989. arXiv:comp-gas/9501006. Verified: Crossref and
  the arXiv abstract page.
- [tsukiji2011recognizing] Tatsuie Tsukiji and Takeo Hagiwara, "Recognizing the repeatable
  configurations of time-reversible generalized Langton's ant is PSPACE-hard", Algorithms 4(1):1–15
  (2011). DOI 10.3390/a4010001. Verified: Crossref (the MDPI page returns 403). The hexagonal-grid
  result comes from a search-engine excerpt of the abstract.
- [gardner1973worms] Martin Gardner, "Mathematical Games: Fantastic patterns traced by programmed
  worms", Scientific American 229(5):116–123 (Nov 1973). DOI 10.1038/scientificamerican1173-116.
  Verified: Crossref (column title only). The subtitle is from Turk 2025.
- [hutton2015twodimensional] Tim Hutton and contributors, "Two Dimensional Turing Machines", GollyGang
  ruletablerepository wiki, https://github.com/GollyGang/ruletablerepository/wiki/TwoDimensionalTuringMachines
  (last edited 2 Jul 2024; Turk cites it as Hutton 2015). Verified: page fetched.

## Software that ran it before JETCARP

- **Golly** turmites: Tim Hutton's generator scripts `Turmite-gen.py`, `AbsoluteTurmite-gen.py`,
  `TriTurmite-gen.py`, `HexTurmite-gen.py` and `AbsoluteHexTurmite-gen.py` turn turmite tables into
  Golly rules, including on triangular and hexagonal grids [hutton2015twodimensional].
- **CelLab** (Rudy Rucker and John Walker, Autodesk 1989; Web edition 1995) [rucker1989cellab]: rule
  LANT (Langton's ant, "invented by Christopher Langton in 1986") and rule TURMITE, which credits
  Dewdney's September 1989 column. Manual rules page fetched.
- **NetLogo** "Vants" model, Uri Wilensky, 2005 (credits Langton; links a 1991 video of Langton
  demonstrating vants), https://ccl.northwestern.edu/netlogo/models/Vants [wilensky2005vants].
  Verified: page fetched.
- **Rudy Rucker's** 1993 software lab with evolvable turmites ("boppers"), as described in Turk 2025.
  Not checked independently.
- **Ed Pegg Jr.'s** Mathematica turmite experiments (Math Puzzle, 19 Feb 2003 update on the
  26 Mar 2003 page: splitting and annihilating turmites) and the MathWorld "Turmite" entry.
  Page fetched; the turn letters are given as "Left, U-turn, Right, No turn".
- **ptiles/ant** (GitHub, MIT licence, undated): Langton's ant on Penrose tilings (from novelty-08) [ptilesnd_ant].
- Turk's own code: https://github.com/gturk1/turmites (cited in Turk 2025; not opened).
- Wolfram, *A New Kind of Science* (2002), pp. 930–931, discusses ants, per MathWorld (not read).

## People JETCARP should credit

Christopher G. Langton (the ant, 1986); Greg Turk (independent rediscovery, multi-colour and
multistate machines); A. K. Dewdney (the name "tur-mite", 1989 column); David Gale, James Propp,
Scott Sutherland and Serge Troubetzkoy (generalised L/R ants, symmetry); L. A. Bunimovich and
S. E. Troubetzkoy, and E. G. D. Cohen and X. P. Kong (unboundedness, lattice-gas versions);
A. Gajardo, A. Moreira and E. Goles (universality); Tim Hutton (Golly turmite rules,
hexagonal and triangular turmites).

## How JETCARP's version relates

JETCARP's `Ant RL`, `Turmite <turns>` and turmite tables run on every tiling through the cell's
clockwise edge-neighbour order. Turns are L, R, N, U, plus L2/R2 on hexagons (Wikipedia's R1/L1 also
parse). Its highway search proves highways exactly and reproduces the published RL onset (9,977) and
period (104) (README "## Ant and turmite search").

## Unverified

- The exact first-highway step 9,977 has not been traced to a primary publication. Langton 1986 and
  Gale's columns could not be read. It is stated in Wikipedia and Hutton's wiki.
- Who first ran multi-colour ants on hexagonal grids (Wikipedia's N/R1/R2/U/L2/L1 section has no
  source).
- Whether Kong and Cohen (1991) used flipping rotators (which is the ant) or fixed ones. Only the
  title was read.
- Attribution of "Further ant-ics": Crossref says Jim Propp, Turk says Gale, MathWorld says Gale and
  Propp.
- Langton 1986's own wording, and whether it reports the highway. The paper itself was not read.
- Rucker's 1993 turmite lab (book title not checked).
- The Gajardo thesis (ENS Lyon 2001) on the ant on other network topologies, listed by MathWorld.

## Search log

- WebFetch: doi.org/10.1016/0167-2789(86)90237-X (redirect), ScienceDirect and Deep Blue (403);
  en.wikipedia.org/wiki/Langton%27s_ant (used as a pointer); arxiv.org/abs/math/9501233;
  faculty.cc.gatech.edu/~turk/turmites/vants_and_turmites_turk_2025.pdf (text extracted locally
  from the saved PDF); github.com/GollyGang/ruletablerepository/wiki/TwoDimensionalTuringMachines;
  mathpuzzle.com/26Mar03.html; mathworld.wolfram.com/LangtonsAnt.html; arxiv.org/abs/nlin/0306022;
  arxiv.org/abs/comp-gas/9501006; ccl.northwestern.edu/netlogo/models/Vants; mdpi.com (403).
- Crossref API (api.crossref.org/works/<doi>): 10.1016/0167-2789(86)90237-X, 10.1038/scientificamerican0989-180,
  10.1007/BF03024370, 10.1007/BF03026614, 10.1007/BF03024194, 10.1007/978-1-4612-2192-0,
  10.1007/BF01049035, 10.1016/S0166-218X(00)00334-6, 10.1038/scientificamerican0794-104,
  10.1007/BF01017981, 10.1016/0167-2789(91)90273-C, 10.1007/BF02179989, 10.3390/a4010001,
  10.1038/scientificamerican1173-116, 10.1162/ARTL.a.9.
- WebSearch: `Dewdney "Computer Recreations" September 1989 "Two-dimensional Turing machines" tur-mites`;
  `Langton's ant hexagonal grid generalized ant six directions history first studied …`;
  `Wang Cohen 1995 "Diffusion in Lorentz lattice gas cellular automata" …`;
  `NetLogo models library "Langton's Ant" model Wilensky`.
