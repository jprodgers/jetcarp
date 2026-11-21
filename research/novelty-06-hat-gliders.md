# Novelty audit 06: hat gliders from JETCARP's evolutionary search

Audit date: 2026-11-10.

## What JETCARP has

README "### Movers on aperiodic tilings (verified to a distance)": hat gliders found by `cart evolve`,
which reimplements Edmond's priority-table genetic search. Example:
`Priority 4: 3|n2>=1->2; 0|n3>=2->0; 0|n2>=1->3; 0|n1>=1->2` (first match wins, default 0), seeded
with tiles 0 (state 1) and 3 (state 2) of hat level 4. It verifies to 2,075 rings (8,867 units) over
4,155 generations on levels 4–8 (and to a million rings on a sliding window). Population is 2–8: a
5-cell glider heading 106–107°, at about 2 generations per ring.

## Verdict: Prior art found (the method, the glider class and the corridor). This specific table is not in Edmond's records, but it is a near-relabelling of Edmond's "hat glider B"

## Prior work (verified)

1. **Edmond, J., "Gliders on Aperiodic Monotilings: Cellular Automata on the Hat and Spectre"**,
   arXiv:2609.22579 (v1 18 Sep 2026, v2 19 Oct 2026; nlin.CG). https://arxiv.org/abs/2609.22579 ;
   code https://github.com/jamesedmond/monotile-ca ; Zenodo DOI 10.5281/zenodo.22836247; essay
   https://offlattice.org/monotile/essay/ . From the abstract: edge-adjacency semi-totalistic searches
   find only mortal travellers. The rule space is extended to vertex neighbourhoods and priority tables,
   and an "evolutionary search finds gliders on both monotilings that travel a million rings at
   constant speed and heading", with headings "quantized onto a six-spoke compass".
