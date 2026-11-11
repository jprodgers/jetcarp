//! Large sandpiles as catalog jobs: a pile job keeps its palette and direction. (The pile itself
//! is tested in jetcarp-core.)

use jetcarp_core::pile::{self, Centre, Ctl, Pile, PileSpec};
use jetcarp_core::{Nb, TilingSpec};
use serde_json::json;

/// The export writes the palette into pile.png, pile.json, and look.json; a pile job keeps
/// its palette and direction, and an unknown palette is an error naming the palettes.
#[test]
fn pile_export_and_jobs_keep_the_palette() {
    use jetcarp_core::library::Library;
    use jetcarp_core::pile::export::{ExportOptions, read_look_file, write_look_file};
    use jetcarp_search::pile::{self as pilejob, PileJobSpec};
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-pile-look-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut p = Pile::new(PileSpec {
        tiling: spec_of("square"),
        nb: Nb::Edge,
        grains: 1500,
        centre: Centre::Cell,
    })
    .unwrap();
    let opts = pile::RunOptions {
        export: ExportOptions {
            palette: "Candy".into(),
            reverse: true,
            preview_side: 64,
            ..Default::default()
        },
        save_steps: false,
        extra: serde_json::Value::Null,
    };
    pile::run_to_dir(&mut p, &dir, None, &mut Ctl::default(), &opts).unwrap();
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("pile.json")).unwrap()).unwrap();
    assert_eq!(meta["image"]["palette"], "Candy");
    assert_eq!(meta["image"]["reverse"], true);
    let png =
        jetcarp_core::media::encode::decode_png(&std::fs::read(dir.join("pile.png")).unwrap())
            .unwrap();
    let candy = jetcarp_core::render::Palette::builtin("Candy").unwrap();
    assert_eq!(&png.rgba[..4], &candy.background_rgba()[..]);
    // look.json round trip.
    write_look_file(&dir, "Glacier", true).unwrap();
    assert_eq!(read_look_file(&dir), Some(("Glacier".into(), true)));
    assert_eq!(read_look_file(&dir.join("nowhere")), None);
    // Jobs: the spec keeps reverse (and old specs without it read as false); start writes
    // look.json; a bad palette is refused with the list.
    let lib = Library::open(dir.join("catalog"));
    let mut spec = PileJobSpec {
        pile: PileSpec {
            tiling: spec_of("square"),
            nb: Nb::Edge,
            grains: 500,
            centre: Centre::Cell,
        },
        folder: dir.join("job"),
        px_per_cell: 2.0,
        margin: 2.0,
        palette: "Lava".into(),
        reverse: true,
        preview_side: 32,
        png: true,
        gpu: false,
        extra: serde_json::Value::Null,
    };
    let job = pilejob::start(&lib, &spec).unwrap();
    assert_eq!(read_look_file(&spec.folder), Some(("Lava".into(), true)));
    let (back, _) = pilejob::status(&lib, job).unwrap();
    assert!(back.reverse && back.palette == "Lava");
    let mut old = serde_json::to_value(&spec).unwrap();
    old.as_object_mut().unwrap().remove("reverse");
    assert!(!serde_json::from_value::<PileJobSpec>(old).unwrap().reverse);
    let run = |job: i64| -> jetcarp_search::pile::PileProgress {
        let o = jetcarp_search::jobs::RunOptions::default();
        jetcarp_search::jobs::Runner::new(lib.clone())
            .run(job, &o)
            .unwrap()
            .progress_as()
    };
    let meta_of = |folder: &std::path::Path| -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(folder.join("pile.json")).unwrap()).unwrap()
    };
    let done = run(job);
    assert_eq!(done.status, "done");
    let meta = meta_of(&spec.folder);
    assert_eq!(meta["image"]["palette"], "Lava");
    assert_eq!(meta["image"]["reverse"], true);
    // The export takes the colours look.json has when the pile is done (the Sandpile window
    // changes them while the pile builds).
    let mut spec2 = spec.clone();
    spec2.folder = dir.join("job-recoloured");
    let job2 = pilejob::start(&lib, &spec2).unwrap();
    write_look_file(&spec2.folder, "Glacier", false).unwrap();
    assert_eq!(run(job2).status, "done");
    let meta = meta_of(&spec2.folder);
    assert_eq!(meta["image"]["palette"], "Glacier");
    assert_eq!(meta["image"]["reverse"], false);
    spec.palette = "Nope".into();
    spec.folder = dir.join("bad");
    let e = pilejob::start(&lib, &spec).unwrap_err();
    assert!(
        e.contains("Nope") && e.contains("Ember") && e.contains("Paper"),
        "{e}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn spec_of(id: &str) -> TilingSpec {
    if jetcarp_core::families::family(id).is_some() {
        TilingSpec::new(id)
    } else {
        let mut s = TilingSpec::new("periodic");
        s.params = Some(json!({ "catalog": id }));
        s
    }
}
