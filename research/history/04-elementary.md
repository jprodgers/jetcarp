# 04. Elementary cellular automata: history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.

## Short history

- **Before Wolfram.** One-dimensional CAs were studied in symbolic dynamics as maps that commute with
  the shift. Hedlund's 1969 paper is the standard reference for that line [hedlund1969endomorphisms].
- **Numbering, 1983.** Stephen Wolfram's "Statistical mechanics of cellular automata" (*Reviews of
  Modern Physics*, July 1983) studied the 256 two-state rules with three-cell neighbourhoods
  systematically. It numbered each rule by reading its eight outputs as a binary number, which is now
  called the Wolfram code [wolfram1983statistical; wikipediand_wolframcode].
- **Four classes, 1984.** "Universality and complexity in cellular automata" (*Physica D*, 1984)
  sorted CA behaviour into four qualitative classes [wolfram1984universality]. The same year Martin,
  Odlyzko and Wolfram worked out the algebra of the additive rules, such as rule 90 (left XOR right)
  and rule 150 (XOR of all three), whose patterns from one cell are Pascal's triangle mod 2
  [martin1984algebraic].
- **Rule 30, 1986.** Wolfram proposed rule 30 as a source of pseudorandom bits
  [wolfram1986random].
- **Rule 110.** Wolfram's 1985 list of open problems asked which CAs are computation-universal
  [wolfram1985twenty]. Wikipedia attributes the conjecture that rule 110 is universal to 1985.
  Matthew Cook proved it by emulating cyclic tag systems. He presented the proof at the CA98
  conference at the Santa Fe Institute. Wolfram Research then took legal action over Cook's
  nondisclosure agreement (*Wolfram Research Inc. v. Cook*, case 2:00-cv-09357), and this delayed
  publication. The proof appeared in *Complex Systems* in 2004 [cook2004universality;
  wikipediand_rule110; giles2002kind]. Neary and Woods (2006) showed that rule 110 simulates Turing
  machines in polynomial time, so predicting it is P-complete [neary2006pcompleteness].
- **Popularisation.** *A New Kind of Science* (2002) made the elementary rules widely known
  [wolfram2002new]. For rule-by-rule notes on the named elementary rules JETCARP ships (18, 22, 45,
  54, 60, 73, 105, 126, 150, 184), see `named-rule-authors.md`.

## Key primary citations

- [wolfram1983statistical] Wolfram, S., "Statistical mechanics of cellular automata", *Reviews of
  Modern Physics* 55(3):601–644 (1983). DOI 10.1103/RevModPhys.55.601. Verified: Crossref record.
- [wolfram1984universality] Wolfram, S., "Universality and complexity in cellular automata",
  *Physica D* 10(1–2):1–35 (1984). DOI 10.1016/0167-2789(84)90245-8. Verified: Crossref record.
- [martin1984algebraic] Martin, O., Odlyzko, A. M. and Wolfram, S., "Algebraic properties of
  cellular automata", *Communications in Mathematical Physics* 93(2):219–258 (1984).
  DOI 10.1007/BF01223745. Verified: Crossref record.
- [wolfram1985twenty] Wolfram, S., "Twenty problems in the theory of cellular automata", *Physica
  Scripta* T9:170–183 (1985). DOI 10.1088/0031-8949/1985/T9/029. Verified: Crossref record.
- [wolfram1986random] Wolfram, S., "Random sequence generation by cellular automata", *Advances in
  Applied Mathematics* 7(2):123–169 (1986). DOI 10.1016/0196-8858(86)90028-X. Verified: Crossref
  record.
- [cook2004universality] Cook, M., "Universality in elementary cellular automata", *Complex Systems*
  15(1):1–40 (2004). DOI 10.25088/ComplexSystems.15.1.1. Verified: Complex Systems abstract page and
  Crossref record.
- [neary2006pcompleteness] Neary, T. and Woods, D., "P-completeness of cellular automaton Rule 110",
  ICALP 2006, LNCS 4051, Springer, pp. 132–143 (2006). DOI 10.1007/11786986_13. Verified: Crossref
  record.
- [hedlund1969endomorphisms] Hedlund, G. A., "Endomorphisms and automorphisms of the shift dynamical
  system", *Mathematical Systems Theory* 3(4):320–375 (1969). DOI 10.1007/BF01691062. Verified:
  Crossref record.
- [giles2002kind] Giles, J., "What kind of science is this?", *Nature* 417(6886):216–218 (2002).
  DOI 10.1038/417216a. Verified: Crossref record. This is a news feature that covers the Cook dispute.
- [wolfram2002new] Wolfram, S., *A New Kind of Science*, Wolfram Media (2002). Verified: NKS online
  (novelty-03).

## Software that ran it before JETCARP

- **Mathematica / Wolfram Language** `CellularAutomaton` (Wolfram Research; the documentation was
  fetched for novelty-03): any rule number, any radius, any number of colours.
- **Golly** supports `Wn` rules for even n from 0 to 254 only. Its QuickLife help says odd rules
  "have the same problem as B0 rules" and are not emulated [gollynd_quicklife].
- Kerekrety's 2020 web tool draws the 256 rules on squares, hexagons and pentagons (novelty-03).
- Unverified: the Wolfram Atlas (atlas.wolfram.com) and earlier 1980s programs (e.g. CelLab's 1-D
  rules).

## People JETCARP should credit

Stephen Wolfram (numbering, classes, rule 30, NKS); Matthew Cook (rule 110 universality); Olivier
Martin and Andrew Odlyzko (additive rules); Gustav Hedlund (1-D CAs as shift maps); Turlough Neary
and Damien Woods (rule 110 complexity).

## How JETCARP's version relates

`W<0-255>` runs every elementary rule, odd ones included, on the square grid with one row per
generation (README "What Phase 0 includes"; "Rule notation"). Unlike Golly's `Wn`, the notation is
not limited to even rules. Hexagonal two-parent and three-parent versions and layered rules on any
tiling are in file 15.

## Unverified

- Who first used the phrase "elementary cellular automaton". The Wikipedia "Wolfram code" article does
  not say, and Wolfram 1983 was not read in full.
- The date of Cook's proof (often given as the mid-1990s), the exact CA98 date, and the conference
  proceedings. Only Wikipedia was seen; the court docket and Giles 2002 were not read.
- Whether rule 30 was later used in Mathematica's random number generator (often stated, not checked).

## Search log

- Crossref API (curl): 10.1103/RevModPhys.55.601, 10.1016/0167-2789(84)90245-8,
  10.1016/0196-8858(86)90028-X, 10.1007/BF01223745, 10.1088/0031-8949/1985/T9/029,
  10.25088/ComplexSystems.15.1.1, 10.1007/11786986_13, 10.1007/BF01691062, 10.1038/417216a.
- Fetched: complex-systems.com/abstracts/v15_i01_a01/; en.wikipedia.org/wiki/{Rule_110,
  Wolfram_code} (pointers only); golly.sourceforge.io/Help/Algorithms/QuickLife.html.
