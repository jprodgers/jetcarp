# 21. Penrose tilings (kites and darts P2, rhombs P3): history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.

## Short history

- **Background: aperiodic sets.** Robert Berger showed in 1966 that the domino (Wang tile) problem is
  undecidable, which needed an aperiodic set of Wang tiles [berger1966undecidability]. Raphael M.
  Robinson simplified the construction in 1971 [robinson1971undecidability].
- **Roger Penrose** found much smaller aperiodic sets in the early 1970s. His first, pentagon-based
  set (P1) is usually dated to his 1974 article on aesthetics in mathematics in the *Bulletin of the
  IMA* (see Unverified). He then reduced it to two tiles: the **kite and dart (P2)** and the **thick and
  thin rhombs (P3)**. His UK patent application dates from 25 Jun 1975; US patent 4,133,152, "Set of
  tiles for covering a surface", was filed 24 Jun 1976 and granted 9 Jan 1979. Its claims describe two
  quadrilateral tiles that fill the plane only non-periodically [penrose1979set]. Penrose's own account
  is "Pentaplexity" (*Eureka* 39, 1978; reprinted in *The Mathematical Intelligencer* 2(1), 1979)
  [penrose1979pentaplexity].
- **Martin Gardner** made the tilings widely known in his "Mathematical Games" column, *Scientific
  American* 236(1), January 1977, "Extraordinary nonperiodic tiling that enriches the theory of tiles"
  [gardner1977extraordinary]. Wikipedia credits John Conway with the names of the vertex
  neighbourhoods (sun, star, ace and so on) and notes that Robert Ammann found the rhomb set
  independently in 1976 (see Unverified).
- **Robinson triangles.** Each Penrose tile splits into two mirror-image golden triangles (acute 36°,
  obtuse 108°). They deflate exactly and generate both P2 and P3. The name comes from Raphael Robinson's
  1975 notes; Grünbaum and Shephard (§10.3 of *Tilings and Patterns*) give the standard account
  [grunbaum1987tilings]. JETCARP's `penrose.rs` cites §10.3.
- **N. G. de Bruijn** (1981) gave an algebraic theory of the tilings in two parts in *Indagationes
  Mathematicae*. It is known for the "pentagrid" (dual-grid) construction and for projection from a
  five-dimensional lattice (description from secondary sources) [debruijn1981algebraic;
  debruijn1981algebraicii].
- **CAs on Penrose tilings.** Hill, Stepney and Wan studied Life's ash on Penrose tilings (ECAL 2005)
  [hill2005penrose]. Owens and Stepney catalogued still lifes and oscillators and found no gliders in
  Life (2010) [owens2010game]. Goucher found the first glider that travels across an aperiodic tiling,
  in a four-state rule (JCA 2012) [goucher2012gliders]. See also Bailey and Lindsey (2017)
  [bailey2017game], sandpile identities by Fersula, Noûs and Perrot (2020) [fersula2020sandpile], and
  Langton's ant on Penrose tilings (ptiles/ant, GitHub, 2024, MIT) [ptilesnd_ant]. All are from novelty-05,
  -07 and -08.

## Key primary citations

- [penrose1979pentaplexity] R. Penrose, "Pentaplexity: A Class of Non-Periodic Tilings of the Plane",
  *The Mathematical Intelligencer* 2(1):32–37 (1979). DOI 10.1007/BF03024384 (first printed in *Eureka*
  39:16–22, 1978). Verified: Crossref (Intelligencer); the Eureka details are from Wikipedia.
- [penrose1979set] Roger Penrose, "Set of tiles for covering a surface", US Patent 4,133,152, filed 24
  Jun 1976 (priority GB 26904/75, 25 Jun 1975), granted 9 Jan 1979.
  https://patents.google.com/patent/US4133152A/en . Verified: Google Patents page.
- [gardner1977extraordinary] Martin Gardner, "Mathematical Games: Extraordinary nonperiodic tiling that
  enriches the theory of tiles", *Scientific American* 236(1):110–121 (January 1977). DOI
  10.1038/scientificamerican0177-110. Verified: Crossref (title "Mathematical Games"; Crossref's page
  range "110–221" is a data error, and 110–121 follows Wikipedia).
