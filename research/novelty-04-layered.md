# Novelty audit 04: layered rules (Wolfram-style totalistic rules on graph-distance layers)

Audit date: 2026-11-10.

## What JETCARP does

`Rule::Layered` (`crates/cart-core/src/rules.rs`, notation `Layer <states>/<outputs by parent sum>`):
layers are graph distance from the seed cell(s) in the tiling's neighbour graph. Each step writes the
next layer. A cell's new state is `digits[sum]`, where `sum` is the total state of its parents, its
neighbours in the previous layer. The digit list repeats for larger sums. It runs on any tiling,
including k-uniform, isohedral and aperiodic tilings (Penrose, hat, spectre). The test is that
parity layers on squares give Pascal's triangle mod 2. In the November 2026 exploratory search,
layered rules grown from one cell rated better than those from random starts.

## Verdict: Nothing found after search (for the exact construction); related work only

We found no prior source that defines a one-dimensional-style (Wolfram) totalistic rule whose "rows"
are the breadth-first distance shells of an arbitrary tiling's neighbour graph, or that runs such
rules on Penrose, hat or spectre tilings. The ingredients are standard: totalistic rules,
Pascal-mod-p rules, embedding 1-D CAs into other tilings, CAs on Penrose tilings, and polar/ring
renderings of ECAs. Confidence is moderate. The construction is simple enough that it may exist
unpublished in hobby code or in Wolfram's network-system or physics-project material, which we did
not search exhaustively.

## Closest related work (verified)

1. **Margenstern, M., "About the embedding of one dimensional cellular automata into hyperbolic
   cellular automata"**, arXiv:1004.1830 (11 Apr 2010). The paper embeds deterministic 1-D CAs into
   CAs on the hyperbolic pentagrid {5,4}, heptagrid {7,3} and dodecagrid {5,3,4}. *Related:* it carries
   1-D rules into non-Euclidean tilings, but along a chosen line of tiles, not by distance shells from
   a seed.
2. **Kerekrety, M., "Elementary cellular automaton in Hexagonal, Pentagonal and Other Tessellations"**,
   2020, https://matejker.github.io/elementary-cellular-automaton/ . The 256 ECA rules (16 on hexagons)
   drawn row by row on squares, hexagons and pentagons. *Related:* rows are fixed lattice rows, not
   graph-distance layers, and only periodic tilings are covered.
3. **Polar and ring renderings of ECAs:** the Wolfram Demonstrations notebook "Periodic Boundary
   Condition" (https://www.wolframcloud.com/objects/demonstrations/PeriodicBoundaryCondition-source.nb ;
   from the search summary: evolution "starts in the innermost ring with a single black cell", on
   concentric polygons), and the R package `cellularautomata` (CRAN vignette "Basic usage", which
   offers `circle = TRUE` with `time_flow = "up"`). *Related:* the spatial layout changes, but
   the rule is still a fixed-width 1-D ECA.
4. **García-Morales, V., "Mathematical Physics of Cellular Automata"**, arXiv:1203.3939, which treats
   totalistic rules that reproduce Pascal simplices mod p ("when ρ = 2, the Pascal simplex coincides
   with the Pascal triangle"; from the search summary, not fetched). *Background for `Layer 2/01` and
   `Layer 3/012`.*
5. CAs on Penrose tilings exist (Goucher 2012, arXiv:1208.2771; Owens and Stepney 2010; Fernique et al.
   2017, arXiv:1708.09301; see novelty-05 and novelty-08), but all are 2-D neighbourhood rules,
   not layered row rules.

## How JETCARP differs

- Rows are BFS shells of the tiling's neighbour graph, so the same rule notation runs on every
  periodic and aperiodic tiling. Parent counts vary from cell to cell, and the totalistic form keeps
  the rule well-defined.
- Parity layers (`Layer 2/01`) reproduce Pascal's triangle mod 2 on squares and give generalised
  "Pascal" patterns on Penrose, hat and spectre graphs.

## Suggested credit wording

> To our knowledge, layered rules are a new presentation: Wolfram-style totalistic rules whose rows
> are the graph-distance shells around a seed, so that they run unchanged on any tiling, including
> aperiodic ones. They build on Wolfram's totalistic rules and on Pascal-mod-p automata, and are
> related to embeddings of 1-D CAs in other tilings (e.g. Margenstern 2010).

Keep the claim modest ("a construction we have not seen published"). It is a natural idea.

## Search log (2026-11-10)

- WebFetch arxiv.org/abs/2308.06370 (Dalai, "Layered Cellular Automata", 2023: two coupled computation
  layers; unrelated despite the name, so do not confuse the two. **Note the name clash:** JETCARP's
  "Layered rules" share a name with Dalai's "Layered Cellular Automata" (LCA), so consider
  "distance-layer rules" or "shell rules" to avoid confusion.)
- WebSearch: `cellular automaton Penrose tiling "Pascal's triangle" OR "rule 90" OR "parity" distance from seed layers graph Sierpinski aperiodic`
- WebSearch: `"cellular automata" "aperiodic tiling" OR "Penrose tiling" elementary rule "one-dimensional" generalization rows "distance" seed hat tiling Wolfram-style`
- WebSearch: `totalistic cellular automaton on arbitrary graph breadth-first layers from seed parents in previous layer generalized Pascal triangle mod 2 graph`
- WebSearch: `elementary cellular automaton drawn in concentric rings radial growth each ring next generation polar "rule 30" circle OR hyperbolic tiling rings`
- WebFetch arxiv.org/abs/1004.1830.

## Gaps / unverified leads

- Wolfram NKS ch. 5 "Network systems" and the Wolfram Physics Project (rules on graphs, distance
  shells, causal graphs) were not checked in detail. They are the most likely place for a close
  precedent.
- Ulam–Warburton and toothpick growth automata on non-square grids (OEIS) are growth-from-seed rules
  on graphs and loosely related. They were not checked.
- "Additive cellular automata on graphs" and Steinhaus/Pascal triangles on graphs (e.g. arXiv:1402.4912,
  seen in a search summary only) were not fetched.
