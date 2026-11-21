# 02. Generations: history and credits

Research date: 2026-11-10. Factual basis for the "Where it came from" section of the encyclopedia page.

## Short history

- **The idea: a Life-like rule whose dying cells linger.** A Generations rule is a Life-like (outer
  totalistic, Moore-neighbourhood) birth/survival rule plus extra "ageing" states. A live cell that
  fails its survival condition does not vanish. It steps through states 2, 3, … up to C−1 and then
  becomes empty. Ageing cells do not count as live neighbours, and nothing can be born on them
  [wojtowicznd_families; gollynd_generations]. The name refers to cells "getting older" before
  they die [lifewikind_generations].
- **Brian's Brain (1987 or earlier) is the earliest dated member found.** Toffoli and Margolus,
  *Cellular Automata Machines* (MIT Press, 1987), sec. 6.1 "Firing of neurons: a three-state rule"
  (pp. 47–49), describe a rule with *ready*, *firing* and *refractory* states. A ready cell fires
  when exactly two of its eight neighbours are firing, a firing cell becomes refractory, and a
  refractory cell becomes ready. In Generations notation this is B2/S/C3. They implemented it on
  CAM-6 as BRIAN'S-BRAIN, and a footnote states that the rule "was suggested by Brian Silverman"
  (the neuron interpretation being theirs) [toffoli1987cellular]. LifeWiki's Brian's Brain page
  says the rule was "first considered by Brian Silverman in the mid-1990s" [lifewikind_briansbrain].
  The 1987 book contradicts that date, so the encyclopedia should use "by 1987". The CelLab manual
  also lists its "Brain" rule as identical to Toffoli and Margolus's Brian's Brain (p. 47)
  [walkernd_cellabrules].
- **CelLab (1989).** Rudy Rucker and John Walker's *Rudy Rucker's Cellular Automata Laboratory*
  (Autodesk, shipped June 1989; Web edition 1995, later WebCA in JavaScript) ran many multi-state
  rules of this kind, including Brain [rucker1989cellab; walkernd_cellabrules]. Several Generations
  rules in the MCell and LifeWiki lists are credited to "Rudy Rucker and John Walker" (Faders,
  RainZha) or to Rucker alone (Cooties, Sticks) [lifewikind_generationslist; wojtowicznd_rules].
  We did not find the dates of these rules.
- **MCell names and organises the family (1999–2001).** Mirek Wójtowicz wrote MLife on 8 January
  1999. It grew into MCLife ("Mirek's Colorful Life") and then MCell, "Mirek's Cellebration" (a name
  David Griffeath suggested in late 1999), a Windows program in Delphi [wojtowicz2024mcell]. Its
  author says that Jean-Philippe Rennard's CAV program prompted him to add colours, states and
  "history" [wojtowicz2024mcell]. MCell made "Generations" a named rule family with an S/B/C
  notation (survival / birth / number of states; Brian's Brain is `/2/3`, Star Wars `345/2/4`). Its
  family page calls Generations "perhaps the most beautiful family in MCell" and names Silverman's
  Brian's Brain and Wójtowicz's own Star Wars as the author's favourites [wojtowicznd_families].
  Wikipedia's reference list cites the MCell Generations lexicon page as dated 2001-09-15 (pointer
  only; that old page is no longer reachable). We found no source that explicitly says who coined
  the name "Generations". It is MCell's family name, and LifeWiki, Golly and others took it up.
- **Star Wars (March 1999).** By Mirek Wójtowicz (MCell lexicon, LifeWiki list, Golly help), dated
  March 1999 on LifeWiki [wojtowicznd_rules; lifewikind_generationslist; gollynd_generations]. In
  his history of MCell, Wójtowicz recalls: "One evening, we discovered the 'Star Wars' rule"
  [wojtowicz2024mcell].
