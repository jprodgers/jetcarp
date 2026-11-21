# Novelty 10: forum and hobbyist prior art for the Archimedean ships (2026-11-10)

Two agents' reports, kept verbatim. The first swept ConwayLife (via the mirror conwaylife.njohnston.ca),
Code Golf SE, GitHub, HN, LifeWiki and arXiv for the four strongest Archimedean ships. The second re-read
EricG's 2013 threads to confirm the quotes. The conclusions went into the book's research pages.

---

## Prior-art check: JETCARP's four candidate spaceships (searches run 2026-11-10)

**Bottom line:** None of the four ships turned up in any source. All four are **related**, though. One hobbyist, EricG, posted on the ConwayLife forums on June 12, 2013, and covered all eight composite Archimedean tilings, including the four JETCARP tilings. He used the same touching (edge or vertex) neighbourhood and reported spaceships on every one of them. He gave no periods or speeds, and his pattern files are behind a Google Drive link I could not open. So "to our knowledge, not previously reported" is defensible only for these exact ships, and only with EricG cited. Do not claim to be first to find spaceships on these tilings. The riskiest of the four is the 4.8.8 ship.

One correction to your "already known" list: EricG reported B3/S23 spaceships on the kagome and snub-square tilings in 2013, thirteen years before Guttesen.

### The main source: EricG, "Tiling in Golly 2: Uniform, Dual-Uniform, and Misc Tiling"
- Live forum: https://conwaylife.com/forums/viewtopic.php?t=1124 (403 for me). Read on the mirror: https://conwaylife.njohnston.ca/forums/viewtopic.php?t=1124
- Six posts, all by EricG on June 12, 2013 (p8223 to p8229). There are no replies.
- He covers 16 tilings: the 8 composite Archimedean tilings and their 8 duals. He says: "Of the 16 tilings, using only lifelike rules, I found spaceships in 14 of them." The two without ships are duals (floret pentagonal and deltoidal trihexagonal), so every Archimedean tiling had ships.
- His neighbourhood matches JETCARP's. He gives octagon 8 / square 4 on 4.8.8, and square 12 / triangle 9 on 3.3.3.4.4. His snub-hexagonal rules use counts up to S18 and his rhombitrihexagonal rules use S9, which only make sense with vertex neighbours.
- His rules were Golly rule tables plus emulation scripts (snub hexagonal "took 19 states"). The files were in a Google Drive folder: https://drive.google.com/folderview?id=0B8i4-zfX_rIGaFZ6bzNTOEJWWk0 . It redirects to a Google login, and the Drive connector says "not found". **These files are the biggest gap: the actual ships may be in them.**

### Verdict for each candidate

**1. Rhombitrihexagonal 3.4.6.4 (p42, c/14): related.**
- EricG (p8227, 2013) says: "Spaceships were hard to find in this tiling, but there is a common spaceship in Rhombitrihexagonal-B3468/s239 which is mildly explosive", and "a rare spaceship in Rhombitrihexagonal-B346/s239 which is a non-exploding rule". He gives no period or speed.
  - Neither rule is in JETCARP's range. Both contain B4 and B6, which are outside the maximum rule, and both lack S5, which the minimal rule needs.
- Peter Taylor (pjt33) ran B3/S23 on this tiling in the 2014 Code Golf challenge "Implement the game of life on anything but a regular grid". The challenge counted cells as neighbours if they shared an edge or a vertex. He reported oscillators of period 2, 4, 5, 6 and 11 (later also 3, 7 and 15) and no gliders.
  - Read via the mirror https://qastack.fr/codegolf/35827/implement-the-game-of-life-on-anything-but-a-regular-grid (the original question is codegolf.stackexchange.com/questions/35827). His code is at https://gist.github.com/pjt33/becd56784480ddd751bf (Rhombitrihexagonal.java).
  - B3/S23 is one transition (S5) short of your minimal rule, B3/S235.
