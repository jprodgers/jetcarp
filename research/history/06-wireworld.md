# 06. WireWorld: history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.

## Short history

- **Invention.** WireWorld is a four-state automaton: empty, conductor ("copper"), electron head and
  electron tail. Brian Silverman devised it. LifeWiki and Wikipedia both say he proposed it in 1987
  as part of his program *Phantom Fish Tank* [lifewikind_wireworld]. A 2025 blog post by someone who
  says he talks with Silverman regularly gives **1984** instead [daneelsan2025wireworld], and a search
  summary of a page derived from Quinapalus also gave about 1984. The CelLab manual's reference list
  (Rucker and Walker) gives the program as Brian Silverman, *The Phantom Fishtank*, Logo Computer
  Systems, Montreal, 1987 [silverman1987phantom], which supports 1987 as the date of the program.
  The rule could still be older (see Unverified). Silverman also created
  Brian's Brain (see file 02) and the Seeds rule.
- **Popularisation (1990).** A. K. Dewdney described WireWorld in his Computer Recreations column,
  "The cellular automata programs that create Wireworld, Rugworld and other diversions", Scientific
  American, January 1990 [dewdney1990cellular]. That column is what made the rule widely known
  [lifewikind_wireworld].
- **Logic and computation.** WireWorld supports small, robust diodes, logic gates, clocks and memory,
  and with an unbounded arrangement of wires it is Turing-complete [lifewikind_wireworld].
- **The Wireworld computer (1990–1992, published 2004).** David Moore and Mark Owen, "with the help
  of many others", designed a complete programmable computer in WireWorld between 1990 and 1992, and
  released it in September 2004 (LifeWiki, and a search-engine excerpt of the Quinapalus page)
  [lifewikind_wireworld]. It has a RISC-style architecture with 64 16-bit registers
  that hold the program, the CPU status and the data, and shows its output on seven-segment displays.
  The copy distributed with Golly computes and shows prime numbers [lifewikind_wireworld].
- **Later.** Ed Pegg Jr. wrote about a WireWorld multiplier in his Math Games column (May 2004),
  and Nyles Heise ran the "World Wide WireWorld" site (both are linked from LifeWiki; neither was
  fetched). Andrew Trevorrow's HatLife (2023) ran WireWorld on the hat tiling (novelty-08).

## Key primary citations

- [dewdney1990cellular] A. K. Dewdney, "Computer Recreations: The cellular automata programs that
  create Wireworld, Rugworld and other diversions", Scientific American 262(1):146–149 (Jan 1990). DOI
  10.1038/scientificamerican0190-146 (JSTOR 24996654). Verified: Crossref gives Dewdney, "Computer
  Recreations", 262(1):146–149, January 1990. The subtitle is the one LifeWiki and Wikipedia give.
- [lifewikind_wireworld] LifeWiki, "WireWorld", https://conwaylife.com/wiki/WireWorld (last edited
  9 Jan 2026). Verified: read through the mirror conwaylife.njohnston.ca.
- [silverman1987phantom] Brian Silverman, *The Phantom Fishtank* (software), Logo Computer Systems,
  Montreal, 1987. Verified (secondary): listed in the CelLab manual's references,
  https://fourmilab.ch/cellab/manual/refs.html (fetched). The software itself was not seen.
- [daneelsan2025wireworld] daneelsan, "Wireworld: Building a Computer from a Cellular Automaton", blog
  post, 6 Nov 2025, https://daneelsan.github.io/2025/11/06/wireworld.html . Secondary; the source of the
  1984 date. Verified: page fetched.

## Software that ran it before JETCARP

- Silverman's *The Phantom Fishtank* (Logo Computer Systems, 1987) [silverman1987phantom].
- **MCell** (Mirek Wójtowicz): WireWorld is a built-in example in the "Rules tables" family
  (mcell.ca families page, fetched). Pegg's 2003 Math Puzzle page recommends MCell for WireWorld
  animations.
- **Golly**: ships WireWorld with the Moore–Owen prime-number computer [lifewikind_wireworld].
- **HatLife** (Trevorrow, 2023): WireWorld on the hat tiling (novelty-08).
- Rosetta Code has implementations in many languages (linked from LifeWiki; not fetched).

## People JETCARP should credit

Brian Silverman (the rule); A. K. Dewdney (the 1990 column); David Moore and Mark Owen (the
Wireworld computer); Mirek Wójtowicz (MCell) and the Golly team for long-standing implementations.

## How JETCARP's version relates

JETCARP runs `WireWorld` on every tiling (CPU and GPU). Its own WireWorld adder demo (2 + 3) uses OR
and XOR junctions that JETCARP found by exhaustive search and simulated annealing, not copied (README
"### WireWorld adder (2 + 3)").

## Unverified

- Mark Owen (and David Moore), "The Wireworld computer", https://www.quinapalus.com/wi-index.html .
  The primary source for the computer. **Not fetched:** HTTP 403 to automated fetches. The 1990–1992
  design dates and the September 2004 release come from LifeWiki and a search-engine excerpt. Check it
  by hand before citing (suggested key `owennd_wireworld`; not in the bibliography).
- The year of invention: 1987 (LifeWiki, Wikipedia) or 1984 (daneelsan 2025 blog; a
  Quinapalus-derived page in a search summary). Ask Brian Silverman, or check Dewdney 1990 itself.
- *The Phantom Fishtank* (1987) is attested by CelLab's reference list only. No copy of the software,
  its manual, or an LCSI catalogue was seen.
- The Quinapalus page's own wording (HTTP 403).
- Dewdney 1990's text was not read. Only the bibliographic record was checked.

## Search log

- WebFetch: en.wikipedia.org/wiki/Wireworld (pointer only); www.quinapalus.com/wi-index.html and
  /wires0.html (403); conwaylife.njohnston.ca/wiki/WireWorld; daneelsan.github.io/2025/11/06/wireworld.html;
  mcell.ca and mcell.ca/pages/families.html (mirekw.com redirects to mcell.ca);
  fourmilab.ch/cellab/manual/refs.html (CelLab references).
- Crossref: 10.1038/scientificamerican0190-146.
- WebSearch: `Wireworld computer David Moore Mark Owen Michael Fryers prime numbers 2004 quinapalus history`;
  `"Phantom Fish Tank" Brian Silverman cellular automata program Logo Computer Systems` (nothing found).
