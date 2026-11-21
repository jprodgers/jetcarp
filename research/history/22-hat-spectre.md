# 22. The hat and the spectre (aperiodic monotiles): history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.

## Short history

- **The einstein problem.** Is there a single tile that tiles the plane, but only non-periodically?
  ("einstein" means "one stone".) Earlier aperiodic sets needed many tiles (Berger 1966, 20,426 Wang
  tiles) or two (Penrose) [berger1966undecidability; file 21]. Socolar and Taylor (2011) gave a single
  regular hexagon that is aperiodic with added matching rules, as Smith et al. note
  [socolar2011aperiodic].
- **The hat (2022–2023).** David Smith, a hobbyist in Yorkshire, England, found the hat in autumn 2022
  ("last fall", Waterloo news, 13 Apr 2023), working largely by hand with the help of Jaap Scherphuis's
  PolyForm Puzzle Solver. He emailed Craig S. Kaplan to ask him to run the shape through his software
  [uwaterloo2023trick; smith2023aperiodic]. Smith, Joseph Samuel Myers (software developer), Kaplan
  (computer scientist, Waterloo) and Chaim Goodman-Strauss (mathematician) posted "An aperiodic
  monotile" on arXiv on 20 Mar 2023. It was published in *Combinatorial Theory* 4(1), 2024
  [smith2023aperiodic].
  - The hat is a polykite: eight kites of the Laves tiling [3.4.6.4], the dual of the 3.4.6.4 tiling.
    It belongs to a continuum Tile(a, b) (hat = Tile(1, √3); turtle = Tile(√3, 1)), all aperiodic
    apart from Tile(0,1) "chevron", Tile(1,0) "comet" and Tile(1,1).
  - Hat tilings need reflected hats. The reflected ones are sparse, and each sits in a "shell" of three
    unreflected hats.
  - The proof groups hats into four metatiles H, T, P and F with a substitution rule.
- **The spectre (May 2023).** In "A chiral aperiodic monotile" (arXiv 28 May 2023; *Combinatorial
  Theory* 4(2), 2024) the same four authors showed that the equilateral Tile(1,1) is a *weakly chiral*
  aperiodic monotile: it tiles only aperiodically if reflections are not allowed. Changing its edges
  gives the **Spectres**, *strictly chiral* aperiodic monotiles. They tile only aperiodically and use
  no reflections even when reflections are allowed. Their tilings follow a hierarchical substitution
  [smith2023chiral].
- **Software from the authors.** Kaplan's **hatviz** (p5.js app; GitHub repo created 23 Mar 2023;
  BSD-3-Clause) builds hat patches from the metatile substitution [kaplannd_hatviz]. Kaplan's hat page
  also has an H7/H8 substitution app and a Python version of the computer-assisted proof, and lists
  third-party tools: Dan Anderson's Mathigon Polypad environment and Christian Lawson-Perfect's outline
  files [kaplannd_hatpage]. The spectre page has an interactive app; its script `spectre.js` (fetched
  2026-11-10) has no licence or copyright line [kaplannd_spectrepage].
- **CAs on the hat and spectre** (from novelty-06 and -08): Andrew Trevorrow's **HatLife** script
  (ConwayLife forum, 11–16 May 2023: Life-like, Larger than Life-style, WireWorld and "Worms" rules on
  the hat) [trevorrow2023hatlife]; James Edmond's gliders on the hat and spectre from evolutionary
  search over priority tables (arXiv:2609.22579, 2026) [edmond2026gliders]; Murdoch Gabbay's "Aperiodic
  Life" (2026, not verified).

## Key primary citations

- [smith2023aperiodic] David Smith, Joseph Samuel Myers, Craig S. Kaplan and Chaim Goodman-Strauss, "An
  aperiodic monotile", *Combinatorial Theory* 4(1) (2024). DOI 10.5070/C64163843; arXiv:2303.10798 (v1
  20 Mar 2023, v3 3 Jul 2024). Verified: arXiv abstract page, arXiv HTML (introduction), Crossref.
