//! `help` (guides with worked examples, and the book's pages) and `reference` (rules, tilings,
//! isohedral types, palettes, seed forms, and the world's commands).

use crate::schema::{self, named, object, words};
use crate::tools::{Call, Res, Tool, ToolOutput, ctx, err};
use jetcarp_api::op::{NoInput, Op, Quiet, Report, input_schema};
use jetcarp_api::ops::reference::{
    ReferenceIsohedral, ReferencePalettes, ReferenceRules, ReferenceSeeds, ReferenceTilings,
    RulesIn, TilingsIn,
};
use serde_json::{Value, json};
use std::fmt::Write as _;

pub(crate) fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "help",
            title: "JETCARP help and worked examples",
            read_only: true,
            destructive: false,
            description: || {
                "Start here if you are new to JETCARP. Explains what JETCARP is and how these tools fit together, with worked examples you can copy. `topic`: \"overview\" (default: concepts, coordinates, a glider in Life, Brian's Brain, a rule on hexagons, a sandpile), \"world\" (drawing, pasting, reseeding, commands, objects), \"library\" (finding, reading and curating patterns, ant runs and Quick Start entries), \"searches\" (long searches as background jobs: start, wait, read the finds, stop, resume), \"scripts\" (every run_script function), \"packs\" (checking, importing and publishing packs), \"setup\" (this installation as its user set it up: profile, folders, packs, GPU, library; read only, since setup is the user's choice) or \"pages\" (the pages of the JETCARP book). `page` returns one book page as Markdown, e.g. {\"page\": \"guides/searching\"} (the resources jetcarp://docs/{page}). `schema` returns the settings of a tool argument that names a jetcarp://schema/ resource, e.g. {\"schema\": \"search_start/hunt\"}.".into()
            },
            schema: || {
                object(
                    named(vec![
                        (
                            "topic",
                            words(
                                &[
                                    "overview", "world", "library", "searches", "scripts", "packs",
                                    "setup", "pages",
                                ],
                                "Which guide (default overview).",
                            ),
                        ),
                        (
                            "page",
                            json!({"type": "string", "description": "A page of the JETCARP book by its path without .md, e.g. \"guides/searching\" (topic pages lists them)."}),
                        ),
                        (
                            "schema",
                            json!({"type": "string", "description": "A JSON Schema: a tool argument's settings (\"search_start/hunt\" is jetcarp://schema/search_start/hunt), \"scene.json\" or \"pack.json\"."}),
                        ),
                    ]),
                    &[],
                )
            },
            run: help,
        },
        Tool {
            name: "reference",
            title: "Rules, tilings, palettes, seeds, commands",
            read_only: true,
            destructive: false,
            description: || {
                "Lists what there is to choose from. `what`: \"rules\" (default: the named rules with notation, family and a line each, and how rule notation works; any name or notation works as a CA's `rule`, e.g. \"Life\", \"B36/S23\", \"Brian's Brain\", \"W30\", \"Ant RL\"), \"tilings\" (tiling ids for world_new and their neighbour counts; with `search`, catalog tilings such as \"k2-005\" or \"3.4.6.4\"), \"isohedral\" (Kaplan's 81 isohedral tile types), \"palettes\" (built-in palettes and their colours), \"seeds\" (the forms a CA's `seed` takes), or \"commands\" (the command objects world_edit's `commands` applies). Examples: {\"what\": \"rules\", \"family\": \"Generations\"}, {\"what\": \"tilings\", \"search\": \"k2-\"}.".into()
            },
            schema: || {
                let rules = schema::props(&input_schema::<RulesIn>());
                let tilings = schema::props(&input_schema::<TilingsIn>());
                let mut p = named(vec![(
                    "what",
                    words(
                        &[
                            "rules",
                            "tilings",
                            "isohedral",
                            "palettes",
                            "seeds",
                            "commands",
                        ],
                        "What to list (default rules).",
                    ),
                )]);
                for (k, v) in rules.into_iter().chain(tilings) {
                    let mut v = v;
                    let d = v["description"].as_str().unwrap_or("").to_string();
                    let which = if k == "family" { "rules" } else { "tilings" };
                    v["description"] = json!(format!("{which}: {d}"));
                    p.insert(k, v);
                }
                schema::retell(
                    &mut p,
                    "family",
                    "rules: only this family (part of its name, any case), e.g. \"Life-like\", \"Generations\", \"Agents\".",
                );
                object(p, &[])
            },
            run: reference,
        },
    ]
}

