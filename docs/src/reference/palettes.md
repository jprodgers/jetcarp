# Palettes

A palette is a list of colour *stops* plus two more colours: the *background*, for empty cells, and
the *stage*, for the area outside the tiling. Each automaton in a scene has its own palette, and the
rule decides how its states and ages are spread along the stops. JETCARP has 30 built-in palettes;
`jetcarp palettes` lists them, and `jetcarp palettes --json` gives the same list as data. For choosing
and editing palettes in the app, see [Palettes and colouring](../guides/palettes.md).

## The built-in palettes

<p class="generated-note">Generated from <code>jetcarp palettes --json</code> (JETCARP 0.4.0). The
bar is each palette's ramp, drawn as JETCARP interpolates it: in a straight line between neighbouring
stops, on the palette's own background.</p>

Candy, Glacier, Desert and Paper have light backgrounds; the rest are dark. Stained Glass and Opal
are closed colour wheels whose last stop repeats the first, so the cyclic automaton and
rock-paper-scissors, which colour states around the whole palette, show no seam. Some palettes, such
as Verdigris, end on a bright stop, so a pattern that has settled stays vivid under age colouring.

| Palette | Colours, young or low to old or high, on its background | Stops | Background | Stage |
| --- | --- | --- | --- | --- |
| Ember | <span style="display:inline-block;padding:4px;border-radius:3px;background:#1d2029"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #ffd27a, #ff8a3c, #d6336c, #6a2c91, #2b2d6e)"></span></span> | <code>#ffd27a</code> <code>#ff8a3c</code> <code>#d6336c</code> <code>#6a2c91</code> <code>#2b2d6e</code> | <code>#1d2029</code> | <code>#101217</code> |
| Tide | <span style="display:inline-block;padding:4px;border-radius:3px;background:#1d2029"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #e8fff7, #58e0c2, #1f9fc9, #2a5bb8, #2b2470)"></span></span> | <code>#e8fff7</code> <code>#58e0c2</code> <code>#1f9fc9</code> <code>#2a5bb8</code> <code>#2b2470</code> | <code>#1d2029</code> | <code>#101217</code> |
| Prism | <span style="display:inline-block;padding:4px;border-radius:3px;background:#1d2029"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #ff4d4d, #ffb02e, #e8e337, #3ddc6d, #2fb6ff, #8a5cff, #ff4dc4)"></span></span> | <code>#ff4d4d</code> <code>#ffb02e</code> <code>#e8e337</code> <code>#3ddc6d</code> <code>#2fb6ff</code> <code>#8a5cff</code> <code>#ff4dc4</code> | <code>#1d2029</code> | <code>#101217</code> |
| Ink | <span style="display:inline-block;padding:4px;border-radius:3px;background:#1d2029"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #7fd0ff, #7fd0ff, #5a9bd0, #3d6a99, #2a4766)"></span></span> | <code>#7fd0ff</code> <code>#7fd0ff</code> <code>#5a9bd0</code> <code>#3d6a99</code> <code>#2a4766</code> | <code>#1d2029</code> | <code>#101217</code> |
| Mono | <span style="display:inline-block;padding:4px;border-radius:3px;background:#1d2029"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #f2f2f2, #c8c8c8, #8c8c8c, #5a5a5a)"></span></span> | <code>#f2f2f2</code> <code>#c8c8c8</code> <code>#8c8c8c</code> <code>#5a5a5a</code> | <code>#1d2029</code> | <code>#101217</code> |
| Aurora | <span style="display:inline-block;padding:4px;border-radius:3px;background:#0a0f24"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #c8ffe8, #4dffb0, #27d3c3, #4f7bff, #9c5cff, #7a3fc4)"></span></span> | <code>#c8ffe8</code> <code>#4dffb0</code> <code>#27d3c3</code> <code>#4f7bff</code> <code>#9c5cff</code> <code>#7a3fc4</code> | <code>#0a0f24</code> | <code>#05081a</code> |
| Synthwave | <span style="display:inline-block;padding:4px;border-radius:3px;background:#1b0b30"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #ff71ce, #c77dff, #3fd0ff, #2fe8b0, #fff59a)"></span></span> | <code>#ff71ce</code> <code>#c77dff</code> <code>#3fd0ff</code> <code>#2fe8b0</code> <code>#fff59a</code> | <code>#1b0b30</code> | <code>#0e0619</code> |
| Moss | <span style="display:inline-block;padding:4px;border-radius:3px;background:#141a11"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #f0f7b8, #b5dc5e, #6faa3e, #468a3c, #3b7046)"></span></span> | <code>#f0f7b8</code> <code>#b5dc5e</code> <code>#6faa3e</code> <code>#468a3c</code> <code>#3b7046</code> | <code>#141a11</code> | <code>#0a0d09</code> |
| Candy (light) | <span style="display:inline-block;padding:4px;border-radius:3px;background:#fff6e8"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #e0407a, #f07fae, #a77fe0, #5fb0e8, #4cbf98)"></span></span> | <code>#e0407a</code> <code>#f07fae</code> <code>#a77fe0</code> <code>#5fb0e8</code> <code>#4cbf98</code> | <code>#fff6e8</code> | <code>#f0e2cc</code> |
| Lava | <span style="display:inline-block;padding:4px;border-radius:3px;background:#140806"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #fff4c0, #ffc23e, #ff6a1a, #d8261c, #9c1424, #6e1022)"></span></span> | <code>#fff4c0</code> <code>#ffc23e</code> <code>#ff6a1a</code> <code>#d8261c</code> <code>#9c1424</code> <code>#6e1022</code> | <code>#140806</code> | <code>#0a0403</code> |
| Glacier (light) | <span style="display:inline-block;padding:4px;border-radius:3px;background:#edf5fa"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #05305e, #15599a, #2f86c4, #5aa7d8, #82bde0)"></span></span> | <code>#05305e</code> <code>#15599a</code> <code>#2f86c4</code> <code>#5aa7d8</code> <code>#82bde0</code> | <code>#edf5fa</code> | <code>#d5e4ee</code> |
| Phosphor | <span style="display:inline-block;padding:4px;border-radius:3px;background:#030d06"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #e0ffe0, #6dff6d, #33d040, #23a032, #1a7a28)"></span></span> | <code>#e0ffe0</code> <code>#6dff6d</code> <code>#33d040</code> <code>#23a032</code> <code>#1a7a28</code> | <code>#030d06</code> | <code>#010603</code> |
| Koi | <span style="display:inline-block;padding:4px;border-radius:3px;background:#0d2a33"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #ffffff, #ffd2a8, #ff8a33, #e0412b, #b3262d)"></span></span> | <code>#ffffff</code> <code>#ffd2a8</code> <code>#ff8a33</code> <code>#e0412b</code> <code>#b3262d</code> | <code>#0d2a33</code> | <code>#071a20</code> |
| Desert (light) | <span style="display:inline-block;padding:4px;border-radius:3px;background:#f4e4c6"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #4a1f0e, #8a3c18, #c0632a, #d58a48, #c79a68)"></span></span> | <code>#4a1f0e</code> <code>#8a3c18</code> <code>#c0632a</code> <code>#d58a48</code> <code>#c79a68</code> | <code>#f4e4c6</code> | <code>#e6d1aa</code> |
| Thermal | <span style="display:inline-block;padding:4px;border-radius:3px;background:#07000f"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #ffffff, #fff05a, #ff9a1a, #e8203c, #a01c9c, #4a22c8)"></span></span> | <code>#ffffff</code> <code>#fff05a</code> <code>#ff9a1a</code> <code>#e8203c</code> <code>#a01c9c</code> <code>#4a22c8</code> | <code>#07000f</code> | <code>#030008</code> |
| Deep Sea | <span style="display:inline-block;padding:4px;border-radius:3px;background:#020c19"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #a8fbff, #3ad8ea, #1a92d2, #1d62b0, #2a4a90, #343c88)"></span></span> | <code>#a8fbff</code> <code>#3ad8ea</code> <code>#1a92d2</code> <code>#1d62b0</code> <code>#2a4a90</code> <code>#343c88</code> | <code>#020c19</code> | <code>#01060d</code> |
| Paper (light) | <span style="display:inline-block;padding:4px;border-radius:3px;background:#f6f2e9"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #111418, #2b3a58, #4b5b7c, #6f7e9a, #8e98ac)"></span></span> | <code>#111418</code> <code>#2b3a58</code> <code>#4b5b7c</code> <code>#6f7e9a</code> <code>#8e98ac</code> | <code>#f6f2e9</code> | <code>#e7e0d1</code> |
| Blueprint | <span style="display:inline-block;padding:4px;border-radius:3px;background:#0f3b70"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #ffffff, #dcefff, #aed3fa, #80b5ec, #5f9be0)"></span></span> | <code>#ffffff</code> <code>#dcefff</code> <code>#aed3fa</code> <code>#80b5ec</code> <code>#5f9be0</code> | <code>#0f3b70</code> | <code>#092a52</code> |
| Neon | <span style="display:inline-block;padding:4px;border-radius:3px;background:#070707"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #fffb7a, #39ff14, #00f0ff, #ff2fd0, #ff5a1f)"></span></span> | <code>#fffb7a</code> <code>#39ff14</code> <code>#00f0ff</code> <code>#ff2fd0</code> <code>#ff5a1f</code> | <code>#070707</code> | <code>#000000</code> |
| Autumn | <span style="display:inline-block;padding:4px;border-radius:3px;background:#1b130d"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #ff5236, #ffa83c, #d9792c, #ae4a2c, #8c6431)"></span></span> | <code>#ff5236</code> <code>#ffa83c</code> <code>#d9792c</code> <code>#ae4a2c</code> <code>#8c6431</code> | <code>#1b130d</code> | <code>#0e0a06</code> |
| Verdigris | <span style="display:inline-block;padding:4px;border-radius:3px;background:#10201f"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #fff1c9, #f2b366, #c7743c, #3fa796, #7fe0c8)"></span></span> | <code>#fff1c9</code> <code>#f2b366</code> <code>#c7743c</code> <code>#3fa796</code> <code>#7fe0c8</code> | <code>#10201f</code> | <code>#081211</code> |
| Marigold | <span style="display:inline-block;padding:4px;border-radius:3px;background:#14213d"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #ffffff, #ffe27a, #ffb81c, #f08a00, #ffc94d)"></span></span> | <code>#ffffff</code> <code>#ffe27a</code> <code>#ffb81c</code> <code>#f08a00</code> <code>#ffc94d</code> | <code>#14213d</code> | <code>#0b1426</code> |
| Coral | <span style="display:inline-block;padding:4px;border-radius:3px;background:#102a43"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #fff0e6, #ff9e80, #ff5e62, #2ec4b6, #9bf6e3)"></span></span> | <code>#fff0e6</code> <code>#ff9e80</code> <code>#ff5e62</code> <code>#2ec4b6</code> <code>#9bf6e3</code> | <code>#102a43</code> | <code>#081a2c</code> |
| Orchid | <span style="display:inline-block;padding:4px;border-radius:3px;background:#170f2e"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #fff0ff, #f4a6ff, #d05cff, #7a6cff, #b8b4ff)"></span></span> | <code>#fff0ff</code> <code>#f4a6ff</code> <code>#d05cff</code> <code>#7a6cff</code> <code>#b8b4ff</code> | <code>#170f2e</code> | <code>#0d081c</code> |
| Citrus | <span style="display:inline-block;padding:4px;border-radius:3px;background:#2a1235"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #ffffd6, #e9ff70, #a7e33b, #ffa62b, #ffd166)"></span></span> | <code>#ffffd6</code> <code>#e9ff70</code> <code>#a7e33b</code> <code>#ffa62b</code> <code>#ffd166</code> | <code>#2a1235</code> | <code>#180a1f</code> |
| Riso | <span style="display:inline-block;padding:4px;border-radius:3px;background:#f7f1e1"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #ff3d7f, #ff6b9d, #3a56d4, #1f3bb3, #14257f)"></span></span> | <code>#ff3d7f</code> <code>#ff6b9d</code> <code>#3a56d4</code> <code>#1f3bb3</code> <code>#14257f</code> | <code>#f7f1e1</code> | <code>#e9e0c9</code> |
| Bone | <span style="display:inline-block;padding:4px;border-radius:3px;background:#15130f"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #ff5a36, #f2c14e, #e8dcc0, #f5eedc, #fffaf0)"></span></span> | <code>#ff5a36</code> <code>#f2c14e</code> <code>#e8dcc0</code> <code>#f5eedc</code> <code>#fffaf0</code> | <code>#15130f</code> | <code>#0b0a08</code> |
| Ultramarine | <span style="display:inline-block;padding:4px;border-radius:3px;background:#05071a"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #ffffff, #9bd1ff, #4aa3ff, #2f6bff, #6f8cff)"></span></span> | <code>#ffffff</code> <code>#9bd1ff</code> <code>#4aa3ff</code> <code>#2f6bff</code> <code>#6f8cff</code> | <code>#05071a</code> | <code>#02030c</code> |
| Stained Glass | <span style="display:inline-block;padding:4px;border-radius:3px;background:#0d0d12"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #e63946, #f4a261, #f1d54b, #2a9d8f, #3a86ff, #9b5de5, #e63946)"></span></span> | <code>#e63946</code> <code>#f4a261</code> <code>#f1d54b</code> <code>#2a9d8f</code> <code>#3a86ff</code> <code>#9b5de5</code> <code>#e63946</code> | <code>#0d0d12</code> | <code>#060608</code> |
| Opal | <span style="display:inline-block;padding:4px;border-radius:3px;background:#1b1d2b"><span style="display:block;width:140px;height:14px;background:linear-gradient(to right, #ffb3c6, #ffd6a5, #fdffb6, #b9fbc0, #a0c4ff, #cdb4ff, #ffb3c6)"></span></span> | <code>#ffb3c6</code> <code>#ffd6a5</code> <code>#fdffb6</code> <code>#b9fbc0</code> <code>#a0c4ff</code> <code>#cdb4ff</code> <code>#ffb3c6</code> | <code>#1b1d2b</code> | <code>#10111a</code> |

