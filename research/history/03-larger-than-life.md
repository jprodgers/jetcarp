# 03. Larger than Life: history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.

## Short history

- **Griffeath's question (early 1990s).** Larger than Life (LtL) widens Life's 3 × 3 neighbourhood
  to a box (or diamond) of range r. A dead cell is born if the number of live cells in its
  neighbourhood lies in an interval [b1, b2], and a live cell survives if the count (in Evans's
  original form, including itself) lies in [s1, s2]. Golly's help credits David Griffeath with first
  imagining LtL "in the early 1990s", to see whether Life sits near a critical point of a
  threshold-range scaling [gollynd_ltl]. Kellie Evans's thesis preface says the original question
  came from Griffeath. In our paraphrase, he asked whether glider-like "digital creatures" would
  emerge from random starts when Life's rule is rescaled to larger neighbourhoods [evans1996larger].
  Golly cites Griffeath's "Self-organization of random cellular automata: four snapshots" (1994)
  as the first reference [griffeath1994self]. We verified its bibliographic data but did not read it.
- **Kellie Evans's thesis (1996).** *Larger than Life: It's So Nonlinear*, a PhD thesis at the
  University of Wisconsin–Madison supervised by David Griffeath [evans1996larger]. The abstract
  defines LtL as a four-parameter family of two-state CA rules (for a fixed range) that generalise
  Life. Life itself is the range-1 rule with birth interval [3, 3] and survival interval [3, 4]
  (counting the cell itself). The thesis treats the infinite system started from random states,
  using probability and particle-system methods (convergence results, bounds on invariant
  measures), and finite configurations as iterated maps: large-range analogues of Life's
  *replicators* (whose space-time diagrams match Pascal's triangle mod 2) and *gliders*, which she
  calls "bugs" [evans1996larger]. LifeWiki and Golly date it 1996 [lifewikind_ltl; gollynd_ltl].
- **Published papers.** Evans, "Larger than Life: digital creatures in a family of two-dimensional
  cellular automata", DMTCS Proceedings AA (DM-CCG 2001), 177–192 [evans2001larger]. This paper
  presents linear and quadratic rescalings of Life to LtL and the matching generalisations of
  gliders and spaceships. It was followed by "Larger than Life: threshold-range scaling of Life's
  coherent structures", Physica D 183 (2003) 45–67 [evans2003larger]; "Replicators and
  Larger-than-Life examples" in Griffeath and Moore (eds.), *New Constructions in Cellular Automata*,
  OUP 2003 [evans2003replicators]; "Is Bosco's rule universal?", MCU 2004, LNCS 3354 (2005), 188–199
  [evans2005bosco]; "Larger than Life's extremes" in Adamatzky (ed.), *Game of Life Cellular
  Automata*, Springer 2010, 179–221 [evans2010larger]; and "Larger Than Life" in *Designing Beauty*,
  Springer 2016, 27–34 [evans2016larger]. Related work by Griffeath: Gravner and Griffeath, "Cellular
  automaton growth on Z²" (1998) [gravner1998cellular], and Griffeath, "Self-organizing
  two-dimensional cellular automata: 10 still frames" (2016) [griffeath2016self].
- **Bugs, Bosco and the CAM-8.** MCell's lexicon says that the "Bugs" rule (R5, birth 34–45,
  survival 34–58 counting the centre) was found while exploring LtL parameter space on the CAM-8
  cellular automaton machine, and credits the rule to Kellie Evans [wojtowicznd_rules]. Golly's
  help says that "Bosco" is a period-166 oscillator discovered by Evans, and that Dean Hickerson used
  MCell to build the first "bug gun" from it [gollynd_ltl]. The rule became known as Bosco's Rule
  (LifeWiki; `R5,C2,S33-57,B34-45`, the same rule with the centre excluded) [lifewikind_ltl].