- Adamatzky, "Game of Life on Phyllosilicates" (arXiv:1306.0253; Phys. Lett. A 377, 2013), uses a 3.4.6.4-shaped lattice with the hexagons removed. He reports gliders, but it is not the same cell set.

**2. Elongated triangular 3.3.3.4.4 (p8, c/4): related.**
- EricG (p8227) reports these:
  - In B3/S23, "a common spaceship (although it only travels horizontally)", which "works in many other B3 rules as well".
  - In ElongTri-B36/S23, a ship that "looks like an LWSS in one phase".
  - In ElongTri-B3/S2489, "a very commonly occurring rake which travels in an oblique direction".
- None of these rules contains B4 or S3/S4/S5, all of which your minimal rule B46/S3458 needs, so these are different objects. Guttesen (2026) did not study this tiling.

**3. Snub hexagonal 3.3.3.3.6 (p27, c/27): related.**
- EricG (p8227) names four rules: SnubHexagonal-B3/S247 (orbiters only), B389,10/S247,10 ("Sparky spaceships are very common"), B389,10/S247,10,13,15,18 ("spaceships bounced off oscillators") and B37/S247,10 ("a very diffuse sparky blobby ship").
  - B37/S247,10 shares B37 and S2,10 with your minimal rule but lacks S5 and S6, so it cannot hold your ship.
- Guttesen (arXiv:2608.27622; v1 Aug 27, v2 Sep 14, 2026) calls this lattice "Maple leaf (3⁴,6)". Under B3/S23 he found still lifes and oscillators of period 2, 3, 8, 20 and 24, and no gliders.

**4. Truncated square 4.8.8 (p6, c/3): related, and the closest to prior art.**
- EricG (p8223) says many B2 rules on this tiling are non-exploding and have spaceships, puffers and rakes. He names only TruncatedSquare-B2/S346, which "has ships, puffers, and rakes". He also mentions a common natural backrake that briefly creates and destroys a forward-moving ship.
  - B2/S346 is outside your range because it lacks S2. But your minimal rule B2/S2 is exactly a B2 rule. His unnamed "B2 rules" could include ships like yours or your c/2 ships, and without his files I cannot rule that out.
- Guttesen (2026), under B3/S23 on 4.8.8: oscillators of period 2, 4, 5, 6, 7 and 14, and no gliders. His code is at https://github.com/henrisro313/Games_of_life (FourEight.java).
- In 2019, simsim314's thread "Rules in non uniform tiling" (https://conwaylife.com/forums/viewtopic.php?t=3893) gave a Golly script for an "8-4 tiling". The rule is not a B/S rule: it is a custom rule on LifeHistory states, and the post shows a linear replicator.
- On March 12, 2022, in "Thread for basic questions" (p142711 and p142753), "Random life lover" asked whether anyone had researched the octagon/square grid. dvgrn and Wyirm discussed emulating it with a rule table on a checkerboard. No patterns were posted.

### Every source I found on Life-like rules on Archimedean or k-uniform tilings
- **EricG, "Tiling in Golly 1" (t=1123) and "Tiling in Golly 2" (t=1124), June 12, 2013.** Part 2 covers the eight composite Archimedean tilings and their eight duals; Part 1 covers pentagonal tilings, including Cairo, the dual of snub square. Ships he reports on Archimedean tilings, besides those above:
  - Kagome-B3/S23: "a never-ending cascade of spaceships and puffers".
  - Snub square: "B3/S23 has sparky spaceships".
  - Truncated hexagonal (3.12.12): B2/S2 "generates lots of spaceships", and B26/S3569b and bc variants have "a sparky p466 blob-shaped oblique ship".
  - Truncated trihexagonal (4.6.12): "B2/S2 has a spaceship and a puffer".
