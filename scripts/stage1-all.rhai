// Stage 1 on every grid: the settings search (Phase 4 catalog, deliverable 9) over a list of
// grid groups in PLAN's order, each with a budget suited to its size.
//
//   squares      Moore (every rule, 131,072) and von Neumann (every rule, 512)
//   hexagons     every rule (8,192)
//   triangles    touching neighbours (2^25 rules: a seeded sample) and edge neighbours (128)
//   archimedean  the other 8 Archimedean tilings (4.8.8, 3.6.3.6, 3.3.3.4.4, 3.3.4.3.4,
//                3.3.3.3.6, 3.4.6.4, 3.12.12, 4.6.12), touching neighbours, sampled
//   kuniform     k-uniform tilings of the catalog (k <= kmax), a small sample each, on 32x32
//   isohedral    a fixed, evenly spaced subset of Kaplan's isohedral types, sampled
//   aperiodic    Spectre, hat, and both Penrose patches: stage 1's board needs a lattice, so
//                these run as census jobs on the World (seeded rules, seeded soups,
//                deterministic), whose rule summaries stage 2 reads
//
// Each group is one job labelled `<label> · stage 1 · <group>`. Run again with the same label,
// a finished group is skipped and an interrupted one resumes from its checkpoint, so the script
// is safe to rerun after a crash or a reboot. Jobs show up in the app's Hunts window.
//
// Run:   jetcarp script scripts/stage1-all.rhai --set mode=quick
//        jetcarp script scripts/stage1-all.rhai --set mode=standard --set groups=squares,hexagons
//        jetcarp script scripts/stage1-all.rhai --set mode=full --set label="big run"
//        JETCARP_LIBRARY=/tmp/cat jetcarp script scripts/stage1-all.rhai --set mode=test
// `jetcarp pipeline` runs it as its first step.
//
// Parameters (all optional): mode (test, quick, standard, full; default quick), label (default
// "catalog"), groups (comma separated, default all, in the order above), threads, and budget
// overrides: sample_triangle, sample_arch, sample_kuniform, sample_iso, sample_aperiodic, kmax,
// iso_types, refine_top, neighbours_top.

let mode = params.mode ?? "quick";
let label = params.label ?? "catalog";
let threads = params.threads ?? 0;

// Budgets per mode. `all`: exhaustive below this many rules; `sample`: else this many rules.
let budgets = #{
    test: #{ square_sample: 48, square_all: 512, hex_all: 64, hex_sample: 64,
             triangle: 24, arch: 8, arch_tilings: 2, kuniform: 6, kmax: 2, kuniform_tilings: 2,
             iso: 6, iso_types: 2, aperiodic: 6, aperiodic_tilings: 1, aperiodic_soups: 2,
             refine: 0, neighbours: 0, small_refine: 0, small_neighbours: 0, gens: 300 },
    quick: #{ square_sample: 3000, square_all: 512, hex_all: 8192, hex_sample: 8192,
              triangle: 2000, arch: 400, arch_tilings: 8, kuniform: 100, kmax: 2,
              kuniform_tilings: 999, iso: 150, iso_types: 6, aperiodic: 150,
              aperiodic_tilings: 4, aperiodic_soups: 4,
              refine: 8, neighbours: 8, small_refine: 0, small_neighbours: 0, gens: 1000 },
    standard: #{ square_sample: 131072, square_all: 131072, hex_all: 8192, hex_sample: 8192,
                 triangle: 20000, arch: 4000, arch_tilings: 8, kuniform: 400, kmax: 3,
                 kuniform_tilings: 999, iso: 1000, iso_types: 24, aperiodic: 1000,
                 aperiodic_tilings: 4, aperiodic_soups: 4,
                 refine: 24, neighbours: 24, small_refine: 4, small_neighbours: 4, gens: 1000 },
    full: #{ square_sample: 131072, square_all: 131072, hex_all: 8192, hex_sample: 8192,
             triangle: 200000, arch: 50000, arch_tilings: 8, kuniform: 5000, kmax: 4,
             kuniform_tilings: 9999, iso: 10000, iso_types: 81, aperiodic: 20000,
             aperiodic_tilings: 4, aperiodic_soups: 8,
             refine: 48, neighbours: 48, small_refine: 12, small_neighbours: 12, gens: 1000 },
};
if !(mode in budgets) {
    throw `mode is test, quick, standard, or full, not ${mode}`;
}
let b = budgets[mode];
// Overrides from --set.
let over = #{ sample_triangle: "triangle", sample_arch: "arch", sample_kuniform: "kuniform",
              sample_iso: "iso", sample_aperiodic: "aperiodic", kmax: "kmax",
              iso_types: "iso_types", refine_top: "refine", neighbours_top: "neighbours" };
for k in over.keys() {
    if params[k] != () { b[over[k]] = params[k]; }
}

let all_groups = ["squares", "hexagons", "triangles", "archimedean", "kuniform", "isohedral",
                  "aperiodic"];