fn help(c: &mut Call) -> Res {
    let a = c.args;
    if let Some(s) = a.get("schema").and_then(Value::as_str) {
        return crate::resources::schema_text(s).map(ToolOutput::text);
    }
    if let Some(page) = a.get("page").and_then(Value::as_str) {
        let key = page
            .trim()
            .trim_start_matches("jetcarp://docs/")
            .trim_end_matches(".md")
            .trim_matches('/');
        return match crate::docs::page(key) {
            Some(text) => Ok(ToolOutput::text(text)),
            None => Err(format!(
                "no book page '{page}'. help {{\"topic\": \"pages\"}} lists them."
            )),
        };
    }
    Ok(ToolOutput::text(
        match a.get("topic").and_then(Value::as_str).unwrap_or("overview") {
            "overview" => HELP.to_string(),
            "world" => HELP_WORLD.to_string(),
            "library" => HELP_LIBRARY.to_string(),
            "searches" | "jobs" => HELP_SEARCHES.to_string(),
            "scripts" | "script" => jetcarp_script::help_text().to_string(),
            "packs" => HELP_PACKS.to_string(),
            "setup" => setup_text(),
            "pages" => pages_text(),
            t => {
                return Err(format!(
                    "`topic` is overview, world, library, searches, scripts, packs, setup, or pages, not {t}"
                ));
            }
        },
    ))
}

/// `help {"topic": "setup"}`: `setup.status` as text, so an assistant knows what this
/// installation has. It never applies a setup: that is the user's choice.
fn setup_text() -> String {
    let st = jetcarp_api::ops::setup::status(ctx());
    format!(
        "This JETCARP installation as its user set it up (setup.status). Setup itself is the user's to run, in the app (Help, Setup...) or with `jetcarp setup`; these tools only read it.\n\n{}",
        st.text()
    )
}

/// The book's pages, by section, with their titles.
pub(crate) fn pages_text() -> String {
    let mut s = String::from(
        "The JETCARP book, page by page. Read one with help {\"page\": \"<path>\"} (or the resource jetcarp://docs/<path>).\n",
    );
    let mut section = "";
    for p in crate::docs::pages() {
        let sec = p.path.split_once('/').map(|x| x.0).unwrap_or("");
        if sec != section {
            section = sec;
            let _ = writeln!(s, "\n{}", if sec.is_empty() { "(top level)" } else { sec });
        }
        let _ = writeln!(s, "- {}: {}", p.path, p.title);
    }
    s
}

fn reference(c: &mut Call) -> Res {
    let a = c.args;
    let text = |k: &str| a.get(k).and_then(Value::as_str).map(String::from);
    let what = text("what").unwrap_or_else(|| "rules".into());
    let given: Vec<&str> = ["family", "search", "limit"]
        .into_iter()
        .filter(|k| a.get(*k).is_some_and(|v| !v.is_null()))
        .collect();
    let allowed: &[&str] = match what.as_str() {
        "rules" => &["family"],
        "tilings" => &["search", "limit"],
        _ => &[],
    };
    if let Some(k) = given.iter().find(|k| !allowed.contains(k)) {
        return Err(format!("`{k}` does not apply to what {what}"));
    }
    Ok(ToolOutput::text(match what.as_str() {
        "rules" => rules_text(text("family"))?,
        "tilings" => tilings_text(
            text("search"),
            a.get("limit")
                .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok())),
        )?,
        "isohedral" => ReferenceIsohedral::run(ctx(), NoInput {}, &Quiet)
            .map_err(err)?
            .text(),
        "palettes" => ReferencePalettes::run(ctx(), NoInput {}, &Quiet)
            .map_err(err)?
            .text(),
        "seeds" => ReferenceSeeds::run(ctx(), NoInput {}, &Quiet)
            .map_err(err)?
            .text(),
        "commands" => COMMANDS.to_string(),
        w => {
            return Err(format!(
                "`what` is rules, tilings, isohedral, palettes, seeds, or commands, not {w}"
            ));
        }
    }))
}

