# 20. Cairo pentagonal, Pythagorean, herringbone, brick and "lizard" tilings: history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.

## Short history

### Cairo pentagonal tiling (`cairo`)

- Named after its use as street paving in Cairo. R. H. Macmillan's note "Pyramids and pavements: some
  thoughts from Cairo" (*Mathematical Gazette*, 1979) is the classic written source
  [macmillan1979pyramids]. Frank Morgan went looking for surviving Cairo pavements in 2019
  [morgan2019undercover].
- Earlier appearances (Wikipedia's history section used as a pointer): Harry Nisbet's *Grammar of
  Textile Design* (1906) [nisbet1906grammar]; a US patent for pentagonal floor tiles by Herbert C. Moore,
  filed 20 Jan 1908 and granted 20 Jul 1909 (US 928,320) [moore1909tile]; P. A. MacMahon's *New
  Mathematical Pastimes* (1921), hence the name "MacMahon's net" [macmahon1921new]; A. P. Rollett's
  Gazette note "A pentagonal tessellation" (1955) [rollett1955pentagonal]; J. A. Dunn, "Tessellations
  with pentagons" (1971) [dunn1971tessellations].
- Mathematically it is the dual (Laves tiling) of the snub square tiling 3^2.4.3.4. Its pentagon
  belongs to Reinhardt's 1918 "type 4" family of convex pentagons that tile the plane, as surveyed by
  Schattschneider (1978) [schattschneider1978tiling]. As an isohedral tiling, McLean's catalogue
  describes IH28 (p4: a pentagon with two pairs of adjacent, equal, perpendicular sides; one two-fold and
  two four-fold rotation centres), which matches JETCARP's Cairo family [mcleannd_isohedral]. This match
  is our reading of the description, not a statement in the source.
- CAs: Carter Bays (2005) found a Life-like glider on the pentagonal (Cairo) tessellation in B346/S23,
  and Kerekrety (2020) showed it again as the period-48 "shrimp" [bays2005note; kerekrety2020game]
  (see novelty-05).

### Pythagorean tiling (`pyth`)

- Two sizes of square, not edge to edge. It is named for the dissection proofs of Pythagoras's theorem
  that it carries: overlaying a square grid whose side is the hypotenuse cuts the two squares into
  pieces that make the larger square. Wikipedia attributes such proofs to al-Nayrizi and Thābit ibn
  Qurra (9th century) and Henry Perigal (19th century), citing Frederickson's *Dissections: Plane &
  Fancy* (1997) [frederickson1997dissections]. Nelsen (2003) traces it in paintings and floor tilings
  [nelsen2003paintings], and Aguiló, Fiol and Fiol (2000) use periodic tilings of this kind as a
  general dissection method [aguilo2000periodic]. As flooring it is called the "hopscotch" or
  "pinwheel" pattern (not the mathematical pinwheel tiling).

### Herringbone (`herring`)

- Rectangles (usually 2:1) in two orientations, not edge to edge; wallpaper group pgg. Grünbaum and
  Shephard list it among the isohedral tilings by quadrangles (Wikipedia cites *Tilings and Patterns*
  p. 476) [grunbaum1987tilings]. As masonry it is the Roman and medieval *opus spicatum*, used mostly in
  paving and wall infill (Wikipedia, citing the 1911 *Encyclopædia Britannica*). Topologically it is
  the hexagonal tiling, so each tile has six edge neighbours.

### Brick / running bond (`brick`)

- Running (stretcher) bond: courses of equal bricks, each course offset by half a brick. As a tiling it
  has the same neighbour graph as the hexagonal grid. Wolfram's *A New Kind of Science* draws two-cell
  rules on cells "arranged like hexagons or staggered bricks" (NKS p. 885 note, verified in novelty-03)
  [wolfram2002new]. No history source beyond general brickwork references was checked.

### "Lizard" (three-fold hexagon, `lizard`)

- JETCARP's tile is a hexagon whose edges pair off by 120° turns about shared corners. McLean's
  catalogue describes IH7 in the same way (a hexagon whose adjacent sides pair up, meeting at 120°;
  symmetry group p3; three kinds of three-fold rotation centre) [mcleannd_isohedral].
- Escher drew lizards on a regular division of the plane in *Regular division drawing with lizards,
  no. 25* (India ink, pencil and watercolour, January 1939) and used the same "angular lizard" in the
  lithograph *Reptiles* (March 1943) and in *Development II* (Escher in het Paleis, 2018)
  [kersten2018repeating]. Schattschneider's *Visions of Symmetry* catalogues these drawings
  [schattschneider1990visions]. JETCARP's family note ("Escher built reptiles on this tile") fits, but no
  fetched source states that drawing no. 25 is IH7.

## Key primary citations

- [macmillan1979pyramids] R. H. Macmillan, "Pyramids and pavements: some thoughts from Cairo", *The
  Mathematical Gazette* 63(426):251–255 (1979). DOI 10.2307/3618038. Verified: Crossref.
- [moore1909tile] Herbert C. Moore, "Tile", US Patent 928,320, filed 20 Jan 1908, granted 20 Jul 1909.
  https://patents.google.com/patent/US928320A/en . Verified: Google Patents page.
- [schattschneider1978tiling] Doris Schattschneider, "Tiling the Plane with Congruent Pentagons",
  *Mathematics Magazine* 51(1):29–44 (1978). DOI 10.1080/0025570X.1978.11976672. Verified: Crossref.
- [rollett1955pentagonal] A. P. Rollett ("A.P.R."), "2530. A pentagonal tessellation", *The
  Mathematical Gazette* 39(329):209 (1955). DOI 10.2307/3608750. Verified: Crossref.
- [morgan2019undercover] Frank Morgan, "My Undercover Mission to Find Cairo Tilings", *The Mathematical
  Intelligencer* 41(3):19–22 (2019). DOI 10.1007/s00283-019-09906-7. Verified: Crossref.
- [nelsen2003paintings] Roger B. Nelsen, "Paintings, Plane Tilings, & Proofs", *Math Horizons*
  11(2):5–8 (2003). DOI 10.1080/10724117.2003.12021741. Verified: Crossref.
- [aguilo2000periodic] F. Aguiló, M. A. Fiol and M. L. Fiol, "Periodic Tilings as a Dissection Method",
  *Amer. Math. Monthly* 107(4):341–352 (2000). DOI 10.2307/2589179. Verified: Crossref (first page).
- [frederickson1997dissections] Greg N. Frederickson, *Dissections: Plane & Fancy*, Cambridge University
  Press, 1997. Verified: archive.org record `dissectionsplane0000fred`.
- [kersten2018repeating] Erik Kersten, "Repeating reptiles", Escher in het Paleis, 15 Sep 2018,
  https://escherinhetpaleis.nl/escher-today/repeating-reptiles/?lang=en . Verified: page fetched.
- [mcleannd_isohedral] Tom P. McLean, *Isohedral tilings* (93 templates), mirror by Jaap Scherphuis,
  https://www.jaapsch.net/tilings/mclean/ (pages ih7.html, ih28.html). Verified: pages fetched.

## Software that ran it before JETCARP

- Cairo: Bays (2005) and Kerekrety (2020) ran Life-like rules on it (novelty-05). Escher-style tile
  editors (Tactile demos) cover IH7 and IH28.
- Pythagorean, herringbone, brick: no CA software found in this pass. A brick grid is commonly used as a
  stand-in for hexagons in hexagonal-CA code, but no specific source was checked.

## People JETCARP should credit

Cairo: R. H. Macmillan (name and early note), A. P. Rollett, P. A. MacMahon, Doris Schattschneider,
Carter Bays (first Cairo glider). Pythagorean: the dissection tradition (al-Nayrizi, Thābit ibn Qurra,
Perigal) via Frederickson. Herringbone: Grünbaum and Shephard. Lizard: M. C. Escher (drawing no. 25,
*Reptiles*); Tom McLean's IH7 description.

## How JETCARP's version relates

`cairo`, `lizard`, `hex`, `brick` and others are reshapeable one-tile families ported from the JETCARP
prototype tiling editor; `herring` and `pyth` are fixed, not edge to edge, with neighbours measured from
contact (`families.rs` notes). The Cairo tile has five edge and seven touching neighbours.

## Unverified

- The page numbers (Nisbet p. 101, MacMahon p. 101) and that those books show the Cairo pattern: only
  the books' existence was checked on archive.org (`grammaroftextile00nisb`, `newmathematicalp00macmuoft`).
- Reinhardt, K., *Über die Zerlegung der Ebene in Polygone*, dissertation, Frankfurt, Borna-Leipzig:
  Noske, 1918 (via Wikipedia).
- Escher's use of the Cairo tiling (*Shells and Starfish*, 1941; *Metamorphosis III*), and Haag (1911)
  in *Z. Kristallographie* 49:360–369 (Wikipedia only).
- The al-Nayrizi, Thābit ibn Qurra and Perigal attributions (Wikipedia, citing Frederickson pp. 30–31).
- Herringbone as G&S isohedral quadrangle type no. 19 (p. 476) and *opus spicatum* history (Wikipedia
  only; 1911 Britannica not fetched).
- That Escher's drawing no. 25 is of type IH7 (or p3).

## Search log

- WebFetch: en.wikipedia.org "Cairo pentagonal tiling", "Pythagorean tiling", "Herringbone pattern",
  "Opus spicatum", "Brickwork", "Reptiles (M. C. Escher)" (all as pointers); patents.google.com
  US928320A; escherinhetpaleis.nl repeating-reptiles; jaapsch.net McLean pages.
- Crossref API: 10.2307/3618038, 10.2307/3608750, 10.1080/0025570X.1978.11976672,
  10.1007/s00283-019-09906-7, 10.2307/3612359, 10.1080/10724117.2003.12021741, 10.2307/2589179.
- archive.org advanced search: MacMahon 1921, Nisbet 1906, Frederickson 1997, Schattschneider.
- WebSearch: Escher Reptiles and drawing no. 25; "C3C3C3" "IH7".
