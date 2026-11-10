# What is new in JETCARP

> **Draft.** This page is a first draft of JETCARP's contributions, written for the v0.4.0 release
> and the paper that will follow. Each claim below is limited to what our prior-art search supports,
> and several are still being checked; see *Checks still to do* at the end. If you know of earlier
> work that we have missed, please open an issue. We will correct the page and credit it.

JETCARP stands on a great deal of earlier work, credited on every page of this encyclopedia and
collected in the [bibliography](bibliography.md). This page lists the few things that, as far as we can
tell, are new, says how we looked, and lists the things we have decided *not* to claim.

## How we searched

In November 2026 we audited each candidate contribution against the literature and the hobbyist
record: web and arXiv searches, Crossref records for every paper cited, the authors' own pages and
repositories, and LifeWiki. Every search query and fetched page is logged with its date in our research
notes, and each source was checked against its primary record before being listed in the bibliography.
The verdicts used three levels: *prior art found*, *related work only*, and *nothing found after
search*. Only the last two can support a claim, and we word such claims "to our knowledge".

The search has known weaknesses:

- **Forums.** The ConwayLife.com site refuses automated reading, so LifeWiki and the forums were read
  through a mirror, and forum coverage is the weakest part of the search. Hobbyists may well have found
  some of the things below without publishing them in papers.
- **Unread full texts.** For a few papers only abstracts or search excerpts could be read; where a
  statement rests on an excerpt, the page that makes it says so.
- **A fast-moving field.** Several closely related works appeared between August and October 2026
  (Guttesen's Life on Archimedean lattices, Edmond's monotile gliders). We will re-run the key searches
  just before publication.

## Contributions

### 1. JACA, an annihilating two-sided family of Life-like rules

**Claim (to our knowledge).** The particular combination in [JACA](../automata/jaca.md) is new: an
independent birth/survival rule for each sign, counting only same-sign neighbours; destruction of every
live cell that touches the other sign; a blast radius measured in neighbourhood steps; cancelled double
births; an exact reduction to the plain rule when only one sign is present; and use on any periodic or
aperiodic tiling. JACA is Jimmie Rodgers's idea (November 2026).

