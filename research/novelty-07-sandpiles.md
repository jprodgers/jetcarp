# Novelty audit 07: abelian sandpile identities and large piles on k-uniform, isohedral and aperiodic tilings

Audit date: 2026-11-10.

## What JETCARP does

- `sandpile.rs`: the identity of the sandpile group, stab(2δ − stab(2δ)) with δ the maximal stable
  configuration, on bounded patches with dead (sink) edges. It is tested with stab(I + I) = I on squares,
  hexagons, Cairo pentagons and the Spectre, and runs on every tiling (lattice, k-uniform up to k = 6,
  81 isohedral types, Penrose, hat, spectre).
- `pile/` ("## Large sandpiles"): single-source piles of N grains on an unbounded board by binary
  doubling (s ← stab(2s + bit·δ_0)), up to 10^8 grains on squares (GPU). It works on any periodic
  tiling and on exact aperiodic patches up to level 8 (about 17 M tiles), with edge or touching
  neighbours and cell, vertex or edge centres.

## Verdict

- **Identity on Penrose tilings: Prior art found** (Fersula, Noûs and Perrot 2020).
- **Identity on hexagonal and 4.8.8 (octagon–square) grids: Prior art found** (Johnson 2019 thesis).
- **Identity on the hat, spectre, k-uniform (k ≥ 2, apart from 4.8.8) and isohedral tilings: nothing
  found after search.**
- **Single-source piles:** standard on Z² (Ostojic 2003; many later papers), and studied on directed
  and other lattices (Dhar and co-authors). We found nothing on single-source piles on the hat or spectre,
  or on k-uniform and isohedral tilings. The doubling method follows directly from the abelian property
  and is presumably known; whether it has been published in this form was not established.

## Prior work (verified)

1. **Fersula, J., Noûs, C. and Perrot, K., "Sandpile toppling on Penrose tilings: identity and isotropic
   dynamics"**, arXiv:2006.06254 (11 Jun 2020, nlin.CG). A book-chapter version appears in a Springer
   volume (https://link.springer.com/chapter/10.1007/978-3-030-92551-2_10 ; the URL was seen in search
   results but the page was not fetched). "We present experiments of sandpiles on grids (square, triangular,
   hexagonal) and Penrose tilings." They present the identity elements of the sandpile group on these
   aperiodic structures, then the stabilisation of the maximum stable configuration plus the identity,
   which produces a near-circular shape.
   *Overlap:* the identity on Penrose tilings, and on square, triangular and hexagonal grids.
2. **Johnson, E. T., "The Sandpile Group on a Hexagonal Grid"**, Master's thesis, University of
   Washington, 2019
   (https://digital.lib.washington.edu/researchworks/items/90e4ec8f-3885-4618-a0f3-ad9d0b7deced ). It
   covers the recurrent identity on the hexagonal grid. According to the search summary of the PDF's list
   of figures (the PDF itself was not read), Fig. 1.1 shows "the recurrent identity element on the
   100 × 100 octagon square tessellation with global sink at the boundary".
3. **Caracciolo, S., Paoletti, G. and Sportiello, A., "Explicit characterization of the identity
   configuration in an Abelian Sandpile Model"**, arXiv:0809.3416 (2008). This is the square-lattice
   identity and the background reference (seen in search results; not fetched).
4. **Ostojic, S., "Patterns formed by addition of grains to only one site of an abelian sandpile"**,
   Physica A 318(1):187–199 (2003) (IDEAS record https://ideas.repec.org/a/eee/phsmap/v318y2003i1p187-199.html ;
   from the search summary: "adding N sand grains at a single site … on an infinite square lattice").
5. **Dhar, D., Sadhu, T. and Chandra, S., "Pattern formation in growing sandpiles"**, arXiv:0808.1732
   (2008; F-lattice and Manhattan directed lattices). Sadhu and Dhar, "Pattern formation in fast-growing
   sandpiles", arXiv:1109.2908 (directed triangular lattice). These are single-source piles beyond Z²
   (from search summaries; not fetched).

## How JETCARP differs

- Identity elements and single-source piles on the hat and spectre monotiles. We found no published
  sandpile work on either.
- Identity elements and piles across the whole k-uniform catalogue (k ≤ 6, 1,248 tilings) and the 81
  isohedral types, with edge or touching neighbourhoods.
- Very large single-source piles (10^8 grains) on non-square tilings, with CPU/GPU agreement and
  resumable doubling. This is an engineering contribution.

## Suggested credit wording

> Sandpile identities have been computed on Penrose tilings (Fersula, Noûs and Perrot 2020) and on the
> hexagonal and octagon–square grids (Johnson 2019); single-source piles go back to Ostojic (2003) and
> Dhar and co-authors. To our knowledge, JETCARP is the first to show sandpile identities and large
> single-source piles on the hat and spectre monotiles and across the k-uniform and isohedral tiling
> catalogues.

## Search log (2026-11-10)

- WebSearch: `abelian sandpile Penrose tiling OR quasicrystal OR "aperiodic" identity element OR avalanches`
- WebSearch: `sandpile identity element hexagonal triangular lattice OR "Archimedean" OR "planar graph" fractal identity pictures paper`
- WebSearch: `University of Washington thesis sandpile "recurrent identity" hexagonal grid octagon-square tessellation`
- WebSearch: `sandpile "hat" OR "spectre" monotile tiling toppling identity OR single source pile` (none)
- WebSearch: `single-source abelian sandpile pattern triangular hexagonal lattice Ostojic 2003 "addition of grains to only one site"`
- WebSearch: `"Pattern formation in growing sandpiles" Dhar Sadhu Chandra triangular hexagonal lattice single site`
- Fetched: arxiv.org/abs/2006.06254; digital.lib.washington.edu item page (Johnson 2019).

## Gaps / unverified leads

- The Springer venue of Fersula et al. (chapter DOI 10.1007/978-3-030-92551-2_10) was not fetched.
- Physics work on sandpile SOC on quasiperiodic lattices (e.g. Penrose or Ammann–Beenker BTW models) may
  exist and was not found.
- Sandpile pictures on Archimedean tilings in hobby or art sources (e.g. "sandpile identity" galleries)
  were not searched.
- Whether the doubling algorithm stab(2·stab(x)) = stab(2x) for large piles has been published (it is
  a standard abelian-property trick) was not checked.
