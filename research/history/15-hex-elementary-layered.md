# 15. Elementary-style rules on hexagons (HexW, HexT) and layered rules: history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.
Builds on novelty-03 (hexagonal elementary rules) and novelty-04 (layered rules), which hold the
detailed comparisons and search logs.

## Short history

**Hexagonal cellular automata in general**

- **Image processing, 1969.** Marcel J. E. Golay's "Hexagonal parallel pattern transformations"
  (*IEEE Transactions on Computers*, 1969) applied local parallel rules on a hexagonal grid to image
  processing [golay1969hexagonal]. It is one of the earliest uses of hexagonal cellular logic.
- **2-D rule spaces, 1985.** Packard and Wolfram's survey of two-dimensional CAs counts and studies
  rules on three neighbourhoods: the 5-cell and 9-cell square ones and the hexagonal one. On the
  hexagonal lattice it counts 2^7 = 128 totalistic and 2^14 = 16,384 outer-totalistic two-state rules
  [packard1985twodimensional, Table 1].
- **Lattice gases, 1986.** Frisch, Hasslacher and Pomeau's lattice gas recovers the Navier–Stokes
  equations because the hexagonal lattice is isotropic enough, where the square HPP gas is not
  [frisch1986latticegas]. See file 09.
- **Life on hexagons.** Carter Bays studied Life-like rules on triangular (1994) and hexagonal and
  pentagonal (2005) tessellations [bays1994cellular; bays2005note]. LifeWiki and its forums use an
  `H` suffix for hexagonal rules such as B2/S34H (novelty-05).

**Elementary rules drawn on hexagons (HexW, HexW3)**

- Wolfram's *A New Kind of Science* (2002, p. 885, note on two-cell neighbourhoods) observes that if
  cells on successive steps are arranged "like hexagons or staggered bricks", each cell depends on two
  cells instead of three. There are then 16 two-colour rules and 19,683 three-colour rules (1,734 of
  them inequivalent) [wolfram2002new]. HexW and HexW3 are this family.
- Kerekrety's 2020 web tool draws the 16 binary rules on hexagons, where each hexagon has two
  "ancestors", and the 256 ECA rules on squares and pentagons [kerekrety2020elementary].

**Three-parent hexagon rules (HexT, HexT3)**

- These are second-order (order-2) one-dimensional rules on a staggered lattice: two parents one step
  back and one two steps back. Second-order and reversible constructions are described in NKS (p. 437
  and the note on p. 1017) [wolfram2002new]. The Wolfram Language's
  `CellularAutomaton` supports half-integer ranges and order-s rules
  [wolframnd_cellularautomaton]. We found no earlier source that numbers and catalogues the full
  256-rule and 3^27-rule hexagonal family (novelty-03).

**Layered (distance-shell) rules**

- JETCARP's own construction (Jimmie Rodgers, 2026). Each step writes the next breadth-first shell
  of the tiling's neighbour graph, as a totalistic function of the cell's parents in the previous
  shell. The nearest related work:
  - Margenstern (2010) embeds 1-D CAs in hyperbolic tilings along chosen lines
    [margenstern2010embedding].
  - Kerekrety (2020) uses fixed lattice rows on periodic tilings [kerekrety2020elementary].
  - Pascal's triangle mod 2 is the textbook case: rule 90 from one cell [martin1984algebraic].
- Name clash: Dalai's "Layered Cellular Automata" (arXiv:2308.06370, 2023) is a different idea, two
  coupled computation layers [dalai2023layered].

## Key primary citations

- [golay1969hexagonal] Golay, M. J. E., "Hexagonal parallel pattern transformations", *IEEE
  Transactions on Computers* C-18(8):733–740 (1969). DOI 10.1109/T-C.1969.222756. Verified: Crossref
  record.
- [packard1985twodimensional] Packard, N. H. and Wolfram, S., "Two-dimensional cellular automata",
  *J. Stat. Phys.* 38(5–6):901–946 (1985). DOI 10.1007/BF01010423. Verified: Crossref record, and
  Table 1 (hexagonal neighbourhood counts) read from the PDF on content.wolfram.com.
- [frisch1986latticegas] Frisch, U., Hasslacher, B. and Pomeau, Y., "Lattice-gas automata for the
  Navier–Stokes equation", *Physical Review Letters* 56(14):1505–1508 (1986).
  DOI 10.1103/PhysRevLett.56.1505. Verified: Crossref record.
