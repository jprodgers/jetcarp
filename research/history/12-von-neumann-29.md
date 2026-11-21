# 12. Von Neumann's 29-state automaton: history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.

## Short history

- **Background, 1948–1949.** John von Neumann's automata-theory work began with "The General and
  Logical Theory of Automata" (Hixon Symposium, September 1948; published 1951). He continued it in
  five lectures at the University of Illinois in December 1949, "Theory and Organization of
  Complicated Automata". Those lectures became Part I of the 1966 book, and they describe a
  *kinematic* self-reproducing machine [vonneumann1966theory, Burks's editor's introduction].
- **The cellular model, 1952–1953.** Burks writes that Stanisław Ulam suggested a cellular framework
  would be easier to treat mathematically than the kinematic model. Von Neumann then designed an
  infinite array of square cells, each holding the same 29-state automaton and talking to its four
  edge neighbours [vonneumann1966theory, Part II, ch. 1]. Burks quotes Klara von Neumann: the
  manuscript "The Theory of Automata: Construction, Reproduction, Homogeneity" was begun in late
  September 1952 and worked on until mid to late 1953. It was left unfinished, with two complete
  chapters and a long, incomplete third. Von Neumann never finished the design of the
  self-reproducing automaton [vonneumann1966theory].
- **Burks's edition, 1966.** Arthur W. Burks edited and completed the work as *Theory of
  Self-Reproducing Automata* (University of Illinois Press, 1966). Chapter 2 states the 29-state
  transition rule, and Burks's Chapter 5 shows how to complete the self-reproducing design
  [vonneumann1966theory]. A footnote there says that Moore's 1962 paper proposed the name
  "tessellation model" [vonneumann1966theory; moore1962machine].
- **Follow-up theory.** Thatcher (1964; in Burks's 1970 *Essays on Cellular Automata*) simplified the
  universality construction. Codd (1968) reduced the state count to 8 (file 13) [codd1968cellular].
- **First implementation, 1994–1995.** Renato Nobili and Umberto Pesavento built the first working
  self-reproducing universal constructor in 1994. To simplify the design they extended the rule:
  Nobili's 32-state variant lets confluent cells cross signals. Pesavento's 1995 *Artificial Life*
  paper describes it [pesavento1995implementation; nobili1996generalised]. Golly's help also dates
  Nobili32 and the "NP-replicator" pattern to 1994 [gollynd_jvn].
- **Later work.**
  - Tim Hutton's Hutton32 (2008) keeps Nobili's states but changes the rule table [gollynd_jvn].
  - William R. Buckley studied signal crossing in the original 29 states and published
    self-replicators that need no crossing (see Unverified).
  - Yinusa and Nehaniv (2011) used Golly to show that the completed machine passes heritable
    mutations to its offspring [yinusa2011study].
  - Turk (2025) summarises the line from von Neumann through Codd, Langton and Hutton
    [turk2025vants].

## Key primary citations

- [vonneumann1966theory] von Neumann, J., *Theory of Self-Reproducing Automata*, edited and completed by
  A. W. Burks, University of Illinois Press, Urbana (1966). archive.org item
  `theoryofselfrepr00vonn_0`. Verified: archive.org metadata (title, creators, 1966, publisher) and
  the OCR text of Burks's introduction (the list of von Neumann's five automata works; Ulam's
  suggestion; Klara von Neumann's dating; the 29 states and four neighbours; the unfinished
  manuscript).
- [pesavento1995implementation] Pesavento, U., "An implementation of von Neumann's self-reproducing
  machine", *Artificial Life* 2(4):337–354 (1995). DOI 10.1162/artl.1995.2.4.337. Verified: Crossref
  record and abstract ("first implemented in 1994, by the author in collaboration with R. Nobili",
  with an extended rule).
