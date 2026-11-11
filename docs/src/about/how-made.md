# How JETCARP was made

This page says plainly how JETCARP was built, who did what, how its claims were checked, and where
mistakes were made and caught. It is meant to help you judge how far to trust it.

## From CART to JETCARP

JETCARP grew out of CART, a short-lived prototype of Jimmie Rodgers's. CART lasted about a week, in
early November 2026, and in that time grew into a Rust program with an app, a command line,
scripting, an MCP server and its searches. Jimmie then decided it was worth serious development
time. CART was copied into a new repository with a fresh history, renamed JETCARP, and reworked for
public release: standard folders, licences
and notices, one program instead of several, a shared layer of operations for every front end, and
one way of running searches. CART itself is kept private and unchanged. Most of the finds reported in
this book were made with CART, and every one was run again on JETCARP 0.4.0 before being reported (see
[Findings](../research/findings.md#re-verification-on-the-release-engine)).

## Rust, with an AI coding assistant

JETCARP is written in Rust: about 170,000 lines in ten crates, tests and generated tables included,
with a small GNOME Shell extension in JavaScript. The simulation engine is `jetcarp-core`; the
others are the GPU stepper, the operations shared by the front ends, the app, the background host
and screensavers, the Rhai scripting, the MCP server, and the `jetcarp` program that ties them
together.

All of its code was written by Claude Code, Anthropic's AI coding assistant. The division of work
was this:

- **Jimmie** was the designer and the tester. Jimmie decided what JETCARP should be and what it should
  do, which rules and tilings it should have, what to search for and how to judge the results, which
  questions were worth asking and which claims could be made. Many of the ideas are Jimmie's own,
  among them JACA and the layered rules. Jimmie asked for the features in the app rather than designing
  its interface, tested the program on Linux, Windows and macOS (by hand, alongside automated tests and
  Claude sessions running test plans on each machine), and decided what is claimed in this book.
- **Claude**, working in Claude Code, wrote all of the code and the tests, designed the app's interface
  to Jimmie's requests, wrote the first drafts of this book, and ran many of the searches, audits and
  checks described below as separate agents, each with a written brief.

Jimmie does not take credit for the code: the ideas and the direction are Jimmie's, the execution
was done by agentic coding.

Claude is acknowledged in the [credits](credits.md), not listed as an author. Every result in this
book was produced and checked by the program and its tests, not by the assistant's say-so, but an AI
writing code and running analyses can be wrong in a confident voice, and some of its mistakes are
listed below. Please treat JETCARP as you would any research software: check the results that
matter to you.

## How the claims were checked

- **Tests.** About 650 automated tests, including the pinned geometry of every tiling, the pinned
  dynamics of every built-in Quick Start entry, CPU and GPU equality, and the resumption of every kind
  of search (see [Reproducibility](../research/reproducibility.md)).
- **Exact verification.** Nothing goes into the catalogue without being run again and compared cell
  by cell; turmite highways are proved. Before release, all 103 spaceship records, 739 long-period
  oscillators and 39,744 turmite highways were run again on the release engine, with no change.
- **Prior-art audits.** Every candidate contribution was searched for in the literature and the
  hobbyist record, with each query and page logged, and judged *prior art found*, *related work only*
  or *nothing found*. Only the last two support a claim, worded "to our knowledge". The method and its
  weak spots, chiefly the ConwayLife.com forums, are set out in
  [What is new in JETCARP](../research/contributions.md#how-we-searched).
- **Skeptic passes.** The candidate spaceships were reviewed by a pass whose job was to find reasons
  not to claim them: ships that are two ships together, ships that carry a smaller one, ships that
  also run on a simpler tiling, and ships too common or in too well-known a rule to be safely new.
- **Lattice-equivalence and congruence checks.** Each candidate's path was mapped into 196 neighbour
  graphs to see whether it is really a ship of a simpler tiling, and every pair of candidates was
  compared tile by tile under rotation, reflection and translation, to merge the same ship catalogued
  twice. The results are in [Findings](../research/findings.md) and on the
  [contributions](../research/contributions.md#how-we-checked) page.
- **Sources.** For the history sections and the [bibliography](../research/bibliography.md), each
  source was checked against its primary record (Crossref, the authors' own pages and repositories)
  before it was cited.

These checks were themselves largely run by Claude agents under Jimmie's direction, set up to argue
against the claims rather than for them, with Jimmie reading the results and deciding. The check by
people who know the field best, the ConwayLife community, is still to come.

## The provenance audit

Before release, an audit checked where every part of JETCARP came from: whether any code, text, data
or pattern had been taken from somewhere else without its licence being honoured. It included an
automated similarity check against Golly's source code, used for the comparison only and never for
writing JETCARP; it found no Golly code, text, rule files or patterns. It did find five things, all
fixed before release:

- **Named-rule descriptions** had been copied from LifeWiki, whose text is under a copyleft licence.
  They were rewritten in JETCARP's own words, keeping the names, notations and credits, which are
  facts.
- **The spectre construction** was ported from Craig S. Kaplan's Spectre visualizer, which had been
  published without a licence. Jimmie asked Kaplan, who made it available under BSD 3-Clause the same
  day (see [Licence](licence.md#third-party-work)).
- **The Penrose rhomb (P3) construction** followed Jeff Preshing's `penrose.py`, which has no licence.
  It was replaced by a clean-room rewrite: a separate Claude session, which could not see JETCARP's
  code, was given only the mathematics (golden triangles, Grünbaum and Shephard, de Bruijn) and the
  interfaces it had to fit. It wrote the construction without seeing the old code or any other Penrose
  implementation, and checked it against de Bruijn's eight vertex types and the tiling's symmetries.
  The tiles changed, so the P3 pins were rewritten.
- **The GNOME extension's window hiding** closely paraphrased the Hanabi extension, which is under the
  GPL. It was replaced by a clean-room implementation, written by a separate Claude session from GNOME
  Shell's and Mutter's own sources and a functional description, without access to Hanabi.

  Both rewrites are independent implementations, but the clean room was not a strict one: the written
  specifications they worked from were drafted by the Claude session that had seen the old code. They
  describe what the code must do, not how the old code did it.
- **Notices** for James Edmond's monotile-ca code, the full CC BY attribution of the k-uniform data,
  and the bundled fonts were added or completed.

## Mistakes that were caught

Some of the AI's work was wrong in ways that mattered, and the checks above caught it before any
claim was made:

- **Isohedral "spaceships" that were Life's glider.** JETCARP's catalogue held spaceships on five
  isohedral types that looked like discoveries. Mapped back onto their lattices, they were Conway's
  glider, the lightweight and middleweight spaceships, and a known ship of the triangular grid: those
  isohedral types simply have the square or triangular grid's neighbour graph. This led to the finding
  that the 81 isohedral types are only eleven neighbour graphs, and to the lattice-equivalence check
  for every other candidate (see
  [Findings](../research/findings.md#isohedral-tilings-are-eleven-neighbour-graphs)).
- **A hat glider that was Edmond's glider B.** JETCARP's evolutionary search returned a table that
  looked unlike any of Edmond's, so its glider at first looked new. With the states renamed, it gives
  the same evolution as his glider B, cell for cell; the credit is his (see
  [Findings](../research/findings.md#the-hat-glider-is-edmonds-glider-b)).
- **Compound ships.** Ten of the 72 candidate ships were two smaller ships flying side by side, eight
  of them pairs of H. S. Guttesen's kagome glider.
- **Missed prior art.** The first prior-art audit missed EricG's 2013 forum threads on Life-like
  rules on the Archimedean tilings, and a 2014 Code Golf answer with a Life spaceship on a 2-uniform
  tiling. Until a second, targeted search of the forums found them, this book claimed ten Archimedean
  ships and said that no Life-like ship on a k-uniform tiling was known. Four of the ten are now
  flagged rather than claimed, and the statement was corrected (see
  [What is new in JETCARP](../research/contributions.md#4-spaceships-on-non-square-tilings)). The same
  search showed that CART had never run Life itself systematically on the k-uniform tilings, so
  statements about which ships run in Life rested on what CART happened to search.
- **Duplicates.** Two ships had each been catalogued twice, at positions with different neighbours
  around them; and importing an old record could give the same pattern several ids, one per phase. The
  duplicates were merged in the claims, and a fix to the ids is planned.
- **Smaller slips**, among them tests that wrote into the user's real Pictures folder (fixed), and
  the speed labels such as "c/4 diagonal" in catalogue names, which use the square grid's words and
  are wrong on tilings whose lattice has 60° angles (still to be fixed). Both were found by looking
  at what the program did, not by reading its code.

The claims in this book are what survived. If you find something we missed, please open an issue: we
will correct the page and credit you.
