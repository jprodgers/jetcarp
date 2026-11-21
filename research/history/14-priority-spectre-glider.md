# 14. Priority-table rules and the Spectre glider: history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.
Builds on novelty-06 (hat gliders), which holds the full search log for Edmond's work.

## Short history

- **Gliders on aperiodic tilings before 2026.**
  - Life rules on Penrose tilings were studied for still lifes and oscillators. Hill, Stepney and Wan
    (2005) and Owens and Stepney (2010) found no moving objects [hill2005penrose; owens2010game].
  - Adam P. Goucher found the first glider on an aperiodic tiling in 2012: a four-state
    outer-totalistic rule on a Penrose tiling [goucher2012gliders]. Edmond's paper says it
    overturned a conjecture, backed by a small cash prize, that no such object exists, and
    attributes the conjecture to a 2012 mailing-list post by Trevorrow [edmond2026gliders].
  - After the hat was announced in 2023, Andrew Trevorrow's HatLife let people explore rules on the
    hat interactively. It included a hand-designed "Worms" rule whose worms cross the patch
    [trevorrow2023hatlife]. Edmond also mentions Trevorrow's SpectreLife.
- **Edmond, 2026: priority tables.** James Edmond's "Gliders on Aperiodic Monotilings: Cellular
  Automata on the Hat and Spectre" (arXiv:2609.22579, v1 18 Sep 2026, v2 19 Oct 2026) found that
  edge-adjacency semi-totalistic searches turn up only mortal travellers. He therefore widened the
  rule space to vertex neighbourhoods and *priority tables* [edmond2026gliders].
  - A priority table is an ordered list of rows. Each row has an own state (or a wildcard), a
    conjunction of per-state neighbour-count thresholds, and a next state. The first matching row
    fires, and a cell that matches no row becomes quiescent.
  - The paper presents this family as its own and notes that it contains the Generations rules. No
    earlier source for the representation is credited.
  - A genetic algorithm over four-state tables on the vertex neighbourhood found gliders on both
    monotilings. They travel a million rings at constant speed and heading, and their headings fall
    on a "six-spoke compass" [edmond2026gliders].
- **The Spectre glider.** This is Edmond's record `spectre-tableevolve-r48-s33` (Table 7 of v2). It is
  a three-state relay that never uses state 2. A head (state 1) excites wings (state 3), a wing next
  to a head becomes a head, and crowded wings die. From the seed (1, 3) it emits a pair of gliders on
  lanes 120° apart, moving at two generations per ring. That is the same clock as the hat gliders
  and Goucher's Penrose glider. The search found it with no known rule planted
  [edmond2026gliders].
- **Later.** Edmond's repository records certificates of immortality for its gliders, attributed to a
  companion paper. It also records Murdoch Gabbay's independent evolutionary hat traveller in
  "Aperiodic Life" (October 2026). Neither was verified as a publication (see novelty-06).

## Key primary citations

- [edmond2026gliders] Edmond, J., "Gliders on Aperiodic Monotilings: Cellular Automata on the Hat and
  Spectre", arXiv:2609.22579 (v1 18 Sep 2026; v2 19 Oct 2026). Code
  https://github.com/jamesedmond/monotile-ca (MIT OR Apache-2.0, per the provenance audit); Zenodo
  DOI 10.5281/zenodo.22836247. Verified: arXiv abstract page and HTML v2 (definitions, Table 7,
  related work), fetched 2026-11-10.
- [goucher2012gliders] Goucher, A. P., "Gliders in Cellular Automata on Penrose Tilings", *Journal of
  Cellular Automata* 7(5–6):385–392 (2012). Verified: Old City Publishing contents page (novelty-05).
- [owens2010game] Owens, N. and Stepney, S., "The Game of Life rules on Penrose tilings: still life and
  oscillators", in Adamatzky (ed.), *Game of Life Cellular Automata*, Springer, pp. 331–378 (2010).
  DOI 10.1007/978-1-84996-217-9_18. Verified: Crossref (17–22 agent).
- [hill2005penrose] Hill, M., Stepney, S. and Wan, F., "Penrose Life: Ash and Oscillators", ECAL 2005,
  LNCS 3630, pp. 471–480. DOI 10.1007/11553090_48. Verified: Crossref (17–22 agent).
- [trevorrow2023hatlife] Trevorrow, A., HatLife, ConwayLife.com forum thread t=6015, May 2023.
  Verified: read through the conwaylife.njohnston.ca mirror (novelty-06).

## Software that ran it before JETCARP

- Edmond's `monotile-ca`, a no_std Rust graph CA for Generations and priority-table rules on the hat,
  spectre and Penrose tilings, with its `evolve_table` search [edmond2026gliders].
- HatLife (Trevorrow, 2023), which later ran Edmond's gliders as EdmondA/B/C (forum post of 18 Oct
  2026, novelty-06).
- Ready, which Edmond used to check Goucher's Penrose glider table (novelty-08).
- Gabbay's "Aperiodic Life" (unverified).

## People JETCARP should credit

James Edmond (the priority-table family, the evolutionary method and the Spectre glider); Adam P.
Goucher (first aperiodic glider); Andrew Trevorrow (HatLife, SpectreLife); Nick Owens, Susan Stepney,
Margaret Hill and Francis Wan (Life on Penrose tilings); Murdoch Gabbay (independent hat traveller,
unverified).

## How JETCARP's version relates

`SpectreGlider` is exactly `Priority 4: 0|n0>=3&n1>=1->3; 3|n3>=3->0; 3|n1>=1->1`. These are the
three live rows of Edmond's Table 7; the other rows send cells to 0, which is the default. `cart
evolve` reimplements Edmond's search "from the paper and the repository's description (no code
copied)" and extends it to every tiling (README "## Evolutionary glider search"). The provenance
audit flags that `priority.rs notation()` and the mutation code were adapted from monotile-ca, so the
README's "no code was copied" needs correcting and the MIT/Apache notice should be carried.
JETCARP verifies movers to a stated distance. Edmond's certificates prove immortality (novelty-06).

## Unverified

- Edmond's companion paper "Certificates of Immortality for Gliders on the Hat and Spectre Tilings"
  (2026). It is named in the repository and was not found on arXiv.
- Gabbay's "Aperiodic Life" app, release 3.7.x (October 2026).
- The 2012 mailing-list conjecture and prize. Only Edmond's description was read, not the original
  post.
- Trevorrow's SpectreLife (date and features).

## Search log

- Fetched: arxiv.org/html/2609.22579v2 (definitions, Spectre glider, related work), 2026-11-10.
- Earlier fetches for this topic are in novelty-06 (arXiv abstract, repository, GLIDERS.md, results
  files, HatLife thread) and novelty-05 (Goucher, Owens and Stepney).
