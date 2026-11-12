# Tiling catalogue

<!-- GENERATED FILE: do not edit by hand. Generator: docs/tools/gen-tiling-catalogue.py, from
     `jetcarp tilings`, `jetcarp isohedral` and crates/jetcarp-core/data/tilings/kuniform.json. -->

> **Generated page.** Built by `docs/tools/gen-tiling-catalogue.py` from the program's own tiling
> lists and data files; thumbnails are drawn by JETCARP (`docs/tools/tiling-thumbs.rhai`). Do not
> edit by hand.

Every tiling JETCARP can run, with its neighbour counts. That is 25 tiling families (some of them reshapeable), 81 isohedral types, and the 1248 k-uniform tilings of Galebach's catalogue, which include the regular and Archimedean tilings again under their catalogue ids.

**How to read the degrees.** A tile's *edge* neighbours share a side with it; its *touch* neighbours share at least one corner (see [Tilings as neighbour graphs](neighbour-graphs.md)). The lists give every value that occurs in the interior of the tiling. Rules that count neighbours (Life-like, Generations, JACA and others) see these numbers, which is why the same rule behaves so differently from one tiling to the next.

Thumbnails show a small patch with each tile in a random colour so the shapes stand out. Reshapeable families and isohedral types are drawn with their default shapes.

## Tiling families

These are the families you pick by name (`tiling: {"family": "cairo"}` in a scene). *Symmetry* is the wallpaper group of the default shape (`none` for the aperiodic tilings). *Storage* is how JETCARP holds the cells: `dense` for a plain lattice, `periodic` for a repeating unit cell, `explicit` for a finite aperiodic patch.

| | Id | Name | Symmetry | Storage | Edge degrees | Touch degrees |
| --- | --- | --- | --- | --- | --- | --- |
| <img src="../images/tilings/thumbs/lizard.png" alt="Three-fold hexagon" width="120" loading="lazy"> | `lizard` | Three-fold hexagon | p3 | periodic | 6 | 6 |
| <img src="../images/tilings/thumbs/hex.png" alt="Hexagon" width="120" loading="lazy"> | `hex` | Hexagon | p1 | dense | 6 | 6 |
| <img src="../images/tilings/thumbs/brick.png" alt="Brick" width="120" loading="lazy"> | `brick` | Brick | p1 | dense | 6 | 6 |
| <img src="../images/tilings/thumbs/square.png" alt="Square (parallelogram)" width="120" loading="lazy"> | `square` | Square (parallelogram) | p1 | dense | 4 | 8 |
| <img src="../images/tilings/thumbs/quad.png" alt="Any quadrilateral" width="120" loading="lazy"> | `quad` | Any quadrilateral | p2 | periodic | 4 | 8 |
| <img src="../images/tilings/thumbs/triangle.png" alt="Any triangle" width="120" loading="lazy"> | `triangle` | Any triangle | p2 | periodic | 3 | 12 |
| <img src="../images/tilings/thumbs/glide.png" alt="Glide rectangle" width="120" loading="lazy"> | `glide` | Glide rectangle | pg | periodic | 4 | 8 |
| <img src="../images/tilings/thumbs/fourfold.png" alt="Four-fold square" width="120" loading="lazy"> | `fourfold` | Four-fold square | p4 | periodic | 4 | 8 |
| <img src="../images/tilings/thumbs/cairo.png" alt="Cairo pentagon" width="120" loading="lazy"> | `cairo` | Cairo pentagon | p4 | periodic | 5 | 7 |
| <img src="../images/tilings/thumbs/spectre.png" alt="Spectre" width="120" loading="lazy"> | `spectre` | Spectre | none | explicit | 4, 5, 6, 7 | 6, 7 |
| <img src="../images/tilings/thumbs/hat.png" alt="Hat" width="120" loading="lazy"> | `hat` | Hat | none | explicit | 4, 5, 6 | 6, 7 |
| <img src="../images/tilings/thumbs/penrose-kd.png" alt="Penrose kites and darts (P2)" width="120" loading="lazy"> | `penrose-kd` | Penrose kites and darts (P2) | none | explicit | 4 | 8, 9, 10 |
| <img src="../images/tilings/thumbs/penrose-rhomb.png" alt="Penrose rhombs (P3)" width="120" loading="lazy"> | `penrose-rhomb` | Penrose rhombs (P3) | none | explicit | 4 | 7, 8, 9, 10, 11 |
| <img src="../images/tilings/thumbs/hex_flat.png" alt="Hexagon, flat top" width="120" loading="lazy"> | `hex_flat` | Hexagon, flat top | p6m | periodic | 6 | 6 |
| <img src="../images/tilings/thumbs/oct.png" alt="4.8.8 truncated square" width="120" loading="lazy"> | `oct` | 4.8.8 truncated square | p4m | periodic | 4, 8 | 4, 8 |
| <img src="../images/tilings/thumbs/trihex.png" alt="3.6.3.6 trihexagonal" width="120" loading="lazy"> | `trihex` | 3.6.3.6 trihexagonal | p6m | periodic | 3, 6 | 6, 12 |
| <img src="../images/tilings/thumbs/elongtri.png" alt="3.3.3.4.4 elongated triangular" width="120" loading="lazy"> | `elongtri` | 3.3.3.4.4 elongated triangular | cmm | periodic | 3, 4 | 9, 12 |
| <img src="../images/tilings/thumbs/snubsq.png" alt="3.3.4.3.4 snub square" width="120" loading="lazy"> | `snubsq` | 3.3.4.3.4 snub square | p4g | periodic | 3, 4 | 9, 12 |
| <img src="../images/tilings/thumbs/snubhex.png" alt="3.3.3.3.6 snub hexagonal" width="120" loading="lazy"> | `snubhex` | 3.3.3.3.6 snub hexagonal | p6 | periodic | 3, 6 | 9, 18 |
| <img src="../images/tilings/thumbs/rhombitrihex.png" alt="3.4.6.4 rhombitrihexagonal" width="120" loading="lazy"> | `rhombitrihex` | 3.4.6.4 rhombitrihexagonal | p6m | periodic | 3, 4, 6 | 6, 8, 12 |
| <img src="../images/tilings/thumbs/trunchex.png" alt="3.12.12 truncated hexagonal" width="120" loading="lazy"> | `trunchex` | 3.12.12 truncated hexagonal | p6m | periodic | 3, 12 | 3, 12 |
| <img src="../images/tilings/thumbs/trunctrihex.png" alt="4.6.12 truncated trihexagonal" width="120" loading="lazy"> | `trunctrihex` | 4.6.12 truncated trihexagonal | p6m | periodic | 4, 6, 12 | 4, 6, 12 |
| <img src="../images/tilings/thumbs/periodic.png" alt="Periodic (custom or catalog)" width="120" loading="lazy"> | `periodic` | Periodic (custom or catalog) | any | periodic | 3, 4, 6 | 6, 8, 12 |
| <img src="../images/tilings/thumbs/herring.png" alt="Herringbone" width="120" loading="lazy"> | `herring` | Herringbone | pgg | periodic | 6 | 6 |
| <img src="../images/tilings/thumbs/pyth.png" alt="Pythagorean squares" width="120" loading="lazy"> | `pyth` | Pythagorean squares | p4 | periodic | 4, 8 | 4, 8 |