- [nobili1996generalised] Nobili, R. and Pesavento, U., "Generalised von Neumann's automata I: A
  revisitation", in Besussi, E. and Cecchini, A. (eds), *Artificial Worlds and Urban Studies*, DAEST
  convegni 1, IUAV, Venice (1996). Verified: search results (catalogue listing of chapter 83; a
  1994 Venice seminar) and Turk 2025's reference list. The chapter itself was not seen.
- [yinusa2011study] Yinusa, A. R. and Nehaniv, C. L., "Study of inheritable mutations in von Neumann
  self-reproducing automata using the GOLLY simulator", *2011 IEEE Symposium on Artificial Life*,
  pp. 211–217 (2011). DOI 10.1109/ALIFE.2011.5954654. Verified: Crossref record.
- [gollynd_jvn] Golly Help, "JvN" algorithm (JvN29; Nobili32, Renato Nobili 1994; Hutton32, Tim
  Hutton 2008). https://golly.sourceforge.io/Help/Algorithms/JvN.html . Verified: fetched.
- [moore1962machine] Moore 1962 (see file 01).
- [turk2025vants] Turk, G., "Vants and Turmites", *Artificial Life* 31(4):401–434 (2025) (see file 05;
  related-work section read from the PDF text saved by the 05–09 agent).

## Software that ran it before JETCARP

- **Golly**: its JvN algorithm runs JvN29, Nobili32 and Hutton32, with demo patterns including the
  1994 Nobili–Pesavento replicator [gollynd_jvn]. Yinusa and Nehaniv's 2011 study used Golly
  [yinusa2011study].
- Nobili and Pesavento's own simulator (1994–1995) [pesavento1995implementation]. Nobili's pages at
  pd.infn.it are linked from Golly's help but were not fetched.

## People JETCARP should credit

John von Neumann (the automaton); Stanisław Ulam (the cellular idea); Arthur W. Burks (editing and
completing the 1966 book); Edward F. Moore (the "tessellation" framing); Renato Nobili and Umberto
Pesavento (first implementation, 1994–1995); Tim Hutton and the Golly team (JvN in Golly); William
R. Buckley (29-state replicators).

## How JETCARP's version relates

JETCARP's `JvN29` is reimplemented from Burks's 1966 edition (secs. 2.8.1–2.8.3) as read from the
archive.org OCR, with no Golly code or rule files. It resolves several ambiguities in the garbled
formal rule using the verbal summary and checks the result against the book's figures (Figs. 12–14).
It uses its own state numbering and runs on the square grid only (README "## Von Neumann's 29-state
automaton").

## Unverified

- Buckley, W. R., "Signal crossing solutions in von Neumann self-replicating cellular automata", in
  Adamatzky, A. et al. (eds), *AUTOMATA-2008: Theory and Applications of Cellular Automata*, Luniver
  Press, pp. 453–502 (2008). Seen in search results only. Also Buckley and Mukherjee (2005), on the
  constructibility of signal-crossing solutions (search results only).
- Thatcher, J. W., "Universality in the von Neumann cellular model", in Burks, A. W. (ed.), *Essays on
  Cellular Automata*, University of Illinois Press (1970), pp. 132–186. Known only from Turk 2025's
  reference list, which dates it 1964. The 1970 volume was not found on archive.org.
- The Nobili and Pesavento chapter's page numbers.

## Search log

- archive.org: metadata API for `theoryofselfrepr00vonn_0`; OCR text
  (`archive.org/stream/…_djvu.txt`) downloaded to a scratch folder and searched (lines
  ~3405–3435, ~8240–8300).
- Crossref API (curl): 10.1162/artl.1995.2.4.337 (with abstract), 10.1109/ALIFE.2011.5954654.
- Fetched: en.wikipedia.org/wiki/Von_Neumann_cellular_automaton (pointer only);
  golly.sourceforge.io/Help/Algorithms/JvN.html.
- WebSearch: `Nobili Pesavento "Generalised von Neumann's automata" 1996 Artificial Worlds and Urban
  Studies Besussi Cecchini`; `William R. Buckley "Signal crossing solutions in von Neumann
  self-replicating cellular automata" Automata 2008`.
