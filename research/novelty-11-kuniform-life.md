# Prior-art search: Life on the 2- and 3-uniform tilings (2026-11-10)

The claim tested, as drafted for the docs after the Life census (from the census report, which is not published):

> To our knowledge, Conway's Life (B3/S23) had not been reported on any 3-uniform tiling, and on no
> 2-uniform tiling other than [3.4².6; 3.6.3.6] (where a 2014 Code Golf answer showed a period-20
> spaceship).

The agent's report is below, verbatim. The raw logs (forum and wiki dumps) are not published; see README.md. Docs updated in main 8c79107.
Notes added by the reviewer:

- k2-001 is [3.4.6.4; 4.6.12]. The census found no Life ship there, so the related work doesn't
  overlap the tilings where the claim rests.
- "henrisro" = Guttesen (the email is in the arXiv PDF), so Guttesen's results date from Code Golf,
  24 Nov 2024. The docs now cite both.
- Not followed up (minor): Bushaw & Cranston (bootstrap percolation on tilings by regular polygons,
  analytic), Unjong Yu (Glauber dynamics on lattice vertices), HyperRogue's CA land on tes-catalog
  tilings, and EricG's B3/S23 ships on kisrhombille (a Laves tiling; isohedral ships aren't claimed).

---

## Verdict: related work only. I found no prior art that breaks the claim.

I found no report of B3/S23 run on any 3-uniform tiling. I found no report of B3/S23 run on any 2-uniform tiling other than [3.4².6; 3.6.3.6]. I also found no Life-like spaceship reported on either class.

Two sources did run other cellular automata on one 2-uniform tiling, [3.4.6.4; 4.6.12]. Neither used B3/S23 as far as can be seen, and neither reports a ship on that tiling (details in section B). The weak spot this exposes is the wording. "Life had not been reported on" holds, but a reader may take it to mean no CA at all. Suggested wording: "Life spaceships had not been reported on … (other Life-like CAs had been run on [3.4.6.4; 4.6.12], without ships)."

Search date: 2026-11-10 local. The forum batch ran 2026-11-11, 02:45–03:14 UTC.

## A. Checks on the "already known" items (corrections and extensions)

1. **Code Golf 35827, Kevin L, answer 35915 (2014-08-08). Confirmed.**
   - Fiddle: http://jsfiddle.net/Therm/5n53auja/
   - The tiling is [3.4².6; 3.6.3.6]. I checked the GIF: rows of squares alternate with rows of hexagons and triangles. Each unit has 2 squares, 2 triangles and 1 hexagon.
   - The fiddle code is B3/S23: `if (temp.a == 3) …=1; else if (temp.a == 2) …=world[i].a; else 0`. The neighbour lists count tiles that share an edge or a vertex (a triangle has 6 neighbours).
   - The answer says "Period 20 spaceship : Found by Martin Büttner". His comment on the answer reads: "Found a glider/spaceship with period 20: world[36].e = 1; …".
   - Note: the fiddle's header comment says "// B2/S23". It is copied from his earlier house-tiling answer. The code itself is B3/S23.

2. **Code Golf 35827, every answer.** I pulled all 22 answers and all comments through the Stack Exchange API. No other 2- or 3-uniform tiling appears.
   - Peter Taylor: Cairo, 3.4.6.4, rhombille, labyrinth, a Penrose-like N=7 tiling, and "Hex Medley 3" (hexagons cut into triangles and rhombi, so not regular polygons).
   - Thaylon, "HexagonSplit" (2014-08-13): reports spaceships. His CSS shows the cells are house-shaped pentagons and non-regular hexagons, so this is not a k-uniform tiling.
   - Stretch Maniac (squares with irregular pentagons), Jaa-c (3-isohedral pentagons), DenDenDo (Voronoi), Keith Randall (tilings read from images), Sparr (Penrose; rectangles in 2018), Kevin L (houses).
   - **New: henrisro, 2024-11-24, six answers.** They cover truncated square, 3.12², 4.6.12, kagome, maple leaf (3⁴.6) and snub square. Quotes: "Finally, a spaceship (+10)" (kagome) and "A spaceship (+10)" (snub square). An update dated "November 7, 2025" adds a kagome glider gun.
   - henrisro is H. S. Guttesen: the arXiv 2608.27622 PDF gives an email address with the handle henrisro, and the paper cites the Code Golf question as Ref. [22]. So the Guttesen results first appeared on Code Golf in Nov 2024. Code: github.com/henrisro313/Games_of_life (Archimedean tilings only).

3. **EricG, t=1124 (2013-06-12). This extends the known item.** He already reported B3/S23 ships on several Archimedean tilings and one Laves tiling, years before Guttesen:
   - Snub square: "B3/S23 has sparky spaceships".
   - Kagome: "Kagome-B3/S23 … patterns readily become a never-ending cascade of spaceships and puffers".
   - Elongated triangular (3³.4²): "B3/S23 resembles Life -- there is a common spaceship (although it only travels horizontally)".
   - Kisrhombille: "B3/S23 has spaceships and puffers".
   - I read t=1123, 1124, 1025 and 3893 in full. There are no k-uniform tilings in them.

