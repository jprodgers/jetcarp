# Credits and acknowledgements

JETCARP is one person's program built on the work of a great many others. This page says who did
what. It is explicit about provenance on purpose: where JETCARP uses someone's code, data, rule or
idea, that person is named here and on the page that explains it, and JETCARP's author is credited only
for what [What is new in JETCARP](../research/contributions.md) supports.

## Author

**Jimmie Rodgers** ([jimmieprodgers.com](https://jimmieprodgers.com); ORCID
[0009-0003-8775-6483](https://orcid.org/0009-0003-8775-6483)) designed JETCARP, chose its research
questions and directions, and tested it on Linux, Windows and macOS. What JETCARP adds that is, to our
knowledge, new, each with its evidence on [What is new in JETCARP](../research/contributions.md):

- [JACA](../automata/jaca.md), the annihilating two-sided family of Life-like rules, is Jimmie's idea
  (November 2026); each of its ingredients has a precedent, credited below.
- [Layered rules](../automata/layered.md), Wolfram-style rules whose rows are the graph-distance layers
  around a seed, so that they run on any tiling, are Jimmie's construction (2026).
- The presentation and catalogue of the [three-parent hexagon rules](../automata/hexagonal-rows.md)
  (`HexT`); the underlying rules are standard second-order automata, and the two-parent rules are
  Wolfram's.
- The spaceships on Archimedean and k-uniform tilings listed on the [Findings](../research/findings.md)
  page, found with JETCARP by Jimmie Rodgers; 29 of them are claimed, to our knowledge, as not
  reported before.
- Sandpile identities and large piles on the hat and spectre and across the tiling catalogues, the
  tiling as a dimension of an exploratory search, and one engine running every family on every
  tiling.

## Acknowledgements

**Emily Teich** applied her aesthetics to JETCARP's exploratory search. She rated most of the
animated clips, hundreds of them, from which the search learned its score. Her ratings decide which
runs the search shows people, and so brought her eye for what is beautiful into the search itself
(see [Search methods](../research/search-methods.md#explore)).

**Craig S. Kaplan**'s work is under a large part of JETCARP. Kaplan is one of the four authors of the
papers that introduced the [hat and the spectre](../tilings/hat-spectre.md); Kaplan's hatviz code is
the basis of JETCARP's hat tiling, Kaplan's Spectre visualizer of its spectre tiling, and Kaplan's
Tactile library of its 81 [isohedral types](../tilings/isohedral.md); and Kaplan's papers, thesis and
book on tiling theory and Escherization explain much of what those pages describe. When asked whether
JETCARP could use the code of the Spectre visualizer, which had never been formally released, Kaplan
answered within hours, and generously: Kaplan made it available under the same BSD licence as
hatviz. Thank you.

**James Edmond**'s work on gliders on the hat and spectre gave JETCARP its
[priority tables](../automata/priority-rules.md), the Spectre glider, and the evolutionary glider
search that JETCARP reimplements on every tiling, with two small functions adapted from Edmond's
monotile-ca code under its MIT licence. The hat glider that JETCARP's search found is Edmond's glider B.

**Claude**, Anthropic's AI model: all of JETCARP's code was written by Claude Code, Anthropic's AI
coding assistant, to Jimmie's design and direction. Claude is acknowledged here, not listed as an author; how the
work was divided and checked is described in [How JETCARP was made](how-made.md).

**LifeWiki's editors and the ConwayLife.com community** keep the lists of named rules and the credits
from which JETCARP's [named rule catalogue](../automata/named-rules.md) is drawn.

## Code and data that JETCARP contains

Listed in full, with their licences, on the [Licence](licence.md#third-party-work) page:

- **Craig S. Kaplan**: hatviz (the hat substitution), the Spectre visualizer (the spectre
  substitution), and tactile-js (the isohedral tiling tables).
- **Brian Galebach**, for the catalogue of k-uniform tilings, and **J. E. Soto Sánchez**, with **Tim
  Weyrich**, **Asla Medeiros e Sá** and **Luiz Henrique de Figueiredo**, for the integer representation
  in which Soto Sánchez publishes it, the data file JETCARP converts
  ([k-uniform tilings](../tilings/k-uniform.md)).
- **James Edmond**: the mutation step and rule notation of monotile-ca.
- **Bert Wang-Chak Chan**: the *Orbium unicaudatus* pattern from Chan's Lenia repository
  ([Lenia](../automata/lenia.md)).
- The authors of the Rust libraries JETCARP is built from, of SQLite, and of the fonts that ship with
  egui (listed in each release's notices).

## The work JETCARP builds on

Grouped by the page that tells each story; the page has the references. Names are given as the
sources give them.

### How JETCARP generalises automata

[How JETCARP generalises automata](../automata/generalising.md)

- **Edward F. Moore** and **John von Neumann**: the two square-grid neighbourhoods that JETCARP's touch
  and edge neighbours generalise.
- **Marcel Golay**: hexagonal cellular logic for image processing (1969).
- **Carter Bays**: Life-like rules on triangular, hexagonal and Cairo pentagon tilings, and their
  gliders.
- **Tim Hutton** and the GollyGang project: Ready, which runs rules on any mesh.

### Life-like rules and Generations

[Life-like rules](../automata/life-like.md), [Generations](../automata/generations.md)

- **John Horton Conway**: the Game of Life (1970).
- **Martin Gardner**: the *Scientific American* columns that brought Life to the world (1970, 1971),
  and later Penrose tilings and Paterson and Conway's worms.
- **Bill Gosper** and the MIT group: the glider gun (1970).
- **Elwyn Berlekamp**, **John Conway** and **Richard Guy**: the sketch of Life's universality in
  *Winning Ways* (1982); **Paul Rendell**: a Turing machine in Life (2002).
- **Norman Packard** and **Stephen Wolfram**: the survey and numbering of two-dimensional rules (1985).
- **David Eppstein**: the classification of Life-like rules by growth (2010), and the idea behind
  running B0 rules without strobing.
- **Robert Wainwright**: the *Lifeline* newsletter, where 3-4 Life appeared (1971).
- **Nathan Thompson**: HighLife (1994) and Day & Night (from 1997).
- **Brian Silverman**: Seeds, Brian's Brain and WireWorld.
- **Tommaso Toffoli** and **Norman Margolus**: the CAM-6 machine and *Cellular Automata Machines*
  (1987), with INKSPOT, Brian's Brain, Critters and Tron.
- **Gérard Vichniac**: the Vote and Anneal rules.
- **Mirek Wójtowicz**: MCell, its rules lexicon, the Generations family and its notation, and many
  named rules.
- **Rudy Rucker** and **John Walker**: CelLab (1989).
- **Andrew Trevorrow** and **Tom Rokicki**: Golly (from 2005).
- **Chris Rowett**: LifeViewer, whose age colouring came before JETCARP's.

### Named rules

The rule authors credited in the [named rule catalogue](../automata/named-rules.md), where each rule's
credit and source are given:

- **John Elliott**: Bloomerang, Fireworks, Glisserati, Glissergy, OrthoGo, SediMental, Soft Freeze,
  Thrill Grill, Transers and Xtasy.
- **Michael Sweney**: Brain 6, Burst, Burst II, Ebb and Flow, Ebb and Flow II and Transers II.
- **Mirek Wójtowicz**: Banners, Caterpillars, Frozen Spirals, Globe, Lava, Living On The Edge, Nova,
  Prairie on fire, Star Wars, Wanderers and Worms, and the name of Seeds.
- **Charles A. Rockafellor**: Circuit Genesis, Flaming Starbows, Mazectric, Meteor Guns and ModernArt.
- **Rudy Rucker**, with **John Walker** for Faders and RainZha: Cooties, Faders, RainZha and Sticks.
- **Kellie Evans**: Bosco's Rule, Gnarl and its von Neumann form, and Waffle.
- **David Griffeath**: Majority, Majorly and Bugsmovie.
- **Jason Rampe**: InverseLife and BelZhab Sediment. **Scott Robert Ladd**: Frogs and Swirl.
  **Brian Prentice**: Rake and Snake.
- **Dean Hickerson**: Diamoeba, and with **David Eppstein**, exploring the rule named Eppstein.
  **Edward Fredkin**: Fredkin's rule. **Andrew Trevorrow**: Long Life. **David Macfarlane**: Walled
  Cities. **Jean-Claude Heudin**: LowLife. **Joshua Herman**, working with **Louis Kauffman**:
  SnowLife. **Janko Gravner**: Flakes (Life without Death), in MCell's credit. **Anders Starmark**:
  Lines. **H. H. P. M. P. Cole**: Factorio. **Stephen Morley**: the study after which Morley is named.
- Forum names: **phdanielli** (Virus), **melwin22** (Castles), **EricG** (9:43 at Knight),
  **bubblegum** (Coexistence) and **drc** (the Hexagonal Life thread).
- **Wentian Li**, and **Krug** and **Spohn**, for the early studies of Rule 184.

### Larger than Life

[Larger than Life](../automata/larger-than-life.md)

- **David Griffeath**: first imagined the family (early 1990s).
- **Kellie Michele Evans**: developed it in a 1996 thesis and later papers; Bosco and Bugs.
- **Dean Hickerson**: the first bug gun.
- **Adam P. Goucher**: the lifelib notation for HROT rules.

### Elementary and hexagonal row rules

[Elementary rules](../automata/elementary.md), [HexW and HexT](../automata/hexagonal-rows.md)

- **Gustav Hedlund**: the symbolic-dynamics view of one-dimensional automata (1969).
- **Stephen Wolfram**: the systematic study and numbering of the elementary rules, their four classes,
  rule 30, and the two-parent hexagon rules (*A New Kind of Science*, 2002).
- **Olivier Martin** and **Andrew Odlyzko**, with Wolfram: the algebra of additive rules (1984).
- **Matthew Cook**: the universality of rule 110 (2004); **Turlough Neary** and **Damien Woods**: its
  P-completeness (2006).
- **Matej Kerekrety**: a web tool drawing elementary rules on hexagons and pentagons (2020).
- **U. Frisch**, **B. Hasslacher** and **Y. Pomeau**: the hexagonal lattice gas (1986).

### Layered rules and JACA

[Layered rules](../automata/layered.md), [JACA](../automata/jaca.md)

- **Maurice Margenstern**: one-dimensional automata embedded in hyperbolic tilings, the closest work
  to layered rules that we found.
- **Mark Levene** and **George Roussos**: two-player Life (2003).
- The **Unsupervision Blog**'s account of the CodinGame game "Game of Life or Death" (2025): +1 and −1
  cells, own-sign counting and cancelled births.
- **KittyTac** and **wildmyron**: matter–antimatter Life and the Energetic rule (2018).
- **Douglas Toussaint** and **Frank Wilczek**, and **Bastien Chopard** and **Michel Droz**:
  annihilation of opposite particles in physics and in cellular automata.

### Langton's ant and turmites

[Langton's ant and turmites](../automata/turmites.md)

- **Christopher G. Langton**: the ant, as *vants* (1986), and the self-reproducing loop (1984).
- **Greg Turk**: rediscovered the ant and built many turmites (around 1989), and their 2025 history.
- **A. K. Dewdney**: the *Scientific American* columns that named the tur-mite and spread the
  hodgepodge machine, cyclic space and WireWorld.
- **David Gale**, **James Propp**, **Scott Sutherland** and **Serge Troubetzkoy**: generalised ants and
  their symmetric patterns; **Ian Stewart**: the ant for a wide audience.
- **L. A. Bunimovich** and **S. E. Troubetzkoy**: the ant's unbounded path; **E. G. D. Cohen** and
  **X. P. Kong**: lattice gases on the triangular lattice.
- **A. Gajardo**, **A. Moreira** and **E. Goles**, and **Tatsuie Tsukiji** and **Takeo Hagiwara**: the
  complexity of the ant.
- **Tim Hutton**: turmite rules for Golly; **Allen H. Brady**: the first study of triangular turmites;
  **Uri Wilensky**: the NetLogo model; the authors of **ptiles/ant**: ants on Penrose tilings.

### WireWorld, cyclic, excitable and forest rules

[WireWorld](../automata/wireworld.md), [Cyclic, Greenberg–Hastings and rock-paper-scissors](../automata/cyclic.md),
[Hodgepodge and forest fire](../automata/hodgepodge-forest-fire.md)

- **Brian Silverman**: WireWorld; **David Moore** and **Mark Owen**: the WireWorld computer.
- **J. M. Greenberg** and **S. P. Hastings**: the Greenberg–Hastings model (1978), with **B. D. Hassard**.
- **David Griffeath**: the cyclic cellular automaton; **Maury Bramson**, **Robert Fisch**, **Janko
  Gravner**, **Richard Durrett** and **Jeffrey Steif**: its theory and that of excitable media.
- **Kei-ichi Tainaka**, **Marcus Frean** and **Edward Abraham**, **Benjamin Kerr**, **Margaret Riley**,
  **Marcus Feldman** and **Brendan Bohannan**, and **Tobias Reichenbach**, **Mauro Mobilia** and **Erwin
  Frey**: rock-paper-scissors dynamics; **Softology** (Visions of Chaos): the deterministic rule
  JETCARP implements.
- **Martin Gerhardt** and **Heike Schuster**: the hodgepodge machine, and with **John J. Tyson**, a
  model of excitable media.
- **Per Bak**, **Kan Chen** and **Chao Tang**: the forest-fire automaton (1990); **Barbara Drossel**
  and **Franz Schwabl**: lightning and the standard model (1992); **Christopher L. Henley**, **Siegfried Clar**
  and **Peter Grassberger**: its theory.

### Margolus block rules

[Margolus block rules](../automata/margolus.md)

- **Edward Fredkin** and **Tommaso Toffoli**: conservative logic and the billiard-ball model (1982).
- **Norman Margolus**: the partitioning scheme named after Margolus, the billiard ball machine and Critters.
- **J. Hardy**, **Y. Pomeau** and **O. de Pazzis**: the HPP lattice gas (1973).
- **dmishin**: the Single Rotation rule (2013).

### Lenia

[Lenia](../automata/lenia.md)

- **Bert Wang-Chak Chan**: Lenia and its creatures (2019), and the reference code.
- **Marcus Pivato** (RealLife) and **Stephan Rafler** (SmoothLife): its continuous predecessors.
- **Erwan Plantec** and co-authors: Flow-Lenia.

### The Abelian sandpile

[The Abelian sandpile](../automata/sandpile.md)

- **Per Bak**, **Chao Tang** and **Kurt Wiesenfeld**: the sandpile and self-organised criticality
  (1987).
- **Deepak Dhar**: the abelian property and the sandpile group (1990); with **S. N. Majumdar**, the link
  to spanning trees.
- **Anders Björner**, **László Lovász** and **Peter Shor**: chip-firing games (1991).
- **Michael Creutz**: the identity and its fractal structure; **Sergio Caracciolo**, **Guglielmo
  Paoletti** and **Andrea Sportiello**, and **Yvan Le Borgne** and **Dominique Rossin**: its study.
- **Srdjan Ostojic**, and **Wesley Pegden** and **Charles Smart**: single-source piles and their limit.
- **Lionel Levine** and **James Propp**: the survey of sandpile groups (2010).
- **Jérémy Fersula**, **Camille Noûs** and **Kévin Perrot**: identities on triangular, hexagonal and
  Penrose grids (2020); **Eli T. Johnson**: identities on hexagonal and 4.8.8 grids (2019).

### Von Neumann's automaton and rule tables

[Von Neumann's 29-state automaton](../automata/von-neumann.md), [Rule tables](../automata/rule-tables.md)

- **John von Neumann**: the 29-state self-reproducing automaton; **Arthur W. Burks**: edited and
  completed it (1966); **Stanisław Ulam**: suggested the cellular framework.
- **Renato Nobili** and **Umberto Pesavento**: the first working self-reproducing constructor (1994).
- **Edgar F. Codd**: the eight-state automaton, advised by **John H. Holland**.
- **Tim Hutton**: Codd's computer built in full, Golly's RuleTable format and the Rule Table
  Repository, with John Devore's variant of Codd's rule and the loops of Langton's successors **Byl**,
  **Chou** and **Reggia**, and **Sayama**.

### Priority rules and searches

[Priority rules and the Spectre glider](../automata/priority-rules.md), [Search methods](../research/search-methods.md)

- **James Edmond**: priority tables, the Spectre glider, the hat gliders and the evolutionary search
  (2026).
- **Adam P. Goucher**: the first glider on an aperiodic tiling (2012).
- **Andrew Trevorrow**: HatLife (2023).
- **Jean-Baptiste Mouret** and **Jeff Clune**: MAP-Elites, the basis of the exploratory search.
- **Paul Christiano** and co-authors: learning a score from human preferences; **Jimmy Secretan** and
  co-authors: Picbreeder.
- **EricG**: Life-like rules on all eight mixed Archimedean tilings and their duals, emulated in
  Golly, with spaceships reported on every Archimedean tiling (ConwayLife forums, 2013); the earliest
  survey of these tilings that we know of, and the reason four of JETCARP's ships are flagged rather
  than claimed. The one Life ship JETCARP's census finds on 3.3.3.4.4 is probably the one EricG reported
  there. See [What is new in JETCARP](../research/contributions.md#4-spaceships-on-non-square-tilings).
- **Peter Taylor**, **Kevin L** and **Martin Büttner** (now Martin Ender): Life on non-square tilings in
  a 2014 Code Golf challenge; Büttner's period-20 spaceship, very likely the one JETCARP's hunts find in
  Life on the 2-uniform tiling k2-007, is the earliest ship on a k-uniform tiling that we know of.
- **H. S. Guttesen**: Life on six Archimedean lattices, with gliders on the kagome and snub square
  tilings (Code Golf answers in 2024, a paper in 2026), which JETCARP's searches found again
  independently.
- **Tessellation Automata** (YouTube, 2024): Life-like rules on many tilings, among them Life gliders on
  the snub square tiling and other rules on the 2-uniform tiling k2-001; **Owen Gillespie** and
  **Forest Kobayashi** (2017): a threshold rule on k2-001.

### Tilings

[Tilings as neighbour graphs](../tilings/neighbour-graphs.md),
[Regular and Archimedean tilings](../tilings/regular-archimedean.md),
[k-uniform tilings](../tilings/k-uniform.md), [Isohedral tilings](../tilings/isohedral.md),
[Editable tile families](../tilings/editable-families.md)

- **Branko Grünbaum** and **G. C. Shephard**: *Tilings and Patterns* and the classification of
  isohedral tilings.
- **Johannes Kepler**: the first systematic treatment of the Archimedean tilings (1619);
  **H. M. Cundy** and **A. P. Rollett**: the book after which the dotted vertex notation is commonly
  named; **Darrah Chavey**: Kepler's results and the
  catalogue of 3-uniform tilings.
- **Otto Krötenheerdt**: the k-uniform tilings with distinct vertex types (1969–70); **Brian
  Galebach**: the complete catalogue up to k = 6; **Marek Čtrnáct** and **Eryk Kopczyński**: counts
  beyond it.
- **Elli** and **Heinrich Heesch**, and **Otto Kienzle**: the 28 types of asymmetric tiles for isohedral
  tilings; **Doris Schattschneider**: Escher's periodic drawings and the pentagon tilings;
  **Tom McLean**: a catalogue of the isohedral types; **M. C. Escher**: the lizards.
- **R. H. Macmillan**, **Frank Morgan** and earlier writers on the Cairo tiling; **Greg Frederickson**
  and **Roger Nelsen**, among others, on the Pythagorean tiling.

### Penrose tilings, the hat and the spectre

[Penrose tilings](../tilings/penrose.md), [The hat and the spectre](../tilings/hat-spectre.md)

- **Robert Berger** and **Raphael M. Robinson**: the first aperiodic sets of tiles.
- **Roger Penrose**: the kites and darts and the rhombs; **N. G. de Bruijn**: their algebraic theory;
  **Jeff Preshing**: a widely read explanation of drawing them by triangle subdivision.
- **Margaret Hill**, **Susan Stepney**, **Francis Wan** and **Nick Owens**: Life on Penrose tilings;
  **D. A. Bailey** and **K. A. Lindsey**: a Game of Life on them.
- **Joshua Socolar** and **Joan Taylor**: a single aperiodic tile with matching rules (2011).
- **David Smith**, who found the hat in 2022 with the help of **Jaap Scherphuis**'s PolyForm Puzzle
  Solver, and **Joseph Samuel Myers**, **Craig S. Kaplan** and **Chaim Goodman-Strauss**: the hat and the
  spectre (2023).