- [smith2023chiral] Same authors, "A chiral aperiodic monotile", *Combinatorial Theory* 4(2) (2024).
  DOI 10.5070/C64264241; arXiv:2305.17743 (v1 28 May 2023, v2 28 Sep 2024). Verified: arXiv abstract
  page, Crossref.
- [kaplannd_hatviz] Craig S. Kaplan, *hatviz*, https://github.com/isohedral/hatviz (created 23 Mar
  2023; BSD-3-Clause). Verified: GitHub API; licence text matches the notice in JETCARP's `hat.rs`.
- [socolar2011aperiodic] Joshua E. S. Socolar and Joan M. Taylor, "An aperiodic hexagonal tile", *J.
  Combin. Theory Ser. A* 118(8):2207–2231 (2011). DOI 10.1016/j.jcta.2011.05.001. Verified: Crossref.
- [uwaterloo2023trick] Cheriton School of Computer Science, "Trick of the hat: finding the elusive
  einstein tile", news item, 13 Apr 2023,
  https://uwaterloo.ca/computer-science/news/trick-of-the-hat-finding-the-elusive-einstein-tile .
  Verified: page fetched.

## Software that ran it before JETCARP

- hatviz and the H7/H8 app (Kaplan, 2023); the spectre app and `spectre.js` (Kaplan, 2023).
- Mathigon Polypad hat environment (Dan Anderson), and Christian Lawson-Perfect's
  github.com/christianp/aperiodic-monotile outlines (listed on Kaplan's page; not opened).
- CAs: Trevorrow's HatLife (2023); Edmond's monotile-ca (2026, MIT OR Apache-2.0, per the provenance
  audit); Gabbay's Aperiodic Life (2026, not verified).

## People JETCARP should credit

**David Smith, Joseph Samuel Myers, Craig S. Kaplan and Chaim Goodman-Strauss** (both tiles). **Craig S.
Kaplan** specifically for hatviz (BSD-3, ported into `hat.rs`) and spectre.js (ported into `spectre.rs`;
no licence, so the provenance audit flags it). Andrew Trevorrow (first CAs on the hat) and James Edmond
(gliders on both tiles; JETCARP's `SpectreGlider` rule and evolutionary search follow his work).

## How JETCARP's version relates

`hat.rs` ports Kaplan's hatviz H/T/P/F metatile substitution; `spectre.rs` ports Kaplan's spectre
substitution (from `reference/core.js` and `spectre.mjs`). Both are built in exact Z[ζ₁₂] coordinates up
to level 8 (about 17–18 million tiles). Tests check that the hat : anti-hat ratio tends to φ⁴ (README
"Hat and Penrose tilings", "Exact aperiodic patches").

## Unverified

- The month of discovery (November 2022 in press reports found by search; the Waterloo item says only
  "last fall").
- Article numbers in *Combinatorial Theory* (Crossref gives volume and issue only; Kaplan's page says
  vol. 4 issue 1 for the hat, June 2024, and the spectre page says vol. 4 issue 2, September 2024).
- Kaplan's "recently published paper" and software that Smith used (not identified).
- Gabbay's Aperiodic Life (see novelty-06).

## Search log

- WebFetch: arxiv.org/abs/2303.10798, arxiv.org/abs/2305.17743, arxiv.org/html/2303.10798v3 (two parts),
  cs.uwaterloo.ca/~csk/hat/, cs.uwaterloo.ca/~csk/spectre/, uwaterloo.ca news item.
- Crossref API: 10.5070/C64163843, 10.5070/C64264241, 10.1016/j.jcta.2011.05.001, 10.1090/memo/0066.
- curl: cs.uwaterloo.ca/~csk/spectre/spectre.js (200, 16,709 bytes; no "licen"/"copyright" text);
  GitHub API isohedral/hatviz.
- WebSearch: Smith hat discovery date and press coverage.
- Failed: escholarship.org item page (empty), arXiv PDF (no text extractor installed).
- Reused (not refetched): HatLife, Edmond, Gabbay from novelty-06 and -08.
