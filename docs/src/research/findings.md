# Findings

This page collects what JETCARP's searches have found and what the checks of November 2026 made of
it. It is the data behind the claims on [What is new in JETCARP](contributions.md): every result was
run again on the release engine (JETCARP 0.4.0), every spaceship was compared with the ships of
simpler tilings, and the list was cut down wherever a check failed. The ConwayLife.com forums have not
been searched yet, so a candidate here is a candidate, not a discovery; we will ask the community when
the results are posted there. Corrections are welcome as issues.

## Re-verification on the release engine

The finds were made with JETCARP's predecessor, CART. Before anything was claimed, the records were run
again on JETCARP 0.4.0:

| Records | Checked | Passed |
| --- | --- | --- |
| Spaceships | 103 | 103 |
| Long-period oscillators (the 40 longest per tiling family) | 739 | 739 |
| Turmite highways on non-square tilings | 39,744 | 39,744 |

Spaceships and oscillators were checked twice: by `jetcarp library verify --all`, and by pasting each
record alone on a fresh wrapped patch and letting JETCARP's object finder name it again after 200
generations. No period, displacement or speed changed. Every highway gave the same onset, period, cells
and speed with its stored limits.

## Isohedral tilings are eleven neighbour graphs

A rule that only counts neighbours sees the neighbour graph, not the shapes (see
[Tilings as neighbour graphs](../tilings/neighbour-graphs.md)). Every isohedral tiling is
combinatorially one of the eleven **Laves tilings**, the duals of the uniform tilings, whatever the
shape of its tile, so JETCARP's 81 isohedral types give rules only eleven different worlds in each
neighbourhood. A survey of all 362 tiling and neighbourhood pairs (19 families, the 81 isohedral types
and the 81 catalogued 2- and 3-uniform tilings, with touching and with edge neighbours) found:

| Laves tiling | Isohedral types in JETCARP | Neighbour graph is that of |
| --- | --- | --- |
| [3<sup>6</sup>], hexagon-like | IH01–IH18, IH20 (19 types) | the hexagonal grid |
| [4<sup>4</sup>], square-like | 30 types from IH41 to IH76 | the square grid |
| [6<sup>3</sup>], triangle-like | IH83–IH86, IH88, IH90, IH91, IH93 (8) | the triangular grid |
| [3<sup>2</sup>.4.3.4], Cairo pentagonal | IH27–IH29 (3) | the Cairo tiling |
| [3<sup>3</sup>.4<sup>2</sup>], prismatic pentagonal | IH22–IH26 (5) | itself |
| [3<sup>4</sup>.6], floret pentagonal | IH21 | itself |
| [3.6.3.6], rhombille | IH33, IH34, IH36, IH37 (4) | itself |
| [3.4.6.4], deltoidal trihexagonal | IH30–IH32 (3) | itself |
| [3.12<sup>2</sup>], triakis triangular | IH38–IH40 (3) | itself |
| [4.8<sup>2</sup>], tetrakis square | IH78, IH79, IH81, IH82 (4) | itself |
| [4.6.12], kisrhombille | IH77 | itself |

Several drawn families are also lattices in disguise: the brick, herringbone and three-fold (lizard)
tilings behave exactly as hexagons, the quadrilateral, glide and four-fold tilings as squares, and the
Pythagorean tiling as 4.8.8. No 2- or 3-uniform tiling has the graph of a lattice, of Cairo or of an
Archimedean tiling. The [isohedral tilings](../tilings/isohedral.md) page therefore presents results on
an isohedral type as results on its Laves tiling.

The survey also explains an early false alarm. JETCARP's catalogue held "spaceships" on IH46, IH54,
IH62, IH72 and IH84; mapped back onto their lattices they are Conway's glider, the lightweight and
middleweight spaceships, and an unnamed ship of the triangular grid. Every ship found on an isohedral
type is a known lattice object, and none is claimed.

## Spaceships

### How the list was cut

Of the 103 spaceship records, 35 were known objects of the square and triangular grids (every ship
on an isohedral type among them) and 5 were H. S. Guttesen's kagome glider, alone, in
pairs or in corridors of kagome tiles. Fresh soup searches in November 2026 re-found Guttesen's
snub-square glider and added 16 objects. That left 72 candidates, which a second, deliberately
skeptical pass then examined:

