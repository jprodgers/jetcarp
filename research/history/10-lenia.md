# 10. Lenia: history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.

## Short history

- **Lenia.** Bert Wang-Chak Chan introduced Lenia in "Lenia — Biology of Artificial Life"
  (arXiv:1812.05433, v1 13 Dec 2018; *Complex Systems* 28(3):251–286, 2019) [chan2019lenia]. The paper
  defines Lenia as a 2-D CA with continuous space, time and state and a generalised local rule. It
  reports over 400 "species" in 18 families, gives them Linnaean-style names (e.g. *Orbium*,
  *Scutium*), and studies their morphology and behaviour like a biologist.
- **Precursors named by Chan.** In the paper's comparison table (Table 1), Chan places Lenia at the end
  of a sequence of generalisations of Life:
  - Life [gardner1970life] and the elementary CAs [wolfram1983statistical].
  - "Primordia", Chan's own multi-state JavaScript/HTML program with extended birth and survival
    intervals, "circa 2005".
  - Larger than Life [evans2001larger; evans2003larger; see file 03].
  - Pivato's RealLife, the continuum limit of LtL [pivato2007reallife].
  - Rafler's SmoothLife [rafler2011generalization].
  Chan writes that SmoothLife and Lenia are "independent developments" that look strikingly alike,
  which he calls "convergent evolution" [chan2019lenia, Table 1 note]. He proposes a "phylogeny of
  the glider": Life's glider, then LtL's "bug with stomach", then SmoothLife's smooth glider, then
  Lenia's *Scutium* [chan2019lenia, sec. 3.2.1]. He also cites MacLennan's 1990 "continuous spatial
  automata" technical report for the continuous limit (not fetched).
- **SmoothLife, 2011.** Stephan Rafler's "Generalization of Conway's 'Game of Life' to a continuous
  domain — SmoothLife" (arXiv:1111.1567, 7 Nov 2011, v2 7 Dec 2011) gave a continuous-domain Life
  with an inner disc and an outer annulus, and an implementation [rafler2011generalization]. Its
  "smooth glider" was the first glider that moves in any direction [chan2019lenia, sec. 3.2.1].
- **Extensions by Chan and others.**
  - "Lenia and Expanded Universe" (ALIFE 2020) adds higher dimensions, several kernels and several
    channels. Chan likens the result to a recurrent convolutional neural network
    [chan2020expanded].
  - Flow-Lenia (Plantec, Hamon, Etcheverry, Oudeyer, Moulin-Frier and Chan, arXiv:2212.07906) makes
    Lenia conserve mass and lets the rule's parameters vary across space [plantec2022flowlenia].
  - Lenia became a standard testbed for automated discovery: IMGEP (Reinke et al., ICLR 2020),
    MAP-Elites (Faldor and Cully 2024) and ASAL (Kumar et al. 2024). See novelty-09.

## Key primary citations

- [chan2019lenia] Chan, B. W.-C., "Lenia: Biology of Artificial Life", *Complex Systems*
  28(3):251–286 (2019). DOI 10.25088/ComplexSystems.28.3.251; arXiv:1812.05433. Verified: arXiv
  abstract page, Crossref record, and the PDF text (Table 1, sec. 3.2.1, references) extracted
  locally.
- [chan2020expanded] Chan, B. W.-C., "Lenia and Expanded Universe", *ALIFE 2020: The 2020 Conference on
  Artificial Life*, MIT Press, pp. 221–229 (2020). DOI 10.1162/isal_a_00297; arXiv:2005.03742.
  Verified: arXiv abstract page and Crossref record.
- [rafler2011generalization] Rafler, S., "Generalization of Conway's 'Game of Life' to a continuous
  domain — SmoothLife", arXiv:1111.1567 (2011). Verified: arXiv abstract page.
- [pivato2007reallife] Pivato, M., "RealLife: the continuum limit of Larger than Life cellular
  automata", *Theoretical Computer Science* 372(1):46–68 (2007). DOI 10.1016/j.tcs.2006.11.019.
  Verified: Crossref record.
- [plantec2022flowlenia] Plantec, E., Hamon, G., Etcheverry, M., Oudeyer, P.-Y., Moulin-Frier, C. and
  Chan, B. W.-C., "Flow-Lenia: Towards open-ended evolution in cellular automata through mass
  conservation and parameter localization", arXiv:2212.07906 (2022). Verified: arXiv abstract page.
- [chakazulnd_lenia] Chan, B. W.-C., *Lenia* source code (Python, JavaScript, Matlab and others), MIT
  licence. https://github.com/Chakazul/Lenia . Verified: repository page fetched.

## Software that ran it before JETCARP

- Chan's own implementations in the Chakazul/Lenia repository: JavaScript (the original), Python with
  GPU support, Matlab, Jupyter, R and Android [chakazulnd_lenia].
- Rafler's SmoothLife implementation (described in the arXiv paper). Chan thanks Mikola Lysenko for an
  FFT implementation of SmoothLife, and cites Tim Hutton's 2012 SmoothLifeL glider video
  [chan2019lenia, acknowledgements and refs 31, 85].
- Ready (GollyGang) explores "continuous and discrete cellular automata". Its README does not mention
  SmoothLife or Lenia by name, so support for them is not confirmed.

## People JETCARP should credit

Bert Wang-Chak Chan (Lenia, the parameters and kernels JETCARP uses, and the *Orbium* data from
`animals.json`, MIT licence); Stephan Rafler (SmoothLife); Marcus Pivato (RealLife); Kellie Evans and
David Griffeath (Larger than Life, the discrete ancestor).

## How JETCARP's version relates

JETCARP follows Chan's paper (equations 10–13) and code numbering for kernel cores and growth
functions. Its *Orbium unicaudatus* comes from Chan's `animals.json` (MIT). It adds portable
deterministic maths so results hash identically on every OS, and runs Lenia on every periodic and
aperiodic tiling using distances between centroids (README "### Lenia (continuous)").

## Unverified

- The year Chan started Lenia (often given as 2015) and the origin of the name. Neither was found in
  the extracted text of the paper or the repository README.
- MacLennan, B., *Continuous spatial automata*, Tech. Rep. CS-90-121, University of Tennessee (1990).
  Known only from Chan's reference [23].
- Flow-Lenia's publication venue (often given as ALIFE 2023); the arXiv page lists none.
- Whether Ready ships SmoothLife or Lenia examples.

## Search log

- Fetched: arxiv.org/abs/1812.05433, arxiv.org/pdf/1812.05433v3 (text extracted with a
  local script), arxiv.org/abs/1111.1567, arxiv.org/abs/2005.03742,
  arxiv.org/abs/2212.07906, github.com/Chakazul/Lenia, github.com/GollyGang/ready.
- Crossref API (curl): 10.25088/ComplexSystems.28.3.251, 10.1162/isal_a_00297,
  10.1016/j.tcs.2006.11.019.