4. **Tiled CA (bprentice, t=1025).** The archived page says "The original version of Tiled CA was written by Jason Rampe" (Visions of Chaos). The archived TiledCA.zip ships 184 grids (.til files). They are built from grid-snapped shapes (right triangles, squares, parallelograms, irregular hexagons), not regular polygons, and none is named as k-uniform. The bundled rules are per-shape Generations rules.

## B. Sources that run a CA on a 2- or 3-uniform tiling (none is B3/S23 with ships)

1. **YouTube channel "Tessellation Automata"** (channel ID UCJK3gmEXm40zoBlyOtkptQA). Three videos use a "3-4-6-12 Tiling":
   - mbONg3rzS9A ("| 0", 2024-05-06)
   - yS53z9dI-I8 ("| 1 (long)", 2024-05-09)
   - lf_e-g1DqlQ ("| 2 (B26789 S2345)", 2024-05-10)

   The descriptions say: "Array: Utilising a 2-uniform tiling of polygons"; "Totalistic neighborhood: Defined by shared edges (and shared vertices) between the polygons"; "Primitive unit: Consisting of two triangles, six squares, three hexagons, and one dodecagon (3-4-6-12 tiling)". That unit is [3.4.6.4; 4.6.12].
   - Only one rule is stated: B26789/S2345. The other two videos state no rule.
   - Thumbnails show explosive growth: from a single cell in video 0 (which B3 cannot do) and from a ring of six squares in video 1.
   - No ship is claimed on this tiling.
   - The same channel claims B3/S23 gliders only on the snub square tiling (SVjp2uwFDQE, 2024-05-17; JJfULCn2YN8, 2024-05-18) and runs B3/S23 on rhombille (4vOWrCnEQJs). I listed all 45 videos and 13 shorts. No other k-uniform tiling appears; "weave", "basket stitch" and "two-sized triangular" are not regular-polygon tilings.

2. **github.com/redpanda1234/Conway-k-regular** (Owen Gillespie and Forest Kobayashi; commits 2017-02-07 to 2017-03-21). README: "An implementation of Conway's Game of Life on a k-uniform tiling of convex polygons!" and "hard-coded for one specific tiling".
   - The code's unit cell is 1 dodecagon, 3 hexagons, 6 squares and 2 triangles, so the tiling is [3.4.6.4; 4.6.12].
   - It is not B3/S23. Neighbours are edge-sharing only. Thresholds scale with side count: born if count ≥ 0.5·sides; dies if ≤ 0.25·sides or ≥ 0.6·sides.
   - No ships are mentioned. Demo video: https://youtu.be/011JXpdWA-8 ("Python Turtle Graphics 3 21 2017").

3. **Bushaw & Cranston, arXiv 1803.09056** (Australasian J. Combin. 74(3), 2019). An analytic study of bootstrap percolation (a monotone CA) on tilings by regular polygons. Quote: "let T denote the set of plane tilings T by regular polygons such that if T contains one instance of a vertex type, then T contains infinitely many instances of that type."
   - T contains every k-uniform tiling, but the paper never names "2-uniform" or "k-uniform".
   - It is not Life and has no simulations.

4. **Unjong Yu, arXiv 1710.09946 (2017).** "zero-temperature Glauber dynamics … in the two-dimensional Archimedean and 2-uniform lattices. Blinker states are found."
   - The cells are the lattice's vertices, not its tiles. The updates are stochastic Glauber dynamics, not a synchronous CA. It is not Life.

## C. Tools able to run CAs on k-uniform tilings (no Life reports found)

- **HyperRogue.** It has a "Cellular Automaton" land. Its description reads "A land for people wanting to experiment with cellular automata in the HyperRogue grid" (classes.cpp). In complex.cpp, rules are indexed by neighbour count. HyperRogue loads .tes tessellation files, and zenorogue/tes-catalog includes the k-uniform tilings (it is the catalogue Elisov used for k ≤ 10). I found nobody reporting Life on a k-uniform tiling with it.
- **Visions of Chaos "Tiled Cellular Automata" mode** (listed on LifeWiki) and **Tiled CA**. Their grids were not checked for combinatorial equivalence with k-uniform tilings.
- **Peter Taylor's GenericLife / AbstractLattice gist** (pjt33/becd56784480ddd751bf): a generic periodic-lattice framework, used so far only for Archimedean and non-regular tilings.
- **Not CA, for completeness:** percolation thresholds for all k ≤ 10 tilings (Elisov, arXiv 2610.31190, Oct 2026), and band structures of 1255 k-uniform lattices (arXiv 2011.11045).

## D. Query log (all 2026-11-10)

