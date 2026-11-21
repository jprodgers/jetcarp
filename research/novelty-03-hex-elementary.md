# Novelty audit 03: elementary (Wolfram-style) rules on hexagon grids

Audit date: 2026-11-10.

## What JETCARP does

- **HexW / HexW3:** pointy-top hexagon rows. Each cell's two parents are the two hexagons above it.
  There are 16 binary rules and 3^9 = 19,683 ternary rules, indexed left·k + right.
- **HexT / HexT3** (grid `hex_flat`): flat-top hexagons. A cell's parents are NW and NE, half a row
  up (layer l−1, columns c∓1), and N, a full row up (layer l−2, column c). Each layer holds every other
  column. There are 256 binary rules and 3^27 ternary rules, plus totalistic (`HexT3 sum`, 2,187;
  `HexT sum`, 16) and left–right symmetric (`HexT3 sym`, 3^18) subsets. The README reports a first
  run classifying all 256 HexT, all 2,187 HexT3-sum and a 2,000-rule HexT3 sample with JETCARP's row
  descriptors.

## Verdict

- **HexW / HexW3: Prior art found.** These are exactly the two-cell-neighbourhood (radius-1/2)
  rules of *A New Kind of Science*, which Wolfram describes as drawn "like hexagons or staggered
  bricks". He gives the same counts (16 and 19,683). A 2020 open-source tool also draws the 16 binary
  rules on hexagons.
- **HexT / HexT3: Related work only.** HexT is a second-order (order-2) one-dimensional CA on a
  staggered lattice: two neighbours at step t−1 and the cell itself at step t−2. Second-order CAs,
  half-integer ranges and order-s rules are all standard (NKS ch. 9; Wolfram Language
  `CellularAutomaton`). The XOR subset g(NW,NE) ⊕ N is exactly Wolfram's/Fredkin's reversible
  second-order construction applied to the 16 two-cell rules. We found no source that presents
  this particular class as a hexagon-grid rule family, numbers all 256 / 3^27 rules this way, or
  catalogues them.

## Closest prior work (verified)

