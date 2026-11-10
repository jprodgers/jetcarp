// Stage 2 on every grid: hunt the rules that stage 1 found worth hunting (Phase 4 catalog,
// deliverable 9). For each space (tiling and neighbourhood) of the stage 1 jobs labelled
// `<from> · stage 1 · ...`, the rules hunted are
//
//   1. the best distinct rules whose settings scored at least `min_score` (default: the score
//      model's candidate threshold; `top` of them; on
//      squares and hexagons the best `top_main`, whatever their score),
//   2. every rule of an approved setting on that grid (the user's picks),
//   3. the rule-space neighbours (one birth or survival count changed) of the best `ship_rules`
//      rules whose soups showed spaceships, since rules with ships cluster,
//
// at most `max_rules` per space (`max_other` outside squares and hexagons). Each space is one
// fast-hunter job labelled `<label> · stage 2 · <space>` with `soups` soups per rule
// (`soups_main` on squares and hexagons). On the aperiodic patches (no lattice, so no fast hunter) the census rows of stage
// 1 pick the rules whose soups mostly oscillate or stay active without exploding, and each patch
// gets one soup search (`lib.search`) labelled the same way; it finds still lifes and
// oscillators (movers need `jetcarp movers` or `jetcarp evolve`).
//
// Rerun with the same label: finished hunts are skipped, interrupted ones resume from their
// checkpoints, searches already stored are not run again. Jobs show up in the Hunts window.
//
// Run:   jetcarp script scripts/stage2-hunt.rhai --set mode=quick
//        jetcarp script scripts/stage2-hunt.rhai --set mode=standard --set soups=100000
//        jetcarp script scripts/stage2-hunt.rhai --set from="pipeline 3" --set label="pipeline 3"
// `jetcarp pipeline` runs it as its fourth step.
//
// Parameters (all optional): mode (test, quick, standard, full; default quick), label (default
// "catalog"), from (the stage 1 label, default the same), threads, and overrides: top,
// top_main, min_score, ship_rules, max_rules, max_other, soups, soups_main, gens, aperiodic_top,
// aperiodic_soups, aperiodic_gens (1000), aperiodic_density (0.12), groups (as in
// stage1-all.rhai: only these groups' spaces), and the hunts'
// storage policy: store_max_cells (64), store_max_volume (period x cells, 4000),
// store_min_seen (2): larger still lifes and oscillators are kept as unstored rows (no full
// record) until a hunt sees them that often; ships, guns, and puffers are always stored.

let mode = params.mode ?? "quick";
let label = params.label ?? "catalog";
let from = params.from ?? label;
let threads = params.threads ?? 0;

let budgets = #{
    test: #{ top: 1, top_main: 2, min_score: 30.0, ship_rules: 1, max_rules: 4, max_other: 4,
             soups: 100, soups_main: 200, gens: 2000, aperiodic_top: 1, aperiodic_soups: 2,
             aperiodic_density: 1.0 },
    quick: #{ top: 2, top_main: 4, min_score: (), ship_rules: 1, max_rules: 10, max_other: 6,
              soups: 2000, soups_main: 5000, gens: 20000, aperiodic_top: 2,
              aperiodic_soups: 8, aperiodic_density: 0.12 },
    standard: #{ top: 3, top_main: 12, min_score: (), ship_rules: 2, max_rules: 40,
                 max_other: 12, soups: 20000, soups_main: 50000, gens: 20000,
                 aperiodic_top: 4, aperiodic_soups: 32, aperiodic_density: 0.12 },
    full: #{ top: 16, top_main: 48, min_score: (), ship_rules: 8, max_rules: 200,
             max_other: 60, soups: 200000, soups_main: 500000, gens: 20000,
             aperiodic_top: 16, aperiodic_soups: 256, aperiodic_density: 0.12 },
};
if !(mode in budgets) {
    throw `mode is test, quick, standard, or full, not ${mode}`;
}
let b = budgets[mode];
for k in b.keys() {
    if params[k] != () { b[k] = params[k]; }
}
let lib = library();
// The threshold defaults to the active score model's candidate threshold (a refit may move
// the whole scale; `jetcarp pipeline` keeps the thresholds at the same ranks).
if b.min_score == () { b.min_score = lib.score_model().candidate; }
let store = #{ max_cells: params.store_max_cells ?? 64,
               max_volume: params.store_max_volume ?? 4000,
               min_seen: params.store_min_seen ?? 2 };
let groups = if params.groups != () { params.groups.to_string().split(",") } else { () };

print(`stage 2 hunts: mode ${mode}, from "${from} · stage 1", label "${label}", catalog ${lib.path()}`);
let t_all = elapsed();

