# 07. Cyclic CA, Greenberg–Hastings and rock-paper-scissors: history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.

## Short history

**Greenberg–Hastings (1978).** J. M. Greenberg and S. P. Hastings introduced a discrete model of an
excitable medium in "Spatial patterns for discrete models of diffusion in excitable media", SIAM
Journal on Applied Mathematics (May 1978) [greenberg1978spatial]. A cell is resting, excited, or in
one of several refractory stages. A resting cell becomes excited when enough neighbours are excited,
and the other stages advance on their own. In the same year Greenberg, Hassard and Hastings published
a long survey of pattern formation and periodic structures in reaction–diffusion systems in the
Bulletin of the AMS [greenberg1978pattern]. The model belongs to the excitable-media tradition
(nerve and heart tissue, the Belousov–Zhabotinsky reaction) and produces target waves and rotating
spirals. Later rigorous work includes Durrett and Steif, "Some rigorous results for the
Greenberg–Hastings model" (1991) [durrett1991some]; Fisch, Gravner and Griffeath, "Metastability in
the Greenberg–Hastings model" (1993) [fisch1993metastability]; and Durrett and Griffeath,
"Asymptotic behavior of excitable cellular automata" (1993) [durrett1993asymptotic].

**Cyclic cellular automaton (late 1980s, Wisconsin).** The cyclic CA (CCA) has n colours on a
"colour wheel". A cell advances to the next colour when at least a threshold number of neighbours
already have that next colour. It came from David Griffeath's probability group at the University of
Wisconsin–Madison. MCell's family page says it was "discovered and explored by David Griffeath"
[mcellnd_families]. Griffeath described it in "Cyclic random competition", Notices of the AMS, December 1988
[griffeath1988cyclic]. Its stochastic ancestor is the cyclic particle system of Bramson and Griffeath
(1989) [bramson1989flux]. Robert Fisch, Griffeath's student, analysed the deterministic
one-dimensional CCA in 1990 [fisch1990onedimensional; fisch1990cyclic]. A. K. Dewdney presented the
two-dimensional rule as "cyclic space" in his Computer Recreations column for August 1989, "A
cellular universe of debris, droplets, defects and demons" [dewdney1989cellular]. The "demons" of
the title are the self-sustaining spiral cores that take over a random start. The standard reference
is Fisch, Gravner and Griffeath, "Threshold-range scaling of excitable cellular automata" (1991)
[fisch1991threshold]. It treats CCA and GH rules together with a range and a threshold, and describes
the traveling waves, spirals and "forest fire" equilibria that these rules produce. In their
terminology a CCA updates every colour by contact, while a GH rule updates only colour 0 by contact
and advances the others automatically [fisch1991threshold]. Griffeath published many CCA and GH
images and notes on his *Primordial Soup Kitchen* website (psoup.math.wisc.edu, now offline; see
Unverified). Cai and Ghrist (2012) extended cyclic automata to networks (novelty-08).

**Rock-paper-scissors (cyclic dominance).** Three species in a cycle of dominance (rock beats
scissors, scissors beats paper, paper beats rock) on a lattice were studied in physics and ecology
from Tainaka's lattice Lotka–Volterra model (1988, and the 1989 PRL on vortices) onward
[tainaka1988lattice; tainaka1989stationary]. Further work includes Frean and Abraham on the
"survival of the weakest" (2001) [frean2001rock]; Kerr, Riley, Feldman and Bohannan's experiment
with *E. coli* strains and lattice simulations (2002) [kerr2002local]; and Reichenbach, Mobilia and
Frey on mobility and spiral waves (2007) [reichenbach2007mobility]. Those are stochastic models. The
deterministic threshold form JETCARP implements (a cell takes its predator's state when more than a
threshold number of its eight neighbours are that predator, Moore neighbourhood) is the one
described by Softology (Jason, maker of Visions of Chaos) in the blog post "Rock Paper Scissors
Cellular Automata", 23 March 2018 [softology2018rock]. That post also covers 5-, 15- and 25-state
variants and a random term added to the threshold. JETCARP's code comment credits Softology. As a
deterministic rule, it is a cyclic CA with a strict threshold: `RPS n/t` behaves like `Cyclic n/(t+1)`.

## Key primary citations

- [greenberg1978spatial] J. M. Greenberg and S. P. Hastings, "Spatial patterns for discrete models
  of diffusion in excitable media", SIAM J. Appl. Math. 34(3):515–523 (May 1978). DOI 10.1137/0134040.
  Verified: Crossref (the SIAM page returns 403).