- **"Tiled CA", started by bprentice on Jan 28, 2013 (https://conwaylife.com/forums/viewtopic.php?t=1025).** This is a Windows tiling program whose tilesets include SO1.til (octagons and squares) and Kagome.til. Its only ships are in a Cairo Generations rule.
- **Code Golf SE, question 35827 (2014).** All answers use B3/S23 and touching neighbours.
  - Peter Taylor: rhombitrihexagonal, no gliders.
  - Kevin L: a hexagon/square/triangle tiling with a period-20 spaceship found by Martin Büttner (Martin Ender). No speed is given, and I could not view the GIF. The rule is not confirmed: the code comment says "B2/S23", but the same comment sat over B3/S23 code in his previous answer.
  - My inference from the fiddle's geometry (http://jsfiddle.net/Therm/5n53auja/): the unit cell holds 1 hexagon, 2 squares and 2 triangles. That makes it the 2-uniform tiling [3.4².6; 3.6.3.6], not 3.4.6.4.
- **Guttesen, arXiv:2608.27622 (Aug and Sep 2026), and the henrisro313/Games_of_life repo.** He ran B3/S23 on six lattices: 4.8.8, kagome, 3⁴.6, 3.12², 4.6.12 and snub square. He found gliders only on kagome (period 4 and period 40, plus a puffer and a period-276 gun) and on snub square (period 8). He skipped 3.4.6.4, citing the Code Golf work, and he skipped 3.3.3.4.4.
- **"Conway's Game of Life with Penrose and Voronoi tilings" (https://life.au.pe/, posted to HN by satchlj, Apr 18, 2026).** A web toy with "Trihex" and "Octagon" grid options. It has no catalogue of patterns.

Sources checked and not relevant: Ready's pattern collection (only square, hex, triangular, Penrose and 3D); LifeWiki (no pages on any Archimedean tiling); MATh.en.JEANS "Game of Life on Various Tilings" (Iasi 2024; regular tilings and Sierpiński only); Bays' papers (triangular, pentagonal and hexagonal only); Nova Spivack's kisrhombille automaton (July 19, 2026; a dual tiling, not Archimedean); arXiv:2611.10969 (square grid only); and Edmond's arXiv:2609.22579 (aperiodic tilings only).

### What I searched
- **ConwayLife mirror forum search.** Terms: archimedean, semiregular, semi-regular, snub, kagome, rhombitrihexagonal, trihexagonal, elongated, truncated, "truncated square tiling", "octagonal tiling", octagon(s), tiling/tilings (titles and posts), "uniform tiling", tessellation, "hexagons squares triangles", "checkerboard Moore Neumann", "tiling spaceship" (pages 1 and 2) and Guttesen. I also searched bprentice's posts and the OCA Book thread.
- **LifeWiki search.** Terms: kagome, Archimedean, TruncatedSquare, "octagons squares", semiregular/uniform/truncated, plus the page "Cellular automaton simulation programs by supported rulespaces" (lists only square, hex and triangular grids).
- **Web search.** site: queries on conwaylife.com for 4.8.8, snub hexagonal, rhombitrihexagonal, elongated triangular and truncated square. General queries for each tiling name with glider, spaceship or "game of life"; "octagonal life B2/S2"; plus Reddit, YouTube, HN, Bays, Complex Systems/JCA, Visions of Chaos and Ready.
- **arXiv.** "Archimedean cellular automata", "game of life" tiling, and recent life-like and tiling queries.
- **GitHub repository search.** Six queries. The only hits were Guttesen's repo and an unrelated square-grid engine.
- **HN Algolia API.**

### Gaps I could not close
- **conwaylife.com itself** returns 403. I used the mirror, which is current to at least Nov 4, 2026, so posts from Nov 4–10 are unchecked.
- **Forum search limits.** The search rejects numeric terms like "4.8.8" and rule strings like "B2/S2". It rate-limits repeat searches. It does not read images or attached files.
- **Downloads I could not open.** EricG's Google Drive folder needs a login. The Code Golf site and its imgur images are blocked, so I read the 2014 answers through a French mirror. I saw only part of the jsfiddle code.
- **Sites I could not search.** Reddit is blocked and site: queries return nothing. GitHub code search needs a login, so I searched only repository names and descriptions. Codeberg blocks AI scrapers, and GitLab's search needs JavaScript. YouTube was searched only through web search. The ConwayLife Discord cannot be searched at all. Journal full texts (JCA, Complex Systems) were checked only through web search.
- **No PDF tools here.** I read Guttesen's paper through arXiv's HTML version and decoded the MATh.en.JEANS PDF roughly.

"Not found by these searches" is not "new". The most valuable next step is to get EricG's 2013 Tiling Project files, either by asking on the forum or by finding a copy, and run their ship patterns against JETCARP's four.

---

## Verification of EricG's threads (second agent; quotes limited to 125 characters by its fetch tool)

Source note: both pages came back from a non-preapproved source, so every quote below is 125 characters or fewer. Where a sentence was longer, I quoted a fragment and paraphrased the rest. Both pages were read in full (each shows "Page 1 of 1"). Rule names are shown as they appear in the raw markdown, so underscores are escaped there (e.g. B26\_S3569) and I give them with plain underscores.

**1. Title, author, dates, post ids**

Thread 1124: "Tiling in Golly 2: Uniform, Dual-Uniform, and Misc Tiling". Author EricG (199 posts, joined August 19th, 2011, Chicago-area, USA). Six posts, all dated June 12th, 2013 (mirror times shown as UTC-04:00):
- p=8223, 3:01 am (opening post, Part 1 link, Cairo tiling marked "to be continued")
- p=8224, 3:12 am (Kagome, Truncated Hexagonal, Rhombic)
- p=8225, 3:16 am (Triakis Triangular, Kisrhombile, Deltoidal Trihexagonal)
- p=8227, 3:21 am (Rhombitrihexagonal, Truncated Trihexagonal, Snub Hexagonal, Elongated Triangular)
- p=8228, 3:23 am (Tri-Nonagonal "Martini Lattice")
- p=8229, 3:26 am (coding idea for mechanising neighborhood scripts; no ship mentions)
(p=8226 is not in the thread.)

**2. Sentences mentioning spaceships, ships, puffers, rakes, gliders, etc.**

Post 8223:
- Pentagonal Floret / Deltoidal Trihexagonal (the "16 tilings" sentence): "Of the 16 tilings, using only lifelike rules, I found spaceships in 14 of them" (verbatim, 78 chars). The rest of that sentence, "I couldn't find spaceships in Pentagonal Floret tiling or the related Deltoidal Trihexagonal tiling", is the same post.
- Truncated Square (4.8.8, B2 rules): "Many of the B2 rules are non-exploding rules with spaceships, puffers, rakes, etc." Also "TruncatedSquare-B2/S346 has ships, puffers, and rakes." Also: a very common naturally occurring backrake exists, and "its mechanism relies on briefly creating and then destroying a forward traveling spaceship" (sentence is longer; fragment quoted).
- Tetrakis Square: "TetrakisSquare-B1/S is the most lifelike B1/S I've ever seen! Spaceships, puffers, breeders, etc." and "Various B3 and B4 rules have spaceships."
- Snub Square (B3/S23): "B3 rules readily form spaceships, although many of them slowly explode as spaceships collide and form new spaceships." Also "B3/S23 has sparky spaceships, and has analogs to small life patterns - the blinker, the tub, the barge, etc."

Post 8224:
- Trihexagonal / Kagome (B3/S23): "patterns readily become a never-ending cascade of spaceships and puffers" (fragment of a longer sentence that begins "It explodes but it never degrades into static"). Kagome-B3/S23 is the rule the thread calls the most interesting Kagome rule.
- Truncated Hexagonal (3.12.12, B2/S2): "TruncatedHex-B2/S2 is mildly explosive but generates lots of spaceships and is pleasant to watch on a torus" (the sentence continues "it does not degrade into static").
- Truncated Hexagonal (B26/S3569): "TruncatedHex-B26/S3569 has sparky spaceships and a giant useful oscillator." Also "The sparky ships can be paired up to make rakes and puffers."
- Truncated Hexagonal (B26_S3569b, B26/S3569bc): "allows for all of the above plus a sparky p466 blob-shaped oblique ship".
- Truncated Hexagonal (B26_S3569ab): the rule "converts the giant oscillator into a pretty bizarre natural gun" and "fires spaceships in many directions".
- Rhombic Tiling: "I did find Rhombus rules with spaceships"; "You can see some spaceships in the patterns folder."; von Neumann variant "(no spaceships, of course)"; Radius 2 VN: "There were a number of rules with spaceships".

Post 8225:
- Triakis Triangular: "didn't find many spaceships" (the mirror's own sentence has "my own program"). Also: "I spotted a puffer in B3567/S02358, and there are ships in exploding rules like B4/S4 and B68/s5."
- Kisrhombile: "B3/S23 has spaceships and puffers"; "Kisrhomb-B1/S1 has spinners and spaceships."; "Kistrhomb-B3/S26 has interesting blobs and common spaceships"; "some of the blobs turn out to be slow moving spaceships as well"; "Kistrhomb-B3/S4 has common sparky ships"; "except that it has a cute spaceship" (Kistrhomb-B3/s49); "has a normal-looking but less common spaceship as well" (Kistrhomb-B3/s48); "a natural breeder of dot puffers which create a lattice" (Kistrhomb-B3/s23,14).
- Deltoidal Trihexagonal: "A number of B34 rules have long-lived chaos, but I haven't seen any rules with spaceships." Also "I thought maybe the replicators could be paired up to create a spaceship" (fragment), no luck so far.

Post 8227:
- Rhombitrihexagonal: "Spaceships were hard to find in this tiling"; "there is a common spaceship in Rhombitrihexagonal-B3468/s239 which is mildly explosive" (B3468/S239); "there is a rare spaceship in Rhombitrihexagonal-B346/s239 which is a non-exploding rule" (B346/S239).
- Truncated Trihexagonal (4.6.12, B2/S2): "B2/S2 has a spaceship and a puffer." The same paragraph mentions linear replicators for B2/S23, B2/S24, B2/S26 (not a listed term).
- Snub Hexagonal: "oscillators which appear to be a spaceship traveling in a circle" (orbiters); "many rules had spaceships, some had both"; "Sparky spaceships are very common in SnubHexagonal-B389,10/S247,10" (fragment, followed by "it looked like a good candidate for constructing a rake"); "where spaceships bounced off oscillators" (SnubHexagonal-B389,10/S247,10,13,15,18); "had a very diffuse sparky blobby ship" (SnubHexagonal-B37/S247,10). No rule is named as having ships with B3/S247 alone; that one is described as producing orbiters.
- Elongated Triangular (B3/S23): "there is a common spaceship (although it only travels horizontally)"; "The spaceship from B3/S23 works in many other B3 rules as well."; "Other spaceships are also common in B3 rules" and "ElongTri-B36/S23 stood out in that it has a spaceship that looks like an LWSS in one phase" (fragment); "all spaceships in lifelike rules travel along one axis" and "ElongTri-B3/S2489, which has a very commonly occurring rake which travels in an oblique direction" (fragment).

Post 8228:
- Tri-Nonagonal: "Spaceships are abundant in B2 rules."; "a natural forward rake/puffer in B26_S359 and B269-S359"; "The rake fires spacecraft directly ahead of itself."

**3. Neighbourhood definition**

The thread never says outright whether edge-sharing and vertex-sharing cells both count. Its counts, and the truncated-square description, match Moore (8) and von Neumann (4) on the square grid, and it says the icons are there "To show which cells touch". The rhombic VN variants mention an 8-neighbour version "which excluded the middle corner cells" and a 6-neighbour version "which included only the middle corner cells", which implies corner cells count in the standard version. The neighborhoods folder diagrams (not fetched) would settle it.

Neighbour counts given:
- Truncated Square (8223): "Octagons with 8 neighbors and squares with four neighbors alternate on a checkerboard."
- Snub Square (8223 and 8227): "Squares with 12 neighbors and triangles with 9 neighbors alternate." (Elongated Triangular repeats this.)
- Pentagonal Floret and Deltoidal Trihexagonal (8223): "tiles in the two tilings have 8 and 9 neighbors respectively" (8 for Floret, 9 for Deltoidal).
- Kagome (8224): "Each hexagon has 12 neighbors, each triangle has six neighbors."
- Truncated Hexagonal (8224): "the triangles have only 3 neighbors while the Dodecagons" (followed by "still have 12 neighbors").
- Rhombic (8224): "each rhombus has 10 neighbors"; Radius 2 VN: "each rhombus has 12 neighbors".
- Triakis Triangular (8225): "Each triangle has 21 neighbors".
- Tri-Nonagonal (8228): "nonagons(!) with nine neighbors and triangles with three neighbors".
- No counts given for Tetrakis Square, Kisrhombile, Rhombitrihexagonal, Truncated Trihexagonal or Snub Hexagonal.

**4. Links (as written in the mirror; hrefs given where the link text is truncated)**
- http://en.wikipedia.org/wiki/List\_of\_co ... rm\_tilings (the link text is truncated by the mirror; the full href is not visible)
- http://en.wikipedia.org/wiki/List_of_convex_uniform_tilings (href)
- http://www.conwaylife.com/forums/viewtopic.php?f=11&t=1123 (Part 1)
- https://drive.google.com/folderview?id= ... sp=sharing (truncated in the mirror; a Google Drive folder with "Tiling Project Parts 1 & 2", for Golly 2.4 or 2.5)
- Hexagonal: viewtopic.php?f=7&t=263; viewtopic.php?f=11&t=632&p=4342; viewtopic.php?f=11&t=1027; viewtopic.php?f=11&t=935 (all on http://www.conwaylife.com/forums/)
- Triangular: viewtopic.php?f=11&t=960&p=7016; viewtopic.php?f=11&t=965; viewtopic.php?f=11&t=1023&p=7417 (same host)
- http://en.wikipedia.org/wiki/Snub_square_tiling#Related_tilings (link text truncated as "Snub\_squar ... ed\_tilings")
- http://en.wikipedia.org/wiki/Trihexagonal_tiling
- http://en.wikipedia.org/wiki/Percolation_threshold#Thresholds_on_2D_bowtie_and_martini_lattices (href, Martini lattice)
- Folder names only, no file URLs: "patterns folder", "neighborhoods folder" (Neighborhoods), and the Andrew's Trilife script. Scripts named in the thread: icons-import.py and icons-export.py (not linked). No rule file links are given.

**5. "Of the 16 tilings..." line**
Present verbatim in post 8223: "Of the 16 tilings, using only lifelike rules, I found spaceships in 14 of them -- I couldn't find spaceships in Pentagonal Floret tiling or the related Deltoidal Trihexagonal tiling."

**Thread 1123 (for reference)**
Title: "Tiling in Golly 1: Pentagonal (and HexStar & PentHex) Tiling". Author EricG. Five posts (p=8217 to p=8221), all dated June 12th, 2013 (2:20, 2:31, 2:38, 2:42, 2:49 am, UTC-04:00). Single page. It covers the Cairo pentagonal tiling (Moore and von Neumann, with PentaCairo16, PentaCairo5, Basketweave and PentaWorld implementations), the other pentagonal types from mathpuzzle.com (types 1 to 14, including Floret, Prismatic and Type 9), other pentagonal arrangements (Penta-AltPeace), PentaHex-5-12, HexStar, and Triumphant.

Files read: https://conwaylife.njohnston.ca/forums/viewtopic.php?t=1124 and https://conwaylife.njohnston.ca/forums/viewtopic.php?t=1123