2. **Edmond's glider compendium**, `GLIDERS.md` in the repository (downloaded 2026-11-10). Hat gliders
   A (`hat-tableevolve-r48-s21`), B (`-s22`) and C (`-s23`, three gliders). All are 4-state priority
   tables on the vertex neighbourhood, at 2.0 generations per ring. A and B head 226.6°; C's settle at
   345.8°, 103.1° and 104.8°. "Hat B is hat A's mechanism in another alphabet". "Every hat object shares
   hat A's cocycle numbers: the corridor is the tiling's." The compendium also records
   **certificates of immortality** for every settled hat glider ("a finite set of situations … closed
   under the rule on every legal environment of the tiling, which proves the object immortal"), credited
   to a companion paper "*Certificates of Immortality for Gliders on the Hat and Spectre Tilings*, 2026".
   That paper was not found on arXiv by search, so it is unverified as a publication. The certificate
   files are in `results/certificates/`.
3. **Trevorrow, A., HatLife** (Lua/Glu script), ConwayLife.com forum thread "HatLife" (t=6015),
   posts of 11–16 May 2023 (https://conwaylife.com/forums/viewtopic.php?p=161425 ; read through the
   njohnston.ca mirror). It has edge or edge+vertex neighbourhoods, BSN, Larger than Life, WireWorld
   and "Worms" rules. Version 2 (16 May 2023) added Worms, designed to send chains between rows of
   reflected hats. Edmond's compendium certifies "Trevorrow's worm" as a glider on the same corridor.
   On 18 Oct 2026 Trevorrow posted Edmond's EdmondA/EdmondB gliders in HatLife.
4. **Gabbay, M., "Aperiodic Life"** (release 3.7.x, October 2026): per Edmond's compendium, Gabbay's
   own evolutionary search found the four-row table `TABLE:HAT-WAKE` (a "wake traveller" heading 345°).
   This is a second, independent evolutionary search on the hat. We could not verify the software
   independently. A web search found only a Heriot-Watt student project proposal supervised by "Jamie
   Gabbay" (https://projects.hw.ac.uk/Proposals/Details/B4ZM6EH3FM6W ).

## Does Edmond's work list JETCARP's rule? (checked)

All 19 4-state `table_rule` records in Edmond's `results/*.jsonl` (gallery, the hat and spectre
tableevolve, lanechange, ignition examples, worms) were downloaded. Each was compared with the JETCARP
rule as a function of (own state, n0, n1, n2, n3), for all neighbour counts up to 14, under every
relabelling of states 1–3 (a short local script over Edmond's results/*.jsonl, not published). **None is identical.** The closest
is **hat glider B (s22)** under the relabelling 1→3, 2→1, 3→2. It then reads
`3|n2>=1->2; 0|n3>=2&n0>=1->0; 0|n0>=2&n1>=2->3; 0|n0>=2&n2>=1->3`. Three of JETCARP's four rows
agree apart from the extra n0 (empty-neighbour) conditions. The difference: JETCARP sends an empty cell
with a state-1 neighbour to 2, where B sends it to 3 when n1 ≥ 2. The functions differ on 561 of about
12,000 (own, counts) inputs. Many of those inputs may never occur along the glider's path. The README's
glider (2 generations per ring, 5 cells, heading 106–107°, which is the 105° spoke of Edmond's
compass) fits Edmond's universal hat corridor. **It is very probably the same mechanism (the A/B relay)
in another table.** This was not tested by simulation, because no CPU jobs were run for this audit.
Trevorrow's worm table and Gabbay's table are not written out in the compendium. Gabbay's
(`results/rules/hat-wake-gabbay.json`) has different rows: `1|n0>=3&n2>=1->2; 0|n0>=4&n2>=1->1;
1|n0>=4->3; 0|n1>=1&n2>=1->3`.

## How JETCARP differs

- The specific table and seed are not in Edmond's published records.
- JETCARP's verification is by nested exact patches (hat/Penrose size classes, Spectre Δ embedding) up
  to level 8, plus a sliding window. It reports "verified to distance D", not immortality. **Edmond's
  certificates are stronger.** The JETCARP docs should say that its result is weaker than a proof and
  point to Edmond's certificates.
- JETCARP's reproduction of the Spectre glider ("as published") is a replication, not a discovery.

## Suggested credit wording

> JETCARP's glider search reimplements Edmond's evolutionary search over 4-state priority tables on the
> vertex neighbourhood (Edmond 2026, arXiv:2609.22579). The hat gliders it finds, such as `Priority 4:
> 3|n2>=1->2; 0|n3>=2->0; 0|n2>=1->3; 0|n1>=1->2`, are new tables but appear to use the same
> corridor mechanism as Edmond's hat gliders A and B (this rule is close to glider B up to a relabelling
> of states). Earlier hat movers include Trevorrow's HatLife worms (2023) and Gabbay's wake traveller
> (2026). JETCARP verifies gliders to a stated distance on nested exact patches. Edmond's certificates
> prove immortality.

Do not claim "new hat glider" without first running the JETCARP rule and Edmond's B side by side and
comparing the orbit of situations. If they match, call it "a re-discovery of Edmond's hat glider in a
different table".

## Search log (2026-11-10)

- WebFetch arxiv.org/abs/2609.22579, arxiv.org/html/2609.22579v2 (related work), github.com/jamesedmond/monotile-ca,
  raw GLIDERS.md, results/ listing, results/rules/hat-wake-gabbay.json.
- curl (raw.githubusercontent.com) of 11 results/*.jsonl files plus GLIDERS.md into a scratch folder;
  a local comparison script (not published).
- WebSearch: `Trevorrow HatLife hat tiling Life app May 2023 SpectreLife`; `"Certificates of Immortality for Gliders on the Hat and Spectre Tilings" Edmond arXiv` (not found);
  `Murdoch Gabbay "Aperiodic Life" cellular automata hat spectre Penrose app` (not found).
- WebFetch conwaylife.njohnston.ca/forums/viewtopic.php?p=161425 (first page of the HatLife thread only).

## Gaps

- The results listing may be incomplete ("View all files"), and `results/certificates/*.json` and
  `results/flights/` were not searched for further rules.
- The paper's own tables (e.g. the ablation atlas) were not read in full for further hat rules.
- JETCARP's other `cart evolve` hat and spectre gliders (not named in the README) were not compared.
  Run the same comparison on the full JETCARP mover table.