- **A community of rule authors (1999–2001).** The MCell lexicon and the LifeWiki list credit rules
  to Mirek Wójtowicz (Banners, Caterpillars, Frozen Spirals, Lava, Living On The Edge, Nova,
  Prairie on fire, Star Wars, Wanderers, Worms), John Elliott (Bloomerang, Fireworks, Glisserati,
  Glissergy, OrthoGo, SediMental, Soft Freeze, Thrill Grill, Transers, Xtasy; dated December 1999 to
  July 2000), Michael Sweney (Brain 6, Burst, Burst II, Ebb and Flow I and II, Transers II), Charles
  A. Rockafellor (Circuit Genesis, Flaming Starbows, Meteor Guns), Scott Robert Ladd (Frogs, Swirl),
  Brian Prentice (Rake, Snake), Jason Rampe (BelZhab Sediment), Anders Starmark (Lines, September
  2000), Rudy Rucker and John Walker, and later EricG (9:43 at Knight, LifeWiki only)
  [wojtowicznd_rules; lifewikind_generationslist]. Brian Prentice defined the Weighted Generations
  extension in 2001 [wojtowicznd_families].
- **Pattern work in Brian's Brain.** LifeWiki credits Michael Sweney with the first oscillators
  (December 1999), Giles Edkins with lasers and logic gates (by May 2001), Yoel Matveyev with a Rule
  110 unit cell (November 2020), which makes the rule Turing-complete, and Rocknlol with a c/5
  orthogonal spaceship (December 2020). These come from forum posts that LifeWiki cites
  [lifewikind_briansbrain]. They are LifeWiki claims and were not checked against the forum.
- **Later software.** Golly added a Generations algorithm in version 2.0 (December 2008)
  [gollynd_changes]. It supports up to 256 states and the hexagonal (`H`) and von Neumann (`V`)
  suffixes [gollynd_generations]. LifeViewer (Chris Rowett) supports non-B0 Generations rules from
  build 180, and CAViewer supports B0 Generations rules [lifewikind_generations]. MCell for Web, a
  2024 JavaScript rewrite of MCell by Wójtowicz, is open source [wojtowicz2024mcell].
- **Notation variants.** MCell writes S/B/C (`/2/3`). Golly accepts that order [gollynd_generations].
  LifeWiki's standard is `Bx/Sy/n`, with the alternatives `Bx/Sy/Cn` and `Bx/Sy/Gn`. LifeWiki warns
  that "C" can be mistaken for a symmetry suffix [lifewikind_generations].

## Key primary citations

- [toffoli1987cellular] T. Toffoli and N. Margolus, *Cellular Automata Machines: A New Environment
  for Modeling*, MIT Press (1987), sec. 6.1, pp. 47–49 (BRIAN'S-BRAIN; credit to Brian Silverman in
  footnote 1). ISBN 0-262-20060-0. PDF: https://people.csail.mit.edu/nhm/cam-book.pdf . Verified:
  the OCR text of the PDF on Margolus's MIT page was read (sec. 6.1, footnote, index entry
  "BRIAN'S-BRAIN, 48"). ISBN from the CiNii Books record.
- [wojtowicznd_rules] M. Wójtowicz, MCell rules lexicon (data file behind
  https://mcell.ca/pages/rules.html ), https://mcell.ca/app/data/rules.txt . Verified: downloaded,
  and the "A rule by …" credit was read for each Generations entry.
- [wojtowicznd_families] M. Wójtowicz, "MCell: Supported CA Families",
  https://mcell.ca/pages/families.html . Verified: downloaded and read the Generations and Weighted
  Generations sections.
- [wojtowicz2024mcell] M. Wójtowicz, "MCell – Mirek's Cellebration" (history of MCell, signed 28
  Dec 2024), https://mcell.ca/pages/mcell.html . Verified: downloaded and read (mirekw.com/ca/
  redirects here).
- [lifewikind_generationslist] LifeWiki, "List of Generations rules" (revision of 22 Dec 2025),
  https://conwaylife.com/wiki/List_of_Generations_rules . Verified: through the mirror
  conwaylife.njohnston.ca (Author and Date columns read).
- [lifewikind_generations] LifeWiki, "Generations" (revision of 27 Aug 2025). Verified: through the
  mirror.
- [gollynd_generations] Golly Help, "Generations",
  https://golly.sourceforge.io/Help/Algorithms/Generations.html . Verified: fetched.
- [rucker1989cellab] R. Rucker and J. Walker, CelLab (*Rudy Rucker's Cellular Automata Laboratory*),
  Autodesk (1989), https://www.fourmilab.ch/cellab/ . Verified: history page fetched.

## Software that ran it before JETCARP

- CAM-6 hardware with CAM Forth (Toffoli and Margolus 1987): BRIAN'S-BRAIN [toffoli1987cellular].
- CelLab / *Rudy Rucker's Cellular Automata Laboratory* (Rucker and Walker; Autodesk 1989; Web
  edition 1995; WebCA in JavaScript, after 2017): the "Brain" rule and other multi-state rules
  [rucker1989cellab; walkernd_cellabrules].