/// The rules, as `reference {"what": "rules"}` and the resource jetcarp://reference/rules show
/// them.
pub(crate) fn rules_text(family: Option<String>) -> Result<String, String> {
    let out = ReferenceRules::run(ctx(), RulesIn { family }, &Quiet).map_err(err)?;
    let mut s = String::from("Named rules (use the name or the notation as `rule`):\n");
    if out.rules.len() <= 80 {
        s.push('\n');
        for e in &out.rules {
            let _ = writeln!(
                s,
                "- {} | {} | {} | {}",
                e.name, e.notation, e.family, e.blurb
            );
        }
    } else {
        // Long lists: names and notation only, grouped by family.
        s.push_str("(Too many to describe each; pass `family` for one-line descriptions.)\n");
        for f in &out.families {
            let names: Vec<String> = out
                .rules
                .iter()
                .filter(|e| e.family == *f)
                .map(|e| format!("{} ({})", e.name, e.notation))
                .collect();
            if !names.is_empty() {
                let _ = writeln!(s, "\n{f}: {}", names.join("; "));
            }
        }
    }
    s.push_str(NOTATION);
    Ok(s)
}

/// The tilings, as `reference {"what": "tilings"}` and the resource jetcarp://reference/tilings
/// show them.
pub(crate) fn tilings_text(search: Option<String>, limit: Option<u64>) -> Result<String, String> {
    let out = ReferenceTilings::run(
        ctx(),
        TilingsIn {
            search: search.clone(),
            limit: limit.map(|l| l as usize),
        },
        &Quiet,
    )
    .map_err(err)?;
    if search.is_some() {
        return Ok(out.matches_text());
    }
    let mut s = String::from(
        "Tilings (id | name | group | neighbours per cell: edge-sharing / touching | note).\n\
         Size: width x height unit cells, or `cells` (about that many); aperiodic tilings use `level`.\n\
         Life-like rules count touching neighbours by default (Moore on squares: 8); add V to a B/S rule \
         (B3/S23V) or neighborhood \"edge\" to count edge neighbours only.\n\n",
    );
    for f in &out.families {
        let deg = match &f.error {
            None => format!("{:?} / {:?}", f.edge, f.touch),
            Some(_) => "?".into(),
        };
        let _ = writeln!(
            s,
            "- {} | {} | {} | {} | {}",
            f.id, f.name, f.group, deg, f.note
        );
    }
    let _ = writeln!(
        s,
        "\nAlso usable as `tiling` (list them with reference {{\"what\": \"tilings\", \"search\": \"...\"}}): {} k-uniform \
         tilings k1-001 .. k6-673 (ids like \"k2-005\"; search \"k2-\" for all 2-uniform ones), {} other \
         catalog tilings, and the {} isohedral tile types ih01 .. ih93.",
        out.catalog.k_uniform, out.catalog.other, out.catalog.isohedral
    );
    Ok(s)
}

// ---------------------------------------------------------------------------------------------
// The texts

