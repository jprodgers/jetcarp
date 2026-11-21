# 08. Hodgepodge machine and forest-fire model: history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.

## Short history

**Hodgepodge machine (1988–1989).** Martin Gerhardt and Heike Schuster of the University of
Bielefeld (West Germany) designed the "hodgepodge machine". It is a many-state automaton, inspired by
catalytic chemical reactions. Dewdney's description (known from a search summary) calls a cell in
state 0 "healthy" and the others "infected" or "ill". A few parameters (JETCARP takes the number of
states and k1, k2 and g) control its behaviour. With suitable values it makes rotating spiral waves
like those of the Belousov–Zhabotinsky reaction. A. K. Dewdney introduced it to the public in his Computer Recreations
column "The hodgepodge machine makes waves", Scientific American, August 1988
[dewdney1988hodgepodge]. The CelLab manual credits it as "invented by Martin Gerhardt and Heike
Schuster of the University of Bielefeld" [rucker1989cellab]. Gerhardt and Schuster's own paper, "A
cellular automaton describing the formation of spatially ordered structures in chemical systems",
appeared in Physica D in August 1989 [gerhardt1989cellular]. Crossref's reference list for that paper
includes a 1988 Dewdney Scientific American column. With John J. Tyson they went on to a more
quantitative CA model of excitable media, published in Science in 1990 [gerhardt1990cellular] and as
a series in Physica D (1990–1991).

**Forest-fire model (1989–1992).** Per Bak, Kan Chen and Chao Tang introduced a forest-fire
automaton in "A forest-fire model and some thoughts on turbulence", Physics Letters A (July 1990)
[bak1990forestfire]. In it, empty sites grow trees with probability p, and fire spreads from burning
trees to neighbouring trees. Fires never start on their own. As p decreases the fire fronts become
more regular and spiral-shaped, according to Clar, Drossel and Schwabl's review [clar1996forest].
Chen, Bak and Jensen published a deterministic variant in the same year [chen1990deterministic].
Barbara Drossel and Franz Schwabl added spontaneous ignition ("lightning") with a small probability f
in "Self-organized critical forest-fire model", Physical Review Letters (14 Sep 1992)
[drossel1992selforganized]. With f ≪ p, so that tree growth and burning happen on separate time
scales, the model reaches a self-organised critical state with power-law fire sizes. This became the
standard forest-fire model; the review describes it as such [clar1996forest]. The review also notes
that C. L. Henley invented the extreme limit independently, in which clusters burn instantly (Bull. Am.
Phys. Soc. 34:838, 1989) [clar1996forest]. Henley later published "Statics of a 'self-organized'
percolation model" (1993) [henley1993statics]. The model descends from Bak, Tang and Wiesenfeld's idea of
self-organised criticality (the 1987 sandpile; see file 11). Grassberger (2002) and others later
showed that its scaling is not simple at very large sizes [grassberger2002critical].

## Key primary citations

- [gerhardt1989cellular] M. Gerhardt and H. Schuster, "A cellular automaton describing the formation
  of spatially ordered structures in chemical systems", Physica D: Nonlinear Phenomena 36(3):209–221
  (Aug 1989). DOI 10.1016/0167-2789(89)90081-X. Verified: Crossref.
- [dewdney1988hodgepodge] A. K. Dewdney, "Computer Recreations: The hodgepodge machine makes waves",
  Scientific American 259(2):104–107 (Aug 1988). DOI 10.1038/scientificamerican0888-104 (JSTOR
  10.2307/24989205 per search summary). Verified: Crossref (Dewdney, "Computer Recreations",
  259(2):104–107, Aug 1988). The subtitle is confirmed by the CelLab manual references and Burkardt's
  hodge page.
- [bak1990forestfire] P. Bak, K. Chen and C. Tang, "A forest-fire model and some thoughts on
  turbulence", Physics Letters A 147(5–6):297–300 (July 1990). DOI 10.1016/0375-9601(90)90451-S.
  Verified: Crossref.
- [drossel1992selforganized] B. Drossel and F. Schwabl, "Self-organized critical forest-fire model",
  Physical Review Letters 69(11):1629–1632 (14 Sep 1992). DOI 10.1103/PhysRevLett.69.1629. Verified:
  Crossref (the APS page returns 403).
- [clar1996forest] S. Clar, B. Drossel and F. Schwabl, "Forest fires and other examples of
  self-organized criticality", J. Phys.: Condens. Matter 8:6803 (1996). DOI 10.1088/0953-8984/8/37/004.
  arXiv:cond-mat/9610201. Verified: arXiv abstract page and the ar5iv HTML introduction, which was
  read for the history.