| Step | Result |
| --- | --- |
| Candidates examined | 72 |
| Compounds: two smaller ships flying side by side without influencing each other | 10, removed |
| The same object as another record | none |
| Re-attributed to a simpler tiling (7 from 3-uniform to 2-uniform tilings, 1 from k3-032 to 3.6.3.6) | 8 |
| Objects that run on more than one tiling | 16 |
| **Distinct surviving objects** | **62** |
| of which single ships (6 of them contain a perturbed phase of a smaller ship) | 49 |
| of which derivative: a smaller ship with a passenger that it pulls along unchanged, or a bound pair of gliders | 13 |
| Flag A: most likely already known or noticed by others | 9 |
| Flag B: check before claiming | 13 |
| Unflagged | 40, of which 33 are single ships |

The checks are described in the box on [What is new in JETCARP](contributions.md#how-we-checked).
No ship depended on dead edges or on the size of the patch, no period or displacement was misreported,
and no candidate is a ship of a lattice, of Cairo or of any Laves tiling.

**Flag A** marks ships that are small, turn up often in soups and run in many rules on an Archimedean
tiling (`69c9912a`, `938f9833`, `c78ceb01`, `b2c790bb`, `1723afa2`, `8aa7e24c`), and the ships built on
Guttesen's gliders (`2ba58828`, `98a3c53b` in Life on 3.3.4.3.4, and `ea5d187b`). Others may well have
seen these; we found no report, and do not write "to our knowledge" for them. **Flag B** marks ships
that run in a well-known rule on an Archimedean tiling (HighLife, Pedestrian Life, or the
`B2/S34` rule of the 4.8.8 hunts, which is Hexagonal Life without its `H`), that contain a phase of one
of Guttesen's gliders, or that were very common in their search.

Life itself (`B3/S23`), the rule Guttesen searched, deserves a separate word. All eleven candidates
that run in Life turned out to be made of Guttesen's gliders: on 3.6.3.6 every one is a pair of his
glider g, so no new single Life ship on the kagome tiling survives; on 3.3.4.3.4 one is a pair of his
snub-square glider, one is a bound pair of it (`2ba58828`, period 16) and one contains a perturbed copy
of it (`98a3c53b`, period 24).

### The 62 objects

Sorted by the simplest tiling each one runs on: Archimedean, then 2-uniform, then 3-uniform. *Runs
on* lists every tiling where a map that keeps all neighbour counts carries the ship and JETCARP
confirmed it as one ship with the same period. *d per period* is the displacement in lattice steps,
in the lattice basis of the patch it was catalogued on; on the tilings with a hexagonal lattice the
two basis vectors are 60° apart, so (1, −1) is as short a step as (1, 0), and direction words such as
"diagonal" would mislead. *Smallest rule* is the rule with the fewest birth and survival conditions in
which the ship runs; it runs identically in every rule between that one and the largest, and the
count says how many that is. *Seen* is how often the search met it, which depends on how long that
search ran.

| | id | runs on (simplest first) | p | d per period | cells | smallest rule (rules in range) | seen | kind | flag |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `0ed091c9` | 4.8.8 | 2 | (0, 1) | 5–9 | `B2/S3` (512) | 270 | single | B |
| 2 | `3c6d5ab0` | 4.8.8 | 2 | (0, −1) | 14–16 | `B2/S2` (256) | 8 | derivative: `69c9912a` with a tag-along |  |
| 3 | `69c9912a` | 4.8.8 | 2 | (0, −1) | 5 | `B2/S2` (512) | 8,034 | single | A |
| 4 | `070a054d` | 4.8.8 | 4 | (−2, 0) | 9–10 | `B2/S2` (256) | 32 | derivative: `938f9833` with a tag-along |  |
| 5 | `3e6ae2f3` | 4.8.8 | 4 | (0, −2) | 9–14 | `B2/S2` (64) | 5 | derivative: `69c9912a` with a tag-along |  |
| 6 | `5aec548b` | 4.8.8 | 4 | (0, −2) | 7–10 | `B2/S2` (256) | 92 | derivative: `69c9912a` with a tag-along |  |
| 7 | `616314c5` | 4.8.8 | 4 | (0, −2) | 14–17 | `B2/S2` (256) | 2 | derivative: `69c9912a` with a tag-along |  |
| 8 | `7c5b3e65` | 4.8.8 | 4 | (0, −2) | 10–15 | `B2/S3` (64) | 54 | derivative: `0ed091c9` with a tag-along | B |
| 9 | `938f9833` | 4.8.8 | 4 | (0, −2) | 5–6 | `B2/S2` (256) | 406 | single | A |
| 10 | `c2f4f14a` | 4.8.8 | 4 | (0, −2) | 8–9 | `B2/S2` (256) | 3 | derivative: `c4c00098` with a tag-along |  |
| 11 | `c4c00098` | 4.8.8 | 4 | (0, −2) | 7 | `B2/S2` (512) | 9 | single, contains a phase of `69c9912a`, `938f9833` |  |
| 12 | `9cee5c6a` | 4.8.8 | 6 | (0, 2) | 7–13 | `B2/S2` (128) | 11 | single |  |
| 13 | `25969982` | 4.8.8 | 12 | (6, 0) | 12–18 | `B2/S34` (64) | 2 | derivative: `0ed091c9` with a tag-along | B |
| 14 | `07ab1946` | 4.8.8 | 16 | (0, 8) | 19–26 | `B2/S34` (16) | 1 | derivative: `0ed091c9` with a tag-along | B |
| 15 | `bdc1c8c7` | 4.8.8 | 16 | (0, 8) | 14–20 | `B2/S34` (32) | 95 | derivative: `0ed091c9` with a tag-along | B |
| 16 | `fff1b887` | 4.8.8 | 16 | (0, 8) | 14–28 | `B2/S34` (32) | 271 | single | B |
| 17 | `7e51c0d1` | 4.8.8 | 24 | (12, 0) | 14–21 | `B2/S3` (128) | 1 | single | B |
| 18 | `852ea9ff` | 3.12.12 | 2 | (0, −1) | 6–9 | `B26/S2` (32,768) | 39 | single, contains a phase of `c78ceb01` |  |
| 19 | `a0229739` | 3.12.12 | 2 | (0, −1) | 7–9 | `B26/S2` (16,384) | 6 | single, contains a phase of `852ea9ff`, `c78ceb01` |  |
| 20 | `c78ceb01` | 3.12.12 | 2 | (0, 1) | 4–5 | `B2/S2` (65,536) | 1,742 | single | A |
| 21 | `b2c790bb` | 3.12.12 | 6 | (−2, 4) | 5–8 | `B2/S2` (32,768) | 1,585 | single | A |
| 22 | `5e1adb91` | 3.12.12 | 8 | (4, 0) | 12–23 | `B26/S26` (1,024) | 1 | derivative: `852ea9ff` with a tag-along |  |
| 23 | `1723afa2` | 4.6.12 | 4 | (0, 1) | 5–6 | `B2/S2` (65,536) | 373 | single | A |
| 24 | `aa7e7357` | 3.4.6.4 | 42 | (0, 3) | 7–15 | `B3/S235` (1,024) | 61 | single |  |
| 25 | `a13d3fbf` | 3.3.3.4.4, k3-051, k3-052 | 8 | (2, 0) | 13–17 | `B46/S3458` (1,024) | 2 | single |  |
| 26 | `acd0fa8b` | 3.3.3.3.6 | 27 | (−1, 1) | 4–18 | `B37/S2,5,6,10` (1,048,576) | 30 | single |  |
| 27 | `2ba58828` | 3.3.4.3.4 | 16 | (0, −4) | 15–33 | `B3/S23` (256) | 1 | derivative: a bound pair of Guttesen's snub-square glider | A |
| 28 | `98a3c53b` | 3.3.4.3.4 | 24 | (0, 6) | 17–50 | `B3/S23` (128) | 1 | single, contains a phase of Guttesen's snub-square glider | A |
| 29 | `0ccbe694` | 3.6.3.6, k3-030, k3-032, k3-039 | 4 | (−1, 0) | 8–9 | `B36/S23` (8,192) | 423 | single, contains a phase of Guttesen's g | B |
| 30 | `8aa7e24c` | 3.6.3.6, k3-030, k3-032, k3-039 | 4 | (−1, 0) | 7–8 | `B3/S345` (16,384) | 1,347 | single | A |
| 31 | `ea5d187b` | 3.6.3.6 | 4 | (−1, 0) | 10–13 | `B36/S23` (4,096) | 2 | derivative: Guttesen's g with a tag-along | A |
| 32 | `095a6bf7` | 3.6.3.6 | 12 | (0, 3) | 5–12 | `B38/S23` (2,048) | 595 | single, contains a phase of Guttesen's g | B |
| 33 | `7860a67a` | k2-003, k2-004, k3-010, k3-016 | 4 | (−1, 0) | 7 | `B45/S34` (256) | 249 | single |  |
| 34 | `f1b6f83a` | k2-003, k2-004, k3-010, k3-016 | 4 | (−1, 0) | 12–16 | `B456/S347` (256) | 7 | single |  |
| 35 | `b36f22f2` | k2-003, k2-004, k3-010, k3-016 | 18 | (−1, 0) | 10–14 | `B4567/S3478` (32) | 45 | single |  |
| 36 | `ffc6a106` | k2-003, k2-004, k3-010, k3-016 | 26 | (1, 0) | 17–26 | `B45679/S3478` (16) | 1 | single |  |
| 37 | `61627500` | k2-004 | 3 | (−1, 0) | 4–6 | `B3/S35` (2,048) | 19 | single |  |
| 38 | `29af9b20` | k2-004 | 5 | (−1, 0) | 5–9 | `B3/S356` (512) | 4 | single |  |
| 39 | `58ce78e3` | k2-006, k3-028, k3-029, k3-030 | 10 | (1, 0) | 4–9 | `B36/S23` (4,096) | 43 | single | B |
| 40 | `dcd6b05f` | k2-006, k3-028 | 16 | (1, 0) | 8–11 | `B3/S1346` (2,048) | 13 | single |  |
| 41 | `da6eca43` | k2-006 | 46 | (−1, 2) | 5–17 | `B38/S345` (512) | 26 | single |  |
| 42 | `f9773263` | k2-007, k3-028, k3-031, k3-032 | 4 | (−1, 0) | 6–13 | `B3/S13468` (4,096) | 17 | single |  |
| 43 | `56ddb3e7` | k2-012 | 6 | (−1, 0) | 7 | `B45/S34` (262,144) | 110 | single |  |
| 44 | `01d41fa4` | k2-012 | 14 | (−1, 0) | 11–17 | `B456/S3468` (32,768) | 11 | single |  |
| 45 | `47978a43` | k2-013 | 6 | (−1, 0) | 9–20 | `B2,10/S1` (4,294,967,296) | 44 | single |  |
| 46 | `4b87931e` | k2-013 | 17 | (1, −1) | 2–20 | `B2,8,13/S1` (2,147,483,648) | 275 | single |  |
| 47 | `931ac6a1` | k2-014, k3-051 | 4 | (−2, 0) | 5–6 | `B3/S2` (16,384) | 179 | single |  |
| 48 | `47587892` | k2-017 | 17 | (0, −1) | 3–13 | `B3/S25` (8,192) | 949 | single | B |
| 49 | `0d9a694c` | k3-003 | 10 | (1, 0) | 4–10 | `B3/S2456` (512) | 254 | single |  |
| 50 | `5b9c9e68` | k3-009 | 16 | (−1, 0) | 4–10 | `B3478/S23` (134,217,728) | 466 | single |  |
| 51 | `25e004f4` | k3-013, k3-019 | 4 | (−1, 0) | 7 | `B45/S34` (4,096) | 198 | single |  |
| 52 | `c15dd310` | k3-013, k3-019 | 5 | (−1, 0) | 4–5 | `B3/S2` (131,072) | 229 | single |  |
| 53 | `f62ba07d` | k3-013, k3-019 | 5 | (−1, 0) | 4–5 | `B3/S2` (131,072) | 177 | single |  |
| 54 | `29fa9994` | k3-015, k3-021 | 32 | (1, 0) | 11–16 | `B4,5,6,7,8,11/S56` (16) | 30 | single |  |
| 55 | `dc81d36e` | k3-015, k3-021 | 44 | (−1, 0) | 11–16 | `B4,5,6,7,8,10/S56` (16) | 35 | single |  |
| 56 | `d37488d1` | k3-022 | 12 | (−1, −1) | 9–11 | `B3/S3457` (1,024) | 1,158 | single | B |
| 57 | `573a43ed` | k3-032 | 3 | (1, 0) | 10–13 | `B3/S3457` (1,024) | 2 | single |  |
| 58 | `e3df3631` | k3-038 | 53 | (−1, 0) | 4–24 | `B3/S136` (2,048) | 4 | single |  |
| 59 | `df78e126` | k3-045 | 37 | (1, 0) | 6–23 | `B4,5,9,11/S2,3,5,8,9,14` (4,096) | 8 | single |  |
| 60 | `9d5c568b` | k3-053 | 26 | (0, 1) | 3–7 | `B36/S25` (16,384) | 642 | single |  |
| 61 | `91205ceb` | k3-058 | 12 | (0, 1) | 6–10 | `B37/S25` (16,777,216) | 1,096 | single | B |
| 62 | `f3d2fa6a` | k3-058 | 22 | (−1, 1) | 4–12 | `B37/S25` (4,194,304) | 359 | single |  |

Two notes on the table. The two k2-013 ships run identically in about 2<sup>32</sup> and 2<sup>31</sup>
rules because they use very few neighbour counts, although that tiling's dodecagons touch up to 24
tiles: the ships are real, but their mechanism is simple. And a *derivative* ship is kept as a distinct
object, as Life catalogues keep tag-alongs, but it is not counted as a separate discovery.

### Not claimed

- **Compounds.** Ten catalogue entries are two ships that fly side by side and never influence each
  other; JETCARP's object finder stored each pair as one ship. Eight are two of Guttesen's kagome
  glider g (`7e188e44`, `0c8ab3fe`, `be288c7d`, `7c25665e`, `c10cc071`, `c7d48e0c`, `b9c14072`,
  `dbeb2dbf`), one is two of his snub-square glider (`80d241d2`) and one is two copies of `095a6bf7`
  (`f338d60a`).
- **Guttesen's gliders.** His glider g on 3.6.3.6 and his period-8 glider on 3.3.4.3.4, both in Life,
  were found by JETCARP's searches independently; they are his
  ([Guttesen 2026](bibliography.md#guttesen2026game)). JETCARP's 4-to-5-cell kagome ship is exactly
  his Fig. 4 glider (the same cells in one phase), and its snub-square ship matches his Fig. 5 by eye;
  a cell-for-cell check awaits his published configurations. His large kagome glider G (period 40) was
  not found. Glider g also runs in corridors of kagome tiles inside the 3-uniform tilings k3-030,
  k3-032 and k3-039; the snub-square glider runs only on 3.3.4.3.4.
- **Lattice objects.** Every ship on an isohedral type, and the unnamed ships of the square and
  triangular grids found along the way, belong to those grids.

## The hat glider is Edmond's glider B

The hat glider that JETCARP's reimplementation of James Edmond's evolutionary search found is
Edmond's hat glider B ([Edmond 2026](bibliography.md#edmond2026gliders)) written in another table.
Rename B's states (1 as 3, 2 as 1, 3 as 2) and the two tables agree on every configuration the glider
meets after its first generation; run side by side, they give the same states every generation, for
180 generations on a level-5 patch and 1,467 generations on a level-7 patch. The glider has one head
and four to six wing tiles, its eight shapes are all among B's ten, and it moves one ring of tiles
every two generations. JETCARP's table has one extra row and its seed launches the glider on a
different lane, which is why it first looked different. The glider is Edmond's. What JETCARP adds is a
long run: on a sliding window that follows the glider, it travelled 1,000,000 rings of tiles at a
steady 2.14 units per generation (Edmond's certified minimum speed is 2.1397), and Edmond's own certificates prove it
immortal. See [Priority rules](../automata/priority-rules.md).

## Still to do

- Ask on the ConwayLife.com forums whether any of the candidates, above all the flagged ones, have
  been seen before. Our prior-art search could not read the forums directly.
- Compare the kagome and snub-square ships with Guttesen's configurations cell for cell, once they
  are published.
- Extend the equivalence survey to the k-uniform tilings with k from 4 to 6, and the ship comparison
  to aperiodic tilings. Neither can turn a claimed ship into a lattice ship, but either could add
  "also runs on" entries.
- Re-run the key searches just before publication, since several closely related works appeared in
  August to October 2026.