- **Other named LtL rules.** David Griffeath: Majority (R4) and Majorly (R7), both voter models, and
  Bugsmovie (R10). Kellie Evans: Bosco's Rule, Waffle (R7) and Gnarl (range 1, von Neumann). MCell
  says Gnarl was named in homage to Rudy Rucker's *Artificial Life Lab* (1993). Mirek Wójtowicz:
  Globe (R8). Charles A. Rockafellor: ModernArt (R10, 255 states). More recent forum rules credited
  by LifeWiki: Factorio (H. H. P. M. P. Cole), Coexistence (bubblegum), Marine (MathAndCode), Hash
  (b-engine) [wojtowicznd_rules; lifewikind_ltl].
- **MCell notation (1999).** Mirek Wójtowicz's MCell gave LtL the notation `Rr,Cc,Mm,Smin..max,
  Bmin..max,Nn`: range 1–10, C ≥ 3 for Generations-style decay, M1 to count the middle cell,
  NM box or NN diamond [wojtowicznd_families; lifewikind_ltl]. Golly's help credits MCell (1999) as
  the home of this notation and records Evans's own five-number form `r,bmin,bmax,smin,smax`
  [gollynd_ltl].
- **HROT (2018–2020).** "Higher-range outer-totalistic" rules generalise LtL from one birth interval
  and one survival interval to arbitrary lists of counts, written `Rr,Cc,S…,B…,Nn`. The centre cell
  is always excluded, and Moore is the default [lifewikind_hrot; gollynd_ltl]. Chris Rowett's
  LifeViewer added LtL and HROT support in build 264 (forum post of 2 January 2019). The post says
  that LifeViewer also accepts Adam P. Goucher's lifelib notation for HROT rules [rowett2019lifeviewer].
  In Golly, the Larger than Life algorithm arrived in version 3.0 (August 2017). Version 4.0
  (October 2020) added HROT-format rules, B0 emulation and many new neighbourhoods [gollynd_changes].
  Golly now supports ranges up to 500 and neighbourhoods including circular, cross, saltire, star,
  hexagonal, triangular, weighted and custom [gollynd_ltl]. LifeWiki says HROT notation "now
  supersedes" the MCell form [lifewikind_ltl].

## Key primary citations

- [evans1996larger] K. M. Evans, *Larger than Life: It's So Nonlinear*, PhD thesis, University of
  Wisconsin–Madison (1996), supervisor D. Griffeath. http://www.csun.edu/~kme52026/thesis.html .
  Verified: contents page fetched. The abstract and preface PDFs were downloaded and their text
  decoded (supervisor, institution, Griffeath's question). The year comes from LifeWiki and Golly,
  because the PDF's digits are not machine-readable.
- [evans2001larger] K. M. Evans, "Larger than Life: Digital Creatures in a Family of
  Two-Dimensional Cellular Automata", DMTCS Proceedings AA (DM-CCG 2001), 177–192 (2001).
  DOI 10.46298/dmtcs.2288. Verified: episciences landing page and EMIS mirror.
- [evans2003larger] K. M. Evans, "Larger than Life: threshold-range scaling of Life's coherent
  structures", Physica D 183(1–2):45–67 (2003). DOI 10.1016/S0167-2789(03)00155-6. Verified:
  Crossref record.
- [evans2005bosco] K. M. Evans, "Is Bosco's Rule Universal?", in MCU 2004, LNCS 3354, 188–199
  (2005). DOI 10.1007/978-3-540-31834-7_15. Verified: Crossref record.
- [griffeath1994self] D. Griffeath, "Self-Organization of Random Cellular Automata: Four
  Snapshots", in G. Grimmett (ed.), *Probability and Phase Transition*, Springer/Kluwer, 49–67
  (1994). DOI 10.1007/978-94-015-8326-8_4. Verified: Crossref record (content not read).
- [wojtowicznd_rules] / [wojtowicznd_families] MCell rules lexicon and families page (mcell.ca).
  Verified: downloaded and read.
- [gollynd_ltl] Golly Help, "Larger than Life". Verified: fetched (credits and reference list).

## Software that ran it before JETCARP

- CAM-8 (MIT cellular automaton machine): LtL exploration in which the Bugs rule was found,
  according to MCell's lexicon [wojtowicznd_rules].
- MCell (Mirek Wójtowicz, from 1999): the LtL family with ranges 1–10 [wojtowicznd_families].
- Golly 3.0+ (August 2017), and HROT in 4.0+ (October 2020) [gollynd_changes].
- LifeViewer (Chris Rowett), from build 264, January 2019 [rowett2019lifeviewer].
- Adam P. Goucher's lifelib/apgsearch and Catagolue use an HROT notation [rowett2019lifeviewer;
  lifewikind_ltl]. The date was not verified.
- On aperiodic tilings: Andrew Trevorrow's HatLife (2023) accepts LtL-style range rules on the hat
  (novelty-06 and novelty-08 in this folder). We did not confirm whether its range is a graph
  distance.

## People JETCARP should credit

- **David Griffeath**, who posed the question and named or explored the first rules (Majority,
  Majorly, Bugsmovie).
- **Kellie Michele Evans**, who defined and studied the family (thesis 1996; papers 2001–2016) and
  found Bosco's Rule, Waffle, Gnarl and the bugs.
- **Mirek Wójtowicz**, for the MCell implementation and notation, and the Globe rule.
- **Dean Hickerson**, for the first bug gun (in MCell).
- **Chris Rowett** (LifeViewer) and **Adam P. Goucher** (lifelib), for HROT support and notation.
  **The Golly developers**, for Golly's LtL/HROT algorithm. Neither the change log nor the help
  page names an implementer, so credit them collectively.
- Authors of the individual rules: see `named-rule-authors.md`.

## How JETCARP's version relates

Per the README, `ltl.rs` runs range-r outer-totalistic rules in LifeWiki's HROT form or MCell's form,
with Moore, von Neumann or cross neighbourhoods. On other tilings, "within r" is measured in
neighbour-graph steps, and range 1 is checked to equal Life exactly.

## Unverified

- The content of Griffeath (1994) and whether it is the first publication of LtL. Only the
  bibliographic record was checked.
- The thesis year (1996): taken from LifeWiki and Golly, not read from the thesis.
- Who first wrote the HROT notation and when. LifeViewer's January 2019 post credits Goucher's
  lifelib notation, but the lifelib specification was not read.
- The CAM-8 origin of Bugs and Hickerson's bug gun: secondary sources only (MCell lexicon, Golly
  help).
- Factorio, Coexistence, Marine and Hash credits: LifeWiki only (forum threads not read).
- Note for JETCARP: `named.json` includes **Coexistence**
  (`R4,C2,S9,…,B3,…,NW1000…`), a weighted (`NW`) HROT rule. Yet the README says that HROT rules
  with custom, weighted or hash neighbourhoods were left out, and `ltl.rs` rejects neighbourhoods
  other than NM, NN and N+. That entry probably does not parse. Check it.

## Search log

- WebFetch: conwaylife.njohnston.ca/wiki/Larger_than_Life;
  /wiki/Higher-range_outer-totalistic_cellular_automaton; /forums/viewtopic.php?p=67127;
  golly.sourceforge.io/Help/Algorithms/Larger_than_Life.html and /Help/changes.html;
  www.csun.edu/~kme52026/thesis.html; dmtcs.episciences.org/2288;
  www.maths.tcd.ie/EMIS/journals/DMTCS/proceedings/html/dmAA0113.abs.html;
  link.springer.com/book/10.1007/b106980 (redirected to a login, not read);
  dblp.dagstuhl.de (blocked by Anubis).
- curl: csun.edu thesis PDFs copyrt, abs, 1preface, 1ch1 (text decoded locally; the font is a
  shifted encoding); mcell.ca/app/data/rules.txt and /pages/families.html.
- Crossref API (api.crossref.org/works?query.bibliographic=…): Evans "Is Bosco's Rule Universal",
  "Larger than Life's Extremes", "Replicators and Larger than Life examples", "Larger than Life"
  (Designing Beauty), "Threshold-range scaling"; Griffeath 1994 and 2016; Gravner and Griffeath 1998.
- WebSearch: `Kellie Evans "Larger than Life: Digital Creatures …" DMTCS 2001`;
  `Evans "Is Bosco's Rule Universal?" … LNCS 3354 doi`;
  `lifelib HROT higher-range outer-totalistic rule notation Goucher calcyman …`.
- Unreachable: psoup.math.wisc.edu (Griffeath's Primordial Soup Kitchen), conwaylife.com (403).
