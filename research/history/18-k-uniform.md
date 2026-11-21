# 18. k-uniform tilings: history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.

## Short history

- A tiling by regular polygons is **k-uniform** if its vertices fall into k orbits under its symmetry
  group. k = 1 gives the 11 Archimedean tilings of Kepler (file 17).
- **Otto Krötenheerdt** (Halle, 1969–1970) classified the "homogeneous mosaics of n-th order", the
  k-uniform tilings with k *different* vertex types (k-Archimedean, k-uniform), in three parts in the
  *Wissenschaftliche Zeitschrift* of Martin-Luther-Universität Halle-Wittenberg
  [krotenheerdt1969homogenen; krotenheerdt1970homogenenii; krotenheerdt1970homogeneniii]. OEIS
  A068600 gives the counts 11, 20, 39, 33, 15, 10, 7 (none for k ≥ 8) and notes that the sequence was
  originally calculated by Krötenheerdt [galebach2002a068600].
- **Grünbaum and Shephard** treat k-uniform tilings in their 1977 survey and in *Tilings and
  Patterns* (1987), Sections 2.1–2.2; OEIS A068599 cites the book's Exercise *6 on p. 70 for the counts
  [grunbaum1977tilings; grunbaum1987tilings].
- **Darrah Chavey**: his 1984 PhD thesis gave the number of 3-uniform tilings, 61
  [chavey1984periodic], and his 1989 paper "Tilings by regular polygons—II: A catalog of tilings"
  (*Computers & Mathematics with Applications*) is the standard printed catalogue [chavey1989tilings].
- **Brian Galebach** wrote a program that generates complete sets of k-uniform tilings. His OEIS
  entry A068599 (28 Mar 2002) records a(4) = 151 and a(5) = 332 (30 Apr 2002), a(6) = 673 (6 Aug 2003)
  and a(7) = 1472 (28 Apr 2020). His page says the 6-uniform run took "approximately one month of
  computation time" and that it reproduced Krötenheerdt's list [galebach2002a068599;
  galebachnd_uniform].
- **Marek Čtrnáct** extended the counts to a(8)–a(13) in 2021 (Tiling mailing list; added to OEIS
  by N. J. A. Sloane, 21 Dec 2021) and, with Eryk Kopczyński, built the online Tessellation catalog
  (RogueViz) [galebach2002a068599; ctrnactnd_tescatalog].
- **Integer representation.** Asla Medeiros e Sá, Luiz Henrique de Figueiredo and José Ezequiel Soto
  Sánchez synthesised periodic regular-polygon tilings (SIBGRAPI 2018) [medeirosesa2018synthesizing];
  Soto Sánchez, Weyrich, Medeiros e Sá and de Figueiredo then gave an integer representation for them
  (*Computers & Graphics* 95, 2021) [sotosanchez2021integer]. Soto Sánchez's online viewer publishes
  the "Galebach" set (and the "Sá e Sá" set) under CC BY 4.0 [sotosancheznd_tiling]. This is JETCARP's
  data source.
- Other generators: Gomez-Jauregui et al.'s GomJau-Hogg notation and ANTWERP v3.0 (2021)
  [gomezjauregui2021gomjau].

## Key primary citations

- [krotenheerdt1969homogenen] Otto Krötenheerdt, "Die homogenen Mosaike n-ter Ordnung in der
  euklidischen Ebene I", *Wiss. Z. Martin-Luther-Univ. Halle-Wittenberg, Math.-Natur. Reihe* 18
  (1969) 273–290; parts II and III in vol. 19 (1970) 19–38 and 97–122. Verified: OEIS A068600
  reference list (no DOI; the original was not seen).
- [chavey1989tilings] D. Chavey, "Tilings by regular polygons—II: A catalog of tilings", *Computers &
  Mathematics with Applications* 17(1–3):147–165 (1989). DOI 10.1016/0898-1221(89)90156-9 (also in the
  book *Symmetry 2*, DOI 10.1016/B978-0-08-037237-2.50019-2). Verified: Crossref.