- [fisch1991threshold] R. Fisch, J. Gravner and D. Griffeath, "Threshold-range scaling of excitable
  cellular automata", Statistics and Computing 1(1):23–39 (1991). DOI 10.1007/BF01890834.
  arXiv:patt-sol/9304001. Verified: Crossref and the arXiv abstract page.
- [dewdney1989cellular] A. K. Dewdney, "Computer Recreations: A cellular universe of debris,
  droplets, defects and demons", Scientific American 261(2):102–105 (Aug 1989). DOI
  10.1038/scientificamerican0889-102 (JSTOR 24987368). Verified: Crossref (Dewdney, "Computer
  Recreations", 261(2):102–105, Aug 1989). The subtitle is from Wikipedia's reference list and a
  search summary of the Scientific American archive page "Computer Recreations, August 1989".
- [griffeath1988cyclic] D. Griffeath, "Cyclic random competition", Notices of the American
  Mathematical Society, December 1988, p. 1474. Verified (secondary): listed in the CelLab manual's
  references (fourmilab.ch/cellab/manual/refs.html, fetched), and CelLab's GRIFF rule cites it. The
  Notices issue itself was not seen.
- [bramson1989flux] M. Bramson and D. Griffeath, "Flux and fixation in cyclic particle systems",
  Annals of Probability 17(1):26–45 (1989). DOI 10.1214/aop/1176991492. Verified: Crossref.
- [fisch1990onedimensional] R. Fisch, "The one-dimensional cyclic cellular automaton: a system with
  deterministic dynamics that emulates an interacting particle system with stochastic dynamics",
  J. Theoretical Probability 3(2):311–338 (1990). DOI 10.1007/BF01045164. Verified: Crossref.
- [fisch1990cyclic] R. Fisch, "Cyclic cellular automata and related processes", Physica D
  45(1–3):19–25 (1990). DOI 10.1016/0167-2789(90)90170-T. Verified: Crossref.
- [softology2018rock] Softology (signed "Jason"), "Rock Paper Scissors Cellular Automata", Softology's
  Blog, 23 Mar 2018, https://softologyblog.wordpress.com/2018/03/23/rock-paper-scissors-cellular-automata/ .
  Verified: page fetched.

Further verified (Crossref):

- [greenberg1978pattern] J. M. Greenberg, B. D. Hassard and S. P. Hastings, "Pattern formation and
  periodic structures in systems modeled by reaction-diffusion equations", Bull. Amer. Math. Soc.
  84(6):1296–1327 (1978). DOI 10.1090/S0002-9904-1978-14560-1.
- [durrett1991some] R. Durrett and J. E. Steif, "Some rigorous results for the Greenberg–Hastings
  model", J. Theoretical Probability 4(4):669–690 (1991). DOI 10.1007/BF01259549.
- [fisch1993metastability] R. Fisch, J. Gravner and D. Griffeath, "Metastability in the
  Greenberg–Hastings model", Annals of Applied Probability 3(4):935–967 (1993). DOI 10.1214/aoap/1177005268.
- [durrett1993asymptotic] R. Durrett and D. Griffeath, "Asymptotic behavior of excitable cellular
  automata", Experimental Mathematics 2(3):183–208 (1993). DOI 10.1080/10586458.1993.10504277.
- [tainaka1988lattice] K. Tainaka, "Lattice model for the Lotka–Volterra system", J. Phys. Soc. Japan
  57(8):2588–2590 (1988). DOI 10.1143/JPSJ.57.2588.
- [tainaka1989stationary] K. Tainaka, "Stationary pattern of vortices or strings in biological
  systems: lattice version of the Lotka–Volterra model", Phys. Rev. Lett. 63(24):2688–2691 (1989).
  DOI 10.1103/PhysRevLett.63.2688.
- [frean2001rock] M. Frean and E. R. Abraham, "Rock–scissors–paper and the survival of the weakest",
  Proc. R. Soc. Lond. B 268(1474):1323–1327 (2001). DOI 10.1098/rspb.2001.1670.
- [kerr2002local] B. Kerr, M. A. Riley, M. W. Feldman and B. J. M. Bohannan, "Local dispersal promotes
  biodiversity in a real-life game of rock–paper–scissors", Nature 418(6894):171–174 (2002).
  DOI 10.1038/nature00823.
- [reichenbach2007mobility] T. Reichenbach, M. Mobilia and E. Frey, "Mobility promotes and
  jeopardizes biodiversity in rock–paper–scissors games", Nature 448(7157):1046–1049 (2007).
  DOI 10.1038/nature06095.
- [rucker1989cellab] R. Rucker and J. Walker, *CelLab* (first edition *Rudy Rucker's Cellular Automata
  Laboratory*, Autodesk, June 1989; Web edition 1995; WebCA since 2017), https://www.fourmilab.ch/cellab/ .
  Verified: home page and the rules and references pages of the manual fetched.
- [mcellnd_families] M. Wójtowicz, MCell (Mirek's Cellebration), "Families" page,
  https://mcell.ca/pages/families.html (mirekw.com now redirects to mcell.ca). Verified: page fetched.
  It says Cyclic CA was "discovered and explored by David Griffeath" and that GH is included as a
  special case. (The page does not name its author; "Mirek" is named on the home page.)

## Software that ran it before JETCARP

- **CelLab** (Rudy Rucker and John Walker; first shipped by Autodesk in June 1989 as *Rudy Rucker's
  Cellular Automata Laboratory*): rule GRIFF, "a cyclic cellular automaton as described by David
  Griffeath" (a randomised variant) [rucker1989cellab].
- **MCell** (Mirek Wójtowicz): a "Cyclic CA" family with Greenberg–Hastings as a special case
  [mcellnd_families].
- **Griffeath's Primordial Soup Kitchen** (psoup.math.wisc.edu, 1990s–2000s): images, recipes and
  software for CCA and GH (from Wikipedia's notes; the site is offline).
- **Visions of Chaos** (Softology): "Cyclic Cellular Automata" (2-D and 3-D) and the RPS CA
  [softology2018rock]; the softology.pro/voc.htm list was fetched.
- **xlockmore's "demon" mode**: a screensaver of Griffeath's CCA, coded from Dewdney's August 1989
  column (from a search summary of the source; not fetched, 401).
- **Cai and Ghrist (2012)**: cyclic automata on networks (novelty-08).
- **Ready** (GollyGang) can run excitable-media rules on meshes (novelty-08). Specific CCA/GH sample
  files were not checked.

## People JETCARP should credit

J. M. Greenberg and S. P. Hastings (GH, 1978); David Griffeath (cyclic CA), with Robert Fisch,
Janko Gravner and Maury Bramson; A. K. Dewdney (the 1989 column); Kei-ichi Tainaka and the
cyclic-dominance literature (Frean and Abraham; Kerr et al.; Reichenbach, Mobilia and Frey) for RPS;
Softology / Visions of Chaos for the deterministic threshold RPS CA; Mirek Wójtowicz (MCell).

## How JETCARP's version relates

`Cyclic <states>/<threshold>` advances on at least the threshold, `GH <states>/<threshold>` is
Greenberg–Hastings, and `RPS <species>/<threshold>` converts on more than the threshold. All three
run on any tiling, on CPU and GPU (README "## Rule notation", Phase 2 item 8). In the November 2026
exploratory search, Greenberg–Hastings rated best of all families (README "## Exploratory search").

## Unverified

- Griffeath 1988 (Notices of the AMS) was not seen directly. Whether it is the first CCA publication
  is not confirmed.
- Griffeath's Primordial Soup Kitchen pages (recipe 6, "Turbulent equilibrium in a cyclic cellular
  automaton"; recipe 29, "Bob's dilemma"): the host no longer resolves and web.archive.org is blocked.
- Dewdney's August 1989 column text was not read (how it credits Griffeath).
- The full name of Softology's author. The post is signed "Jason". Softology is widely known as Jason
  Rampe's, but this was not confirmed on a fetched page.
- Whether any earlier source than Softology (2018) used the deterministic "more than threshold"
  RPS rule. A Reddit post (r/GeometryIsNeat, Aug 2020) describes the same rule but is later.
- Wiener and Rosenblueth (1946) as the classical origin of excitable-media models: not checked.

## Search log

- WebFetch: en.wikipedia.org/wiki/Cyclic_cellular_automaton (pointer); arxiv.org/abs/patt-sol/9304001;
  psoup.math.wisc.edu/archive/recipe6.html (DNS failure); epubs.siam.org/doi/10.1137/0134040 (403);
  fossies.org xlockmore demon.c (401); softologyblog.wordpress.com search and 2018/03/23 post;
  softology.pro/voc.htm; mcell.ca and mcell.ca/pages/families.html; fourmilab.ch/cellab/,
  /cellab/manual/rules.html and /cellab/manual/refs.html.
- Crossref searches and DOI lookups for every DOI above.
- WebSearch: `Dewdney Scientific American August 1989 "cellular universe of debris, droplets, defects and demons" …`;
  `"rock paper scissors" cellular automaton deterministic rule … threshold …`;
  `Softology blog rock paper scissors cellular automata Jason Rampe`.
- Code check (to name the source, not for behaviour claims): `crates/jetcarp-core/src/rules.rs`
  comments the RPS rule as "Softology's rock-paper-scissors CA, Moore neighbourhood", and `step.rs`
  runs RPS as cyclic with threshold + 1.