## Choosing a palette

- **In a scene**, each automaton's `palette` field names its palette (see the
  [scene file format](scene-format.md#automata-cas)). A JACA automaton has a second one,
  `palette_neg`, for its −1 cells. The defaults are Ember, and for JACA Tide (+1) and Ember (−1).
- **On the command line**, commands that draw pictures, such as `jetcarp wallpaper`,
  `jetcarp hashlife`, `jetcarp sheet` and `jetcarp search pile`, take `--palette NAME`.
- **Your own colours.** Wherever a palette name is accepted you can also write colours: `#4fc3ff`
  is one colour, and `#103050-#4fc3ff` a ramp from the first to the last. Stops are separated by
  `-`, commas or spaces, young or low first, as three- or six-digit hex. A scene can also carry named
  palettes of its own in its `palettes` list, each with its stops and, optionally, its own background
  and stage; the app saves the palettes you make there.

Names are matched exactly, capitals included: the scene's own palettes first, then the built-in
ones, then hex colours. A scene
whose automaton names a palette that does not exist still loads, and that automaton is drawn in Ember.

## How a palette colours cells

Every automaton has a *colour by* setting (`color_by` in a scene):

| Setting | What it shows |
| --- | --- |
| `auto` (the default) | The rule's natural choice, in the table below |
| `age` | How long each cell has been in its state (see [Age colouring](#age-colouring)) |
| `state` | One colour per state: state 0 is the background and states 1 and up are spread evenly from the first stop to the last |
| `row` | Each live cell by its height in the picture, top to bottom, across the whole palette |

Sandpiles are always coloured by height, and continuous fields (Lenia) by value, unless coloured by
row. With `auto`, each family is coloured like this:

| Family | Colours |
| --- | --- |
| Life-like, Generations, Larger than Life, priority tables, Margolus, and the two-state row and layered rules | By age. Row and layered rules write each cell once, so their cells keep the first stop |
| JACA | +1 cells by age from `palette`, −1 cells by age from `palette_neg`; destroyed cells glow pale yellow over the background and fade (`glow`, on by default; drawing only, not part of the state) |
| Generations' dying states, and states above 1 of other multi-state rules coloured by age | By state, between 45% and 95% of the way along the palette |
| Cyclic automaton, rock-paper-scissors | Every state, 0 included, at an even step around the palette |
| Hodgepodge | Every state from 0 at the first stop to the highest at the last |
| Greenberg–Hastings | Resting cells are background, excited cells the first stop, refractory stages from about a third of the way along towards the end |
| Ants and turmites | Colour 0 is background, colours 1 and up spread from the first stop; the ants themselves are drawn on top in a fixed pink-red (`#ff3b6b`) |
| Three-state hexagon rules, rule tables | State 0 is background, states 1 and up spread from the first stop to the last |
| Spectre glider | States 1, 2 and 3 at the start, 80% and 50% of the way along |
| Sandpile | Height 0 is background; heights 1 up to one below the tile's threshold are spread from the first stop; a tile still at or above its threshold shows the last stop |
| Lenia | Empty cells are background; low values take the last stop and a value of 1 the first |
| WireWorld | Fixed colours: electron heads blue, tails red, wire amber |
| Forest fire | Fixed colours: trees green, fire orange |
| Von Neumann's 29 states | A fixed colour for each state (see [Von Neumann's automaton](../automata/von-neumann.md#notation-and-display)) |

### Several automata

Each field is coloured by the first automaton in the scene's list that uses it, and fields are drawn
in order, later ones over earlier ones, with ants on top. Empty cells take the background of the palette of the first
automaton on the bottom field, and the area outside the tiling takes its stage colour; tile outlines
are a shade of that background. See [Several automata at once](../guides/several-automata.md).

## Age colouring

Each live cell carries an *age*: the number of generations it has been in its current state,
starting at 1 when it is born or changes state (the count stops at about 65,000). Under age colouring a
newborn cell takes the first stop, and older cells move along the palette on a logarithmic scale,
reaching the last stop at an age of 400. A still life therefore ends at the last stop, an oscillator
flickers near the start, and the debris of an old soup shows its history: recently active regions at
the young end, settled ones at the old end. Every stop of the built-in palettes is chosen to contrast
with the palette's background, since any of them can end up next to an empty cell.

The ages are part of the world's state hash (see [Reproducibility](../research/reproducibility.md)).
HashLife jumps keep the cells but not their ages: after a jump every live cell has age 1.

JETCARP makes no claim for age colouring: Chris Rowett's LifeViewer and other programs colour cells by
age, and fade cells that die, and did so first (see
[What is new in JETCARP](../research/contributions.md#what-we-do-not-claim)).