- [galebach2002a068599] B. Galebach (author), "Number of n-uniform tilings", OEIS A068599,
  https://oeis.org/A068599 . Verified: page fetched (author line, extensions, references).
- [galebachnd_uniform] Brian Galebach, "n-Uniform Tilings", http://www.probabilitysports.com/tilings.html
  (copyright 2002–2026). Verified: page fetched.
- [sotosanchez2021integer] José Ezequiel Soto Sánchez, Tim Weyrich, Asla Medeiros e Sá and Luiz Henrique
  de Figueiredo, "An integer representation for periodic tilings of the plane by regular polygons",
  *Computers & Graphics* 95:69–80 (2021). DOI 10.1016/j.cag.2021.01.007. Verified: Crossref.
- [sotosancheznd_tiling] J. E. Soto Sánchez, "On Periodic Tilings with Regular Polygons" (online
  viewer and data, "Galebach" set), https://chequesoto.info/tiling/ , CC BY 4.0. Verified: page fetched
  (licence badge, set selector).

## Software that ran it before JETCARP

- Galebach's own enumeration program (not named or published, per his page).
- Soto Sánchez's viewer and JSON data (chequesoto.info/tiling).
- Čtrnáct and Kopczyński's Tessellation catalog (github.com/zenorogue/tes-catalog, created Nov 2020;
  README Sep 2026 says RogueViz now draws the images) [ctrnactnd_tescatalog].
- ANTWERP v3.0 (Gomez-Jauregui, Hogg, Manchado and Otero 2021).
- CAs: Guttesen (2026) studied Life only on Archimedean (1-uniform) lattices. No CA on k ≥ 2 tilings
  was found in the novelty audit (novelty-05; spaceships on 2- and 3-uniform tilings: "nothing found").

## People JETCARP should credit

Otto Krötenheerdt; Branko Grünbaum and G. C. Shephard; Darrah Chavey; **Brian Galebach** (the catalogue
JETCARP uses, k ≤ 6); **José Ezequiel Soto Sánchez, Tim Weyrich, Asla Medeiros e Sá and Luiz Henrique
de Figueiredo** (the integer representation and the CC BY 4.0 data file); Marek Čtrnáct (k = 8–13
counts and the tessellation catalog).

## How JETCARP's version relates

JETCARP vendors Soto Sánchez's Galebach JSON unchanged (`tools/data/JSON_Galebach.json`), rebuilds each
of the 1,248 tilings (k ≤ 6: 11, 20, 61, 151, 332, 673) exactly in Z[ω] with `tools/gen_kuniform.py`,
and names them `kN-NNN` in Galebach's order (README "k-uniform tilings"). The provenance audit notes
that the CC BY 4.0 attribution still needs a licence URL and a change note.

## Unverified

- Krötenheerdt's papers were not seen; the citation is from OEIS A068600 only.
- Nils Lenngren, "k-uniform tilings by regular polygons" (Uppsala, 2009), cited by Wikipedia (archived
  PDF), was not fetched.
- Chavey, "Tilings by regular polygons III: dodecagon-dense tilings", *Symmetry: Culture and Science*
  25(3):193–210 (2014), from Wikipedia only.
- The Sá e Sá set's exact contents were not checked.

## Search log

- Crossref API: query "An integer representation for periodic tilings…" (found 10.1016/j.cag.2021.01.007,
  Chavey 1989, SIBGRAPI 2018); query Krötenheerdt (no record).
- WebFetch: oeis.org/A068599, oeis.org/A068600, probabilitysports.com/tilings.html, chequesoto.info/tiling/,
  chequesoto.info/, zenorogue.github.io/tes-catalog/ (JavaScript only); en.wikipedia.org "Euclidean
  tilings by convex regular polygons" (pointer).
- curl: raw.githubusercontent.com zenorogue/tes-catalog README; api.github.com repo metadata.
- Local: head of `CART/tools/data/JSON_Galebach.json` (format check only).
