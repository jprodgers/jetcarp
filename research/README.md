# Research notes

These are the notes behind the history and credits in the JETCARP book and behind the claims on its
page [What is new in JETCARP](../docs/src/research/contributions.md). They were compiled in November
2026 by Claude Code research agents for Jimmie Rodgers, who read the results and decided what the book
would claim. Each file records the searches it ran (the queries, and the pages fetched, with their
dates), what was found, and what could not be verified. They are kept as they were written, with a few
local file paths and one email address elided; the book is where the conclusions are kept current.

- [`novelty-summary.md`](novelty-summary.md): the verdict for each candidate contribution, with the
  main gaps in the search.
- `novelty-01-jaca.md` to `novelty-11-kuniform-life.md`: one prior-art search per candidate
  contribution (JACA, colouring, the three-parent hexagon rules, layered rules, spaceships on
  non-square tilings, the hat gliders, sandpiles, how the families are generalised, the exploratory
  search, the ConwayLife forums, and Life on the 2- and 3-uniform tilings).
- [`history/`](history/): the history of each cellular automaton family and tiling family the book
  describes, with its sources, and `named-rule-authors.md`, the credits of the named rules. The
  bibliography, [`docs/data/bibliography.bib`](../docs/data/bibliography.bib), was compiled from these
  files; each file's *Unverified* list holds the items that were left out of it.

## What is left out, and why

The searches downloaded copies of other people's work to read them. Those copies are not ours to
publish, so they are not here; each file names the pages and papers it read, with their addresses,
so they can be found at the source:

- the raw logs of the search for Life on the 2- and 3-uniform tilings (`novelty-11-logs/`): its
  forum and wiki query logs, which are mostly the text of the posts and pages found; the text of four
  forum threads (`t1025.txt`, `t1123.txt`, `t1124.txt`, `t3893.txt`); and what it downloaded to
  read, other people's pages, posts, answers, source code, papers and pictures (`dl/`, `pdf/`,
  `img/`, `tiledca/`), with the small scripts that fetched them;
- the papers and PDFs the other searches downloaded, and the text extracted from them;
- the JSON result files of James Edmond's monotile repository, which the comparison in
  `novelty-06-hat-gliders.md` read.

The query lists themselves are in the files here: each search log names its queries and the
addresses it fetched.

Also left out are two short local scripts the notes mention (a rule comparison for the hat gliders and
a comparison of the named rules with LifeWiki and MCell), and working notes that are not part of the
prior-art search: an audit of the code's provenance and a list of dependency licences, whose
conclusions are in the book's [Licence](../docs/src/about/licence.md) and
[How JETCARP was made](../docs/src/about/how-made.md) pages.
