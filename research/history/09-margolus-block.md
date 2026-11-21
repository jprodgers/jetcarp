# 09. Margolus neighbourhood and block CAs: history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.

## Short history

- **The billiard-ball model (1982).** Edward Fredkin and Tommaso Toffoli's "Conservative logic"
  (1982) set out reversible, conservative computation. Its billiard-ball model carries logic signals
  as hard balls whose elastic collisions act as gates [fredkin1982conservative].
- **Margolus's partitioning CA (1984).** Norman Margolus (MIT Information Mechanics Group, with
  Toffoli) found a very simple reversible CA that runs the billiard-ball model. He published it in
  "Physics-like models of computation", Physica D 10 (1984) [margolus1984physicslike]. The grid is cut
  into 2×2 blocks, every block is replaced through a fixed table, and the blocking shifts by one cell
  diagonally on alternate steps. This is now called the **Margolus neighbourhood**, and rules of this
  kind are **block** or **partitioned** cellular automata. A block CA is reversible exactly when its
  block table is a bijection. Margolus later wrote that the partitioning technique "was first
  developed" for a universal invertible CA "modeled after Fredkin's Billiard Ball Model", and that
  Toffoli had built the first universal invertible CA earlier by another route
  [margolus1999crystalline].
- **CAM machines and the 1987 book.** Toffoli and Margolus built dedicated CA hardware at MIT
  (the CAM series; Toffoli designed the first boards, which Margolus called a "glorified
  frame-buffer"; Margolus later designed CAM-8) [margolus1999crystalline]. Their book *Cellular
  Automata Machines: A New Environment for Modeling* (MIT Press, 1987) was written around the CAM-6
  machine. A whole chapter, "The Margolus neighborhood" (pp. 123–142), covers block rules
  [toffoli1987cellular]. The book introduced rules that are still standard examples:
  - **Critters**, a reversible, particle-conserving rule with glider-like objects that can also
    simulate billiard-ball computing. Margolus calls it "a universal invertible CA that evolves
    interesting complexity" [margolus1999crystalline].
  - **Tron**, which complements a block when all four cells agree, giving rectilinear patterns.
  - **HPP gas** and related lattice gases on the Margolus neighbourhood.
  (Credit of Critters and Tron to the 1987 book: CelLab's rule index and Wikipedia's "Block cellular
  automaton" both give the book's page numbers. The book itself was not read.)
- **HPP lattice gas (1973–1976).** J. Hardy, Y. Pomeau and O. de Pazzis introduced the
  four-direction lattice gas on the square lattice in 1973 [hardy1973time; hardy1973timeevolution]
  and analysed its transport properties in 1976 [hardy1976molecular]. Margolus recalls that the MIT
  group rediscovered it in the early 1980s, "about a decade" after its originators
  [margolus1999crystalline]. The hexagonal FHP gas of Frisch, Hasslacher and Pomeau (1986)
  [frisch1986latticegas] removed HPP's unphysical extra conservation laws. Margolus, Toffoli and
  Vichniac described CA hardware for fluid dynamics the same year [margolus1986cellularautomata].
- **Later analysis.** Toffoli and Margolus reviewed invertible CAs in 1990 [toffoli1990invertible].
- **Single Rotation (2013).** The blog "Random thoughts and fancy math" (author handle dmishin)
  posted "The Single Rotation rule: remarkably simple and rich reversible cellular automaton" in
  November 2013 [dmishin2013single]. The rule turns clockwise by 90° every block with exactly one live
  cell. The post gives MCell-style code [0,2,8,3,1,5,6,7,4,9,10,11,12,13,14,15], notes that the rule
  has no mirror symmetry, and says that the author doubts he was the first to find so simple a rule
  but had not seen it elsewhere. A follow-up post in 2014 listed 205 spaceships.

## Key primary citations

- [margolus1984physicslike] Norman Margolus, "Physics-like models of computation", Physica D:
  Nonlinear Phenomena 10(1–2):81–95 (Jan 1984). DOI 10.1016/0167-2789(84)90252-5. Verified: Crossref.
  Wikipedia notes a reprint in Wolfram (ed.), *Theory and Applications of Cellular Automata* (World
  Scientific, 1986), pp. 232–246 (not checked).
- [toffoli1987cellular] Tommaso Toffoli and Norman Margolus, *Cellular Automata Machines: A New
  Environment for Modeling*, MIT Press, Cambridge MA (1987). DOI 10.7551/mitpress/1763.001.0001; chapter
  "The Margolus neighborhood", pp. 123–142, DOI 10.7551/mitpress/1763.003.0016. Verified: Crossref
  (book and chapter records; the MIT Press page returns 403).
- [fredkin1982conservative] Edward Fredkin and Tommaso Toffoli, "Conservative logic", International
  Journal of Theoretical Physics 21(3–4):219–253 (1982). DOI 10.1007/BF01857727. Verified: Crossref.
- [hardy1973time] J. Hardy, Y. Pomeau and O. de Pazzis, "Time evolution of a two-dimensional model
  system. I. Invariant states and time correlation functions", Journal of Mathematical Physics
  14(12):1746–1759 (1973). DOI 10.1063/1.1666248. Verified: Crossref.
- [hardy1976molecular] J. Hardy, O. de Pazzis and Y. Pomeau, "Molecular dynamics of a classical
  lattice gas: transport properties and time correlation functions", Physical Review A 13(5):1949–1961
  (1976). DOI 10.1103/PhysRevA.13.1949. Verified: Crossref.
- [margolus1999crystalline] Norman Margolus, "Crystalline computation", in A. J. G. Hey (ed.),
  *Feynman and Computation*, Perseus Books (1999), pp. 267–305 (CRC Press reissue DOI
  10.1201/9780429500459-18). arXiv:comp-gas/9811002. Verified: arXiv abstract page, the ar5iv HTML
  (history statements read) and Crossref (chapter record).
- [dmishin2013single] dmishin, "The Single Rotation rule: remarkably simple and rich reversible
  cellular automaton", blog *Random thoughts and fancy math*, Nov 2013,
  https://dmishin.blogspot.com/2013/11/the-single-rotation-rule-remarkably.html . Verified: page
  fetched. Simulator source: https://github.com/dmishin/js-revca (linked from the post; not opened).

Further verified (Crossref):

- [hardy1973timeevolution] J. Hardy, Y. Pomeau and O. de Pazzis, "Time evolution of a
  two-dimensional classical lattice system", Physical Review Letters 31(5):276–279 (1973).
  DOI 10.1103/PhysRevLett.31.276.
- [frisch1986latticegas] U. Frisch, B. Hasslacher and Y. Pomeau, "Lattice-gas automata for the
  Navier–Stokes equation", Physical Review Letters 56(14):1505–1508 (1986). DOI 10.1103/PhysRevLett.56.1505.
- [margolus1986cellularautomata] N. Margolus, T. Toffoli and G. Vichniac, "Cellular-automata
  supercomputers for fluid-dynamics modeling", Physical Review Letters 56(16):1694–1696 (1986).
  DOI 10.1103/PhysRevLett.56.1694.
- [toffoli1990invertible] T. Toffoli and N. H. Margolus, "Invertible cellular automata: a review",
  Physica D 45(1–3):229–253 (1990). DOI 10.1016/0167-2789(90)90185-R.

## Software that ran it before JETCARP

- **CAM-6 and later CAM machines** (Toffoli and Margolus, MIT, 1980s), the hardware for which the
  1987 book was written [toffoli1987cellular; margolus1999crystalline].
- **CelLab** (Rucker and Walker, Autodesk 1989; Web edition 1995) [rucker1989cellab]: a "margolus"
  evaluator with rules BBM, CRITTERS, TRON, HPPGAS and PERFUMEM (HPP gas with walls). The manual
  credits the Margolus neighbourhood trick to Norman Margolus and keys these rules to pages of
  *Cellular Automata Machines*. Manual rules page fetched.
- **MCell** (Mirek Wójtowicz): a "Margolus" rule family with its `MS,D…` notation
  [mcellnd_families]. JETCARP accepts the same notation and uses MCell's block numbering. Golly's
  converter script cites MCell's page at psoup.math.wisc.edu/mcell/rullex_marg.html and thanks Mirek
  for "MCell's implementation of the Margolus neighbourhood".
- **Golly**: Margolus and other partitioned rules through emulation. `EmulateMargolus.py` builds a
  two-layer CA, and Tim Hutton's `convert-MCell-string.py` turns an MCell Margolus string into a rule
  table (both read from Debian's Golly source packages). LifeWiki says LifeViewer supports Margolus
  rules natively (search summary only).
- **dmishin's js-revca** (2013): a JavaScript simulator for Single Rotation and other reversible
  block rules [dmishin2013single].

## People JETCARP should credit

Norman Margolus (the neighbourhood and the BBM CA); Tommaso Toffoli and Norman Margolus (Critters,
Tron, CAM machines, the 1987 book); Edward Fredkin and Tommaso Toffoli (the billiard-ball model);
J. Hardy, Y. Pomeau and O. de Pazzis (the HPP gas); dmishin (Single Rotation, 2013); Mirek
Wójtowicz (MCell's Margolus family and notation, which JETCARP accepts).

## How JETCARP's version relates

`Margolus <16 outputs>`, MCell's `MS,D…`, and the named tables `Margolus BBM`, `Critters`, `Tron`,
`SingleRotation` and `HPP` run on the square grid with MCell's block numbering. The named tables are
built in code from the published rule statements, not copied from rule files (README "### Margolus
block rules").

## Unverified

- The contents of *Cellular Automata Machines*. That Critters and Tron were introduced there is
  supported only by CelLab's rule index (book page numbers) and Wikipedia. The exact pages and
  wording were not seen.
- Whether Margolus 1984 contains the BBM block rule in exactly the form JETCARP implements. The paper
  was not read; its abstract was not available through Crossref.
- The real name of the Single Rotation author. JETCARP's README says "D. Mishin". The blog is by
  "dmishin", and a commenter on the post calls the author Dmitry Shintyakov. Check this before the
  docs print a name.
- The CAM-6 dates and commercial availability (Systems Concepts) were not checked.
- LifeViewer's native Margolus support (from a search summary of LifeWiki's Golly page).

## Search log

- Crossref searches and DOI lookups: Margolus 1984; Fredkin and Toffoli 1982; Hardy, Pomeau and
  de Pazzis 1973 (two papers) and 1976; the Toffoli–Margolus book and chapter; Toffoli and Margolus
  1990; Margolus, Toffoli and Vichniac 1986; Frisch, Hasslacher and Pomeau 1986; Margolus 1999 chapter.
- WebFetch: en.wikipedia.org/wiki/Block_cellular_automaton (pointer); direct.mit.edu book page (403);
  arxiv.org/abs/comp-gas/9811002; ar5iv.labs.arxiv.org/html/comp-gas/9811002 (first 100k characters);
  dmishin.blogspot.com/2013/11/the-single-rotation-rule-remarkably.html;
  golly.sourceforge.io/Help/formats.html; sources.debian.org Golly 2.1 `convert-MCell-string.py` and
  Golly 2.8 `EmulateMargolus.py`; fourmilab.ch/cellab/manual/rules.html.
- WebSearch: `Mishin "Single Rotation rule" …`; `Golly "Margolus" emulated rule …`.
