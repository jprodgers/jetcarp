# 17. Regular and Archimedean tilings: history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.

## Short history

- **The three regular tilings** (triangles 3^6, squares 4^4, hexagons 6^3) are ancient as patterns. The
  first systematic mathematical treatment of edge-to-edge tilings by regular polygons, including the
  eight further **Archimedean (semi-regular, 1-uniform)** tilings, is Johannes Kepler's in Book II of
  *Harmonices Mundi* (Linz, 1619). The 1619 edition's title page names Book II "Architectonicus, seu
  ex geometria figurata" [kepler1619harmonices]. Book II is commonly described as the first catalogue
  of the 11 uniform tilings of the plane; it also treats star polygons and tilings of the sphere
  (Jardine's "Imperfect Congruence" site, used only as a pointer). An English translation was edited
  by Duncan and Aiton (American Philosophical Society, 1997) [kepler1997harmony].
- Kepler's results were examined in detail by Darrah Chavey in his 1984 PhD thesis on periodic
  tilings by regular polygons (University of Wisconsin–Madison) [chavey1984periodic].
- **Modern standard reference.** Branko Grünbaum and G. C. Shephard surveyed the subject "from Kepler
  to the present" in "Tilings by regular polygons", *Mathematics Magazine* 50(5) (1977)
  [grunbaum1977tilings], and then in the book *Tilings and Patterns* (W. H. Freeman, 1987; second
  edition, Dover, 2016) [grunbaum1987tilings; grunbaum2016tilings]. They also classified the 91 types
  of isogonal (vertex-transitive) tilings [grunbaum1978ninetyone].
- **Vertex-configuration notation.** A vertex type is written as the cyclic list of polygons round a
  vertex, e.g. 3.4.6.4 or, with exponents, 3^2.4.3.4. The style used in JETCARP's family names
  (`3.6.3.6`, `4.8.8`) is commonly called Cundy–Rollett notation, after H. M. Cundy and A. P. Rollett,
  *Mathematical Models* (Oxford, Clarendon Press; 2nd ed. 1961) [cundy1961mathematical]. Grünbaum and
  Shephard use the exponent form (3^6, 3^2.4.3.4). Later notations for automatic generation include
  Gomez-Jauregui et al.'s (2012) and the GomJau-Hogg notation with the ANTWERP v3.0 generator (2021)
  [gomezjauregui2012generation; gomezjauregui2021gomjau].
- **The eleven 1-uniform tilings** in JETCARP's names: 3^6 (triangles), 4^4 (squares), 6^3 (hexagons),
  3.6.3.6 trihexagonal (kagome), 4.8.8 truncated square, 3.12.12 truncated hexagonal, 4.6.12 truncated
  trihexagonal, 3.4.6.4 rhombitrihexagonal, 3.3.3.4.4 elongated triangular, 3.3.4.3.4 snub square, and
  3.3.3.3.6 snub hexagonal (chiral). These are the 1-uniform terms of OEIS A068599 (a(1) = 11)
  [galebach2002a068599].
- **CAs on these tilings.** Carter Bays studied Life-like rules on triangles (1994) and on hexagons and
  Cairo pentagons (2005), and wrote an encyclopedia overview (2009) [bays1994cellular; bays2005note;
  bays2009cellular]. Guttesen (2026) ran B3/S23 on six composite Archimedean lattices and found gliders
  on 3.6.3.6 and 3.3.4.3.4 and a kagome glider gun [guttesen2026game] (see novelty-05).

## Key primary citations

- [kepler1619harmonices] Johannes Kepler, *Harmonices mundi libri V*, Linz (Lincii Austriae): Gottfried
  Tampach, printed by Johann Planck, 1619. Scan: https://archive.org/details/ioanniskepplerih00kepl
  (Smithsonian Libraries, Burndy Library copy). Verified: archive.org metadata API (title, printer,
  book list incl. "II. Architectonicus").
- [kepler1997harmony] Johannes Kepler, *The Harmony of the World*, eds. A. M. Duncan and E. J. Aiton,
  American Philosophical Society, 1997. DOI 10.70249/9798893982398. Verified: Crossref record (editors,
  publisher, year).
- [grunbaum1977tilings] Branko Grünbaum and G. C. Shephard, "Tilings by Regular Polygons", *Mathematics
  Magazine* 50(5):227–247 (1977). DOI 10.1080/0025570X.1977.11976655 (old JSTOR DOI 10.2307/2689529
  redirects to it). Verified: doi.org redirect plus Crossref metadata.
- [grunbaum1987tilings] Branko Grünbaum and G. C. Shephard, *Tilings and Patterns*, W. H. Freeman,
  New York, 1987. ISBN 0-7167-1193-1. Verified: archive.org record `isbn_0716711931`.
- [grunbaum2016tilings] Same authors, *Tilings and Patterns*, 2nd ed., Dover, 2016. ISBN
  978-0-486-46981-2. Verified: Open Library ISBN record (Dover, 2016, 720 pp.).
- [grunbaum1978ninetyone] Branko Grünbaum and G. C. Shephard, "The ninety-one types of isogonal tilings
  in the plane", *Trans. Amer. Math. Soc.* 242:335–353 (1978). DOI 10.1090/S0002-9947-1978-0496813-3.
  Verified: Crossref (volume 242; Wikipedia's "252" is wrong).
- [cundy1961mathematical] H. Martyn Cundy and A. P. Rollett, *Mathematical Models*, 2nd ed., Oxford:
  Clarendon Press, 1961. Verified: several archive.org records (1954 printing and 1961 edition).
- [chavey1984periodic] Darrah P. Chavey, *Periodic Tilings and Tilings by Regular Polygons*, PhD
  thesis, University of Wisconsin–Madison, 1984. Verified: OEIS A068599 reference list.
- [bays2009cellular] Carter Bays, "Cellular Automata in Triangular, Pentagonal and Hexagonal
  Tessellations", *Encyclopedia of Complexity and Systems Science*, Springer, 2009, pp. 892–900.
  DOI 10.1007/978-0-387-30440-3_58. Verified: Crossref.
- [guttesen2026game] H. S. Guttesen, "Game of Life on Archimedean Lattices: Glider Guns and Phase
  Dynamics", arXiv:2608.27622 (2026). Verified in novelty-05 (arXiv abs page).

## Software that ran it before JETCARP

- Tiling generators: ANTWERP v3.0 (Gomez-Jauregui et al. 2021) for k-uniform tilings including the
  Archimedean ones [gomezjauregui2021gomjau]; Čtrnáct and Kopczyński's Tessellation catalog, displayed
  with RogueViz/HyperRogue (repository created Nov 2020) [ctrnactnd_tescatalog].
- CAs on Archimedean tilings: Ready (GollyGang; "polygonal and polyhedral meshes"; GitHub repository
  since 2015, GPL-3.0) [huttonnd_ready]; Kerekrety's 2020 web tools (hexagons, Cairo, rhombille)
  [kerekrety2020game]; Golly and LifeViewer support hexagonal and triangular grids (novelty-08);
  Guttesen's 2026 code for B3/S23 on six Archimedean lattices (from the paper; code not checked).

## People JETCARP should credit

Johannes Kepler (first enumeration); Branko Grünbaum and G. C. Shephard (modern theory and the
standard reference); H. M. Cundy and A. P. Rollett (vertex-configuration notation as commonly named);
Darrah Chavey (analysis of Kepler's results); Carter Bays and H. S. Guttesen (Life-like CAs on these
tilings).

## How JETCARP's version relates

The six less common Archimedean tilings are fixed catalogue tilings in `data/tilings/archimedean.json`
built by `tools/gen_archimedean.py` (README "What Phase 2 adds", item 1); triangles, squares and
hexagons are reshapeable families. All 11 are also the k = 1 entries of the k-uniform catalogue.

## Unverified

- That Cundy and Rollett introduced the notation (sources only say tilings are "usually named" by it).
  The first edition date (1951) was not checked; archive.org shows 1954 and 1961 printings.
- The Allendoerfer Award (1978) for Grünbaum and Shephard's 1977 article (from a search summary of an
  MAA page that returned 403/521).
- Exact figure and page numbers for Kepler's tilings in Book II (not checked in the scan).
- Kerekrety 2020 and Ready entries reuse novelty-05 and novelty-08 checks; not refetched here.

## Search log

- Crossref API: DOI 10.1080/0025570X.1977.11976655; 10.1090/S0002-9947-1978-0496813-3; 10.3390/sym13122376;
  10.1061/(ASCE)ST.1943-541X.0000532; 10.70249/9798893982398; 10.1007/978-0-387-30440-3_58; queries for
  Chavey and Kepler translation.
- archive.org metadata/advanced search: `ioanniskepplerih00kepl`, `isbn_0716711931`, Cundy & Rollett.
- Open Library ISBN 9780486469812.
- WebFetch: en.wikipedia.org "Euclidean tilings by convex regular polygons" (pointer to sources);
  gruze.org/tilings/kepler; probabilitysports.com/tilings.html; oeis.org/A068599.
- WebSearch: Grünbaum–Shephard 1977 DOI; Kepler Harmonices Mundi digitised copies.
- Blocked: tandfonline.com (403), maa.org (403/521), loc.gov (403).
