# Novelty audit 09: exploratory search and the atlas (MAP-Elites with a human-rated interest score)

Audit date: 2026-11-10.

## What JETCARP does (README "## Exploratory search and the atlas", `crates/cart-core/src/explore/`)

- Each round picks a tiling, a rule family, a rule and a seed. A run is screened and its features are
  stored (settling time, early and late activity, density, structure, motion).
- A MAP-Elites archive keeps the best three per niche (family, tiling group, behaviour). Elites are
  confirmed on bigger grids with three seeds and scored by the worst seed.
- The score is a logistic fit to human keep/maybe/drop ratings of animated clips (four rounds, 1,043
  clips), with a term per family. It reached AUC 0.85 on a held-out round. 42% of top picks were kept,
  against 15% of ordinary surviving runs.
- First full search: 15.3 M runs over 1,216 tilings and 21 families. The atlas holds 8,260 entries
  over 942 tilings.

## Verdict: Related work only for the method (each component is established); the tiling dimension and the cross-family scope appear to be new

## Prior work (verified)

1. **Mouret, J.-B. and Clune, J., "Illuminating search spaces by mapping elites"**, arXiv:1504.04909
   (20 Apr 2015). MAP-Elites: "Many fields use search algorithms, which automatically explore a search
   space to find high-performing solutions" … The archive keeps the best solution in each cell of a
   behaviour-descriptor grid. *JETCARP's archive is MAP-Elites (three per niche).*
2. **Faldor, M. and Cully, A., "Toward Artificial Open-Ended Evolution within Lenia using
   Quality-Diversity"**, arXiv:2406.04235 (6 Jun 2024; ALIFE 2024; project page
   https://leniabreeder.github.io ). It applies MAP-Elites (with "manually defined diversity
   criteria") and AURORA (unsupervised descriptors) to Lenia. *This is the closest QD-on-CA precedent,
   but it covers one substrate and one rule family.*
3. **Reinke, C., Etcheverry, M. and Oudeyer, P.-Y., "Intrinsically Motivated Discovery of Diverse
   Patterns in Self-Organizing Systems"**, ICLR 2020, arXiv:1908.06663. POP-IMGEP on a continuous Game
   of Life (Lenia), with goal spaces learned by an autoencoder. The paper finds the learned space does "as
   well as" one built from expert-labelled patterns.
4. **Kumar, A., Lu, C., Kirsch, L., Tang, Y., Stanley, K. O., Isola, P. and Ha, D., "Automating the Search
   for Artificial Life with Foundation Models" (ASAL)**, arXiv:2412.17799 (23 Dec 2024; v2 16 May 2025).
   Vision-language foundation models are used to find target phenomena, open-ended novelty and
   "illumination" of diverse simulations. Substrates are Boids, Particle Life, Game of Life (Life-like
   rules), Lenia and NCA. *The foundation model stands in for human judgement.*
5. **Plantec, E. et al., "Flow-Lenia"**, arXiv:2212.07906 (2022/2023). **Chan, B. W.-C., "Lenia — Biology
   of Artificial Life"**, Complex Systems 28(3):251–286 (2019), arXiv:1812.05433. Over 400 species,
   many "discovered via interactive evolutionary computation".
6. **Human judgement in the loop:** Secretan, J., Stanley, K. O. et al., "Picbreeder: Evolving Pictures
   Collaboratively Online", CHI 2008, pp. 1759–1768, DOI 10.1145/1357054.1357328 (humans select
   directly; no learned model). Christiano, P. et al., "Deep reinforcement learning from human
   preferences", arXiv:1706.03741 (2017): a reward model fitted to human comparisons of short clips.
   This is the general pattern behind JETCARP's fitted score, at a much larger model scale.
7. **Human aesthetics of CA patterns:** Javaheri Javid, M. A., al-Rifaie, M. M. and Zimmer, R., "An
   Informational Model for Cellular Automata Aesthetic Measure", AISB 2015 Computational Creativity
   (https://research.gold.ac.uk/id/eprint/17258 ), and the companion "Correlation Between Human Aesthetic
   Judgement and Spatial Complexity Measure", LNCS 9596:79–91 (2016) (from a search summary, not
   fetched). These correlate complexity measures with human aesthetic judgements of CA images.

## How JETCARP differs

- **The tiling as a search dimension.** Prior CA QD and illumination work searches one substrate:
  a square grid, Lenia's continuous space, or a particle system. Edmond (2026) searched three aperiodic
  tilings for gliders, and Guttesen (2026) six Archimedean lattices, but with fixed objectives (gliders),
  not open-ended diversity. We found no prior work that explores tiling × rule family × seed jointly over
  ~1,200 tilings with a diversity archive niched by tiling group.
- **Cross-family scope.** The search spans 21 families (Life-like, Generations, GH, cyclic, Hodgepodge,
  LtL, Lenia, turmites, WireWorld, JACA, row and layered rules, …) in one archive. ASAL covers five
  substrates but searches each one separately.
- **Score.** A small, transparent logistic model over hand-designed run features, fitted to a few
  rounds of a single rater's keep/maybe/drop ratings of clips, with a per-family term. This differs from
  Picbreeder (direct human selection, no model), from ASAL (foundation-model embeddings, no human labels)
  and from IMGEP (learned goal spaces, no interest labels). Fitting a preference model to human clip
  ratings is a known general technique (Christiano et al. 2017), so the novelty is in applying it to CA
  exploration, not in the technique. The README's own caveat should stay: it finds runs worth looking
  at but ranks the top runs poorly.

## Suggested credit wording

> The exploratory search combines a MAP-Elites archive (Mouret and Clune 2015), as used for Lenia by
> Faldor and Cully (2024), with an interest score fitted to human keep/maybe/drop ratings of clips, in
> the spirit of learning from human preferences (Christiano et al. 2017) and complementary to IMGEP
> (Reinke et al. 2020) and foundation-model search (ASAL, Kumar et al. 2024). To our knowledge it is the
> first such search to treat the tiling itself as a search dimension, across about 1,200 periodic and
> aperiodic tilings and 21 rule families in one archive.

## Search log (2026-11-10)

- WebFetch arxiv.org/abs/1504.04909, /1908.06663, /2412.17799, /2212.07906, /1812.05433, /1706.03741.
- WebSearch: `Faldor Cully "Toward Artificial Open-Ended Evolution within Lenia using Quality-Diversity" Leniabreeder MAP-Elites arXiv`
- WebSearch: `cellular automata interestingness model trained on human ratings OR human preference judgments search rule space classifier`
- WebSearch: `Javaheri Javid al-Rifaie Zimmer "An informational model for cellular automata aesthetic measure" human aesthetic judgements`
- WebSearch: `Secretan Picbreeder "evolving pictures collaboratively online" CHI 2008 interactive evolution`

## Gaps / unverified leads

- Etcheverry et al. (HOLMES, NeurIPS 2020) and other IMGEP follow-ups on Lenia and Flow-Lenia were not
  checked.
- "Adaptive Exploration in Lenia with Intrinsic Multi-Objective Ranking" (arXiv:2506.02990; title seen in
  search results only) may be related.
- Wolfram's systematic rule-space enumerations (NKS) and Eppstein's surveys of Life-like rules are
  exhaustive searches rather than QD. They were not cited.
- No search was made for CA atlases or galleries across tilings (e.g. Tilings-CA websites or Ready
  galleries).