- [wolfram2002new] Wolfram, S., *A New Kind of Science*, Wolfram Media (2002): p. 885 note "Cellular
  automata with two-cell neighborhoods"; p. 437 and p. 1017 note "Second-order cellular automata".
  Verified: NKS online pages (novelty-03).
- [kerekrety2020elementary] Kerekrety, M., "Elementary cellular automaton in Hexagonal, Pentagonal and
  Other Tessellations" (2020). https://matejker.github.io/elementary-cellular-automaton/ . Verified:
  page and repository fetched (novelty-03).
- [margenstern2010embedding] Margenstern, M., "About the embedding of one dimensional cellular
  automata into hyperbolic cellular automata", arXiv:1004.1830 (2010). Verified: arXiv abstract page
  (novelty-04).
- [dalai2023layered] Dalai, A., "Layered Cellular Automata", arXiv:2308.06370 (25 Jul 2023). Verified: arXiv
  abstract page (novelty-04; author name rechecked 2026-11-10).
- [wolframnd_cellularautomaton] Wolfram Research, `CellularAutomaton` documentation,
  https://reference.wolfram.com/language/ref/CellularAutomaton.html . Verified: fetched (novelty-03).
- [bays1994cellular; bays2005note] Bays, C., *Complex Systems* 8(2) (1994) and 15(3):245–252 (2005).
  Verified: Complex Systems abstract pages (novelty-05).

## Software that ran it before JETCARP

- Mathematica / Wolfram Language `CellularAutomaton`, for staggered and second-order 1-D rules (not
  under a hexagon name) [wolframnd_cellularautomaton].
- Kerekrety's web tool, for binary two-parent rules on hexagons and pentagons [kerekrety2020elementary].
- Golly's RuleTable supports a `hexagonal` neighbourhood for 2-D hexagonal rules [gollynd_formats].
- Layered rules: no earlier software found (novelty-04).

## People JETCARP should credit

Stephen Wolfram (two-cell rules on hexagons; second-order rules); Matej Kerekrety (hexagon ECA tool);
Norman Packard (2-D and hexagonal rule spaces, with Wolfram); Marcel Golay (hexagonal cellular
logic); Carter Bays (Life on hexagons); Maurice Margenstern (1-D CAs in other tilings). Layered rules:
Jimmie Rodgers (JETCARP, 2026).

## How JETCARP's version relates

- `HexW`/`HexW3` index rules as left·k + right and match NKS's counts. novelty-03 recommends checking
  `HexW3 7743` against Wolfram's figure.
- `HexT`/`HexT3` (flat-top grid) add a third parent, N, two layers up, with totalistic and symmetric
  subsets. All 256 HexT rules and all 2,187 HexT3-sum rules were classified with JETCARP's row
  descriptors (README "### Three-parent hexagon rules").
- `Layer <states>/<outputs>` runs on any tiling. `Layer 2/01` gives Pascal's triangle mod 2 on squares
  (README "What Phase 2 adds", item 12).

## Unverified

- Whether Golay 1969 is the *first* hexagonal CA work. Earlier hexagonal cellular-logic work (e.g.
  Preston's image processing) was not checked.
- NKS p. 885 three-colour pictures and any full atlas of the 19,683 rules (novelty-03 gap).
- García-Morales, "Mathematical Physics of Cellular Automata", arXiv:1203.3939 (Pascal simplices mod
  p). Seen in a search summary only (novelty-04).
- Alonso-Sanz's "CA with memory" literature and Toffoli and Margolus's treatment of second-order rules,
  both relevant background for HexT (not fetched).
- Who first used the XOR-with-two-steps-back trick for reversible CAs. It is commonly credited to
  Edward Fredkin, but this was not checked.

## Search log

- Crossref API (curl): 10.1109/T-C.1969.222756, 10.1103/PhysRevLett.56.1505.
- Fetched: stephenwolfram.com/publications/two-dimensional-cellular-automata/ (listing only); the PDF
  content.wolfram.com/sw-publications/2020/07/two-dimensional-cellular-automata.pdf (36 MB, text
  extracted locally; Table 1).
- Reused: novelty-03 and novelty-04 search logs.