1. **Wolfram, S., *A New Kind of Science*, Wolfram Media, 2002, p. 885, Note (j) "Cellular automata
   with two-cell neighborhoods"**
   (https://www.wolframscience.com/nks/notes-3-2--cellular-automata-with-two-cell-neighborhoods/ ).
   "By having cells on successive steps be arranged like hexagons or staggered bricks" each cell
   depends on two cells instead of three. k = 2 gives 16 rules, and the most complicated is nested like
   rule 90. For k = 3, "there are 19,683 possible rules", of which 1,734 are fundamentally
   inequivalent. The example is rule 7743.
   *This is HexW/HexW3 exactly, including the hexagon picture.* (Recommended check: confirm that
   JETCARP's `HexW3 7743` reproduces Wolfram's figure, so the numbering is shown to agree.)

2. **Kerekrety, M., "Elementary cellular automaton in Hexagonal, Pentagonal and Other Tessellations"**,
   2020, https://matejker.github.io/elementary-cellular-automaton/ , code
   https://github.com/matejker/elementary-cellular-automaton . "An easy tool for generating any of 256
   one-dimensional cellular automaton on various tiling such as squares, hexagons, pentagons". "Each
   hexagon has 2 neighbours / ancestors", so there are 4 patterns and 16 rules (e.g. hexagonal rule 9).
   *This is the binary HexW on hexagons, plus pentagon variants, as a web tool.*

3. **Wolfram, S., NKS p. 437 (main text) and p. 1017, note "Second-order cellular automata"**
   (https://www.wolframscience.com/nks/p437--the-notion-of-reversibility/ ,
   https://www.wolframscience.com/nks/notes-9-2--second-order-cellular-automata/ ). Reversible rules
   are made by adding dependence on the colours two steps back: the new colour "should be inverted
   whenever the cell was black two steps back". The note shows that a second-order k = 2, r = 1 rule
   "corresponds to evolution according to a first-order k = 4, r = 1 rule". The same applies to HexT:
   pairing consecutive layers makes it a first-order rule with four states per site.

4. **Wolfram Language `CellularAutomaton` documentation**
   (https://reference.wolfram.com/language/ref/CellularAutomaton.html ). It supports half-integer
   ranges (e.g. `{Total[#]&, {}, 1/2}` for Pascal's triangle, `{10008, 2, 3/2}`), explicit neighbour
   offsets, and "an order-s cellular automaton … each step depends on s preceding steps".
   *So HexT-type rules can be expressed in standard tools, though not under a hexagon name or with
   this numbering.*

5. Related only: Liang Zhang, "Hexagonal Two-Color Four-Neighbor Totalistic Cellular Automata",
   Wolfram Demonstrations Project
   (https://demonstrations.wolfram.com/HexagonalTwoColorFourNeighborTotalisticCellularAutomata/ ; the
   page did not render, so the rule could not be read; it is listed by title only).

## Analysis: what HexT is

Write x_l for layer l. Then x_l(c) = f(x_{l−1}(c−1), x_{l−2}(c), x_{l−1}(c+1)). This is a binary
or ternary order-2 CA with a two-cell (radius-1/2, staggered) neighbourhood at t−1 and the
same-column cell at t−2.

- The 16 HexT rules that ignore N are the 16 NKS two-cell rules (HexW) turned a quarter turn.
- The 16 rules of the form g(NW,NE) ⊕ N are the reversible second-order versions of those 16 rules,
  in the sense of NKS p. 437. HexT 150 (NW⊕N⊕NE) is the second-order version of rule 6/90-type XOR.
- The rest (224 binary rules and nearly all 3^27 ternary rules) are general non-reversible
  second-order staggered rules. We found no named atlas or published classification of these.

Whether some HexT rules equal known first-order r = 1 ECAs under a change of coordinates was not
checked rigorously. The N parent is the "grandparent" reached through both NW and NE, so most rules
are not first-order.

## Suggested credit wording

> The two-parent hexagon rules (HexW, HexW3) are Wolfram's two-cell-neighbourhood cellular automata,
> drawn on hexagons as in *A New Kind of Science* (2002, p. 885; 16 binary and 19,683 three-colour
> rules); Kerekrety (2020) also draws the binary ones on hexagons. The three-parent flat-top rules
> (HexT, HexT3) are order-2 one-dimensional CAs on a staggered lattice (cf. NKS's second-order rules,
> p. 437). Their XOR-with-N subset is the reversible second-order version of the two-cell rules. To
> our knowledge, JETCARP is the first to present the full 256-rule / 3^27-rule family as hexagon
> pictures with a Wolfram-style numbering, totalistic and symmetric subsets, and an automatic
> classification of all binary and totalistic rules.

Keep the claim to presenting and cataloguing the family; the underlying rule class is standard.

## Search log (2026-11-10)

- WebSearch: `Wolfram "A New Kind of Science" cellular automata "r = 1/2" "k = 3" rules two neighbors offset 19683`
- WebSearch (wolframscience.com): `wolframscience.com nks notes "two-cell neighborhoods" cellular automata note`
- WebSearch: `hexagonal elementary cellular automaton three parents flat-top hexagon one-dimensional rule drawn on hexagons Wolfram Demonstration`
- WebSearch: `second-order one-dimensional cellular automaton new state depends on two neighbours previous step and own cell two steps back all rules enumerated`
- WebSearch: `"19683" three-color two-neighbor cellular automata all rules images atlas "r=1/2" OR "radius 1/2"`
- Fetched: NKS notes-3-2 (numbers of rules; two-cell neighborhoods), NKS p437, notes-9-2 (numbers of
  reversible rules; second-order CAs), reference.wolfram.com CellularAutomaton, matejker.github.io
  and its GitHub repo, demonstrations.wolfram.com (Zhang; did not render).

## Gaps / unverified leads

- NKS p. 885 may have pictures of three-colour two-cell rules. We did not check whether Wolfram or
  anyone else published all 19,683 (or the 1,734 inequivalent) as an atlas. None was found.
- "Cellular automata with memory" (Alonso-Sanz and co-authors) and "Elementary CA with memory"
  papers, and the Toffoli–Margolus *Cellular Automata Machines* (1987) treatment of second-order
  rules, were not fetched. They are likely relevant background for HexT.
- Wolfram Demonstrations "Shifted Elementary Cellular Automata" (title seen in a search summary only).
- The Domany–Kinzel / directed-percolation literature uses the same staggered two-parent lattice
  (stochastic). It was not cited because it was not fetched.
