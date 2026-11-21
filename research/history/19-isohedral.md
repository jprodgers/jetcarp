# 19. Isohedral tilings: history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.

## Short history

- A tiling is **isohedral** when its symmetries act transitively on its tiles: one tile shape, and
  every tile looks the same from inside the pattern.
- **Elli and Heinrich Heesch** completed a classification of 28 types of asymmetric tiles that give
  isohedral tilings in 1932. They submitted it in 1934 but chose not to publish, and tried for years to
  patent it (Reichenberger's BSHM article and blog post, based on a confidential 1944 report)
  [reichenberger2023elli]. In 1935 Heinrich Heesch gave the first anisohedral tile in the plane
  (Hilbert's 18th problem), which Reichenberger attributes to both siblings.
- **Heesch and Otto Kienzle**, *Flächenschluß* (Springer, 1963), published the 28 "HK" types with
  boundary "words" (letters T, C, G for translation, centre/half-turn and glide). Their scheme leaves
  out tilings with mirror lines [heesch1963flachenschluss; kaplan2025isohedral].
- **Grünbaum and Shephard** gave the complete classification with "incidence symbols" in "The
  eighty-one types of isohedral tilings in the plane" (1977), and a polygonal version in 1978. There
  are 93 types, IH1–IH93. Twelve of them are *marked* types, which need a motif inside the tile to cut
  the symmetry down, so 81 types remain for unmarked tiles. The HK types are a subset of the IH types
  [grunbaum1977eightyone; grunbaum1978isohedral; grunbaum1987tilings; kaplan2025isohedral].
- Bigalke and Wippermann's *Reguläre Parkettierungen* (1994) gave an intermediate 43-type list with
  an added letter S for mirrors (from Kaplan and Schattschneider 2025; book not checked).
- **Escher.** Nearly all of M. C. Escher's regular divisions of the plane can be read as isohedral
  tilings (Kaplan and Schattschneider 2025) [kaplan2025isohedral]. Doris Schattschneider's *Visions of
  Symmetry* (W. H. Freeman 1990; Abrams 2004) is the standard catalogue of his periodic drawings
  [schattschneider1990visions].
- **Computer graphics.** Craig S. Kaplan wrote the first versions of his *Tactile* library in the late
  1990s during his PhD (thesis *Computer Graphics and Geometric Ornamental Design*, University of
  Washington, 2002, advisor David Salesin per the Mathematics Genealogy Project, with a chapter on
  Escher's tilings) [kaplan2002computer]. With David Salesin he published "Escherization"
  (SIGGRAPH 2000) [kaplan2000escherization]; the thesis abstract describes it as an algorithm that
  derives Escher-like tessellations from shapes the user supplies. His
  book *Introductory Tiling Theory for Computer Graphics* (2009) explains the IH types for programmers
  [kaplan2009introductory]. Modern Tactile in C++ and TactileJS (GitHub, Nov 2018, BSD-3-Clause) supports
  the 81 non-trivial types; Kaplan's README calls the other 12 "boring" [kaplannd_tactile;
  kaplannd_tactilejs].
- Tom P. McLean's online catalogue of all 93 templates (mirrored by Jaap Scherphuis) gives each type's
  symmetry group and many tiles based on Escher designs [mcleannd_isohedral].

## Key primary citations

- [grunbaum1977eightyone] Branko Grünbaum and G. C. Shephard, "The eighty-one types of isohedral tilings
  in the plane", *Math. Proc. Cambridge Philos. Soc.* 82(2):177–196 (1977). DOI
  10.1017/S0305004100053810. Verified: Crossref.
- [grunbaum1978isohedral] Branko Grünbaum and G. C. Shephard, "Isohedral tilings of the plane by
  polygons", *Comment. Math. Helv.* 53(1):542–571 (1978). DOI 10.1007/BF02566098. Verified: Crossref.
- [heesch1963flachenschluss] Heinrich Heesch and Otto Kienzle, *Flächenschluß: System der Formen
  lückenlos aneinanderschließender Flachteile*, Springer, Berlin, 1963 (series Wissenschaftliche
  Normung). DOI 10.1007/978-3-642-94883-1. Verified: Crossref.
- [kaplannd_tactilejs] Craig S. Kaplan, *TactileJS*, https://github.com/isohedral/tactile-js (repository
  created 19 Nov 2018; BSD-3-Clause; C++ original https://github.com/isohedral/tactile). Verified: GitHub
  API and README.
- [kaplan2009introductory] Craig S. Kaplan, *Introductory Tiling Theory for Computer Graphics*, Synthesis
  Lectures on Computer Graphics and Animation, Morgan & Claypool, 2009 (now Springer). DOI
  10.1007/978-3-031-79543-5. Verified: Crossref.
- [reichenberger2023elli] Andrea Reichenberger, "Elli Heesch, Heinrich Heesch and Hilbert's eighteenth
  problem: collaborative research between philosophy, mathematics and application", *British Journal for
  the History of Mathematics* 38(3):208–228 (2023). DOI 10.1080/26375451.2023.2297522. Verified: Crossref,
  plus the BSHM blog post that summarises it.
- [kaplan2025isohedral] Craig S. Kaplan and Doris Schattschneider, "Isohedral Tiles, Tilings, and
  Notations", *Isohedral* (blog), 1 Sep 2025, https://isohedral.ca/isohedral-tiles-tilings-and-notations/ .
  Verified: page fetched.

## Software that ran it before JETCARP

- Kaplan's Tactile (late 1990s; C++ and JavaScript rewrites 2018) and its demos; the provenance audit
  confirms JETCARP's tables come from tactile-js.
- Kaplan's Escherization software (2000) and many Escher-style tiling editors (not surveyed).
- CAs on isohedral tilings: Kerekrety (2020) ran Life-like rules on rhombille and Cairo tilings
  (novelty-05). The novelty audit found no spaceships reported on IH46, IH54, IH62, IH72 or IH84.

## People JETCARP should credit

**Craig S. Kaplan** (Tactile and its tables, which JETCARP converts); Branko Grünbaum and G. C. Shephard
(the IH numbering); Heinrich and Elli Heesch and Otto Kienzle (the first classification); Doris
Schattschneider (Escher's tilings); Tom McLean (template catalogue, useful for the docs).

## How JETCARP's version relates

All 81 Tactile-supported types are reshapeable families `ih01`…`ih93`, converted from tactile-js
tables (BSD-3-Clause, `isohedral/LICENSE-tactile`) by `tools/gen_isohedral.py`, with Tactile's edge
shapes J, U, S and I and its parameters (README "Isohedral tile types").

## Unverified

- Heesch, "Aufbau der Ebene aus kongruenten Bereichen", *Nachr. Ges. Wiss. Göttingen, Neue Folge* 1
  (1935) 115–117 (the anisohedral tile). The details come from a search summary of reference lists
  (Springer, arXiv:2303.10798); the paper was not fetched.
- Bigalke and Wippermann, *Reguläre Parkettierungen*, BI-Wiss.-Verlag, 1994 (from Kaplan and
  Schattschneider's reference list only).

## Search log

- Crossref API: Grünbaum–Shephard 1977 and 1978; Kaplan 2009 and Kaplan–Salesin 2000; Heesch–Kienzle
  1963; Reichenberger 2023 (DOI); Heesch 1935 (no record).
- WebFetch: mathgenealogy.org id 78067 (Kaplan's PhD); isohedral.ca post (twice); bshm.ac.uk blog; cs.uwaterloo.ca/~csk/other/phd/;
  jaapsch.net/tilings/mclean (index, IH7, IH27, IH28, pentagon templates); en.wikipedia.org
  "Isohedral figure" (little use).
- curl: tactile and tactile-js READMEs; GitHub API for both repositories.
- WebSearch: Heesch 1935 citation; Heesch 28 types.
- Failed: cambridge.org article URL (404), cs.uwaterloo.ca/~csk/phd/ (404).