Pages: [Regular and Archimedean tilings](regular-archimedean.md), [Editable tile families](editable-families.md), [Penrose tilings](penrose.md), [The hat and the spectre](hat-spectre.md).

## Isohedral types

The 81 isohedral types that Craig Kaplan's Tactile library supports (IH01 to IH93 in Grünbaum and Shephard's numbering, less the 12 types Tactile does not support). *Parameters* is the number of shape sliders; *aspects* is the number of differently oriented copies of the tile in one unit cell. See [Isohedral tilings](isohedral.md).

| | Id | Symmetry | Parameters | Aspects | Edge degrees | Touch degrees |
| --- | --- | --- | --- | --- | --- | --- |
| <img src="../images/tilings/thumbs/ih01.png" alt="IH01" width="120" loading="lazy"> | `ih01` | p1 | 4 | 1 | 6 | 6 |
| <img src="../images/tilings/thumbs/ih02.png" alt="IH02" width="120" loading="lazy"> | `ih02` | pg | 4 | 2 | 6 | 6 |
| <img src="../images/tilings/thumbs/ih03.png" alt="IH03" width="120" loading="lazy"> | `ih03` | pg | 4 | 2 | 6 | 6 |
| <img src="../images/tilings/thumbs/ih04.png" alt="IH04" width="120" loading="lazy"> | `ih04` | p2 | 6 | 2 | 6 | 6 |
| <img src="../images/tilings/thumbs/ih05.png" alt="IH05" width="120" loading="lazy"> | `ih05` | pgg | 5 | 4 | 6 | 6 |
| <img src="../images/tilings/thumbs/ih06.png" alt="IH06" width="120" loading="lazy"> | `ih06` | pgg | 5 | 4 | 6 | 6 |
| <img src="../images/tilings/thumbs/ih07.png" alt="IH07" width="120" loading="lazy"> | `ih07` | p3 | 2 | 3 | 6 | 6 |
| <img src="../images/tilings/thumbs/ih08.png" alt="IH08" width="120" loading="lazy"> | `ih08` | p2 | 4 | 1 | 6 | 6 |
| <img src="../images/tilings/thumbs/ih09.png" alt="IH09" width="120" loading="lazy"> | `ih09` | pgg | 3 | 2 | 6 | 6 |
| <img src="../images/tilings/thumbs/ih10.png" alt="IH10" width="120" loading="lazy"> | `ih10` | p3 | 0 | 1 | 6 | 6 |
| <img src="../images/tilings/thumbs/ih11.png" alt="IH11" width="120" loading="lazy"> | `ih11` | p6 | 0 | 1 | 6 | 6 |
| <img src="../images/tilings/thumbs/ih12.png" alt="IH12" width="120" loading="lazy"> | `ih12` | cm | 2 | 1 | 6 | 6 |
| <img src="../images/tilings/thumbs/ih13.png" alt="IH13" width="120" loading="lazy"> | `ih13` | pmg | 3 | 2 | 6 | 6 |
| <img src="../images/tilings/thumbs/ih14.png" alt="IH14" width="120" loading="lazy"> | `ih14` | cm | 2 | 1 | 6 | 6 |
| <img src="../images/tilings/thumbs/ih15.png" alt="IH15" width="120" loading="lazy"> | `ih15` | pmg | 3 | 2 | 6 | 6 |
| <img src="../images/tilings/thumbs/ih16.png" alt="IH16" width="120" loading="lazy"> | `ih16` | p31m | 1 | 3 | 6 | 6 |
| <img src="../images/tilings/thumbs/ih17.png" alt="IH17" width="120" loading="lazy"> | `ih17` | cmm | 2 | 1 | 6 | 6 |
| <img src="../images/tilings/thumbs/ih18.png" alt="IH18" width="120" loading="lazy"> | `ih18` | p31m | 0 | 1 | 6 | 6 |
| <img src="../images/tilings/thumbs/ih20.png" alt="IH20" width="120" loading="lazy"> | `ih20` | p6m | 0 | 1 | 6 | 6 |
| <img src="../images/tilings/thumbs/ih21.png" alt="IH21" width="120" loading="lazy"> | `ih21` | p6 | 2 | 6 | 5 | 8 |
| <img src="../images/tilings/thumbs/ih22.png" alt="IH22" width="120" loading="lazy"> | `ih22` | cm | 3 | 2 | 5 | 7 |
| <img src="../images/tilings/thumbs/ih23.png" alt="IH23" width="120" loading="lazy"> | `ih23` | p2 | 4 | 2 | 5 | 7 |
| <img src="../images/tilings/thumbs/ih24.png" alt="IH24" width="120" loading="lazy"> | `ih24` | pmg | 4 | 4 | 5 | 7 |
| <img src="../images/tilings/thumbs/ih25.png" alt="IH25" width="120" loading="lazy"> | `ih25` | pgg | 3 | 4 | 5 | 7 |
| <img src="../images/tilings/thumbs/ih26.png" alt="IH26" width="120" loading="lazy"> | `ih26` | cmm | 2 | 2 | 5 | 7 |
| <img src="../images/tilings/thumbs/ih27.png" alt="IH27" width="120" loading="lazy"> | `ih27` | pgg | 3 | 4 | 5 | 7 |
| <img src="../images/tilings/thumbs/ih28.png" alt="IH28" width="120" loading="lazy"> | `ih28` | p4 | 2 | 4 | 5 | 7 |
| <img src="../images/tilings/thumbs/ih29.png" alt="IH29" width="120" loading="lazy"> | `ih29` | p4g | 1 | 4 | 5 | 7 |
| <img src="../images/tilings/thumbs/ih30.png" alt="IH30" width="120" loading="lazy"> | `ih30` | p31m | 1 | 6 | 4 | 9 |
| <img src="../images/tilings/thumbs/ih31.png" alt="IH31" width="120" loading="lazy"> | `ih31` | p6 | 0 | 6 | 4 | 9 |
| <img src="../images/tilings/thumbs/ih32.png" alt="IH32" width="120" loading="lazy"> | `ih32` | p6m | 0 | 6 | 4 | 9 |
| <img src="../images/tilings/thumbs/ih33.png" alt="IH33" width="120" loading="lazy"> | `ih33` | p3 | 0 | 3 | 4 | 10 |
| <img src="../images/tilings/thumbs/ih34.png" alt="IH34" width="120" loading="lazy"> | `ih34` | p6 | 0 | 3 | 4 | 10 |
| <img src="../images/tilings/thumbs/ih36.png" alt="IH36" width="120" loading="lazy"> | `ih36` | p31m | 0 | 3 | 4 | 10 |
| <img src="../images/tilings/thumbs/ih37.png" alt="IH37" width="120" loading="lazy"> | `ih37` | p6m | 0 | 3 | 4 | 10 |
| <img src="../images/tilings/thumbs/ih38.png" alt="IH38" width="120" loading="lazy"> | `ih38` | p31m | 0 | 6 | 3 | 21 |
| <img src="../images/tilings/thumbs/ih39.png" alt="IH39" width="120" loading="lazy"> | `ih39` | p6 | 0 | 6 | 3 | 21 |
| <img src="../images/tilings/thumbs/ih40.png" alt="IH40" width="120" loading="lazy"> | `ih40` | p6m | 0 | 6 | 3 | 21 |
| <img src="../images/tilings/thumbs/ih41.png" alt="IH41" width="120" loading="lazy"> | `ih41` | p1 | 2 | 1 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih42.png" alt="IH42" width="120" loading="lazy"> | `ih42` | pm | 2 | 2 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih43.png" alt="IH43" width="120" loading="lazy"> | `ih43` | pg | 2 | 2 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih44.png" alt="IH44" width="120" loading="lazy"> | `ih44` | pg | 2 | 2 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih45.png" alt="IH45" width="120" loading="lazy"> | `ih45` | cm | 2 | 2 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih46.png" alt="IH46" width="120" loading="lazy"> | `ih46` | p2 | 4 | 2 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih47.png" alt="IH47" width="120" loading="lazy"> | `ih47` | p2 | 2 | 2 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih49.png" alt="IH49" width="120" loading="lazy"> | `ih49` | pmg | 3 | 4 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih50.png" alt="IH50" width="120" loading="lazy"> | `ih50` | pmg | 2 | 4 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih51.png" alt="IH51" width="120" loading="lazy"> | `ih51` | pgg | 3 | 4 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih52.png" alt="IH52" width="120" loading="lazy"> | `ih52` | pgg | 1 | 4 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih53.png" alt="IH53" width="120" loading="lazy"> | `ih53` | pgg | 3 | 4 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih54.png" alt="IH54" width="120" loading="lazy"> | `ih54` | cmm | 2 | 4 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih55.png" alt="IH55" width="120" loading="lazy"> | `ih55` | p4 | 0 | 4 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih56.png" alt="IH56" width="120" loading="lazy"> | `ih56` | p4g | 1 | 8 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih57.png" alt="IH57" width="120" loading="lazy"> | `ih57` | p2 | 2 | 1 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih58.png" alt="IH58" width="120" loading="lazy"> | `ih58` | pmg | 2 | 2 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih59.png" alt="IH59" width="120" loading="lazy"> | `ih59` | pgg | 1 | 2 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih61.png" alt="IH61" width="120" loading="lazy"> | `ih61` | p4 | 0 | 2 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih62.png" alt="IH62" width="120" loading="lazy"> | `ih62` | p4 | 0 | 1 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih64.png" alt="IH64" width="120" loading="lazy"> | `ih64` | pm | 1 | 1 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih66.png" alt="IH66" width="120" loading="lazy"> | `ih66` | pmg | 1 | 2 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih67.png" alt="IH67" width="120" loading="lazy"> | `ih67` | cmm | 2 | 2 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih68.png" alt="IH68" width="120" loading="lazy"> | `ih68` | cm | 1 | 1 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih69.png" alt="IH69" width="120" loading="lazy"> | `ih69` | pmg | 2 | 2 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih71.png" alt="IH71" width="120" loading="lazy"> | `ih71` | p4g | 0 | 4 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih72.png" alt="IH72" width="120" loading="lazy"> | `ih72` | pmm | 1 | 1 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih73.png" alt="IH73" width="120" loading="lazy"> | `ih73` | p4g | 0 | 2 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih74.png" alt="IH74" width="120" loading="lazy"> | `ih74` | cmm | 1 | 1 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih76.png" alt="IH76" width="120" loading="lazy"> | `ih76` | p4m | 0 | 1 | 4 | 8 |
| <img src="../images/tilings/thumbs/ih77.png" alt="IH77" width="120" loading="lazy"> | `ih77` | p6m | 0 | 12 | 3 | 16 |
| <img src="../images/tilings/thumbs/ih78.png" alt="IH78" width="120" loading="lazy"> | `ih78` | cmm | 1 | 4 | 3 | 14 |
| <img src="../images/tilings/thumbs/ih79.png" alt="IH79" width="120" loading="lazy"> | `ih79` | p4 | 0 | 4 | 3 | 14 |
| <img src="../images/tilings/thumbs/ih81.png" alt="IH81" width="120" loading="lazy"> | `ih81` | p4g | 0 | 8 | 3 | 14 |
| <img src="../images/tilings/thumbs/ih82.png" alt="IH82" width="120" loading="lazy"> | `ih82` | p4m | 0 | 4 | 3 | 14 |
| <img src="../images/tilings/thumbs/ih83.png" alt="IH83" width="120" loading="lazy"> | `ih83` | cm | 1 | 2 | 3 | 12 |
| <img src="../images/tilings/thumbs/ih84.png" alt="IH84" width="120" loading="lazy"> | `ih84` | p2 | 2 | 2 | 3 | 12 |
| <img src="../images/tilings/thumbs/ih85.png" alt="IH85" width="120" loading="lazy"> | `ih85` | pmg | 2 | 4 | 3 | 12 |
| <img src="../images/tilings/thumbs/ih86.png" alt="IH86" width="120" loading="lazy"> | `ih86` | pgg | 1 | 4 | 3 | 12 |
| <img src="../images/tilings/thumbs/ih88.png" alt="IH88" width="120" loading="lazy"> | `ih88` | p6 | 0 | 6 | 3 | 12 |
| <img src="../images/tilings/thumbs/ih90.png" alt="IH90" width="120" loading="lazy"> | `ih90` | p6 | 0 | 2 | 3 | 12 |
| <img src="../images/tilings/thumbs/ih91.png" alt="IH91" width="120" loading="lazy"> | `ih91` | cmm | 1 | 2 | 3 | 12 |
| <img src="../images/tilings/thumbs/ih93.png" alt="IH93" width="120" loading="lazy"> | `ih93` | p6m | 0 | 2 | 3 | 12 |

