# Rule notation

Every automaton in JETCARP has a rule, written as a short piece of text such as `B3/S23` or
`Cyclic 14/1`. The same text works everywhere: in the app's rule box, in a scene file, on the command
line, in scripts and in the MCP tools. This page lists every form the parser accepts, with its
limits. The encyclopedia page for each family explains what the rules mean; this page only says how
to write them.

A few things hold for every form:

- **Case does not matter**, and spaces are ignored except where they separate words (`HexT3 sum
  588`). `b3/s23`, `B3/S23` and `B3 / S23` are the same rule.
- **JETCARP writes rules back in one canonical form.** A saved scene, a report or the app shows the
  canonical text, so `Rule 30` comes back as `W30` and `B2/S/3` as `B2/S/C3`. The table
  [at the end of this page](#checked-against-the-program) shows what each example becomes.
- **Names are resolved by the front ends, not by scene files.** The app, the command line, scripts
  and the MCP tools accept a rule's name (`Life`, `Brian's Brain`) wherever they accept notation. A
  scene file's `rule` field must hold notation; a name there is refused with *"'Life' is not a
  rule"*. See [Named rules](#named-rules).
- **Some rules need a particular grid.** Elementary rules, Margolus block rules and von Neumann's
  29-state rule need the square grid, the two-parent hexagon rules the `hex` grid and the
  three-parent ones the flat-top `hex_flat` grid. Everything else runs on any tiling. A rule that
  cannot run on the chosen tiling is refused with a message that says which grid it needs.

| Family | Example | Page |
| --- | --- | --- |
| Life-like (birth/survival) | `B3/S23`, `B3/S23V`, `B3,10/S2,3` | [Life-like rules](../automata/life-like.md) |
| Generations | `B2/S/C3` | [Generations](../automata/generations.md) |
| Larger than Life | `R5,C2,S33-57,B34-45` | [Larger than Life](../automata/larger-than-life.md) |
| Elementary | `W30`, `Rule 30` | [Elementary rules](../automata/elementary.md) |
| Hexagonal rows | `HexW 6`, `HexT3 sum 588` | [HexW and HexT](../automata/hexagonal-rows.md) |
| Layered | `Layer 2/01` | [Layered rules](../automata/layered.md) |
| JACA | `JACA +B3/S23 -B36/S23 blast 2` | [JACA](../automata/jaca.md) |
| Ants and turmites | `Ant RL`, `Turmite 1R1,1L0/1R1,0N0` | [Langton's ant and turmites](../automata/turmites.md) |
| WireWorld | `WireWorld` | [WireWorld](../automata/wireworld.md) |
| Cyclic, Greenberg–Hastings, rock-paper-scissors | `Cyclic 16/1`, `GH 3/1`, `RPS 3/3` | [Cyclic rules](../automata/cyclic.md) |
| Hodgepodge, forest fire | `Hodgepodge 200/3/3/28`, `ForestFire 0.01/0.0001` | [Hodgepodge and forest fire](../automata/hodgepodge-forest-fire.md) |
| Margolus block rules | `Margolus BBM` | [Margolus block rules](../automata/margolus.md) |
| Lenia | `Lenia R=13,T=10,b=1,m=0.15,s=0.015` | [Lenia](../automata/lenia.md) |
| Abelian sandpile | `Sandpile` | [The Abelian sandpile](../automata/sandpile.md) |
| Von Neumann's 29 states | `JvN29` | [Von Neumann's automaton](../automata/von-neumann.md) |
| Priority tables | `SpectreGlider`, `Priority 4: …` | [Priority rules](../automata/priority-rules.md) |
| Rule tables | `Table parity` | [Rule tables](../automata/rule-tables.md) |

## Life-like rules: B/S

`B<birth counts>/S<survival counts>`. A dead cell with a birth count of live neighbours is born; a
live cell with a survival count survives. Either list may be empty (`B2/S` is Seeds). The counts may
be written in any order and are sorted when written back (`B32/S2` becomes `B23/S2`).

| Form | Meaning |
| --- | --- |
| `B3/S23` | Touch neighbours (the default) |
| `B3/S23V` | A trailing `V` picks edge neighbours, as on LifeWiki |
| `B2/S34H` | A trailing `H` is accepted and ignored: it records that a rule was meant for hexagons, and the tiling decides what the rule runs on |
| `B3,10/S2,3` | Commas separate the counts as soon as one of them is 10 or more; counts up to 31 are allowed |
| `B10,/S2` | A single count above 9 keeps a trailing comma, so it does not read as 1 and 0 |
| `B014/S2` | Birth on 0 is allowed (see [B0 rules](../automata/life-like.md#b0-rules)) |

Only the birth-first order, with its letters, is read: the older survival/birth forms `23/3` and
`S23/B3` are refused.

## Generations

`B<birth>/S<survival>/C<states>`, with 2 to 255 states. The third part may also be written without
the `C` (`B2/S/3`) or with a `G` (`B2/S/G3`); JETCARP writes it with `C`. With two states the rule is
an ordinary B/S rule (`B3/S23/C2` is `B3/S23`). A trailing `V` picks edge neighbours (`B2/S/C3V`).

## Larger than Life

Two forms, both with fields separated by commas:

| Form | Meaning |
| --- | --- |
| `R5,C2,S33-57,B34-45` | HROT form: range `R`, states `C`, survival `S` and birth `B` counts. The cell itself is not counted |
| `R5,C0,M1,S34..58,B34..45,NM` | MCell's form: `M1` counts the cell itself, so survival bounds are one higher; JETCARP converts it to the HROT form |
| `…,NM`, `…,NN`, `…,N+` | Neighbourhood: Moore (the default; within `R` steps over touch neighbours), von Neumann (within `R` steps over edge neighbours) or the cross (square grid only) |

The range runs from 1 to 20. `C0` and `C2` both mean two states; up to `C255` gives
Generations-style decay. Counts can be single values, ranges written `a-b` or `a..b`, or lists of
both separated by commas (`S2-3,5,7-9`). Weighted and custom neighbourhoods (`NW…` and other `N`
suffixes) are not supported yet, so a few named rules from other programs, such as *Coexistence*,
cannot be run.

## Elementary rules

`W<n>` or `Rule <n>`, with `n` from 0 to 255 (`Rule110` and `W 30` also work). They need the square
grid and write one row per generation.

## Hexagonal row rules: HexW and HexT

| Form | Grid | Numbers |
| --- | --- | --- |
| `HexW n` (also `HexW2 n`) | `hex` | two parents, 2 states: 0 to 15 |
| `HexW3 n` | `hex` | two parents, 3 states: 0 to 19,682 |
| `HexT n` (also `HexT2 n`) | `hex_flat` | three parents, 2 states: 0 to 255 |
| `HexT sum n` | `hex_flat` | three parents, 2 states, totalistic: 0 to 15 |
| `HexT sym n` | `hex_flat` | three parents, 2 states, left-right symmetric: 0 to 63 |
| `HexT3 n` | `hex_flat` | three parents, 3 states: 0 to 3<sup>27</sup> − 1 |
| `HexT3 sum n` | `hex_flat` | three parents, 3 states, totalistic: 0 to 2,186 |
| `HexT3 sym n` | `hex_flat` | three parents, 3 states, symmetric: 0 to 3<sup>18</sup> − 1 |

How a number is read (which digit belongs to which parents) is on the
[hexagonal rows](../automata/hexagonal-rows.md#notation) page.

## Layered rules

`Layer <states>/<outputs>` (also `Layered`), with 2 to 9 states and 1 to 64 output digits. Digit `k`
is the new state of a tile whose parents' states add up to `k`; the list repeats for larger sums.
Every digit must be a state below the state count, so `Layer 2/02` is refused. Layered rules run on
any tiling.

## JACA

`JACA <rule for +1> [<rule for −1>] [blast <r>]`, where each rule is a B/S rule.

| Form | Meaning |
| --- | --- |
| `JACA B3/S23` | Both signs follow the same rule |
| `JACA +B3/S23 -B36/S23` | +1 follows the first rule, −1 the second |
| `JACA B3/S23 -B36/S23` | The signs are optional: the first rule is +1's, the second −1's |
| `JACA B3/S23 blast 2` | Each annihilation also clears every cell within 2 steps; `blast=2` and `x2` mean the same |
| `JACA B13/S234V` | A `V` on a B/S part picks edge neighbours |

The blast radius runs from 0 to 64. Rules with birth on 0 are refused. Counts of 10 or more use
commas, as in other B/S rules.

## Ants and turmites

| Form | Meaning |
| --- | --- |
| `Ant RL`, `Ant`, `Ant: RL` | Langton's ant (`Ant` alone is `RL`) |
| `Turmite LLRR` | A turn string: on a tile of colour `c`, turn by letter `c` and repaint it `c + 1` |
| `Turmite RL2NU` | Turns for hexagons: `R2` and `L2` turn two slots, `N` not at all, `U` back |
| `Turmite 1R1,1L0/1R1,0N0` | A table: one group per state, separated by `/`, one entry per colour, separated by `,` |

Turns are `L`, `R`, `N` (none), `U` (half the tile's degree) and `L2` and `R2` (two slots);
`L1` and `R1` are the same as `L` and `R`. A turn string has 2 to 16 turns. A table entry is
`<colour to write><turn><next state>`; tables have up to 16 states and 2 to 16 colours, with 10 to 15
written `a` to `f`, and every state must have one entry per colour. Ants always walk across edges,
whatever neighbourhood is set. How a turn is measured on tiles that are not squares is on the
[turmites](../automata/turmites.md#turns-on-any-tiling) page.

## Multi-state families with parameters

Parameters follow the name, separated by slashes. Each name alone gives the defaults shown.

| Form | Parameters | Default | Limits |
| --- | --- | --- | --- |
| `Cyclic s/t` | states, threshold | `Cyclic 16/1` | 2 to 255 states; threshold 1 to 32 |
| `GH s/t` (also `Greenberg-Hastings`) | states, threshold | `GH 3/1` | 3 to 255 states; threshold 1 to 32 |
| `RPS s/t` | species, threshold | `RPS 3/3` | 2 to 255 species; threshold 1 to 32 |
| `ForestFire p/f` (also `Forest`) | growth and lightning probabilities | `ForestFire 0.01/0.0001` | each 0 to 1 |
| `Hodgepodge n/k1/k2/g` | n, k1, k2, g | `Hodgepodge 200/3/3/28` | n 2 to 254; k1, k2 1 to 1,000; g 0 to 1,000 |

The cyclic automaton uses edge neighbours by default; the others use touch neighbours.

## Margolus block rules

| Form | Meaning |
| --- | --- |
| `Margolus 0,8,4,3,2,5,9,7,1,6,10,11,12,13,14,15` | The new state of a 2 × 2 block for each block state 0 to 15 (commas or semicolons) |
| `MS,D0;8;4;3;2;5;9;7;1;6;10;11;12;13;14;15` | MCell's form of the same table |
| `Margolus BBM`, `Margolus Critters`, `Margolus Tron`, `Margolus SingleRotation`, `Margolus HPP` | The built-in tables by name (`BilliardBall` also works for BBM) |

Blocks number their cells as MCell does: upper left 1, upper right 2, lower left 4, lower right 8.
Named tables are written back as their 16 outputs. Margolus rules need the square grid.

## Lenia

`Lenia R=…,T=…,b=…,m=…,s=…` with optional `kn=…` and `gn=…`. Parameters may come in any order, may
be fractions (`b=1/2,1`), and Chan's bracketed lists (`b=[1/2,1]`) are read too. `Lenia` alone is
Orbium's rule, `Lenia R=13,T=10,b=1,m=0.15,s=0.015`.

| Parameter | Limits |
| --- | --- |
| `R`, kernel radius in cells | 1 to 64 |
| `T`, time resolution | 1 to 10,000 |
| `b`, kernel peaks | 1 to 8 values from 0 to 1 |
| `m`, `s`, growth centre and width | `s` above 0 |
| `kn`, kernel core | 1 to 4 (default 1) |
| `gn`, growth function | 1 to 3 (default 1) |

## Priority tables

`Priority <states>: <row>; <row>; …` (`PT` is accepted for `Priority`), with 2 to 8 states and at most
64 rows. A row is `<own state>|<conditions>-><next state>`: the own state is a number or `*` for any,
and the conditions are terms `nS>=m` ("at least m neighbours in state S") joined by `&`, or `always`.
The first row that matches wins; a cell that matches none becomes 0.

`SpectreGlider` (also `table:spectre-glider`) is James Edmond's Spectre glider rule, the three-row
table `Priority 4: 0|n0>=3&n1>=1->3; 3|n3>=3->0; 3|n1>=1->1`. See
[Priority rules and the Spectre glider](../automata/priority-rules.md).

## Rule tables

`Table <name>` runs a rule table by name (the name is not case sensitive). One table is built in,
`Table parity`. Other tables come from the scene's own `tables` list (see the
[scene file format](scene-format.md#the-top-level-fields)) or are loaded in the app; a name that no
table has is refused when the automaton is added (*"no rule table named 'codd'"*). The table format
itself is on the [rule tables](../automata/rule-tables.md#jetcarps-own-format) page.

## Single-word rules

| Form | Also accepted |
| --- | --- |
| `WireWorld` | `Wire World` |
| `Sandpile` | `Abelian Sandpile` |
| `JvN29` | `JvN`, `VonNeumann29`, `VonNeumann` |
| `SpectreGlider` | `table:spectre-glider` |

## Named rules

Where the front ends take a rule (the app, `jetcarp` commands, scripts and the MCP tools), they also
take a rule's name. Names are compared by their letters and digits only, in any case, so
`Brian's Brain`, `brians-brain` and `BRIANS BRAIN` are one name, and `Game of Life`, `Conway` and
`Conway's Life` are nicknames for `Life`. Text that is already valid notation is always read as
notation first. `jetcarp rules` lists every name with its notation; the
[named rule catalogue](../automata/named-rules.md) has them all with their credits.

Scene files, again, store notation only: the app and `jetcarp run --save` write the notation, and
the scene's automaton keeps the name in its own `name` field.

## Neighbourhoods

Rules that count neighbours run on touch or edge neighbours (see
[How JETCARP generalises automata](../automata/generalising.md)). The notation can choose
(`V` on a B/S, Generations or JACA rule; `NM`, `NN` and `N+` on Larger than Life); otherwise an
automaton's `neighborhood` setting does, and the rule's default applies when neither does. In a
scene file the setting is `"touch"` or `"edge"`. The command-line `--nb` options, script options
and MCP tools also accept other names, in any case and with `-` or a space in place of `_`: `moore`,
`touching`, `corner`, `vertex`, `t` or `8` for touch, and `von_neumann`, `vonneumann`, `vn`, `e` or
`4` for edge. Ants, elementary and hexagonal row rules, Margolus rules, Lenia, Larger than Life and
von Neumann's rule fix their own neighbourhood, and the setting is ignored for them.

## Checked against the program

<div class="checked">

Every example on this page was parsed by JETCARP 0.4.0 through `jetcarp api call world.new` (each on
a tiling it can run on), and the refusals were checked to fail with the messages quoted. The
neighbourhood names are covered by JETCARP's unit tests, and `moore`, `von-neumann` and a refused
`hex` were also tried on `jetcarp search census --nb`; the scene-file cases were run with
`jetcarp run`. This table shows what JETCARP writes back for the examples whose canonical form
differs from what was typed:

| Typed | Written back |
| --- | --- |
| `b3/s23` | `B3/S23` (named *Life*) |
| `B2/S34H` | `B2/S34` |
| `B32/S2` | `B23/S2` |
| `B3,10/S2,3` | `B3,10/S23` |
| `B2/S/3`, `B2/S/G3` | `B2/S/C3` (named *Brian's Brain*) |
| `B3/S23/C2` | `B3/S23` |
| `R5,C0,M1,S34..58,B34..45,NM` | `R5,C2,S33-57,B34-45` (named *Bosco's Rule*) |
| `Rule 30`, `Rule110`, `W 30` | `W30`, `W110`, `W30` |
| `HexW2 6`, `HexT2 30` | `HexW 6`, `HexT 30` |
| `Layered 2/0110` | `Layer 2/0110` |
| `JACA B3/S23 -B36/S23` | `JACA +B3/S23 -B36/S23` |
| `JACA B3/S23 blast=2`, `JACA B3/S23 x2` | `JACA B3/S23 blast 2` |
| `Ant`, `Ant: RL`, `Turmite R1L1` | `Ant RL` (named *Langton's ant*) |
| `Cyclic`, `GH`, `RPS`, `ForestFire`, `Hodgepodge` | `Cyclic 16/1`, `GH 3/1`, `RPS 3/3`, `ForestFire 0.01/0.0001`, `Hodgepodge 200/3/3/28` |
| `Margolus BBM`, `Margolus BilliardBall`, `MS,D0;8;4;…` | `Margolus 0,8,4,3,2,5,9,7,1,6,10,11,12,13,14,15` |
| `Lenia`, `Lenia R=13,T=10,b=[1/2,1],m=0.15,s=0.015` | `Lenia R=13,T=10,b=1,m=0.15,s=0.015`, `Lenia R=13,T=10,b=1/2,1,m=0.15,s=0.015` |
| `PT 4: 0\|n0>=3&n1>=1->3` | `Priority 4: 0\|n0>=3&n1>=1->3` |
| `table:spectre-glider` | `SpectreGlider` |
| `JvN`, `VonNeumann29` | `JvN29` |
| `Abelian Sandpile`, `wire world` | `Sandpile`, `WireWorld` |
| `Life`, `Game of Life`, `Conway` | `B3/S23` |
| `Brian's Brain`, `brians-brain` | `B2/S/C3` |
| `Gnarl (von Neumann)` | `B1/S0V` |
| `Critters` | `Margolus 15,14,13,3,11,5,6,1,7,9,10,2,12,4,8,0` |
| `Rule 184` (a name) | `W184` |

Refused as expected: `23/3`, `S23/B3`, `W256`, `HexT 256`, `HexT sym 64`, `Layer 2/02`,
`JACA B03/S23`, `JACA … blast 65`, `B2/S/C1`, `B2/S/C256`, `R21,…`, `…,NW0010`, `R3,C2,S2,B3,N+` on
hexagons, `Lenia R=65`, `Priority 9: …`, `Table codd`, `Cyclic 1/1`, `Hodgepodge 255/3/3/28`, an
incomplete turmite table, `W30`, `Margolus BBM` and `JvN29` off the square grid, `HexW 6` off `hex`,
`HexT 30` off `hex_flat`, the name `Coexistence`, and a name in a scene file.

</div>