**Web search** (each phrase as typed):
- Uniform terms: "game of life" "2-uniform" tiling; "game of life" "k-uniform" tilings; "cellular automata" "k-uniform tilings"; "game of life" demiregular/demi-regular; "game of life" "3-uniform"; "cellular automata" Archimedean "2-uniform"; "uniform tilings" "game of life" github; k-uniform CA simulator.
- Archimedean and semiregular: Life on semiregular/Archimedean gliders; "on Archimedean tilings/lattices" Life-like; semi-regular tessellations CA; specific vertex configurations (3.3.4.3.4, 3.4.6.4, 3.12.12, 4.6.12); tiling names (trihexagonal, elongated triangular, snub hexagonal, truncated hexagonal, snub square).
- Venues: Wolfram Demonstrations (×2); Mathematica SE; Wolfram Community; YouTube; Reddit (×2); Shadertoy/Observable; itch.io/Observable; Hacker News; JCA/Complex Systems; Softology/Visions of Chaos (×3); Ready.
- People and tools: Carter Bays (×2); GomJau-Hogg/Antwerp; Craig Kaplan; HyperRogue (×2); Zeno Rogue/RogueViz; "Tessellation Automata" (×2); Japanese ライフゲーム 半正則.
- Physics: 2-uniform lattices with bootstrap/sandpile/majority; Parviainen percolation; k-uniform with voter/sandpile/forest fire.
- General: "any tiling" Life apps.

**ConwayLife forum mirror** (conwaylife.njohnston.ca search.php, posts):
- Terms: uniform (142 posts), demiregular (0), "demi regular" (0), semiregular (0), "semi regular", polymorph (0), Galebach (0), Chavey (0), Kaplan tiling, mixed tiling, regular polygons, Archimedean, kagome, trihexagonal, rhombitrihexagonal, snub, dodecagon(s), dodecagonal, octagons squares, hexagons triangles squares, hexagons squares, triangles squares, hexagons triangles, Laves (0), tessellation(s), tilings (213), Martini, elongated, truncated hexagonal, truncated square, isohedral, vertex types, vertex configuration, Tiling Golly, octagonal tiling, tileset, polygon tiling, polygons, codegolf, irregular grid, periodic tiling.
- The mirror has posts up to 2026-11-04.
- phpBB drops short tokens, so "2-uniform" is searched as "uniform".

**LifeWiki mirror**: uniform tiling, 2-/3-/k-uniform, demiregular, semiregular, Archimedean, kagome, tiling(s), regular polygons, trihexagonal, snub (square), dodecagon, octagon, other tilings, Penrose, Tiled CA, EricG, Cairo, hexagons and triangles. Pages read: "Cellular automaton simulation programs by supported rulespaces", "Cellular automaton", "Category:Euclidean tilings".

**Stack Exchange API**: Q35827 (all answers and comments); searches on codegolf, math, mathoverflow, mathematica and cs ("game of life tiling", "cellular automaton (uniform) tiling", "life triangles squares hexagons", "k-uniform"); read Q83988 and Q235379 (Life on 4.8.8 with edge neighbours).

**GitHub API**: repository search for k-uniform tiling, uniform tiling life, tiling game of life, tessellation game of life, kagome game of life, demiregular, 2-uniform, polygon tiling cellular automata, archimedean tiling life. Read: Conway-k-regular, Games_of_life, pjt33 gist, m-ender/webgl-tilings, TheChurro/game_of_life, yitzhaks/gameoflife, Arctenik/arbitrary-tiling-ca, percolation-k-uniform-tilings, tes-catalog, the hyperrogue sources, Ready patterns and mesh generators, the Golly rule-table repository, fiacrerougieux/emergence (1-uniform only).

**arXiv API**: "k-uniform tilings"; "2-uniform" AND lattice; "uniform tilings" AND automat*; Archimedean AND "cellular automat*"; "game of life" AND tiling; demiregular; "3-uniform" AND tiling.

**YouTube** (16 searches) plus the full Tessellation Automata channel listing. **HN Algolia**: 5 queries.

**Other pages read**: matejker game-of-life, cell-auto.com neighbourhoods, Golly feature request #7, Wayback copy of TiledCA, glu1.sourceforge.io, the Spivack kisrhombille post, NKS p.929 note, the ehouais Observable notebook, the Atlantis Press hyperbolic-Life paper, arXiv 2609.22579.

## E. Places I could not search (weak spots)

- conwaylife.com itself (403). I used the mirror.
- Direct Code Golf pages (fetch blocked). The API does not show deleted answers.
- Reddit (login wall; the Pullpush archive API was down).
- EricG's Google Drive "Tiling Project" (login).
- Visions of Chaos tile sets (closed-source Windows app).
- Tiled CA's 184 grids: listed, but not checked for topological equivalence with k-uniform tilings.
- HyperRogue on tes-catalog tilings: not run, and no reports looked for beyond web search.
- YouTube video contents and comments: I saw titles, descriptions and thumbnails only. Two 3-4-6-12 videos have unknown rules.
- Discord, X/Twitter, Mastodon, Bluesky, Facebook.
- Shadertoy: no API access; web search only.
- Google Scholar: no direct access; I used web search and the arXiv API.
- Paywalled full texts (Complex Systems, JCA, Physica D).
- Non-English sources beyond one Japanese query.