let groups = if params.groups != () { params.groups.to_string().split(",") } else { all_groups };
for g in groups {
    if !all_groups.contains(g) { throw `unknown group ${g}; groups are ${all_groups}`; }
}

fn mx(a, b) { if a > b { a } else { b } }
fn mn(a, b) { if a < b { a } else { b } }
fn refine_of(top) {
    if top == 0 { false } else { #{ top: top } }
}
fn neighbours_of(top) {
    if top == 0 { false } else { #{ top: top, rounds: 1 } }
}

let lib = library();
print(`stage 1 on every grid: mode ${mode}, label "${label}", catalog ${lib.path()}`);
let t_all = elapsed();
let report = [];

for g in all_groups {
    if !groups.contains(g) { continue; }
    let job_label = `${label} · stage 1 · ${g}`;
    let t0 = elapsed();
    let p = ();
    if g == "squares" {
        p = lib.stage1(#{ label: job_label, tilings: ["square"], nb: "both",
            all_max: mx(b.square_all, b.square_sample), sample: b.square_sample,
            gens: b.gens, refine: refine_of(b.refine), neighbours: neighbours_of(b.neighbours),
            threads: threads });
    } else if g == "hexagons" {
        p = lib.stage1(#{ label: job_label, tilings: ["hex"], all_max: b.hex_all,
            sample: b.hex_sample, gens: b.gens, refine: refine_of(b.refine),
            neighbours: neighbours_of(b.neighbours), threads: threads });
    } else if g == "triangles" {
        p = lib.stage1(#{ label: job_label, tilings: ["triangle"], nb: "both", all_max: 8192,
            sample: b.triangle, gens: b.gens, refine: refine_of(b.refine),
            neighbours: neighbours_of(b.neighbours), threads: threads });
    } else if g == "archimedean" {
        let ts = ["oct", "trihex", "elongtri", "snubsq", "snubhex", "rhombitrihex", "trunchex",
                  "trunctrihex"];
        ts.truncate(b.arch_tilings);
        p = lib.stage1(#{ label: job_label, tilings: ts, all_max: b.arch, sample: b.arch,
            gens: b.gens, refine: refine_of(mx(b.small_refine, b.refine / 3)),
            neighbours: neighbours_of(mx(b.small_neighbours, b.neighbours / 3)),
            threads: threads });
    } else if g == "kuniform" {
        let ts = [];
        for k in 2..=b.kmax {
            for t in catalog_tilings(k) { ts.push(t); }
        }
        ts.truncate(b.kuniform_tilings);
        p = lib.stage1(#{ label: job_label, tilings: ts, all_max: b.kuniform,
            sample: b.kuniform, grid: [32, 32], gens: b.gens,
            refine: refine_of(b.small_refine), neighbours: neighbours_of(b.small_neighbours),
            threads: threads });
    } else if g == "isohedral" {
        // Evenly spaced types, so a small budget still covers the whole range.
        let all = isohedral_tilings();
        let n = mn(b.iso_types, all.len());
        let ts = [];
        for i in 0..n { ts.push(all[i * all.len() / n]); }
        p = lib.stage1(#{ label: job_label, tilings: ts, all_max: b.iso, sample: b.iso,
            gens: b.gens, refine: refine_of(b.small_refine),
            neighbours: neighbours_of(b.small_neighbours), threads: threads });
    } else if g == "aperiodic" {
        // The census on the World: stage 1's board needs a lattice. One job per patch.
        let ts = ["spectre", "hat", "penrose-kd", "penrose-rhomb"];
        ts.truncate(b.aperiodic_tilings);
        let done = 0;
        for t in ts {
            let q = lib.census(#{ label: `${job_label} · ${t}`, tiling: t, all_max: b.aperiodic,
                sample: b.aperiodic, soups: b.aperiodic_soups, gens: b.gens, density: 0.35,
                threads: threads });
            print(`  ${t}: census job ${q.job}, ${q.rules_done}/${q.rules_total} rules, ${q.status}`);
            if q.status == "done" { done += 1; }
            if stopped() { break; }
        }
        p = #{ job: (), status: if done == ts.len() { "done" } else { "paused" }, evaluated: 0,
               spaces: [] };
    }
    let secs = elapsed() - t0;
    if p.job != () {
        print(`${g}: job ${p.job} ${p.status}, ${p.evaluated} settings, ${p.statuses.auto ?? 0} auto, ${p.statuses.candidate ?? 0} candidates (${secs.round()} s)`);
        for s in p.spaces {
            print(`  ${s.space}: ${s.evaluated} settings, ${s.auto} auto, ${s.candidate} candidates, best ${s.best_score.round()} ${s.best_rule}`);
        }
    }
    report.push(#{ group: g, job: p.job, status: p.status, seconds: secs });
    if stopped() { print("stopped: run again with the same label to resume"); break; }
}
print(`stage 1 on every grid: ${(elapsed() - t_all).round()} s`);
report