Further verified (Crossref):

- [gerhardt1990cellular] M. Gerhardt, H. Schuster and J. J. Tyson, "A cellular automaton model of
  excitable media including curvature and dispersion", Science 247(4950):1563–1566 (1990).
  DOI 10.1126/science.2321017.
- [chen1990deterministic] K. Chen, P. Bak and M. H. Jensen, "A deterministic critical forest fire
  model", Physics Letters A 149(4):207–210 (1990). DOI 10.1016/0375-9601(90)90328-L.
- [henley1993statics] C. L. Henley, "Statics of a 'self-organized' percolation model", Physical
  Review Letters 71(17):2741–2744 (1993). DOI 10.1103/PhysRevLett.71.2741.
- [grassberger2002critical] P. Grassberger, "Critical behaviour of the Drossel–Schwabl forest fire
  model", New Journal of Physics 4:17 (2002). DOI 10.1088/1367-2630/4/1/317.

## Software that ran it before JETCARP

- **CelLab** (Rucker and Walker, Autodesk 1989; Web edition 1995) [rucker1989cellab]: rule HODGE,
  "inspired by" Gerhardt and Schuster's machine; BOB, modelled on it; and FOREST, a forest-fire model
  "as described in" Drossel and Schwabl (1992). Manual rules page fetched.
- **Visions of Chaos** (Softology): "Hodgepodge Machine" (2-D, 3-D, 4-D) and "Forest Fire"
  (softology.pro/voc.htm, fetched).
- **John Burkardt's C program `hodge`** (University of South Carolina site), which credits Gerhardt and
  Schuster and cites Dewdney 1988 (page fetched).
- Hobby and blog implementations, e.g. John Hearn's Julia version (2019). Seen in search results only.

## People JETCARP should credit

Martin Gerhardt and Heike Schuster (the hodgepodge machine), with John J. Tyson (the follow-up
excitable-media CA); A. K. Dewdney (the 1988 column); Per Bak, Kan Chen and Chao Tang (the forest-fire
model, 1990); Barbara Drossel and Franz Schwabl (lightning and self-organised criticality, 1992);
C. L. Henley (an independent invention of the instant-burning limit).

## How JETCARP's version relates

`Hodgepodge <n>/<k1>/<k2>/<g>` takes the classic hodgepodge parameters, and `ForestFire <p>/<f>`
takes Drossel–Schwabl's growth and lightning probabilities. Both run on any tiling, and Hodgepodge
also runs on the GPU (README "## Rule notation", Phase 2 item 8). Hodgepodge rated among the best
families in the November 2026 exploratory search (README "## Exploratory search").

## Unverified

- Dewdney's August 1988 column and the Gerhardt–Schuster paper were not read. The meanings of k1, k2
  and g, and the exact update formulas, come from general knowledge of the rule and are not cited
  here. Read one of these sources before the docs state the formulas.
- Crossref's reference list for Gerhardt and Schuster (1989) gives the Dewdney citation with DOI
  10.1038/scientificamerican1288-116 (December 1988) but first page 86. That entry seems garbled. We
  take the August 1988 column (pp. 104–107) to be the hodgepodge column, from CelLab's and Burkardt's
  references.
- Henley 1989 (Bull. Am. Phys. Soc. 34:838) is an abstract and was not seen. It is known from the
  Clar–Drossel–Schwabl review only.
- Who first ran either model off the square grid. Triangular and hexagonal forest-fire variants appear
  in the physics literature (the review discusses changes of lattice symmetry), but none was fetched.

## Search log

- Crossref searches and DOI lookups: Gerhardt and Schuster 1989; Gerhardt, Schuster and Tyson 1990;
  Bak, Chen and Tang 1990; Chen, Bak and Jensen 1990; Drossel and Schwabl 1992; Henley 1993;
  Grassberger 2002; 10.1038/scientificamerican0888-104.
- WebSearch: `Dewdney "hodgepodge machine makes waves" Scientific American August 1988 …`;
  `Bak Chen Tang 1990 forest-fire model abstract … Drossel Schwabl added lightning f`.
- WebFetch: en.wikipedia.org/wiki/Forest-fire_model (pointer); journals.aps.org (403);
  arxiv.org/abs/cond-mat/9610201; ar5iv.labs.arxiv.org/html/cond-mat/9610201;
  people.math.sc.edu/Burkardt/c_src/hodge/hodge.html; fourmilab.ch/cellab/manual/rules.html and
  refs.html; softology.pro/voc.htm.