const NOTATION: &str = "
Rule notation (any of these works as `rule`):
- Life-like B/S: B<birth counts>/S<survival counts>, e.g. B3/S23 (Life), B36/S23. Counts >= 10 use commas: B3,10/S2,3. Add V for edge-only neighbours: B3/S23V.
- Generations: B<..>/S<..>/C<states>, e.g. B2/S/C3 (Brian's Brain). State 1 is firing; higher states are dying.
- Elementary (square grid only, one row per generation from the top): W<0-255>, e.g. W30, W110.
- Hexagonal elementary (hex grid only): HexW <0-15>, HexW3 <0-19682>.
- Three-parent hexagonal elementary (flat-top hexagon grid `hex_flat` only; parents NW, N, NE): HexT <0-255> (index NW*4+N*2+NE), HexT3 <0-7625597484986>, HexT3 sum <0-2186> (by parent sum), HexT3 sym <n>, HexT sum <0-15>.
- Margolus block rules (square grid; 2x2 blocks, partition shifts every generation): Margolus <16 block outputs, upper left 1, upper right 2, lower left 4, lower right 8>, or Margolus BBM / Critters / Tron / SingleRotation / HPP.
- Lenia (continuous values 0-1): Lenia R=13,T=10,b=1,m=0.15,s=0.015 (Orbium; seed \"lenia:orbium\", or blobs \"random:0.5\", \"gauss:3\").
- Turmites/ants: Ant RL (Langton's ant), Turmite LLRR (letters L, R, N, U per colour).
- Cyclic <states>/<threshold>, GH <states>/<threshold> (Greenberg-Hastings), RPS <species>/<threshold>, ForestFire <p>/<f>, Hodgepodge <n>/<k1>/<k2>/<g>.
- JACA (Jimmie's Annihilating CA; cells -1, 0, +1): JACA B3/S23 (one B/S rule for both signs), JACA +B3/S23 -B36/S23 (positive, then negative), with optional blast <0-64>: JACA B3/S23 blast 2. Each sign counts only its own neighbours, so one sign alone runs the plain B/S rule; live cells touching the opposite sign are destroyed next generation, with every cell within `blast` neighbourhood steps of them. When drawing, state 1 is +1 and state 2 is -1. Seeds \"halves:0.3\" (+1 left, -1 right) and \"signed:0.3:0.5\" (share of -1). CA options palette (for +1), palette_neg (for -1), glow (false hides the annihilation flash); palettes may be colours, \"#4fc3ff\" or \"#103050-#4fc3ff\".
- Others: WireWorld, Sandpile, SpectreGlider, Table <name>.
";

/// `reference {"what": "commands"}`: the command objects of `world_edit {"commands": [...]}`.
pub const COMMANDS: &str = r#"Commands (world_edit {"commands": [...]}): the same commands the app's buttons issue, applied in order and recorded in the scene's command log. A failing command stops the list and says which. Each is an object with "op":

- {"op": "add_ca", "ca": {"rule": "B3/S23", "seed": "empty"}}: add a CA (returns its id). The CA object takes rule, seed, name, neighborhood ("touch" or "edge"), palette, palette_neg (JACA), steps_per_gen, enabled.
- {"op": "remove_ca", "id": 2}
- {"op": "update_ca", "id": 1, "change": {"rule": "B36/S23", "enabled": false, "palette": "Tide", "steps_per_gen": 10, "neighborhood": "edge"}}
- {"op": "seed", "id": 1, "seed": "random:0.2"}: add a seed to a CA's field (reference {"what": "seeds"} lists the forms).
- {"op": "clear", "id": 1}: empty a CA's field.
- {"op": "set_cells", "id": 1, "cells": [[index, state], ...]}: cells by index (world_edit's `cells` draws by x and y).
- {"op": "step", "gens": 10}
- {"op": "reset"}: clear and reseed every CA, back to generation 0.
- {"op": "set_rng_state", "state": 42}: the random state the next seed uses.
- {"op": "set_tiling", "tiling": {"family": "hex", "size": {"rect": {"w": 60, "h": 60}}, "boundary": "wrap"}}: rebuild the grid (the CAs stay, their fields are emptied).
- {"op": "sandpile", "id": 1, "action": "stabilize"}: also "identity", {"add_everywhere": {"grains": 3}}, {"add_random": {"count": 1000}}.
- {"op": "set_palette", "palette": {"name": "Mine", "stops": [[0, 0, 0], [255, 200, 0]], "background": [250, 245, 235]}}: a palette of your own (optional background and stage colours).
- {"op": "reseed", "mode": {"kind": "rain"}} and {"op": "set_reseed", "policies": [...]}: as world_edit's `reseed`.

Palettes: Ember, Tide, Prism, Ink, Mono, Aurora, Synthwave, Moss, Candy, Lava, Glacier, Phosphor, Koi, Desert, Thermal, Deep Sea, Paper, Blueprint, Neon, Autumn (Candy, Glacier, Desert and Paper have light backgrounds; reference {"what": "palettes"} shows their colours).
"#;

pub const HELP: &str = r#"JETCARP: a cellular-automaton (CA) research tool.

Concepts
- A world = a tiling (a grid of cells: squares, hexagons, triangles, Archimedean and k-uniform tilings, or the aperiodic Spectre, hat and Penrose tilings) + one or more CAs (rules) running on it.
- Each CA works on a field (one value per cell). Binary rules (Life-like) share one field by default; multi-state rules (Brian's Brain, WireWorld) and sandpiles get their own.
- Every change is a command recorded in the world's scene, so a run can be saved (world_status {"save_path": "glider.json"}) and replayed exactly. The state hash identifies a state.
- Coordinates on grid tilings: x = column, y = row, (0, 0) at the top-left; world_status shows width and height. Aperiodic tilings use cell indices.
- Life-like rules count touching neighbours (8 on squares, 6 on hexagons, 12 on triangles).
- The library holds what searches found: verified still lifes, oscillators, spaceships and guns, ant runs, and the entries of the app's Quick Start menu.

The tools
- help, reference: these guides; rules, tilings, palettes, seed forms, commands.
- world_new, world_status, world_read, world_edit, world_step, world_render: one world, built, looked at, changed, run and pictured.
- run_script: a Rhai script, for many runs at once (sweeps over rules or tilings, folders of pictures).
- library_query, library_curate: list and read records; rename, tag, rate, hide, remove, add to the Quick Start.
- search_start, job_status, job_stop, search_runs: long searches in the background, and an explore search's runs.
- media_render: animations and posters of library records. pack: packs and the website export. files_list: what runs wrote.

Example 1: draw a glider in Life and find it 8 generations later
1. world_new {"tiling": "square", "width": 40, "height": 40, "boundary": "wrap", "cas": [{"rule": "Life", "seed": "empty"}]}
2. world_edit {"cells": {"pattern": ".O.\n..O\nOOO", "x": 5, "y": 5}}
3. world_step {"generations": 8}
4. world_read {}   -> the glider has moved 2 cells right and 2 down (bounding box x 7..9, y 7..9). A glider moves 1 cell diagonally every 4 generations.
5. world_read {"format": "objects"}   -> glider (c/4) x 1, with where it is.

Example 2: Brian's Brain, then a picture; and a rule that lives on hexagons
1. world_new {"tiling": "square", "width": 120, "height": 80, "boundary": "wrap", "cas": ["Brian's Brain"]}   (random start)
2. world_step {"generations": 200}
3. world_render {}
4. world_new {"tiling": "hex", "width": 60, "height": 60, "cas": ["B24/S35"]}, world_step {"generations": 300}, world_render {}   (Brian's Brain dies out on hexagons, where each cell has only 6 neighbours: rules behave differently on each tiling)

Example 3: a sandpile
1. world_new {"tiling": "square", "width": 41, "height": 41, "boundary": "dead", "cas": [{"rule": "Sandpile", "seed": "pile:20000"}]}
2. world_step {"generations": 5000, "stop_when_settled": true}
3. world_render {}

Example 4: open something ready-made
- world_new {"quickstart": "life"}   (an entry of the app's Quick Start; library_query {"kind": "quickstart"} lists them)
- world_new {"pattern": "glider"}   (a library pattern alone on a grid sized for it)

Example 5: a short soup hunt in the background, then its finds
1. search_start {"hunt": {"rules": ["B3/S23"], "soups": 2000}, "wait_s": 60}
2. job_status {"job": <job>, "finds": 10}
3. library_query {"class": "spaceship", "limit": 5}

Tips
- world_read with no arguments shows the live cells' bounding box and a text picture of it; {"cell": {"x": 3, "y": 4}} shows one cell's neighbours (useful on hexagons and triangles).
- world_step with stop_when_settled reports when a pattern dies out, freezes, or repeats (period found by exact comparison). world_status reports `done` when nothing can change any more.
- Errors explain what to change; fix the arguments and call again.
- Files: tools read and write only in JETCARP's output folder, the library's folder, and folders the user allowed; relative paths start in the output folder. files_list shows what runs wrote.
- More: help {"topic": "world"}, {"topic": "library"}, {"topic": "searches"}, {"topic": "scripts"}, {"topic": "packs"}, {"topic": "setup"} (this installation: its folders, packs and GPU); the book: help {"topic": "pages"}.
"#;

/// `help {"topic": "world"}`.
pub const HELP_WORLD: &str = r#"One world: building, looking, changing, running.

Build or open (world_new replaces the current world)
- {"tiling": "hex", "width": 60, "height": 60, "cas": ["B24/S35"]}; size by `cells` ({"cells": 4000}) on any periodic tiling, `level` on aperiodic ones ({"tiling": "spectre", "level": 4}).
- A CA is a rule string, or an object {"rule": "Life", "seed": "random:0.2", "neighborhood": "edge", "palette": "Tide", "steps_per_gen": 10}. reference {"what": "seeds"} lists the seeds; {"seed": "empty"} starts blank.
- {"quickstart": "<handle>"}, {"pattern": "<id or name>"}, {"scene": {...}}, {"path": "scene.json"} open one.

Look
- world_status: tiling and size, generation, hashes, status (running, empty, frozen, repeating with its period), populations, the CAs with their ids. {"scene": true} returns the scene JSON; {"save_path": "x.json"} writes it.
- world_read: a text picture ('.' empty, 'O' live, digits for states): {} the live cells' box, or a region {"x": 0, "y": 0, "width": 40, "height": 20}; {"format": "list"} gives [x, y, state]; {"format": "objects"} the still lifes, oscillators and spaceships found (with {"catalog": true} they are verified and stored in the library); {"cell": {"x": 3, "y": 4}} one cell and its neighbours in clockwise order.
- world_render: a PNG of the whole world, returned inline ({"save_path": "a.png"} also writes it); {"wallpaper": {"size": "4K"}} writes desktop-sized pictures instead.

Change (world_edit; every change is a logged command)
- cells: {"cells": {"pattern": ".O.\n..O\nOOO", "x": 10, "y": 10}}, {"cells": {"rle": "bo$2bo$3o!"}}, {"cells": {"cells": [[3, 4], [5, 6, 2]]}}, {"cells": {"indices": [[120, 1]]}} (aperiodic tilings), {"cells": {"clear_first": true}}, {"cells": {"pattern": "OO", "erase": true}}. On sandpiles values are grains added.
- paste: {"paste": {"id": "glider", "x": 10, "y": 10, "orientation": 1}} (a library pattern; orientation turns it, 0-7 on squares, 0-11 on hexagons; library_query {"id": ...} shows which way a spaceship then moves).
- reseed: {"reseed": {"mode": "blobs"}} now; {"reseed": {"policies": [{"mode": "rain", "when": {"kind": "settled"}, "cooldown": 300}]}} automatically when the world settles; {"reseed": {}} reports the policies and recent activity. Modes: "random", "rain", "blobs", or objects such as {"kind": "rain", "drops": 24, "min_size": 2, "max_size": 10, "density": 0.5}.
- commands: {"commands": [{"op": "add_ca", "ca": {"rule": "Ant RL", "seed": "ants:3"}}, {"op": "step", "gens": 60}]}; reference {"what": "commands"} lists them.
- `ca` picks the CA (id or name; default the first) for cells, paste and reseed: {"ca": 2, "cells": {...}}.

Run
- world_step {"generations": 100}; {"stop_when_settled": true} stops once the world is empty, frozen or periodic (up to 5000 generations unless given).
- Limits per call: worlds up to 4 million cells, 1,000,000 generations, and 2 billion cell updates (cells x generations); world_step says how far to go when a call is too big.

Two CAs on one grid
1. world_new {"tiling": "square", "width": 120, "height": 80, "cas": [{"rule": "W30", "seed": "top_center"}, {"rule": "Ant RL", "seed": "ants:3", "steps_per_gen": 20}]}
2. world_edit {"commands": [{"op": "step", "gens": 60}, {"op": "update_ca", "id": 1, "change": {"enabled": false}}, {"op": "step", "gens": 500}]}
3. world_status {}
"#;

/// `help {"topic": "library"}`.
pub const HELP_LIBRARY: &str = r#"The library: one database of what JETCARP has found, shared by the app, the command line, scripts and these tools.

Kinds of record (library_query "kind")
- pattern (default): verified still lifes, oscillators, spaceships, guns, puffers and movers. Rule-free: `rules` lists every rule it was verified in and `range` the rules it runs identically in. Ids are hex (a unique prefix works), or a name such as "glider".
- ant: one ant or turmite run (a turn string such as RL or LLRR, or a table) on a tiling: outcome highway (proved exactly: period, displacement per period, onset step), periodic, symmetric (grows mirror-symmetrically), bounded, or chaotic. Ids start ant-.
- quickstart: the entries of the app's Quick Start menu, by handle: built-in examples (such as life), entries from packs, an explore search's atlas (atlas:...), and your own (find:...). `source` picks one source; entries are shown or hidden.
- summary: how many of each the library holds.

Example 1: the best spaceships on hexagons, then one of them in a world
1. library_query {"class": "spaceship", "tiling": "hex", "limit": 5}
2. library_query {"id": "<id from 1>", "phases": 2}   (text drawings of its phases, rules, range, sightings)
3. world_new {"pattern": "<id>"}, then world_step and world_read; or paste it into a world of your own: world_edit {"paste": {"id": "<id>"}}

Example 2: Langton's ant highway and other highways on hexagons
1. search_start {"ants": {"rules": ["RL"]}, "wait_s": 300}
2. library_query {"kind": "ant", "outcome": "highway", "sort": "onset"}   (RL: period 104, d (-2, 2), onset 9977)
3. media_render {"ant": "<id>", "poster": true}

Example 3: curate (library_curate)
- {"id": "<pattern id>", "changes": {"rating": 4, "append_notes": "LLM: clean p3 oscillator"}}   (mark text you wrote as yours)
- {"id": "ant-...", "changes": {"name": "slow hex highway", "rating": 5}}
- {"id": "find:<id>", "hidden": true}   (hides a Quick Start entry from the menu, packs and the website; false shows it again)
- {"quickstart": {"pattern": "<id>", "title": "Trihex glider"}}   (adds it to the Quick Start under "My finds"; also {"ant": ...}, {"search_run": <run>, "job": <job>}, {"folder": ..., "run": ...} for a sweep run, {"scene": "file.json"})
- {"id": "<id>", "remove": true}   (deletes a record for good)
- {"compounds": {}}   (lists stored patterns that are really several objects; {"compounds": {"apply": true}} splits them)
"#;

/// `help {"topic": "searches"}`.
pub const HELP_SEARCHES: &str = r#"Long searches run in the background as library jobs: search_start returns the job id at once (or, with wait_s, when the job ends or the wait runs out).

Strategies (search_start takes one, as a key holding its settings)
- hunt: small random soups on an unbounded plane (B/S and Generations rules, any periodic tiling); every object is classed, new patterns are verified and stored. Thousands of soups a second for Life.
- search: soups on a bounded grid of any tiling, aperiodic ones too.
- ants: ant and turmite rules (highways proved exactly).
- census: rules classed by what their soups do (die, freeze, oscillate, grow, ...).
- mover: one pattern on an aperiodic tiling followed far on a sliding window.
- pile: one large sandpile, built by doubling (pictures in its folder under the output folder).
- collide: one pattern shot into another at every offset and phase; new products are stored.
- evolve: the evolutionary glider search (rules that make seeds travel).
- explore: the broad search over tilings, rule families, rules and seeds, keeping the best of each kind of behaviour (not deterministic; it runs in a process of its own where it can).

Watch, read, stop
- job_status {"job": 3}: progress now; {"job": 3, "wait_s": 300} waits up to 5 minutes for it to end; {"job": 3, "finds": 10} adds its newest and rarest finds; {"job": 3, "report": true} a report of what it found; job_status {} lists jobs ({"kind": "ants"} only ant searches).
- job_stop {"job": 3}: stops at the next checkpoint; search_start {"resume": 3} carries it on.
- Results: library_query (patterns, {"kind": "ant"}), files_list for folders (piles, evolve runs), and for explore searches search_runs: their runs best first, contact sheets, a run's scene, trying a configuration of your own, and recording a verdict.
- Budgets: {"budget": {"minutes": 10}} or {"budget": {"items": 5000}} end this run at a checkpoint; the job stays resumable.

Example: a hunt, then its spaceships
1. search_start {"hunt": {"rules": ["B3/S23"], "soups": 20000}, "wait_s": 120}   -> job 1, done
2. job_status {"job": 1, "finds": 10}
3. library_query {"class": "spaceship", "rule": "B3/S23"}

Example: an explore search for two minutes, then its best runs
1. search_start {"explore": {"tilings": "square,hex", "families": "life_like,cyclic", "screen_cells": 4000}, "budget": {"minutes": 2}}
2. job_status {"job": <job>, "wait_s": 150}
3. search_runs {"job": <job>, "limit": 10}; search_runs {"job": <job>, "sheet": {"ids": [<run ids>]}}
4. search_runs {"job": <job>, "scene": {"run": <id>}} gives its scene: open it with world_new {"scene": ...}.
"#;

/// `help {"topic": "packs"}`.
pub const HELP_PACKS: &str = r#"Packs: signed folders of library records (Parquet tables and JSON) that anyone can read, check and import.

- pack {"info": {"path": "<folder>"}}: what a store or export holds (title, publisher and signature, tables, searches), without reading every file.
- pack {"check": {"path": "<folder>"}}: check every hash, id and signature, importing nothing.
- pack {"import": {"path": "<folder>"}}: import it into the library (records merged by id, a sample verified again).
- pack {"publish": {"feed": "my-finds", "to": "packs/my-finds", "ids": ["<id>", ...]}}: publish the library, or a selection, as a new signed version of a feed. It signs with the publisher key in JETCARP's settings, which only the user makes (`jetcarp pack key` at a command line).
- pack {"export": {"to": "my-finds-1", "from": "packs/my-finds"}}: one version with readable file names, for a single download.
- pack {"site": {"to": "site"}}: the website export (the public part of the library as JSON and media, and a viewer page).

Paths are inside JETCARP's output folder (relative paths start there), the library's folder, or the folder of packs that came with JETCARP. Recording a rater's consent to publish their ratings is for people only, at a command line (`jetcarp pack rater`).
"#;