## k-uniform tilings, k = 1 to 3

Edge-to-edge tilings by regular polygons with k kinds of vertex (orbits under the tiling's symmetries), from Brian Galebach's catalogue in José Ezequiel Soto Sánchez's encoding (CC BY 4.0). The name lists the vertex type of each orbit. *Tiles* is the number of tiles in one unit cell; *mix* is the share of each polygon; *mean touch* is the average touch degree over the tiles. k = 4 to 6 are on [their own page](catalogue-kuniform.md). See [k-uniform tilings](k-uniform.md).

Counts: k = 1: 11, k = 2: 20, k = 3: 61, k = 4: 151, k = 5: 332, k = 6: 673.

### k = 1 (11 tilings)

| | Id | Vertex types | Symmetry | Tiles | Mix | Edge degrees | Touch degrees | Mean touch |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| <img src="../images/tilings/thumbs/k1-001.png" alt="6^3 hexagonal" width="120" loading="lazy"> | k1-001 | `6^3 hexagonal` | p6m | 1 | hexagons 100% | 6 | 6 | 6.00 |
| <img src="../images/tilings/thumbs/k1-002.png" alt="4.8^2 truncated square" width="120" loading="lazy"> | k1-002 | `4.8^2 truncated square` | p4m | 2 | squares 50%, octagons 50% | 4, 8 | 4, 8 | 6.00 |
| <img src="../images/tilings/thumbs/k1-003.png" alt="4.6.12 truncated trihexagonal" width="120" loading="lazy"> | k1-003 | `4.6.12 truncated trihexagonal` | p6m | 6 | squares 50%, hexagons 33%, dodecagons 17% | 4, 6, 12 | 4, 6, 12 | 6.00 |
| <img src="../images/tilings/thumbs/k1-004.png" alt="3.12^2 truncated hexagonal" width="120" loading="lazy"> | k1-004 | `3.12^2 truncated hexagonal` | p6m | 3 | triangles 67%, dodecagons 33% | 3, 12 | 3, 12 | 6.00 |
| <img src="../images/tilings/thumbs/k1-005.png" alt="4^4 square" width="120" loading="lazy"> | k1-005 | `4^4 square` | p4m | 1 | squares 100% | 4 | 8 | 8.00 |
| <img src="../images/tilings/thumbs/k1-006.png" alt="3.4.6.4 rhombitrihexagonal" width="120" loading="lazy"> | k1-006 | `3.4.6.4 rhombitrihexagonal` | p6m | 6 | triangles 33%, squares 50%, hexagons 17% | 3, 4, 6 | 6, 8, 12 | 8.00 |
| <img src="../images/tilings/thumbs/k1-007.png" alt="3.6.3.6 trihexagonal" width="120" loading="lazy"> | k1-007 | `3.6.3.6 trihexagonal` | p6m | 3 | triangles 67%, hexagons 33% | 3, 6 | 6, 12 | 8.00 |
| <img src="../images/tilings/thumbs/k1-008.png" alt="3^3.4^2 elongated triangular" width="120" loading="lazy"> | k1-008 | `3^3.4^2 elongated triangular` | cmm | 3 | triangles 67%, squares 33% | 3, 4 | 9, 12 | 10.00 |
| <img src="../images/tilings/thumbs/k1-009.png" alt="3^2.4.3.4 snub square" width="120" loading="lazy"> | k1-009 | `3^2.4.3.4 snub square` | p4g | 6 | triangles 67%, squares 33% | 3, 4 | 9, 12 | 10.00 |
| <img src="../images/tilings/thumbs/k1-010.png" alt="3^4.6 snub hexagonal" width="120" loading="lazy"> | k1-010 | `3^4.6 snub hexagonal` | p6 | 9 | triangles 89%, hexagons 11% | 3, 6 | 9, 18 | 10.00 |
| <img src="../images/tilings/thumbs/k1-011.png" alt="3^6 triangular" width="120" loading="lazy"> | k1-011 | `3^6 triangular` | p6m | 2 | triangles 100% | 3 | 12 | 12.00 |

### k = 2 (20 tilings)

| | Id | Vertex types | Symmetry | Tiles | Mix | Edge degrees | Touch degrees | Mean touch |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| <img src="../images/tilings/thumbs/k2-001.png" alt="[3.4.6.4; 4.6.12]" width="120" loading="lazy"> | k2-001 | `[3.4.6.4; 4.6.12]` | p6m | 12 | triangles 17%, squares 50%, hexagons 25%, dodecagons 8% | 3, 4, 6, 12 | 6, 8, 12 | 7.00 |
| <img src="../images/tilings/thumbs/k2-002.png" alt="[3.4.3.12; 3.12^2]" width="120" loading="lazy"> | k2-002 | `[3.4.3.12; 3.12^2]` | p4m | 6 | triangles 67%, squares 17%, dodecagons 17% | 3, 4, 12 | 5, 8, 16 | 7.33 |
| <img src="../images/tilings/thumbs/k2-003.png" alt="[3^3.4^2; 4^4] #1" width="120" loading="lazy"> | k2-003 | `[3^3.4^2; 4^4] #1` | cmm | 5 | triangles 40%, squares 60% | 3, 4 | 8, 9, 10 | 9.20 |
| <img src="../images/tilings/thumbs/k2-004.png" alt="[3^3.4^2; 4^4] #2" width="120" loading="lazy"> | k2-004 | `[3^3.4^2; 4^4] #2` | cmm | 4 | triangles 50%, squares 50% | 3, 4 | 9, 10 | 9.50 |
| <img src="../images/tilings/thumbs/k2-005.png" alt="[3.4^2.6; 3.4.6.4]" width="120" loading="lazy"> | k2-005 | `[3.4^2.6; 3.4.6.4]` | p6m | 18 | triangles 33%, squares 50%, hexagons 17% | 3, 4, 6 | 6, 8, 12 | 8.00 |
| <img src="../images/tilings/thumbs/k2-006.png" alt="[3.4^2.6; 3.6.3.6] #1" width="120" loading="lazy"> | k2-006 | `[3.4^2.6; 3.6.3.6] #1` | cmm | 5 | triangles 40%, squares 40%, hexagons 20% | 3, 4, 6 | 6, 8, 12 | 8.00 |
| <img src="../images/tilings/thumbs/k2-007.png" alt="[3.4^2.6; 3.6.3.6] #2" width="120" loading="lazy"> | k2-007 | `[3.4^2.6; 3.6.3.6] #2` | pmm | 5 | triangles 40%, squares 40%, hexagons 20% | 3, 4, 6 | 6, 8, 12 | 8.00 |
| <img src="../images/tilings/thumbs/k2-008.png" alt="[3^3.4^2; 3.4.6.4]" width="120" loading="lazy"> | k2-008 | `[3^3.4^2; 3.4.6.4]` | p6m | 15 | triangles 53%, squares 40%, hexagons 7% | 3, 4, 6 | 8, 9, 10, 12 | 9.20 |
| <img src="../images/tilings/thumbs/k2-009.png" alt="[3^2.4.3.4; 3.4.6.4]" width="120" loading="lazy"> | k2-009 | `[3^2.4.3.4; 3.4.6.4]` | p6m | 15 | triangles 53%, squares 40%, hexagons 7% | 3, 4, 6 | 8, 9, 10, 12 | 9.20 |
| <img src="../images/tilings/thumbs/k2-010.png" alt="[3^6; 3^2.6^2]" width="120" loading="lazy"> | k2-010 | `[3^6; 3^2.6^2]` | p6m | 8 | triangles 75%, hexagons 25% | 3, 6 | 8, 12 | 9.00 |
| <img src="../images/tilings/thumbs/k2-011.png" alt="[3^2.6^2; 3.6.3.6]" width="120" loading="lazy"> | k2-011 | `[3^2.6^2; 3.6.3.6]` | pmm | 3 | triangles 67%, hexagons 33% | 3, 6 | 6, 12 | 8.00 |
| <img src="../images/tilings/thumbs/k2-012.png" alt="[3^4.6; 3^2.6^2]" width="120" loading="lazy"> | k2-012 | `[3^4.6; 3^2.6^2]` | cmm | 5 | triangles 80%, hexagons 20% | 3, 6 | 8, 14 | 9.20 |
| <img src="../images/tilings/thumbs/k2-013.png" alt="[3^6; 3^2.4.12]" width="120" loading="lazy"> | k2-013 | `[3^6; 3^2.4.12]` | p6m | 16 | triangles 75%, squares 19%, dodecagons 6% | 3, 4, 12 | 8, 24 | 9.00 |
| <img src="../images/tilings/thumbs/k2-014.png" alt="[3^6; 3^3.4^2] #1" width="120" loading="lazy"> | k2-014 | `[3^6; 3^3.4^2] #1` | pmm | 5 | triangles 80%, squares 20% | 3, 4 | 10, 11, 12 | 10.80 |
| <img src="../images/tilings/thumbs/k2-015.png" alt="[3^6; 3^3.4^2] #2" width="120" loading="lazy"> | k2-015 | `[3^6; 3^3.4^2] #2` | cmm | 7 | triangles 86%, squares 14% | 3, 4 | 10, 11, 12 | 11.14 |
| <img src="../images/tilings/thumbs/k2-016.png" alt="[3^3.4^2; 3^2.4.3.4] #1" width="120" loading="lazy"> | k2-016 | `[3^3.4^2; 3^2.4.3.4] #1` | p4g | 18 | triangles 67%, squares 33% | 3, 4 | 9, 12 | 10.00 |
| <img src="../images/tilings/thumbs/k2-017.png" alt="[3^3.4^2; 3^2.4.3.4] #2" width="120" loading="lazy"> | k2-017 | `[3^3.4^2; 3^2.4.3.4] #2` | pgg | 12 | triangles 67%, squares 33% | 3, 4 | 9, 12 | 10.00 |
| <img src="../images/tilings/thumbs/k2-018.png" alt="[3^6; 3^2.4.3.4]" width="120" loading="lazy"> | k2-018 | `[3^6; 3^2.4.3.4]` | p6m | 11 | triangles 73%, squares 27% | 3, 4 | 9, 10, 12 | 10.36 |
| <img src="../images/tilings/thumbs/k2-019.png" alt="[3^6; 3^4.6] #1" width="120" loading="lazy"> | k2-019 | `[3^6; 3^4.6] #1` | p6m | 13 | triangles 92%, hexagons 8% | 3, 6 | 10, 18 | 10.62 |
| <img src="../images/tilings/thumbs/k2-020.png" alt="[3^6; 3^4.6] #2" width="120" loading="lazy"> | k2-020 | `[3^6; 3^4.6] #2` | p6 | 21 | triangles 95%, hexagons 5% | 3, 6 | 10, 11, 12, 18 | 11.14 |

### k = 3 (61 tilings)

| | Id | Vertex types | Symmetry | Tiles | Mix | Edge degrees | Touch degrees | Mean touch |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| <img src="../images/tilings/thumbs/k3-001.png" alt="[3^2.6^2; 3.6.3.6; 6^3] #1" width="120" loading="lazy"> | k3-001 | `[3^2.6^2; 3.6.3.6; 6^3] #1` | p6m | 15 | triangles 53%, hexagons 47% | 3, 6 | 6, 10 | 7.60 |
| <img src="../images/tilings/thumbs/k3-002.png" alt="[3^2.6^2; 3.6.3.6; 6^3] #2" width="120" loading="lazy"> | k3-002 | `[3^2.6^2; 3.6.3.6; 6^3] #2` | cmm | 4 | triangles 50%, hexagons 50% | 3, 6 | 6, 9 | 7.50 |
| <img src="../images/tilings/thumbs/k3-003.png" alt="[3^4.6; 3^2.6^2; 6^3]" width="120" loading="lazy"> | k3-003 | `[3^4.6; 3^2.6^2; 6^3]` | pmg | 6 | triangles 67%, hexagons 33% | 3, 6 | 8, 10 | 8.67 |
| <img src="../images/tilings/thumbs/k3-004.png" alt="[3^6; 3^2.6^2; 6^3]" width="120" loading="lazy"> | k3-004 | `[3^6; 3^2.6^2; 6^3]` | p6m | 9 | triangles 67%, hexagons 33% | 3, 6 | 8, 10 | 8.67 |
| <img src="../images/tilings/thumbs/k3-005.png" alt="[3.4^2.6; 3.6.3.6; 4.6.12]" width="120" loading="lazy"> | k3-005 | `[3.4^2.6; 3.6.3.6; 4.6.12]` | p6m | 24 | triangles 33%, squares 38%, hexagons 25%, dodecagons 4% | 3, 4, 6, 12 | 6, 8, 10, 12 | 7.50 |
| <img src="../images/tilings/thumbs/k3-006.png" alt="[3^6; 3^2.4.12; 4.6.12]" width="120" loading="lazy"> | k3-006 | `[3^6; 3^2.4.12; 4.6.12]` | p3m1 | 11 | triangles 55%, squares 27%, hexagons 9%, dodecagons 9% | 3, 4, 6, 12 | 6, 8, 18 | 8.18 |
| <img src="../images/tilings/thumbs/k3-007.png" alt="[3^2.4.12; 3.4.6.4; 3.12^2]" width="120" loading="lazy"> | k3-007 | `[3^2.4.12; 3.4.6.4; 3.12^2]` | p6m | 21 | triangles 57%, squares 29%, hexagons 5%, dodecagons 10% | 3, 4, 6, 12 | 5, 6, 8, 12, 18 | 7.71 |
| <img src="../images/tilings/thumbs/k3-008.png" alt="[3.4.3.12; 3.4.6.4; 3.12^2]" width="120" loading="lazy"> | k3-008 | `[3.4.3.12; 3.4.6.4; 3.12^2]` | p6m | 21 | triangles 57%, squares 29%, hexagons 5%, dodecagons 10% | 3, 4, 6, 12 | 5, 6, 8, 12, 18 | 7.71 |
| <img src="../images/tilings/thumbs/k3-009.png" alt="[3^2.4.12; 3.4.3.12; 3.12^2]" width="120" loading="lazy"> | k3-009 | `[3^2.4.12; 3.4.3.12; 3.12^2]` | cmm | 9 | triangles 67%, squares 22%, dodecagons 11% | 3, 4, 12 | 5, 6, 8, 20 | 7.78 |
| <img src="../images/tilings/thumbs/k3-010.png" alt="[3^3.4^2; 4^4; 4^4] #1" width="120" loading="lazy"> | k3-010 | `[3^3.4^2; 4^4; 4^4] #1` | cmm | 7 | triangles 29%, squares 71% | 3, 4 | 8, 9, 10 | 8.86 |
| <img src="../images/tilings/thumbs/k3-011.png" alt="[3.4^2.6; 3.6.3.6; 4^4] #1" width="120" loading="lazy"> | k3-011 | `[3.4^2.6; 3.6.3.6; 4^4] #1` | cmm | 9 | triangles 22%, squares 67%, hexagons 11% | 3, 4, 6 | 6, 8, 12 | 8.00 |
| <img src="../images/tilings/thumbs/k3-012.png" alt="[3.4^2.6; 3.6.3.6; 4^4] #2" width="120" loading="lazy"> | k3-012 | `[3.4^2.6; 3.6.3.6; 4^4] #2` | pmm | 9 | triangles 22%, squares 67%, hexagons 11% | 3, 4, 6 | 6, 8, 12 | 8.00 |
| <img src="../images/tilings/thumbs/k3-013.png" alt="[3^3.4^2; 3^3.4^2; 4^4] #1" width="120" loading="lazy"> | k3-013 | `[3^3.4^2; 3^3.4^2; 4^4] #1` | pmm | 8 | triangles 50%, squares 50% | 3, 4 | 8, 9, 10, 12 | 9.50 |
| <img src="../images/tilings/thumbs/k3-014.png" alt="[3^6; 3^3.4^2; 4^4] #1" width="120" loading="lazy"> | k3-014 | `[3^6; 3^3.4^2; 4^4] #1` | pmm | 7 | triangles 57%, squares 43% | 3, 4 | 8, 10, 11 | 10.00 |
| <img src="../images/tilings/thumbs/k3-015.png" alt="[3^6; 3^3.4^2; 4^4] #2" width="120" loading="lazy"> | k3-015 | `[3^6; 3^3.4^2; 4^4] #2` | cmm | 9 | triangles 67%, squares 33% | 3, 4 | 8, 10, 11, 12 | 10.44 |
| <img src="../images/tilings/thumbs/k3-016.png" alt="[3^3.4^2; 4^4; 4^4] #2" width="120" loading="lazy"> | k3-016 | `[3^3.4^2; 4^4; 4^4] #2` | cmm | 6 | triangles 33%, squares 67% | 3, 4 | 8, 9, 10 | 9.00 |
| <img src="../images/tilings/thumbs/k3-017.png" alt="[3.4^2.6; 3.6.3.6; 4^4] #3" width="120" loading="lazy"> | k3-017 | `[3.4^2.6; 3.6.3.6; 4^4] #3` | cmm | 7 | triangles 29%, squares 57%, hexagons 14% | 3, 4, 6 | 6, 8, 12 | 8.00 |
| <img src="../images/tilings/thumbs/k3-018.png" alt="[3.4^2.6; 3.6.3.6; 4^4] #4" width="120" loading="lazy"> | k3-018 | `[3.4^2.6; 3.6.3.6; 4^4] #4` | pmm | 7 | triangles 29%, squares 57%, hexagons 14% | 3, 4, 6 | 6, 8, 12 | 8.00 |
| <img src="../images/tilings/thumbs/k3-019.png" alt="[3^3.4^2; 3^3.4^2; 4^4] #2" width="120" loading="lazy"> | k3-019 | `[3^3.4^2; 3^3.4^2; 4^4] #2` | pmm | 7 | triangles 57%, squares 43% | 3, 4 | 9, 10, 12 | 9.71 |
| <img src="../images/tilings/thumbs/k3-020.png" alt="[3^6; 3^3.4^2; 4^4] #3" width="120" loading="lazy"> | k3-020 | `[3^6; 3^3.4^2; 4^4] #3` | pmm | 6 | triangles 67%, squares 33% | 3, 4 | 10, 11 | 10.33 |
| <img src="../images/tilings/thumbs/k3-021.png" alt="[3^6; 3^3.4^2; 4^4] #4" width="120" loading="lazy"> | k3-021 | `[3^6; 3^3.4^2; 4^4] #4` | cmm | 8 | triangles 75%, squares 25% | 3, 4 | 10, 11, 12 | 10.75 |
| <img src="../images/tilings/thumbs/k3-022.png" alt="[3.4^2.6; 3.4.6.4; 4^4]" width="120" loading="lazy"> | k3-022 | `[3.4^2.6; 3.4.6.4; 4^4]` | p4g | 14 | triangles 29%, squares 57%, hexagons 14% | 3, 4, 6 | 6, 8, 12 | 8.00 |
| <img src="../images/tilings/thumbs/k3-023.png" alt="[3^3.4^2; 3^2.4.3.4; 4^4]" width="120" loading="lazy"> | k3-023 | `[3^3.4^2; 3^2.4.3.4; 4^4]` | p4 | 13 | triangles 62%, squares 38% | 3, 4 | 9, 11, 12 | 9.85 |
| <img src="../images/tilings/thumbs/k3-024.png" alt="[3^6; 3^2.4.3.4; 3.4^2.6]" width="120" loading="lazy"> | k3-024 | `[3^6; 3^2.4.3.4; 3.4^2.6]` | p6m | 23 | triangles 52%, squares 39%, hexagons 9% | 3, 4, 6 | 7, 8, 10, 12 | 9.13 |
| <img src="../images/tilings/thumbs/k3-025.png" alt="[3^2.4.3.4; 3.4^2.6; 3.4.6.4]" width="120" loading="lazy"> | k3-025 | `[3^2.4.3.4; 3.4^2.6; 3.4.6.4]` | cmm | 9 | triangles 44%, squares 44%, hexagons 11% | 3, 4, 6 | 7, 8, 9, 12 | 8.67 |
| <img src="../images/tilings/thumbs/k3-026.png" alt="[3.4^2.6; 3.4.6.4; 3.4.6.4]" width="120" loading="lazy"> | k3-026 | `[3.4^2.6; 3.4.6.4; 3.4.6.4]` | p6m | 24 | triangles 33%, squares 50%, hexagons 17% | 3, 4, 6 | 6, 8, 12 | 8.00 |
| <img src="../images/tilings/thumbs/k3-027.png" alt="[3^3.4^2; 3^2.6^2; 3.4^2.6]" width="120" loading="lazy"> | k3-027 | `[3^3.4^2; 3^2.6^2; 3.4^2.6]` | cmm | 11 | triangles 55%, squares 27%, hexagons 18% | 3, 4, 6 | 7, 8, 10, 12 | 8.55 |
| <img src="../images/tilings/thumbs/k3-028.png" alt="[3.4^2.6; 3.4^2.6; 3.6.3.6]" width="120" loading="lazy"> | k3-028 | `[3.4^2.6; 3.4^2.6; 3.6.3.6]` | cmm | 10 | triangles 40%, squares 40%, hexagons 20% | 3, 4, 6 | 6, 8, 12 | 8.00 |
| <img src="../images/tilings/thumbs/k3-029.png" alt="[3^2.6^2; 3.4^2.6; 3.6.3.6] #1" width="120" loading="lazy"> | k3-029 | `[3^2.6^2; 3.4^2.6; 3.6.3.6] #1` | cmm | 8 | triangles 50%, squares 25%, hexagons 25% | 3, 4, 6 | 6, 8, 12 | 8.00 |
| <img src="../images/tilings/thumbs/k3-030.png" alt="[3.4^2.6; 3.6.3.6; 3.6.3.6] #1" width="120" loading="lazy"> | k3-030 | `[3.4^2.6; 3.6.3.6; 3.6.3.6] #1` | pmg | 8 | triangles 50%, squares 25%, hexagons 25% | 3, 4, 6 | 6, 8, 12 | 8.00 |
| <img src="../images/tilings/thumbs/k3-031.png" alt="[3^2.6^2; 3.4^2.6; 3.6.3.6] #2" width="120" loading="lazy"> | k3-031 | `[3^2.6^2; 3.4^2.6; 3.6.3.6] #2` | pmm | 8 | triangles 50%, squares 25%, hexagons 25% | 3, 4, 6 | 6, 8, 12 | 8.00 |
| <img src="../images/tilings/thumbs/k3-032.png" alt="[3.4^2.6; 3.6.3.6; 3.6.3.6] #2" width="120" loading="lazy"> | k3-032 | `[3.4^2.6; 3.6.3.6; 3.6.3.6] #2` | cmm | 8 | triangles 50%, squares 25%, hexagons 25% | 3, 4, 6 | 6, 8, 12 | 8.00 |
| <img src="../images/tilings/thumbs/k3-033.png" alt="[3^4.6; 3^3.4^2; 3.4^2.6]" width="120" loading="lazy"> | k3-033 | `[3^4.6; 3^3.4^2; 3.4^2.6]` | pmm | 10 | triangles 60%, squares 30%, hexagons 10% | 3, 4, 6 | 8, 9, 10, 14 | 9.20 |
| <img src="../images/tilings/thumbs/k3-034.png" alt="[3^6; 3^2.4.3.4; 3.4.6.4] #1" width="120" loading="lazy"> | k3-034 | `[3^6; 3^2.4.3.4; 3.4.6.4] #1` | p6m | 23 | triangles 52%, squares 39%, hexagons 9% | 3, 4, 6 | 7, 8, 10, 12 | 9.13 |
| <img src="../images/tilings/thumbs/k3-035.png" alt="[3^3.4^2; 3^2.4.12; 3.4.6.4]" width="120" loading="lazy"> | k3-035 | `[3^3.4^2; 3^2.4.12; 3.4.6.4]` | p6m | 33 | triangles 55%, squares 36%, hexagons 6%, dodecagons 3% | 3, 4, 6, 12 | 7, 8, 10, 12, 24 | 8.55 |
| <img src="../images/tilings/thumbs/k3-036.png" alt="[3^6; 3^3.4^2; 3.4.6.4]" width="120" loading="lazy"> | k3-036 | `[3^6; 3^3.4^2; 3.4.6.4]` | p6m | 28 | triangles 64%, squares 32%, hexagons 4% | 3, 4, 6 | 8, 10, 12 | 9.86 |
| <img src="../images/tilings/thumbs/k3-037.png" alt="[3^6; 3^2.4.3.4; 3.4.6.4] #2" width="120" loading="lazy"> | k3-037 | `[3^6; 3^2.4.3.4; 3.4.6.4] #2` | p6m | 28 | triangles 64%, squares 32%, hexagons 4% | 3, 4, 6 | 8, 10, 12 | 9.86 |
| <img src="../images/tilings/thumbs/k3-038.png" alt="[3^6; 3^4.6; 3^2.6^2] #1" width="120" loading="lazy"> | k3-038 | `[3^6; 3^4.6; 3^2.6^2] #1` | pmg | 14 | triangles 86%, hexagons 14% | 3, 6 | 8, 9, 10, 14 | 10.00 |
| <img src="../images/tilings/thumbs/k3-039.png" alt="[3^2.6^2; 3.6.3.6; 3.6.3.6]" width="120" loading="lazy"> | k3-039 | `[3^2.6^2; 3.6.3.6; 3.6.3.6]` | cmm | 6 | triangles 67%, hexagons 33% | 3, 6 | 6, 12 | 8.00 |
| <img src="../images/tilings/thumbs/k3-040.png" alt="[3^6; 3^4.6; 3^2.6^2] #2" width="120" loading="lazy"> | k3-040 | `[3^6; 3^4.6; 3^2.6^2] #2` | pmm | 7 | triangles 86%, hexagons 14% | 3, 6 | 9, 10, 14 | 10.00 |
| <img src="../images/tilings/thumbs/k3-041.png" alt="[3^6; 3^4.6; 3^2.6^2] #3" width="120" loading="lazy"> | k3-041 | `[3^6; 3^4.6; 3^2.6^2] #3` | cmm | 9 | triangles 89%, hexagons 11% | 3, 6 | 9, 11, 14 | 10.44 |
| <img src="../images/tilings/thumbs/k3-042.png" alt="[3^6; 3^4.6; 3.6.3.6] #1" width="120" loading="lazy"> | k3-042 | `[3^6; 3^4.6; 3.6.3.6] #1` | p6m | 17 | triangles 82%, hexagons 18% | 3, 6 | 6, 8, 10, 14 | 9.53 |
| <img src="../images/tilings/thumbs/k3-043.png" alt="[3^6; 3^4.6; 3.6.3.6] #2" width="120" loading="lazy"> | k3-043 | `[3^6; 3^4.6; 3.6.3.6] #2` | p6m | 14 | triangles 86%, hexagons 14% | 3, 6 | 8, 10, 15 | 9.86 |
| <img src="../images/tilings/thumbs/k3-044.png" alt="[3^6; 3^4.6; 3.6.3.6] #3" width="120" loading="lazy"> | k3-044 | `[3^6; 3^4.6; 3.6.3.6] #3` | cmm | 11 | triangles 91%, hexagons 9% | 3, 6 | 8, 10, 11, 16 | 10.55 |
| <img src="../images/tilings/thumbs/k3-045.png" alt="[3^4.6; 3^4.6; 3.6.3.6] #1" width="120" loading="lazy"> | k3-045 | `[3^4.6; 3^4.6; 3.6.3.6] #1` | pmg | 14 | triangles 86%, hexagons 14% | 3, 6 | 8, 9, 16 | 9.71 |
| <img src="../images/tilings/thumbs/k3-046.png" alt="[3^4.6; 3^4.6; 3.6.3.6] #2" width="120" loading="lazy"> | k3-046 | `[3^4.6; 3^4.6; 3.6.3.6] #2` | p2 | 7 | triangles 86%, hexagons 14% | 3, 6 | 8, 9, 16 | 9.71 |
| <img src="../images/tilings/thumbs/k3-047.png" alt="[3^6; 3^3.4^2; 3^2.4.12]" width="120" loading="lazy"> | k3-047 | `[3^6; 3^3.4^2; 3^2.4.12]` | p6m | 33 | triangles 79%, squares 18%, dodecagons 3% | 3, 4, 12 | 8, 9, 10, 11, 12, 24 | 10.00 |
| <img src="../images/tilings/thumbs/k3-048.png" alt="[3^6; 3^2.4.3.4; 3^2.4.12]" width="120" loading="lazy"> | k3-048 | `[3^6; 3^2.4.3.4; 3^2.4.12]` | p6m | 27 | triangles 74%, squares 22%, dodecagons 4% | 3, 4, 12 | 8, 9, 10, 24 | 9.56 |
| <img src="../images/tilings/thumbs/k3-049.png" alt="[3^6; 3^6; 3^3.4^2] #1" width="120" loading="lazy"> | k3-049 | `[3^6; 3^6; 3^3.4^2] #1` | pmm | 9 | triangles 89%, squares 11% | 3, 4 | 10, 11, 12 | 11.33 |
| <img src="../images/tilings/thumbs/k3-050.png" alt="[3^6; 3^6; 3^3.4^2] #2" width="120" loading="lazy"> | k3-050 | `[3^6; 3^6; 3^3.4^2] #2` | cmm | 11 | triangles 91%, squares 9% | 3, 4 | 10, 11, 12 | 11.45 |
| <img src="../images/tilings/thumbs/k3-051.png" alt="[3^6; 3^3.4^2; 3^3.4^2] #1" width="120" loading="lazy"> | k3-051 | `[3^6; 3^3.4^2; 3^3.4^2] #1` | cmm | 8 | triangles 75%, squares 25% | 3, 4 | 9, 10, 11, 12 | 10.50 |
| <img src="../images/tilings/thumbs/k3-052.png" alt="[3^6; 3^3.4^2; 3^3.4^2] #2" width="120" loading="lazy"> | k3-052 | `[3^6; 3^3.4^2; 3^3.4^2] #2` | pmg | 10 | triangles 80%, squares 20% | 3, 4 | 9, 10, 11, 12 | 10.80 |
| <img src="../images/tilings/thumbs/k3-053.png" alt="[3^3.4^2; 3^3.4^2; 3^2.4.3.4]" width="120" loading="lazy"> | k3-053 | `[3^3.4^2; 3^3.4^2; 3^2.4.3.4]` | pgg | 18 | triangles 67%, squares 33% | 3, 4 | 9, 12 | 10.00 |
| <img src="../images/tilings/thumbs/k3-054.png" alt="[3^6; 3^3.4^2; 3^2.4.3.4]" width="120" loading="lazy"> | k3-054 | `[3^6; 3^3.4^2; 3^2.4.3.4]` | p6m | 20 | triangles 70%, squares 30% | 3, 4 | 9, 10, 12 | 10.20 |
| <img src="../images/tilings/thumbs/k3-055.png" alt="[3^3.4^2; 3^2.4.3.4; 3^2.4.3.4]" width="120" loading="lazy"> | k3-055 | `[3^3.4^2; 3^2.4.3.4; 3^2.4.3.4]` | p2 | 9 | triangles 67%, squares 33% | 3, 4 | 9, 12 | 10.00 |
| <img src="../images/tilings/thumbs/k3-056.png" alt="[3^4.6; 3^3.4^2; 3^2.4.3.4]" width="120" loading="lazy"> | k3-056 | `[3^4.6; 3^3.4^2; 3^2.4.3.4]` | p6m | 27 | triangles 74%, squares 22%, hexagons 4% | 3, 4, 6 | 9, 12, 18 | 10.00 |
| <img src="../images/tilings/thumbs/k3-057.png" alt="[3^6; 3^2.4.3.4; 3^2.4.3.4]" width="120" loading="lazy"> | k3-057 | `[3^6; 3^2.4.3.4; 3^2.4.3.4]` | p6m | 20 | triangles 70%, squares 30% | 3, 4 | 9, 10, 12 | 10.20 |
| <img src="../images/tilings/thumbs/k3-058.png" alt="[3^6; 3^6; 3^4.6] #1" width="120" loading="lazy"> | k3-058 | `[3^6; 3^6; 3^4.6] #1` | p6m | 19 | triangles 95%, hexagons 5% | 3, 6 | 10, 11, 18 | 11.05 |
| <img src="../images/tilings/thumbs/k3-059.png" alt="[3^6; 3^6; 3^4.6] #2" width="120" loading="lazy"> | k3-059 | `[3^6; 3^6; 3^4.6] #2` | p6m | 27 | triangles 96%, hexagons 4% | 3, 6 | 10, 11, 12, 18 | 11.33 |
| <img src="../images/tilings/thumbs/k3-060.png" alt="[3^6; 3^6; 3^4.6] #3" width="120" loading="lazy"> | k3-060 | `[3^6; 3^6; 3^4.6] #3` | p6 | 33 | triangles 97%, hexagons 3% | 3, 6 | 10, 11, 12, 18 | 11.45 |
| <img src="../images/tilings/thumbs/k3-061.png" alt="[3^6; 3^4.6; 3^4.6]" width="120" loading="lazy"> | k3-061 | `[3^6; 3^4.6; 3^4.6]` | cmm | 11 | triangles 91%, hexagons 9% | 3, 6 | 9, 10, 18 | 10.36 |