// Spaces in the order the stage 1 jobs ran (PLAN's order), settings best first.
let spaces = [];       // [#{ key, name, tiling, edge, rules: [], ships: [], main }]
let index = #{};       // key -> position in spaces
fn space_key(s) {
    let edge = s.rule.ends_with("V");
    to_json(s.setting.tiling) + if edge { "|edge" } else { "|touch" }
}
let stage1_jobs = lib.jobs(#{ kind: "stage1", prefix: `${from} · stage 1 · ` });
for j in stage1_jobs {
    let g = j.label.split(" · ");
    let group = g[g.len() - 1];
    if groups != () && !groups.contains(group) { continue; }
    let main = group == "squares" || group == "hexagons";
    let want = if main { b.top_main } else { b.top };
    // Settings come best first, so each space keeps only the rules it can use: the first
    // `want` distinct rules and the first `ship_rules` with ships (a map per space for
    // membership, so a whole search scans in one pass).
    let tiling_key = #{};   // tiling map as text, per tiling name
    for s in lib.settings(#{ job: j.id, order: "score", limit: 1000000, brief: true }) {
        let edge = s.rule.ends_with("V");
        if !(s.tiling in tiling_key) { tiling_key[s.tiling] = to_json(s.setting.tiling); }
        let k = tiling_key[s.tiling] + if edge { "|edge" } else { "|touch" };
        if !(k in index) {
            index[k] = spaces.len();
            spaces.push(#{ key: k, name: `${s.tiling} (${if edge { "edge" } else { "touch" }})`,
                           tiling: s.setting.tiling, rules: [], ships: [], seen: #{},
                           main: main });
        }
        let i = index[k];
        // Squares and hexagons always hunt their best rules; other spaces only those at or
        // above the threshold.
        let keep = (s.score >= b.min_score || main) && spaces[i].rules.len() < want;
        let ship = s.descriptors.ship_kinds > 0 && spaces[i].ships.len() < b.ship_rules;
        if (keep || ship) && !(s.rule in spaces[i].seen) {
            spaces[i].seen[s.rule] = true;
            if keep { spaces[i].rules.push(s.rule); }
            if ship { spaces[i].ships.push(s.rule); }
        } else if ship && !spaces[i].ships.contains(s.rule) {
            spaces[i].ships.push(s.rule);
        }
    }
}
// The user's approved settings, on the spaces this run covers.
let approved = lib.settings(#{ status: ["approved"], order: "score", limit: 100000,
                              brief: true });

let report = [];
for sp in spaces {
    let top = if sp.main { b.top_main } else { b.top };
    let pick = [];
    for r in sp.rules {
        if pick.len() >= top { break; }
        pick.push(r);
    }
    for s in approved {
        if space_key(s) == sp.key && !pick.contains(s.rule) { pick.push(s.rule); }
    }
    let edge = sp.key.ends_with("|edge");
    let nb = if edge { "edge" } else { "touch" };
    let n_ship = 0;
    for r in sp.ships {
        if n_ship >= b.ship_rules { break; }
        n_ship += 1;
        if !pick.contains(r) { pick.push(r); }
        for x in rule_neighbours(r, #{ tiling: sp.tiling, nb: nb }) {
            if !pick.contains(x) { pick.push(x); }
        }
    }
    pick.truncate(if sp.main { b.max_rules } else { b.max_other });
    if pick.is_empty() { continue; }
    let soups = if sp.main { b.soups_main } else { b.soups };
    let job_label = `${label} · stage 2 · ${sp.name}`;
    let t0 = elapsed();
    let p = lib.hunt(#{ label: job_label, rules: pick, tilings: [sp.tiling], soups: soups,
                        gens: b.gens, threads: threads, store: store });
    let ships = p.by_class.spaceship ?? 0;
    print(`${sp.name}: hunt ${p.job} ${p.status}, ${pick.len()} rules x ${soups} soups, ${p.distinct} distinct, ${p.new_patterns} new, ${ships} ships (${(elapsed() - t0).round()} s)`);
    report.push(#{ space: sp.name, job: p.job, status: p.status, rules: pick,
                   seconds: elapsed() - t0 });
    if stopped() { print("stopped: run again with the same label to resume"); break; }
}

// Aperiodic patches: rules from the census rows, then one soup search per patch.
if !stopped() && (groups == () || groups.contains("aperiodic")) {
    for j in lib.jobs(#{ kind: "census", prefix: `${from} · stage 1 · aperiodic` }) {
        if j.status != "done" { continue; }
        let t = j.spec.tiling.family;
        let ranked = [];
        for r in lib.census_rows(j.id) {
            if r.error != () { continue; }
            let lively = r.oscillating + r.active;
            // Dense fields (soups ending more than `aperiodic_density` full, 12% by default)
            // are not catalog material, and the World's object finder is slow on them.
            if r.exploded > 0.0 || r.died >= 0.5 || lively < 0.5
                || r.median_final_density > b.aperiodic_density { continue; }
            let calm = if r.mean_activity == () { 1.0 } else { if r.mean_activity > 1.0 { 0.0 } else { 1.0 - r.mean_activity } };
            let long = if r.median_period != () && r.median_period >= 3.0 { 0.5 } else { 0.0 };
            ranked.push(#{ rule: r.notation, key: 2.0 * r.oscillating + r.active * calm + long });
        }
        ranked.sort(|a, b| if a.key > b.key { -1 } else if a.key < b.key { 1 } else { 0 });
        let rules = [];
        for r in ranked {
            if rules.len() >= b.aperiodic_top { break; }
            rules.push(r.rule);
        }
        if rules.is_empty() { continue; }
        let job_label = `${label} · stage 2 · ${t}`;
        let t0 = elapsed();
        let s = lib.search(#{ label: job_label, rules: rules, tilings: [t],
                              soups: b.aperiodic_soups, gens: params.aperiodic_gens ?? 1000 });
        print(`${t}: soup search ${s.job}${if s.skipped ?? false { " (already done)" } else { "" }}, ${rules.len()} rules x ${b.aperiodic_soups} soups, ${s.new_patterns} new patterns (${(elapsed() - t0).round()} s)`);
        report.push(#{ space: t, job: s.job, status: "done", rules: rules,
                       seconds: elapsed() - t0 });
    }
}
print(`stage 2 hunts: ${(elapsed() - t_all).round()} s`);
report
