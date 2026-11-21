# 13. Rule tables and Codd's 8-state CA: history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.

## Short history

- **Codd's thesis and book.** Edgar F. Codd wrote his PhD thesis, "Propagation, Computation, and
  Construction in Two-Dimensional Cellular Spaces", at the University of Michigan in 1965, advised by
  John H. Holland [codd1965propagation]. He developed it into the book *Cellular Automata* (Academic
  Press, ACM Monograph Series, 1968) [codd1968cellular]. Codd kept von Neumann's four-neighbour
  square grid but cut the 29 states to 8. Signals run along wires wrapped in a sheath, and he argued
  that computation and construction universality survive the reduction [wikipediand_codd;
  hutton2010codd].
- **Variants.**
  - John Devore's 1973 variant changed the rules to allow much more compact machines
    [huttonnd_ruletablerepository].
  - Christopher Langton's self-reproducing loop (1984) is derived from Codd's periodic emitter. It
    replicates with a few dozen cells but does not compute universally [langton1984selfreproduction;
    huttonnd_ruletablerepository].
  - The loop line went on with Byl (1989), Chou and Reggia (1993) and Sayama's SDSR loop (1998) and
    Evoloop (1999) [huttonnd_ruletablerepository].
- **Hutton's implementation, 2009–2010.** Codd's self-replicating computer was too large to build by
  hand. Tim Hutton found four flaws in the published specification that would have stopped it from
  working. He made small changes to the design and transition table and built a complete working
  machine. Its body is 22,254 × 55,601 cells, its tape is 208 million cells long, and one
  replication is estimated to take at least 1.7 × 10^18 steps [hutton2010codd, abstract]. The Rule
  Table Repository lists the corrected rule as Codd2 (2009), which adds three transitions for
  sheath-collision cases Codd had not covered [huttonnd_ruletablerepository].
- **Rule tables as a format.** Golly's RuleTable algorithm (Golly 2.0, December 2008) and its
  successor RuleLoader (2.5, June 2013) read explicit transition tables. A `.rule` file has `@TABLE`
  lines with `n_states`, a neighbourhood (vonNeumann, Moore, hexagonal, oneDimensional), symmetries,
  variables and transitions. The first matching transition wins, and an unlisted case leaves the cell
  unchanged [gollynd_formats; gollynd_changes]. Golly's credits name Tim Hutton as author of the
  RuleTable algorithm and of the Rule Table Repository [gollynd_credits], and Golly's formats page
  says the table format was inspired by Gianluca Tempesti's thesis [gollynd_formats].

## Key primary citations

- [codd1968cellular] Codd, E. F., *Cellular Automata*, ACM Monograph Series, Academic Press, New York
  (1968); e-book ISBN 978-1-4832-0014-9 (Elsevier). Verified: archive.org record
  `cellularautomata0000efco` (E. F. Codd, 1968, Academic Press) and Crossref chapter records
  (DOIs 10.1016/B978-1-4832-0014-9.50004-2 etc.).
- [codd1965propagation] Codd, E. F., *Propagation, Computation, and Construction in Two-Dimensional
  Cellular Spaces*, PhD thesis, University of Michigan (1965). ProQuest 302172044. Verified:
  Mathematics Genealogy Project id 100501 (degree, year, title, advisor J. H. Holland).
- [langton1984selfreproduction] Langton, C. G., "Self-reproduction in cellular automata", *Physica D*
  10(1–2):135–144 (1984). DOI 10.1016/0167-2789(84)90256-2. Verified: Crossref record.
- [hutton2010codd] Hutton, T. J., "Codd's self-replicating computer", *Artificial Life*
  16(2):99–117 (2010). DOI 10.1162/artl.2010.16.2.16200. Verified: Crossref record and abstract.
- [gollynd_formats] Golly Help, "File formats" (`.rule`, `@TABLE`, `@TREE`).
  https://golly.sourceforge.io/Help/formats.html . Verified: fetched.
- [huttonnd_ruletablerepository] Hutton, T. et al., *Rule Table Repository*, "TheRules" wiki page,
  https://github.com/GollyGang/ruletablerepository/wiki/TheRules . Verified: fetched (Codd 1968, Codd2
  2009, Devore 1973, Langton's loops 1984, Byl 1989, Chou–Reggia 1993, SDSR 1998, Evoloop 1999). The
  repository says LifeWiki now holds a larger collection.

## Software that ran it before JETCARP

- **Golly** RuleTable/RuleLoader, with Codd, Codd2, Devore and Langton's loops in its rule collection
  and the Rule Table Repository [huttonnd_ruletablerepository; gollynd_formats]. Hutton's full Codd
  machine is distributed as a Golly pattern (per Wikipedia; the download was not fetched).
- Earlier simulations of Codd's components (Codd's own, at IBM) were not looked into.

## People JETCARP should credit

Edgar F. Codd (the 8-state rule); John H. Holland (thesis advisor); John Devore (variant);
Christopher Langton (loops); Tim Hutton (the working Codd computer, the RuleTable format and the
repository); Gianluca Tempesti (inspiration for the table format, per Golly).

## How JETCARP's version relates

JETCARP's rule tables use its own JSON format, not Golly files or code. Entries are
`centre,n1,…,nk:new` in each cell's clockwise neighbour order, with wildcards and alternatives, and
symmetry `none`, `rotate`, `rotate_reflect` or `permute`. As in Golly, the first match wins and an
unmatched cell keeps its state (README "## Rule tables"). Codd's rule is not included. The README
says no usable copy of his original table exists outside Golly. The table is printed in the 1968
book (sec. 4.27, per the README) and can be added once it is transcribed.

## Unverified

- The Devore 1973 master's thesis (title, institution). It is known only from the repository wiki and
  Wikipedia. Turk 2025 also lists a Devore and Hightower presentation at the Third Workshop on
  Artificial Life (1992).
- That the table is in sec. 4.27 of Codd's book (README statement). The book is a restricted item on
  archive.org and was not read.
- Langton's loop size (86 cells is often quoted; Wikipedia and the repository wiki give it).
- The 1977 *Acta Cybernetica* "Codd-ICRA" table mentioned in the README was not looked up.
- That this E. F. Codd is the Codd of the relational database model. This is widely stated, but no
  source fetched here says so.

## Search log

- archive.org advanced search and metadata API (`cellularautomata0000efco`).
- Crossref API (curl): bibliographic query "Codd Cellular Automata 1968 Academic Press";
  10.1016/0167-2789(84)90256-2; 10.1162/artl.2010.16.2.16200 (abstract).
- Fetched: mathgenealogy.org/id.php?id=100501; en.wikipedia.org/wiki/Codd%27s_cellular_automaton
  (pointer only); golly.sourceforge.io/Help/formats.html; sources.debian.org copy of Golly 2.3
  Help/Algorithms/RuleTable.html; github.com/GollyGang/ruletablerepository/wiki/TheRules.
- WebSearch: `E. F. Codd 1965 PhD thesis University of Michigan "Propagation, computation, and
  construction …"`; `Golly Rule Table Repository Tim Hutton Codd rule table …`.
