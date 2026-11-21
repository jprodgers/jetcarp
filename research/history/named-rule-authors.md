# Named-rule authors (all 166 rules in `named.json`)

Research date: 2026-11-10. Use this to credit rule authors in the generated named-rule catalogue.
Rows follow `crates/jetcarp-core/data/rules/named.json` in order. Authors are stated in our own
words. The `blurb` text in `named.json` is LifeWiki wording (GFDL; see `provenance-audit.md`) and
was not reused.

## Summary

| Status | Rules |
| --- | --- |
| author named by source | 74 |
| no author named | 90 |
| conflict | 2 (Life without death; Bombers) |
| source unreachable | 0 |

By family: Life-like 18 named / 69 none / 1 conflict; Life-like (B0) 1 / 5; Generations 41 / 3 /
1 conflict; Larger than Life 9 / 0; hexagonal and von Neumann Life 2 / 0; turmites 0 / 4;
elementary 1 / 9; Layered 2 (JETCARP's own).

"Author named by source" means that a source credits a named person with the rule, as author,
discoverer, namer, explorer or main student. The role is given in the author column. A person who
appears only as a cited reference (e.g. a web page by them) is listed under "no author named", with a
note.

**Notation check.** Every Life-like, B0, Generations and Larger-than-Life notation in `named.json`
matches its source row (LifeWiki lists, revisions below), and every MCell form converts to the same
rule (MCell's `M1` counts the centre cell; HROT does not). Points to note:

- LifeWiki's Generations list leaves out the "S" in three rows (Burst `B3468/0235678/C9`, Chenille
  `B24567/05678/C6`, Glissergy `B245678/035678/C5`). `named.json` has the S. These are LifeWiki
  typos, not conflicts.
- Spelling: `named.json` "Walled cities" vs LifeWiki "Walled Cities"; `2x2` and `2x2 2` vs LifeWiki
  "2×2" and "2×2 2".
- **Coexistence** (`…,NW1000…`) uses a weighted HROT neighbourhood. The README says such rules were
  left out, and `ltl.rs` rejects neighbourhoods other than NM, NN and N+. The entry probably fails to
  parse. Check it.
- Several LifeWiki rule pages are redirects: InverseLife → B3678/S23, Fredkin → Replicator, Mazectric
  and both "with Mice" rules → Maze, Long Life → LongLife, Gems Minor → Gems, Eppstein → B35/S236,
  Land Rush 2 → Land Rush, Morley → Move, DryLife without Death and Plow World → Life without death,
  Vote 4/5 → Vote.

**Two attributions found in the primary literature that LifeWiki does not give** (Toffoli and
Margolus, *Cellular Automata Machines*, MIT Press 1987, read from the PDF on Margolus's MIT page):
Life without death (B3/S012345678) is their INKSPOT rule (sec. 1.2, pp. 6–7), and Plow World
(B378/S012345678) is their LICHENS rule (sec. 5.2, pp. 39–40). Brian's Brain is in sec. 6.1
(pp. 47–49), with a footnote crediting Brian Silverman. The Vote 4/5 ("Anneal") rule is on p. 41,
credited to Gérard Vichniac.

## Sources (codes used in the table)

- **LW:<page>** = `https://conwaylife.com/wiki/<page>`, read through the mirror
  `https://conwaylife.njohnston.ca/wiki/<page>` (conwaylife.com returns HTTP 403). Revisions read:
  List of Life-like rules, 26 Sep 2026 (oldid 175445); List of Generations rules, 22 Dec 2025;
  Larger than Life, 18 Jan 2024 (oldid 144616); the 67 OCA rule pages linked from the Life-like
  list, downloaded on 2026-11-10 (e.g. OCA:Seeds 24 Jun 2026, OCA:HighLife 20 Jan 2026, OCA:Day &
  Night 25 Oct 2026). BibTeX keys `lifewikind_*`.
- **MC** = Mirek Wójtowicz's MCell rules lexicon, `https://mcell.ca/app/data/rules.txt` (the data
  behind `https://mcell.ca/pages/rules.html`; mirekw.com/ca/ now redirects to mcell.ca). Each entry
  ends "A rule by …" or "Author unknown". BibTeX `wojtowicznd_rules`.
- **CAM** = Toffoli & Margolus 1987, `https://people.csail.mit.edu/nhm/cam-book.pdf`. BibTeX
  `toffoli1987cellular`.
- **WP:<page>** = `https://en.wikipedia.org/wiki/<page>` (pointer to primary sources).
- Golly LtL help: `https://golly.sourceforge.io/Help/Algorithms/Larger_than_Life.html`.

## Table

| Name | Notation | Family | Discoverer / author (as named by the source) | Source checked | Status |
| --- | --- | --- | --- | --- | --- |
| AntiLife | `B0123478/S01234678` | Life-like (B0) | — | LW:List_of_Life-like_rules | no author named |
| InverseLife | `B0123478/S34678` | Life-like (B0) | Jason Rampe (named it; MCell: "a rule by") | LW:List_of_Life-like_rules; LW:OCA:B3678/S23; MC | author named by source |
| Wickstretcher And The Parasites | `B01356/S012345` | Life-like (B0) | — *(LifeWiki cites a forum thread but names no person)* | LW:List_of_Life-like_rules; LW:OCA:Wickstretcher_And_The_Parasites | no author named |
| Oils | `B014/S2` | Life-like (B0) | — | LW:List_of_Life-like_rules | no author named |
| Invertamaze | `B028/S0124` | Life-like (B0) | — | LW:List_of_Life-like_rules; LW:OCA:Invertamaze | no author named |
| Neon Blobs | `B08/S4` | Life-like (B0) | — | LW:List_of_Life-like_rules | no author named |
| H-trees | `B1/S012345678` | Life-like | — *(also called Christmas Life)* | LW:List_of_Life-like_rules; LW:OCA:H-trees | no author named |
| Fuzz | `B1/S014567` | Life-like | — | LW:List_of_Life-like_rules | no author named |
| Gnarl | `B1/S1` | Life-like | Kellie Evans *(MCell: "provided by Kellie Evans")* | LW:List_of_Life-like_rules; MC | author named by source |
| Snakeskin | `B1/S134567` | Life-like | — | LW:List_of_Life-like_rules | no author named |
| Solid islands grow amongst static | `B12678/S15678` | Life-like | — | LW:List_of_Life-like_rules | no author named |
| Replicator | `B1357/S1357` | Life-like | — *(MCell: "Author unknown". LifeWiki calls it one of two "Fredkin replicator" rules (the replication property, not authorship))* | LW:List_of_Life-like_rules; LW:OCA:Replicator; MC | no author named |
| Fredkin | `B1357/S02468` | Life-like | Edward Fredkin *(MCell only; LifeWiki redirects to Replicator and names no author)* | MC (Vote for Life "Fredkin", 13579); LW:OCA:Replicator | author named by source |
| Feux | `B1358/S0247` | Life-like | — *(MCell: "Author unknown")* | LW:List_of_Life-like_rules; MC (Vote for Life 1358) | no author named |
| Forrest of Ls | `B168/S236` | Life-like | — | LW:List_of_Life-like_rules | no author named |
| Seeds | `B2/S` | Life-like | Brian Silverman (first investigated it); named by Mirek Wójtowicz | LW:List_of_Life-like_rules; LW:OCA:Seeds; MC | author named by source |
| Live Free or Die | `B2/S0` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:Live_Free_or_Die | no author named |
| Serviettes | `B234/S` | Life-like | — *(MCell: "Author unknown"; LifeWiki cites Andreas Rejbrand's "Persian rugs" web page as a reference, not as the author)* | LW:List_of_Life-like_rules; LW:OCA:Serviettes; MC | no author named |
| Iceballs | `B25678/S5678` | Life-like | — *(also called Ice nine)* | LW:List_of_Life-like_rules; LW:OCA:Iceballs | no author named |
| Life without death | `B3/S012345678` | Life-like | Toffoli & Margolus describe it as INKSPOT (1987); MCell credits "Flakes" to Janko Gravner *(Earliest description found: Toffoli & Margolus 1987, sec. 1.2, pp. 6–7 (INKSPOT, birth on 3, no death); LifeWiki: "originally considered by Toffoli and Margolus"; MCell: "Flakes … A rule by Janko Gravner")* | LW:List_of_Life-like_rules; LW:OCA:Life_without_death; MC; CAM | conflict |
| DotLife | `B3/S023` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:DotLife | no author named |
| Star Trek | `B3/S0248` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:Star_Trek | no author named |
| Flock | `B3/S12` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:Flock | no author named |
| Mazectric | `B3/S1234` | Life-like | Charles A. Rockafellor *(MCell only)* | LW:List_of_Life-like_rules; LW:OCA:Maze; MC | author named by source |
| Maze | `B3/S12345` | Life-like | — *(MCell: "Author unknown")* | LW:List_of_Life-like_rules; LW:OCA:Maze; MC | no author named |
| Magnezones | `B3/S123678` | Life-like | — | LW:List_of_Life-like_rules | no author named |
| SnowLife | `B3/S1237` | Life-like | Joshua Herman (found by accident while working with Louis Kauffman) | LW:List_of_Life-like_rules; LW:OCA:SnowLife | author named by source |
| Corrosion of Conformity | `B3/S124` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:Corrosion_of_Conformity | no author named |
| EightFlock | `B3/S128` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:EightFlock | no author named |
| LowLife | `B3/S13` | Life-like | Jean-Claude Heudin (subject of his 1996 Complex Systems paper) *(Heudin is credited with studying it; LifeWiki does not say he found it first)* | LW:List_of_Life-like_rules; LW:OCA:LowLife | author named by source |
| Conway's Life | `B3/S23` | Life-like | John Horton Conway (published by Martin Gardner, Oct 1970; see 01-life-like.md) | LW:List_of_Life-like_rules; MC | author named by source |
| EightLife | `B3/S238` | Life-like | — *(also called Pulsar Life)* | LW:List_of_Life-like_rules; LW:OCA:EightLife | no author named |
| Shoots and Roots | `B3/S245678` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:Shoots_and_Roots | no author named |
| Lifeguard 2 | `B3/S4567` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:Lifeguard_2 | no author named |
| Coral | `B3/S45678` | Life-like | — *(MCell: "Author unknown")* | LW:List_of_Life-like_rules; LW:OCA:Coral; MC | no author named |
| 3-4 Life | `B34/S34` | Life-like | — *(LifeWiki: one of the earliest Life variants studied, citing Robert Wainwright's *Lifeline* vol. 4 (Dec 1971); MCell: "Author unknown", "early 1970s")* | LW:List_of_Life-like_rules; LW:OCA:3-4_Life; MC | no author named |
| Dance | `B34/S35` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:Dance | no author named |
| Bacteria | `B34/S456` | Life-like | — *(LifeWiki's reference is David Eppstein's "Interesting Rules" page)* | LW:List_of_Life-like_rules; LW:OCA:Bacteria | no author named |
| Never happy | `B345/S0456` | Life-like | — | LW:List_of_Life-like_rules | no author named |
| Blinkers | `B345/S2` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:Blinkers | no author named |
| Assimilation | `B345/S4567` | Life-like | — *(MCell: "Author unknown")* | LW:List_of_Life-like_rules; LW:OCA:Assimilation; MC | no author named |
| Long Life | `B345/S5` | Life-like | Andrew Trevorrow *(MCell: "A rule by Andrew Trevorrow"; LifeWiki: "studied by")* | LW:List_of_Life-like_rules; LW:OCA:LongLife; MC | author named by source |
| Spiral and polygonal growth | `B34568/S15678` | Life-like | — *(LifeWiki's reference is Dean Hickerson's page on B34568/S15678; his role is not stated)* | LW:List_of_Life-like_rules | no author named |
| Gems | `B3457/S4568` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:Gems | no author named |
| Gems Minor | `B34578/S456` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:Gems | no author named |
| Grounded Life | `B35/S23` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:Grounded_Life | no author named |
| Land Rush | `B35/S234578` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:Land_Rush | no author named |
| Eppstein | `B35/S236` | Life-like | Dean Hickerson and David Eppstein (explored it) | LW:List_of_Life-like_rules; LW:OCA:B35/S236 | author named by source |
| Bugs | `B3567/S15678` | Life-like | — *(LifeWiki's reference is Eppstein's "Interesting Rules" page)* | LW:List_of_Life-like_rules; LW:OCA:Bugs_(Life-like_cellular_automaton) | no author named |
| Cheerios | `B35678/S34567` | Life-like | — | LW:List_of_Life-like_rules | no author named |
| Holstein | `B35678/S4678` | Life-like | — *(LifeWiki cites "Paul Rendell's Holstein Cellular Automata Page" as a reference; authorship not stated)* | LW:List_of_Life-like_rules; LW:OCA:Holstein | no author named |
| Diamoeba | `B35678/S5678` | Life-like | Dean Hickerson *(MCell only)* | LW:List_of_Life-like_rules; LW:OCA:Diamoeba; MC | author named by source |
| Amoeba | `B357/S1358` | Life-like | — *(MCell: "Author unknown")* | LW:List_of_Life-like_rules; LW:OCA:Amoeba; MC | no author named |
| Pseudo Life | `B357/S238` | Life-like | — *(MCell: "Author unknown")* | LW:List_of_Life-like_rules; LW:OCA:Pseudo_Life; MC | no author named |
| Geology | `B3578/S24678` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:Geology | no author named |
| HighFlock | `B36/S12` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:HighFlock | no author named |
| 2x2 | `B36/S125` | Life-like | — *(MCell: "Author unknown")* | LW:List_of_Life-like_rules; LW:OCA:2×2; MC | no author named |
| IronFlock | `B36/S128` | Life-like | — | LW:List_of_Life-like_rules | no author named |
| HighLife | `B36/S23` | Life-like | Nathan Thompson (first considered it, 1994); named by John Conway | LW:List_of_Life-like_rules; LW:OCA:HighLife; MC | author named by source |
| Land Rush 2 | `B36/S234578` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:Land_Rush | no author named |
| Virus | `B36/S235` | Life-like | phdanielli (name comes from the forum thread "Virus (B36/S235)", 22 May 2013) *(LifeWiki footnotes the name to that thread)* | LW:List_of_Life-like_rules; LW:OCA:Virus | author named by source |
| IronLife | `B36/S238` | Life-like | — | LW:List_of_Life-like_rules | no author named |
| sqrt replicator rule | `B36/S245` | Life-like | — *(formerly "logarithmic replicator rule")* | LW:List_of_Life-like_rules; LW:OCA:Sqrt_replicator_rule | no author named |
| Slow Blob | `B367/S125678` | Life-like | — | LW:List_of_Life-like_rules | no author named |
| DrighLife | `B367/S23` | Life-like | — | LW:List_of_Life-like_rules | no author named |
| 2x2 2 | `B3678/S1258` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:2×2_2 | no author named |
| Castles | `B3678/S135678` | Life-like | melwin22 (ConwayLife.com forums) | LW:List_of_Life-like_rules; LW:OCA:Castles | author named by source |
| Stains | `B3678/S235678` | Life-like | — *(MCell: "Author unknown")* | LW:List_of_Life-like_rules; LW:OCA:Stains; MC | no author named |
| Day & Night | `B3678/S34678` | Life-like | Nathan Thompson (explored it from April 1997; David Bell discussed it in Nov 1997) | LW:List_of_Life-like_rules; LW:OCA:Day_&_Night; MC | author named by source |
| LowDeath without Death | `B368/S012345678` | Life-like | — | LW:List_of_Life-like_rules | no author named |
| LowFlockDeath | `B368/S128` | Life-like | — | LW:List_of_Life-like_rules | no author named |
| Life SkyHigh | `B368/S236` | Life-like | — | LW:List_of_Life-like_rules | no author named |
| LowDeath | `B368/S238` | Life-like | — *(named after a Geometry Dash level)* | LW:List_of_Life-like_rules; LW:OCA:LowDeath | no author named |
| Morley | `B368/S245` | Life-like | Stephen Morley (namesake: Golly named it after him because he studied it extensively); discoverer not named *(MCell calls it "Move", "Author unknown")* | LW:List_of_Life-like_rules; LW:OCA:Move; MC ("Move") | author named by source |
| DryLife without Death | `B37/S012345678` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:Life_without_death | no author named |
| DryFlock | `B37/S12` | Life-like | — | LW:List_of_Life-like_rules | no author named |
| Mazectric with Mice | `B37/S1234` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:Maze | no author named |
| Maze with Mice | `B37/S12345` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:Maze | no author named |
| DryLife | `B37/S23` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:DryLife | no author named |
| Plow World | `B378/S012345678` | Life-like | — *(Same rule as LICHENS in Toffoli & Margolus 1987, sec. 5.2, pp. 39–40 (births on 3, 7, 8; no deaths). The identification is ours, read from the book's table)* | LW:List_of_Life-like_rules; LW:OCA:Life_without_death; CAM | no author named |
| Coagulations | `B378/S235678` | Life-like | — *(MCell: "Author unknown")* | LW:List_of_Life-like_rules; LW:OCA:Coagulations; MC | no author named |
| Pedestrian Life without Death | `B38/S012345678` | Life-like | — | LW:List_of_Life-like_rules | no author named |
| Pedestrian Flock | `B38/S12` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:Pedestrian_Flock | no author named |
| HoneyFlock | `B38/S128` | Life-like | — | LW:List_of_Life-like_rules | no author named |
| Pedestrian Life | `B38/S23` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:Pedestrian_Life | no author named |
| HoneyLife | `B38/S238` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:HoneyLife | no author named |
| Electrified Maze | `B45/S12345` | Life-like | — | LW:List_of_Life-like_rules | no author named |
| Oscillators Rule | `B45/S1235` | Life-like | — *(LifeWiki cites the archived page "Cool Universes in the Cosmos of Cellular Automata")* | LW:List_of_Life-like_rules | no author named |
| Walled cities | `B45678/S2345` | Life-like | David Macfarlane *(LifeWiki capitalises it "Walled Cities")* | LW:List_of_Life-like_rules; LW:OCA:Walled_Cities; MC | author named by source |
| Majority | `B45678/S5678` | Life-like | — *(LifeWiki's reference is Eppstein's "Interesting Rules" page)* | LW:List_of_Life-like_rules | no author named |
| Vote 4/5 | `B4678/S35678` | Life-like | Gérard Vichniac (his "Anneal" variant of the voting rule) *(Toffoli & Margolus 1987, p. 41, credit ANNEAL (this rule) to Vichniac (1986))* | LW:List_of_Life-like_rules; LW:OCA:Vote; MC; CAM | author named by source |
| Lifeguard 1 | `B48/S234` | Life-like | — | LW:List_of_Life-like_rules | no author named |
| Rings 'n' Slugs | `B56/S14568` | Life-like | — | LW:List_of_Life-like_rules; LW:OCA:Rings_%27n%27_Slugs | no author named |
| Vote | `B5678/S45678` | Life-like | Gérard Vichniac (standard voting rule) *(Majority vote over the 3 × 3 block; MCell lists it without an author)* | LW:List_of_Life-like_rules; LW:OCA:Vote; MC | author named by source |
| 9:43 at Knight | `B34578/S237/C4` | Generations | EricG *(LifeWiki only; not in MCell)* | LW:List_of_Generations_rules | author named by source |
| Banners | `B3457/S2367/C5` | Generations | Mirek Wójtowicz | LW:List_of_Generations_rules; MC | author named by source |
| BelZhab | `B23/S23/C8` | Generations | — *(MCell: "Author unknown")* | LW:List_of_Generations_rules; MC | no author named |
| BelZhab Sediment | `B23/S145678/C8` | Generations | Jason Rampe | LW:List_of_Generations_rules; MC | author named by source |
| Bloomerang | `B34678/S234/C24` | Generations | John Elliott (July 2000) | LW:List_of_Generations_rules; MC | author named by source |
| Bombers | `B24/S345/C25` | Generations | LifeWiki: Mirek Wójtowicz; MCell: "Author unknown" *(The two sources disagree)* | LW:List_of_Generations_rules; MC | conflict |
| Brain 6 | `B246/S6/C3` | Generations | Michael Sweney | LW:List_of_Generations_rules; MC | author named by source |
| Brian's Brain | `B2/S/C3` | Generations | Brian Silverman *(Date conflict: LifeWiki says "mid-1990s", but Toffoli & Margolus already published it in 1987 (sec. 6.1, with a footnote crediting Silverman))* | LW:List_of_Generations_rules; LW:Brian%27s_Brain; MC; CAM | author named by source |
| Burst | `B3468/S0235678/C9` | Generations | Michael Sweney *(LifeWiki writes B3468/0235678/C9 (missing "S"; a typo there))* | LW:List_of_Generations_rules; MC | author named by source |
| Burst II | `B3468/S235678/C9` | Generations | Michael Sweney | LW:List_of_Generations_rules; MC | author named by source |
| Caterpillars | `B378/S124567/C4` | Generations | Mirek Wójtowicz | LW:List_of_Generations_rules; MC | author named by source |
| Chenille | `B24567/S05678/C6` | Generations | — *(MCell: "Author unknown"; LifeWiki writes B24567/05678/C6 (missing "S"))* | LW:List_of_Generations_rules; MC | no author named |
| Circuit Genesis | `B1234/S2345/C8` | Generations | Charles A. Rockafellor | LW:List_of_Generations_rules; MC | author named by source |
| Cooties | `B2/S23/C8` | Generations | Rudy Rucker | LW:List_of_Generations_rules; MC | author named by source |
| Ebb and Flow | `B36/S012478/C18` | Generations | Michael Sweney | LW:List_of_Generations_rules; MC | author named by source |
| Ebb and Flow II | `B37/S012468/C18` | Generations | Michael Sweney | LW:List_of_Generations_rules; MC | author named by source |
| Faders | `B2/S2/C25` | Generations | Rudy Rucker and John Walker | LW:List_of_Generations_rules; MC | author named by source |
| Fireworks | `B13/S2/C21` | Generations | John Elliott | LW:List_of_Generations_rules; MC | author named by source |
| Flaming Starbows | `B23/S347/C8` | Generations | Charles A. Rockafellor | LW:List_of_Generations_rules; MC | author named by source |
| Frogs | `B34/S12/C3` | Generations | Scott Robert Ladd | LW:List_of_Generations_rules; MC | author named by source |
| Frozen Spirals | `B23/S356/C6` | Generations | Mirek Wójtowicz | LW:List_of_Generations_rules; MC | author named by source |
| Glisserati | `B245678/S035678/C7` | Generations | John Elliott (May 2000) | LW:List_of_Generations_rules; MC | author named by source |
| Glissergy | `B245678/S035678/C5` | Generations | John Elliott (May 2000) *(LifeWiki writes B245678/035678/C5 (missing "S"))* | LW:List_of_Generations_rules; MC | author named by source |
| Lava | `B45678/S12345/C8` | Generations | Mirek Wójtowicz | LW:List_of_Generations_rules; MC | author named by source |
| Lines | `B458/S012345/C3` | Generations | Anders Starmark (September 2000) | LW:List_of_Generations_rules; MC | author named by source |
| Living On The Edge | `B3/S345/C6` | Generations | Mirek Wójtowicz (February 2000) | LW:List_of_Generations_rules; MC | author named by source |
| Meteor Guns | `B3/S01245678/C8` | Generations | Charles A. Rockafellor | LW:List_of_Generations_rules; MC | author named by source |
| Nova | `B2478/S45678/C25` | Generations | Mirek Wójtowicz | LW:List_of_Generations_rules; MC | author named by source |
| OrthoGo | `B2/S3/C4` | Generations | John Elliott (December 1999) | LW:List_of_Generations_rules; MC | author named by source |
| Prairie on fire | `B34/S345/C6` | Generations | Mirek Wójtowicz | LW:List_of_Generations_rules; MC | author named by source |
| RainZha | `B23/S2/C8` | Generations | Rudy Rucker and John Walker | LW:List_of_Generations_rules; MC | author named by source |
| Rake | `B2678/S3467/C6` | Generations | Brian Prentice | LW:List_of_Generations_rules; MC | author named by source |
| SediMental | `B25678/S45678/C4` | Generations | John Elliott | LW:List_of_Generations_rules; MC | author named by source |
| Snake | `B25/S03467/C6` | Generations | Brian Prentice | LW:List_of_Generations_rules; MC | author named by source |
| Soft Freeze | `B38/S13458/C6` | Generations | John Elliott | LW:List_of_Generations_rules; MC | author named by source |
| Spirals | `B234/S2/C5` | Generations | — *(MCell: "Author unknown")* | LW:List_of_Generations_rules; MC | no author named |
| Star Wars | `B2/S345/C4` | Generations | Mirek Wójtowicz (March 1999) | LW:List_of_Generations_rules; MC; MCell history page | author named by source |
| Sticks | `B2/S3456/C6` | Generations | Rudy Rucker | LW:List_of_Generations_rules; MC | author named by source |
| Swirl | `B34/S23/C8` | Generations | Scott Robert Ladd | LW:List_of_Generations_rules; MC | author named by source |
| Thrill Grill | `B34/S1234/C48` | Generations | John Elliott (July 2000) | LW:List_of_Generations_rules; MC | author named by source |
| Transers | `B26/S345/C5` | Generations | John Elliott | LW:List_of_Generations_rules; MC | author named by source |
| Transers II | `B26/S0345/C6` | Generations | Michael Sweney | LW:List_of_Generations_rules; MC | author named by source |
| Wanderers | `B34678/S345/C5` | Generations | Mirek Wójtowicz | LW:List_of_Generations_rules; MC | author named by source |
| Worms | `B25/S3467/C6` | Generations | Mirek Wójtowicz (8 April 1999) *(The date comes from the Worms example file quoted on mcell.ca/pages/mcell.html)* | LW:List_of_Generations_rules; MC; MCell .mcl example | author named by source |
| Xtasy | `B2356/S1456/C16` | Generations | John Elliott (July 2000) | LW:List_of_Generations_rules; MC | author named by source |
| Majority (LtL) | `R4,C2,S40-80,B41-81` | Larger than Life | David Griffeath *(MCell: a voter model)* | LW:Larger_than_Life; MC | author named by source |
| Bosco's Rule | `R5,C2,S33-57,B34-45` | Larger than Life | Kellie Evans *(MCell calls it "Bugs" and says it was found on the CAM-8; Bosco is a period-166 oscillator Evans found (Golly help))* | LW:Larger_than_Life; MC ("Bugs"); Golly LtL help | author named by source |
| Waffle | `R7,C2,S99-199,B75-170` | Larger than Life | Kellie Evans | LW:Larger_than_Life; MC | author named by source |
| Majorly | `R7,C2,S112-224,B113-225` | Larger than Life | David Griffeath | LW:Larger_than_Life; MC | author named by source |
| Globe | `R8,C2,S163-223,B74-252` | Larger than Life | Mirek Wójtowicz | LW:Larger_than_Life; MC | author named by source |
| Bugsmovie | `R10,C2,S122-211,B123-170` | Larger than Life | David Griffeath | LW:Larger_than_Life; MC | author named by source |
| ModernArt | `R10,C255,S1-2,B3` | Larger than Life | Charles A. Rockafellor | LW:Larger_than_Life; MC | author named by source |
| Factorio | `R3,C2,S2,B3,N+` | Larger than Life | H. H. P. M. P. Cole *(LifeWiki only (forum thread t=6166 not read))* | LW:Larger_than_Life | author named by source |
| Coexistence | `R4,C2,S9,14,16-17,21,23-24,30-31,37-38,44-45,51-52,58-59,B3,10,17,21,24,31,38,45,52,59,NW100010001000000000000000000000777000100707001000777000000000000000000000100010001` | Larger than Life | bubblegum *(LifeWiki only (forum thread t=5174 not read). Weighted NW neighbourhood: ltl.rs accepts only NM, NN, N+, so this entry probably does not parse)* | LW:Larger_than_Life | author named by source |
| Hexagonal Life | `B2/S34H` | Life-like, hexagonal | drc (name comes from the title of drc's forum thread "B2/S34H (Hexagonal Life)", 18 Jan 2016) *(Name unconfirmed as a LifeWiki rule name (as the README notes))* | LW:Hexagonal_neighbourhood; forum p=26870 (see novelty-05) | author named by source |
| Gnarl (von Neumann) | `B1/S0V` | Life-like, von Neumann | Kellie Evans *(MCell: named in homage to Rudy Rucker; LifeWiki's disambiguation page names no author)* | LW:Gnarl; MC (LtL "Gnarl", R1,…,NN) | author named by source |
| Turmite RLR | `Turmite RLR` | Agents | — *(Wikipedia shows this ant without credit. Multi-colour ants as a family: Greg Turk and Jim Propp (Wikipedia, citing Gale, Propp, Sutherland & Troubetzkoy 1995))* | WP:Langton%27s_ant | no author named |
| Turmite LRRRRRLLR | `Turmite LRRRRRLLR` | Agents | — *(Wikipedia shows this ant without credit. Multi-colour ants as a family: Greg Turk and Jim Propp (Wikipedia, citing Gale, Propp, Sutherland & Troubetzkoy 1995))* | WP:Langton%27s_ant | no author named |
| Turmite LLRRRLRLRLLR | `Turmite LLRRRLRLRLLR` | Agents | — *(Wikipedia shows this ant without credit. Multi-colour ants as a family: Greg Turk and Jim Propp (Wikipedia, citing Gale, Propp, Sutherland & Troubetzkoy 1995))* | WP:Langton%27s_ant | no author named |
| Turmite RRLLLRLLLRRR | `Turmite RRLLLRLLLRRR` | Agents | — *(Wikipedia shows this ant without credit. Multi-colour ants as a family: Greg Turk and Jim Propp (Wikipedia, citing Gale, Propp, Sutherland & Troubetzkoy 1995))* | WP:Langton%27s_ant | no author named |
| Rule 18 | `W18` | Elementary | — *(Rule number in Wolfram's 1983 numbering (see history file 04); no individual discoverer named)* | WP:Elementary_cellular_automaton | no author named |
| Rule 22 | `W22` | Elementary | — *(Rule number in Wolfram's 1983 numbering (see history file 04); no individual discoverer named)* | WP:Elementary_cellular_automaton | no author named |
| Rule 45 | `W45` | Elementary | — *(Rule number in Wolfram's 1983 numbering (see history file 04); no individual discoverer named)* | WP:Elementary_cellular_automaton | no author named |
| Rule 54 | `W54` | Elementary | — *(Rule number in Wolfram's 1983 numbering (see history file 04); no individual discoverer named)* | WP:Elementary_cellular_automaton | no author named |
| Rule 60 | `W60` | Elementary | — *(Rule number in Wolfram's 1983 numbering (see history file 04); no individual discoverer named)* | WP:Elementary_cellular_automaton | no author named |
| Rule 73 | `W73` | Elementary | — *(Rule number in Wolfram's 1983 numbering (see history file 04); no individual discoverer named)* | WP:Elementary_cellular_automaton | no author named |
| Rule 105 | `W105` | Elementary | — *(Rule number in Wolfram's 1983 numbering (see history file 04); no individual discoverer named)* | WP:Elementary_cellular_automaton | no author named |
| Rule 126 | `W126` | Elementary | — *(Rule number in Wolfram's 1983 numbering (see history file 04); no individual discoverer named)* | WP:Elementary_cellular_automaton | no author named |
| Rule 150 | `W150` | Elementary | — *(Rule number in Wolfram's 1983 numbering (see history file 04); no individual discoverer named)* | WP:Elementary_cellular_automaton | no author named |
| Rule 184 | `W184` | Elementary | Earliest studies: Wentian Li (1987) and Krug & Spohn (1988); later used for traffic flow (e.g. Biham, Middleton & Levine 1992) *(Wolfram numbering; Wikipedia says the first traffic use is hard to pinpoint)* | WP:Elementary_cellular_automaton; WP:Rule_184 | author named by source |
| Layered parity | `Layer 2/01` | Layered | Jimmie Rodgers (JETCARP, 2026) *(JETCARP's own rule family; see history file 15 and novelty-04)* | JETCARP README ("Layered") | author named by source |
| Layered sum mod 3 | `Layer 3/012` | Layered | Jimmie Rodgers (JETCARP, 2026) *(JETCARP's own rule family; see history file 15 and novelty-04)* | JETCARP README ("Layered") | author named by source |

## Notes for the catalogue

- Credit lines could take the form "Rule by X (MCell lexicon)" or "First considered by X in YEAR
  (LifeWiki)". Keep the source, because MCell and LifeWiki sometimes disagree (Bombers) or credit
  different people (Life without death: Toffoli and Margolus's INKSPOT in 1987, but "Flakes" by
  Janko Gravner in MCell).
- Brian's Brain: use "Brian Silverman (published by Toffoli and Margolus, 1987)". Do not use
  LifeWiki's "mid-1990s".
- Turmites: the four named ants have no individual author. Credit the multi-colour generalisation
  to Greg Turk and Jim Propp (Gale, Propp, Sutherland and Troubetzkoy, *Math. Intelligencer* 17(3),
  1995, DOI 10.1007/BF03024370).
- Elementary rules: credit Wolfram's numbering (see history file 04). Rule 184 also has the
  credits given in its row.

## Unverified

- Credits found in only one source: Fredkin (Edward Fredkin), Mazectric (Charles A. Rockafellor),
  Diamoeba (Dean Hickerson) and Gnarl von Neumann (Kellie Evans) are in MCell only. 9:43 at Knight
  (EricG), Factorio (H. H. P. M. P. Cole), Coexistence (bubblegum), Castles (melwin22), Virus
  (phdanielli), SnowLife (Joshua Herman) and LowLife (Heudin) are in LifeWiki only. The forum posts
  behind them were not read.
- Rule 184's earliest-study credits (Li 1987; Krug and Spohn 1988) come from Wikipedia's article.
  Krug and Spohn's DOI was confirmed on Crossref, but neither paper was read.
- Hexagonal Life: the thread title is from novelty-05's reading of forum post p=26870 (mirror). The
  thread was not re-read here.

## Search log

- curl (mirror, data only, parsed with `python3 -I`): List_of_Life-like_rules,
  List_of_Generations_rules, and the 67 `OCA:` pages linked from the Life-like list.
  mcell.ca/app/data/rules.txt, /pages/families.html, /pages/mcell.html.
  people.csail.mit.edu/nhm/cam-book.pdf (OCR text layer).
- WebFetch: the same two lists (author columns); /wiki/Larger_than_Life; en.wikipedia.org
  Langton%27s_ant, Rule_184, Brian%27s_Brain; complex-systems.com/abstracts/v10_i05_a03 (Heudin).
- Crossref API: Vichniac 1984 and 1986; Gale et al. 1995; Krug and Spohn 1988.
- Comparison scripts (scratch scripts, not kept): `compare.py` (named.json vs LifeWiki and MCell),
  `gen_named.py` (this table).