**Evidence.** Each ingredient has a precedent, and all are credited: two-player Life with a coin flip
for tied births ([Levene and Roussos 2003](bibliography.md#levene2003twoplayer)); the +1/−1/0 encoding,
own-sign counting and cancelled births of the "Game of Life or Death"
([Unsupervision Blog 2025](bibliography.md#unsupervision2025playable)); matter–antimatter Life whose
touching cells turn into energy ([KittyTac and wildmyron 2018](bibliography.md#kittytac2018rulerequest);
[LifeWiki: Rule:Energetic](bibliography.md#lifewikind_energetic)); and A + B → 0 annihilation in physics
([Toussaint and Wilczek 1983](bibliography.md#toussaint1983particle);
[Chopard and Droz 1998](bibliography.md#chopard1998cellular)). No source combined them. Not searched:
MCell's weighted rules with negative weights, Golly's rule-table collection, and most of the ConwayLife
rule-request thread.

**Not claimed.** "The first signed or ternary Life", "the first matter–antimatter Life", and anything
about JACA's colouring.

### 2. Layered rules on any tiling

**Claim (to our knowledge).** [Layered rules](../automata/layered.md), Wolfram-style totalistic rules
whose rows are the graph-distance layers around a seed so that they run unchanged on any tiling,
including aperiodic ones, are a construction we have not seen published. They are Jimmie Rodgers's
(2026).

**Evidence.** The closest work embeds one-dimensional rules in hyperbolic tilings along a chosen line
([Margenstern 2010](bibliography.md#margenstern2010embedding)) or draws elementary rules on the fixed rows
of hexagon and pentagon lattices ([Kerekrety 2020](bibliography.md#kerekrety2020elementary)). Dalai's
"Layered Cellular Automata" is unrelated despite the name ([Dalai 2023](bibliography.md#dalai2023layered)).
The idea is simple, so it may exist in unpublished code or in work on rules over graphs that our search
did not reach; Wolfram's work on network systems was not checked in detail.

### 3. Presenting and cataloguing the three-parent hexagon rules

**Claim (to our knowledge).** JETCARP is the first to present the [three-parent hexagon rules](../automata/hexagonal-rows.md)
(`HexT`, `HexT3`) as a family of hexagon pictures with a Wolfram-style numbering of all 256 binary and
3<sup>27</sup> three-state rules, totalistic and symmetric subsets, and an automatic classification of
all binary and totalistic rules. The claim is to the presentation and catalogue only: the underlying
rules are standard second-order one-dimensional automata.

**Evidence.** Second-order rules and staggered neighbourhoods are in
[Wolfram 2002](bibliography.md#wolfram2002new) and the
[Wolfram Language](bibliography.md#wolframnd_cellularautomaton); no source numbered or catalogued this
family as hexagon rules.

**Not claimed.** The two-parent rules `HexW` and `HexW3`, which are Wolfram's two-cell rules
([Wolfram 2002](bibliography.md#wolfram2002new), p. 885) and are also drawn by
[Kerekrety 2020](bibliography.md#kerekrety2020elementary).

### 4. Spaceships on non-square tilings

**Claim (to our knowledge, pending checks).** JETCARP's soup searches have found spaceships in
Life-like rules on the 4.8.8 tiling (in rules other than Life), on 3.12.12, 4.6.12, 3.4.6.4, 3.3.3.4.4
and 3.3.3.3.6, and on several 2- and 3-uniform tilings. We found no earlier report of Life-like
spaceships on these tilings in the rules found.

**Evidence.** Gliders on triangles, hexagons and Cairo pentagons go back to Carter Bays
([Bays 1994](bibliography.md#bays1994cellular); [Bays 2005](bibliography.md#bays2005note);
[Kerekrety 2020](bibliography.md#kerekrety2020game)). Guttesen studied Life (B3/S23) on six Archimedean
lattices and found no gliders on 4.8.8, 4.6.12, 3.12.12 or 3.3.3.3.6
([Guttesen 2026](bibliography.md#guttesen2026game)); JETCARP's ships there are in other rules. No
Life-like spaceship on a k-uniform tiling with k of 2 or more was found in the literature.

**Not claimed.** The small Life gliders on the kagome (3.6.3.6) and snub square tilings: Guttesen
reported them, with a kagome glider gun ([Guttesen 2026](bibliography.md#guttesen2026game)), and JETCARP
found what appear to be the same objects independently. Spaceships on isohedral tilings are also not
claimed: two isohedral types (IH46 and IH54) turned out to have the square grid's touch graph, so their
ships were Conway's glider.

**Pending.** Each remaining ship is being re-verified on the release engine and checked for whether
its tiling's neighbour graph is the same as the square, hexagonal or triangular lattice's. The details,
with scenes, will be published in [Findings](findings.md) once the checks are done. See also
[Life-like rules](../automata/life-like.md#what-is-new-here).

### 5. Sandpiles on the monotiles and across the tiling catalogues

**Claim (to our knowledge).** JETCARP is the first to show sandpile identities and large single-source
sandpiles on the hat and spectre monotiles, and across the k-uniform catalogue (beyond the regular and
Archimedean tilings already studied) and the isohedral types.

**Evidence.** Identities have been computed on triangular, hexagonal and Penrose grids
([Fersula, Noûs and Perrot 2020](bibliography.md#fersula2020sandpile)); single-source piles have a long
literature on the square grid ([Ostojic 2003](bibliography.md#ostojic2003patterns);
[Pegden and Smart 2013](bibliography.md#pegden2013convergence)). No sandpile work on the hat or spectre
was found at all. Identities on the hexagonal and 4.8.8 grids also appear in E. T. Johnson's 2019
master's thesis at the University of Washington, so 4.8.8 is excluded.

**Not claimed.** The doubling method JETCARP uses for very large piles, which follows directly from the
abelian property and is presumably known. See [The Abelian sandpile](../automata/sandpile.md).

### 6. Tilings as a dimension of an exploratory search

**Claim (to our knowledge).** JETCARP's exploratory search is the first to treat the tiling itself as a
search dimension: it explores tiling, rule family, rule and starting pattern together, over about 1,200
periodic and aperiodic tilings and 21 rule families in one archive. Its first full run covered about 15
million runs.

**Evidence.** The method's parts are established: the archive keeps the best few runs per niche, in
the manner of MAP-Elites quality-diversity search, which has been applied to Lenia; and its score is a
small model fitted to human keep/maybe/drop ratings of animated clips, a general technique for learning
from human preferences. Most of the clips were rated by Emily Teich, whose eye for what is beautiful is
built into the score. Earlier cellular automaton searches of this kind work on one substrate, such as
Lenia ([Chan 2019](bibliography.md#chan2019lenia)); searches over several tilings had fixed goals, such as
gliders on three aperiodic tilings ([Edmond 2026](bibliography.md#edmond2026gliders)) or Life on six
Archimedean lattices ([Guttesen 2026](bibliography.md#guttesen2026game)).

The search will be described in full in [Search methods](search-methods.md).

### 7. One engine for every family on every tiling

**Claim (to our knowledge).** No earlier program runs as many cellular automaton families (Life-like,
Generations, Larger than Life, JACA, layered, turmites, WireWorld, cyclic, Greenberg–Hastings,
rock-paper-scissors, Hodgepodge, forest fire, Lenia, sandpiles, priority tables and rule tables) on as
many classes of tiling (lattices, 1,248 k-uniform tilings, 81 isohedral types, drawn tiles, and the
Penrose, hat and spectre tilings) with one neighbour model, tested for agreement across thread counts
and between CPU and GPU.

**Evidence.** Breadth is the contribution, not any one family on any one tiling: Ready runs arbitrary
rules on meshes ([Ready](bibliography.md#huttonnd_ready)); HatLife ran Life-like, Larger than Life-style
and WireWorld rules on the hat ([Trevorrow 2023](bibliography.md#trevorrow2023hatlife)); Edmond ran
Generations and priority-table rules on the hat and spectre ([Edmond 2026](bibliography.md#edmond2026gliders));
Life has long been studied on Penrose tilings ([Owens and Stepney 2010](bibliography.md#owens2010game)).
Our search found no aperiodic-tiling version of the cyclic, Greenberg–Hastings, rock-paper-scissors,
Hodgepodge or forest-fire rules, and no turmites on the hat or spectre, but these are direct applications
of known rules to known graphs, and we do not count them as discoveries. See
[How JETCARP generalises automata](../automata/generalising.md).

## What we do not claim

- **Age colouring**, including colouring cells by how long they have been alive and fading destroyed
  cells: Chris Rowett's LifeViewer and other programs did this first.
- **The two-parent hexagon rules** (`HexW`, `HexW3`): Wolfram's ([Wolfram 2002](bibliography.md#wolfram2002new)).
- **Life gliders on the kagome and snub square tilings**: Guttesen's ([Guttesen 2026](bibliography.md#guttesen2026game)).
- **Hat gliders as a new kind of object**: Edmond's. The hat glider JETCARP's search found is closest to
  his "hat glider B" after relabelling its states, and probably the same mechanism
  ([Edmond 2026](bibliography.md#edmond2026gliders); see [Priority rules](../automata/priority-rules.md)).
- **"The first cellular automata on aperiodic tilings"**: Life on Penrose tilings was studied from 2005,
  and Goucher found the first aperiodic glider in 2012 ([Goucher 2012](bibliography.md#goucher2012gliders)).
- **"The first matter–antimatter Life"**: see JACA above.

## Checks still to do

Before the claims above are final, and before the paper:

- Compare JETCARP's kagome 5-cell and snub-square 12-cell ships with Guttesen's figures cell for cell.
- Run JETCARP's hat glider beside Edmond's glider B.
- Re-verify every claimed spaceship on the release engine, and check each tiling's neighbour graph
  against the regular lattices.
- Re-run the key searches just before publication, and ask on the ConwayLife forums when the results
  are posted there.

## Credits

These findings were made with JETCARP by Jimmie Rodgers. The evolutionary glider search is James
Edmond's method, reimplemented. Emily Teich rated most of the clips that taught the exploratory search
what to show.