- [debruijn1981algebraic] N. G. de Bruijn, "Algebraic theory of Penrose's non-periodic tilings of the
  plane. I", *Indagationes Mathematicae (Proceedings)* 84(1):39–52 (1981). DOI
  10.1016/1385-7258(81)90016-0. Verified: Crossref.
- [debruijn1981algebraicii] Same, part II, pp. 53–66. DOI 10.1016/1385-7258(81)90017-2. Verified:
  Crossref.
- [grunbaum1987tilings] Grünbaum and Shephard, *Tilings and Patterns* (1987), ch. 10 (Penrose tilings,
  §10.3 Robinson's triangles). Verified: archive.org record. The section number comes from JETCARP's
  source comment; Wikipedia cites pp. 537–547.

## Software that ran it before JETCARP

- **Jeff Preshing**, "Penrose Tiling Explained" (blog, 31 Aug 2011) with `penrose.py`: subdivision of
  red (36°) and blue (108°) golden triangles drawn with pycairo. No licence is stated
  [preshing2011penrose]. The provenance audit notes that JETCARP's `deflate_p3` and P3 seed wheel follow
  this script.
- CA research code: Hill/Stepney/Wan (2005), Owens/Stepney (2010), Goucher (2012), Fersula et al.
  (2020); Ready (GollyGang) meshes, which Edmond used to check Goucher's glider (novelty-08); ptiles/ant.

## People JETCARP should credit

**Roger Penrose**; Martin Gardner (popularisation); Raphael M. Robinson (triangles); N. G. de Bruijn
(algebraic theory); Robert Ammann (independent discovery, if confirmed); Jeff Preshing (the deflation
script JETCARP follows); for CAs: Margaret Hill, Susan Stepney, Francis Wan, Nick Owens, Adam P.
Goucher.

## How JETCARP's version relates

`penrose.rs` builds P2 and P3 by exact Robinson-triangle deflation from a five-fold "sun", pairing
mirror triangles at the end. Levels are size classes (500 / 4,000 / 35,000 … tiles), and levels 7–8
(up to about 18 million tiles) are built in exact Z[ζ₁₀] coordinates. Tests check vertex stars and the
φ tile ratios (README "Hat and Penrose tilings", "Exact aperiodic patches").

## Unverified

- R. Penrose, "The role of aesthetics in pure and applied mathematical research", *Bull. Inst. Math.
  Appl.* 10:266ff (1974). Cited by Wikipedia as the first publication (P1); no DOI, not fetched.
- Ammann's independent 1976 discovery of the rhombs; Conway's naming of vertex types; that Conway named
  the kite and dart (Wikipedia does not say who named them).
- Robinson's 1975 unpublished notes as the source of "Robinson triangles".
- Exact *Eureka* page numbers; Gardner's reprint as ch. 1 of *Penrose Tiles to Trapdoor Ciphers*
  (Freeman 1989).
- The link to quasicrystals (Shechtman et al. 1984), a common part of the story, was not checked.
- The pentagrid description of de Bruijn's papers (the papers' text was not read: TU/e host refused the
  connection and ScienceDirect returned 403).

## Search log

- WebFetch: en.wikipedia.org "Penrose tiling" (pointer to sources; the first 100,000 characters were
  read), patents.google.com US4133152A, preshing.com 2011-08-31.
- Crossref API: 10.1007/BF03024384, 10.1038/scientificamerican0177-110, 10.1016/1385-7258(81)90016-0 and
  -90017-2, 10.1007/BF01418780, 10.1090/memo/0066, 10.1007/11553090_48; query for Owens and Stepney
  (10.1007/978-1-84996-217-9_18); Penrose 1974 (no record).
- GitHub API: ptiles/ant (created 21 Feb 2024, MIT).
- Failed: alexandria.tue.nl (connection refused), sciencedirect.com (403).