- MCell (Mirek Wójtowicz; Windows from 1999; web from 2024; https://github.com/MiDevel/mcell-webapp ):
  the Generations family, 45 built-in rules, the S/B/C notation [wojtowicz2024mcell; wojtowicznd_rules].
- Golly (from 2.0, December 2008) [gollynd_changes]; LifeViewer (from build 180) and CAViewer
  [lifewikind_generations].
- On aperiodic tilings: Generations-style rules ran on the hat in Andrew Trevorrow's HatLife (2023)
  and on the hat and spectre in James Edmond's monotile-ca (2026). See novelty-06 and novelty-08 in
  this folder.

## People JETCARP should credit

- **Brian Silverman**, Brian's Brain (credited by Toffoli and Margolus 1987) and Seeds.
- **Tommaso Toffoli and Norman Margolus**, the first published description of Brian's Brain we
  found (CAM-6, 1987).
- **Mirek Wójtowicz**, for MCell, the Generations family and its notation, Star Wars and many other
  rules.
- The rule authors named above (John Elliott, Michael Sweney, Charles A. Rockafellor, Scott Robert
  Ladd, Brian Prentice, Jason Rampe, Anders Starmark, Rudy Rucker, John Walker, EricG), per rule in
  `named-rule-authors.md`.
- **Rudy Rucker and John Walker**, for CelLab.

## How JETCARP's version relates

The README's notation is `B…/S…/C<states>` (e.g. `B2/S/C3`). Generations runs on every tiling's
neighbour graph and is among the families with GPU simulation. The 45 named Generations rules come
from LifeWiki's list.

## Unverified

- Who coined the family name "Generations", and when. MCell is the earliest user we found. The old
  MCell page (dated 2001-09-15 in Wikipedia's references) could not be fetched
  (psoup.math.wisc.edu and mirekw.com/ca/rullex_gene.html are unreachable or redirected).
- Whether Silverman's own software ("Phantom Fishtank", cited as [Silverman87] in the CelLab manual
  for the Balloons rule) or Resnick and Silverman's "Exploring Emergence" (1996, archived; listed by
  Wikipedia) describes Brian's Brain. Neither was read.
- Dates of the rules by Rucker and Walker, Sweney, Rockafellor, Ladd and Prentice (LifeWiki leaves
  them blank).
- The Brian's Brain pattern credits (Sweney, Edkins, Matveyev, Rocknlol) rest on LifeWiki only.
- Whether CelLab's 1989 rule set already contained B2/S/C3-style rules under other names (only the
  first 100,000 characters of the CelLab rules page were read).
- Bombers: LifeWiki credits Mirek Wójtowicz, but MCell's lexicon says "Author unknown" (see
  `named-rule-authors.md`).

## Search log

- WebFetch: conwaylife.njohnston.ca/wiki/List_of_Generations_rules (twice: authors, dates),
  /wiki/Generations, /wiki/Brian%27s_Brain; en.wikipedia.org/wiki/Brian%27s_Brain (reference list
  only); golly.sourceforge.io/Help/Algorithms/Generations.html, /Help/changes.html;
  www.fourmilab.ch/cellab/ and /cellab/manual/rules.html; mirekw.com/ca/rullex_life.html (301 to
  mcell.ca); mcell.ca; mcell.ca/pages/rules.html (JavaScript only).
- curl (data only, parsed with `python3 -I`): mcell.ca/app/data/rules.txt, /pages/families.html,
  /pages/mcell.html; people.csail.mit.edu/nhm/cam-book.pdf (text extracted from the OCR layer with
  a local zlib script; sec. 6.1 and the index read).
- WebSearch: `"Brian's Brain" Toffoli Margolus "Cellular Automata Machines" 1987 Silverman rule`;
  `Toffoli Margolus "Cellular Automata Machines: A New Environment for Modeling" MIT Press 1987 ISBN`.
- Unreachable: psoup.math.wisc.edu (no response), conwaylife.com (HTTP 403).
