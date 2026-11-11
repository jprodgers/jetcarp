//! `media.*`, `site.export`, `wallpaper.export`, and `sheet.*`: files written, up-to-date
//! media kept, the JSON maps the MCP tools and Rhai read, and error kinds.

mod common;

use common::{catalog, fails, keys, scratch};
use jetcarp_api::op::{Op, Quiet, Report, WorldOp, to_value};
use jetcarp_api::ops::media::*;
use jetcarp_api::ops::sheet::*;
use jetcarp_api::ops::wallpaper::{WallpaperExport, WallpaperIn};
use jetcarp_api::ops::world::WorldNew;
use jetcarp_api::world::{Host, Session};
use jetcarp_api::{Ctx, ErrorKind, Limits};
use serde_json::json;

fn render(cx: &Ctx, v: serde_json::Value) -> Result<MediaReport, jetcarp_api::ApiError> {
    MediaRender::run(cx, MediaRenderIn::from_map(&v).unwrap(), &Quiet)
}

#[test]
fn media_for_patterns_scenes_and_what_is_missing() {
    let dir = scratch("media");
    let (lib, cx) = catalog(&dir);
    let r = render(&cx, json!({"pattern": "glider", "frames": false})).unwrap();
    assert_eq!((r.written, r.skipped, r.failed.len()), (1, 0, 0));
    let d = &r.done[0];
    assert_eq!((d.kind.as_str(), d.mode.as_str()), ("pattern", "alone"));
    assert_eq!(
        keys(&d.files),
        ["anim", "height", "poster", "thumb", "width"]
    );
    assert!(std::path::Path::new(d.files["anim"].as_str().unwrap()).exists());
    // The JSON the MCP tool and Rhai return: no scene keys.
    assert_eq!(
        keys(&to_value(&r)),
        ["done", "failed", "skipped", "written"]
    );
    assert!(r.text().contains(" [alone] rendered: "), "{}", r.text());
    assert!(r.notes().ends_with("1 rendered, 0 up to date, 0 failed\n"));
    assert!(r.check().is_ok());
    // Again: up to date.
    let again = render(&cx, json!({"pattern": "glider", "frames": false})).unwrap();
    assert_eq!((again.written, again.skipped), (0, 1));
    assert!(again.text().contains(" up to date: "), "{}", again.text());
    // Stored rows.
    let id = lib.get("glider").unwrap().id;
    let l = MediaListOp::run(
        &cx,
        MediaListIn {
            kind: "pattern".into(),
            id: id.clone(),
        },
        &Quiet,
    )
    .unwrap();
    assert!(l.media.len() >= 3);
    assert!(l.text().starts_with("gif "), "{}", l.text());
    let none = MediaListOp::run(
        &cx,
        MediaListIn {
            kind: "ant".into(),
            id: "ant-000000000000".into(),
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!(none.text(), "no media for ant ant-000000000000\n");
    // Missing media of oscillators only.
    let m = render(
        &cx,
        json!({"missing": true, "class": "oscillator", "frames": false}),
    )
    .unwrap();
    assert_eq!(m.done.len(), 1);
    assert_eq!(m.done[0].id, lib.get("blinker").unwrap().id);
    // A scene from its seed, into a folder of its own.
    let mut sc = jetcarp_core::Scene::new(jetcarp_core::TilingSpec::rect(
        "square",
        20,
        16,
        jetcarp_core::Boundary::Wrap,
    ));
    sc.cas.push(
        jetcarp_core::CaSpec::new("B3/S23").seed(jetcarp_core::SeedSpec::Random { density: 0.3 }),
    );
    let out = dir.join("scene-media");
    let s = MediaRender::run(
        &cx,
        MediaRenderIn {
            scene: Some(serde_json::to_value(&sc).unwrap()),
            out: Some(out.display().to_string()),
            gens: Some(4),
            frames: Some(false),
            ..Default::default()
        },
        &Quiet,
    )
    .unwrap();
    assert_eq!(s.folder.as_deref(), Some(out.as_path()));
    assert_eq!(s.frames_count, Some(5));
    assert!(s.files.as_ref().unwrap()["anim"].is_string());
    assert!(s.text().starts_with("webp ") || s.text().starts_with("gif "));
    // A target that does not exist is a failure in the report, and the check fails.
    let bad = render(&cx, json!({"pattern": "nonesuch"})).unwrap();
    assert_eq!((bad.done.len(), bad.failed.len()), (0, 1));
    assert_eq!(
        fails(bad.check(), ErrorKind::Engine).message,
        "nothing rendered"
    );
    // Invalid options.
    for v in [
        json!({}),
        json!({"pattern": "glider", "mode": "bogus"}),
        json!({"pattern": "glider", "format": "bogus"}),
        json!({"pattern": "glider", "palette": "Bogus"}),
        json!({"missing": true, "class": "bogus"}),
    ] {
        fails(render(&cx, v), ErrorKind::Invalid);
    }
    // The map reader refuses unknown keys and wrong types, as the tools always have.
    let e = MediaRenderIn::from_map(&json!({"bogus": 1})).unwrap_err();
    assert!(e.starts_with("unknown media option `bogus`"), "{e}");
    let e = MediaRenderIn::from_map(&json!({"gens": "x"})).unwrap_err();
    assert!(e.contains("`gens` should be a whole number"), "{e}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn site_export_writes_once_and_keeps_its_page() {
    let dir = scratch("site");
    let (lib, cx) = catalog(&dir);
    lib.update(
        "glider",
        &jetcarp_core::library::Update {
            state: Some(jetcarp_core::library::CurationState::Reviewed),
            ..Default::default()
        },
    )
    .unwrap();
    let to = dir.join("site");
    let site = |a: SiteIn| SiteExport::run(&cx, a, &Quiet);
    let a = SiteIn {
        to: to.display().to_string(),
        render: false,
        page: true,
        ..Default::default()
    };
    let r = site(a.clone()).unwrap();
    assert_eq!((r.0.patterns, r.0.ants), (1, 0));
    assert!(r.0.written.iter().any(|p| p == "index.html"));
    assert!(r.0.written.iter().any(|p| p == "manifest.json"));
    assert!(to.join("index.html").exists());
    assert!(r.text().contains("wrote   patterns/"), "{}", r.text());
    assert!(r.notes().contains(": 1 patterns, 0 Quick Start entries; "));
    // Unchanged the second time; the page too.
    let again = site(a).unwrap();
    assert!(again.0.written.is_empty(), "{:?}", again.0.written);
    assert_eq!(again.0.unchanged, 1);
    // As the MCP tool and Rhai give it: no page, `state` as text, `quickstart: "none"`.
    let to2 = dir.join("site2");
    let m = SiteIn::from_map(
        &json!({"to": "ignored", "state": "new,reviewed", "quickstart": "none", "render": false}),
        &to2.display().to_string(),
    )
    .unwrap();
    assert!(!m.page);
    assert_eq!(m.quickstart, Some(vec![]));
    let r2 = site(m).unwrap();
    assert_eq!(r2.0.patterns, 2);
    assert!(!to2.join("index.html").exists());
    // Errors.
    let bad = |v: serde_json::Value| {
        let mut a: SiteIn = serde_json::from_value(v).unwrap();
        a.to = to.display().to_string();
        a.render = false;
        site(a)
    };
    fails(
        bad(json!({"to": "x", "state": ["bogus"]})),
        ErrorKind::Invalid,
    );
    fails(
        bad(json!({"to": "x", "quickstart": ["bogus"]})),
        ErrorKind::Invalid,
    );
    fails(
        bad(json!({"to": "x", "class": ["bogus"]})),
        ErrorKind::Invalid,
    );
    let e = SiteIn::from_map(&json!({"to": "x", "bogus": 1}), "x").unwrap_err();
    assert!(e.starts_with("unknown site_export option `bogus`"), "{e}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn wallpapers_of_the_world_or_a_scene() {
    let dir = scratch("wallpaper");
    let cx = Ctx::interactive();
    let mut h = Session::new();
    WorldNew::run(
        &cx,
        &mut h,
        serde_json::from_value(json!({"tiling": "square", "width": 20, "height": 16,
                                      "cas": [{"rule": "Life", "seed": "random:0.3"}]}))
        .unwrap(),
        &Quiet,
    )
    .unwrap();
    let wp = |h: &mut Session, v: serde_json::Value| {
        WallpaperExport::run(&cx, h, serde_json::from_value(v).unwrap(), &Quiet)
    };
    // One image of the world, named after the given file.
    let file = dir.join("one.png");
    let w = wp(&mut h, json!({"size": "200x120", "out": file})).unwrap();
    assert_eq!(w.files.len(), 1);
    assert_eq!(
        (w.files[0].path.clone(), w.files[0].width, w.files[0].height),
        (file.clone(), 200, 120)
    );
    assert!(file.exists());
    assert_eq!(w.generation, h.world().generation);
    assert_eq!(w.text(), format!("{} (200 x 120)\n", file.display()));
    // One image per monitor, with the monitor in the JSON and the PNG.
    let w = wp(
        &mut h,
        json!({"monitors": "100x60+0+0,60x60+100+0", "split": true, "out": dir.join("m.png")}),
    )
    .unwrap();
    assert_eq!(w.files.len(), 2);
    assert_eq!(w.files[1].monitor.as_deref(), Some("60x60+100+0"));
    assert!(dir.join("m-2.png").exists());
    let single = to_value(&wp(&mut h, json!({"size": "50x40", "out": dir.join("s.png")})).unwrap());
    assert_eq!(keys(&single["files"][0]), ["height", "path", "width"]);
    // A folder: a stamped name from the scene's name.
    let w = wp(&mut h, json!({"size": "50x40", "out": dir.join("folder")})).unwrap();
    let name = w.files[0]
        .path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert!(name.starts_with("wallpaper-50x40-"), "{name}");
    // A palette recolours the world itself (a logged command); fresh leaves it alone.
    let before = h.world().log().len();
    wp(
        &mut h,
        json!({"size": "50x40", "palette": "Paper", "out": dir.join("p.png")}),
    )
    .unwrap();
    assert_eq!(h.world().log().len(), before + 1);
    let generation = h.world().generation;
    let f = wp(
        &mut h,
        json!({"size": "160x100", "fresh": true, "cell_px": 8, "gens": 3, "out": dir.join("f.png")}),
    )
    .unwrap();
    assert_eq!((f.generation, h.world().generation), (3, generation));
    // Errors and limits.
    for (v, kind) in [
        (json!({"split": true}), ErrorKind::Invalid),
        (json!({"center": [1.0, 2.0]}), ErrorKind::Invalid),
        (json!({"size": "bogus"}), ErrorKind::Invalid),
        (json!({"fit": "bogus"}), ErrorKind::Invalid),
        (json!({"gens": 4}), ErrorKind::Invalid),
        (json!({"palette": "Bogus"}), ErrorKind::NotFound),
    ] {
        fails(wp(&mut h, v), kind);
    }
    // A fresh grid or a run larger than this context's limits (four times the world limits).
    let small = Ctx::new().with_limits(Limits {
        max_cells: 1000,
        max_work: 10_000,
        ..Limits::INTERACTIVE
    });
    for v in [
        json!({"size": "200x120", "fresh": true, "cell_px": 2}),
        json!({"size": "160x100", "fresh": true, "cell_px": 8, "gens": 1000}),
    ] {
        let e = fails(
            WallpaperExport::run(&small, &mut h, serde_json::from_value(v).unwrap(), &Quiet),
            ErrorKind::Limit,
        );
        assert!(e.message.contains("cell_px"), "{e}");
    }
    // From a scene file (the command line's way), with generations of its own.
    let scene = dir.join("soup.json");
    std::fs::write(&scene, h.scene().to_json()).unwrap();
    let w = wp(
        &mut h,
        json!({"source": scene, "size": "64x48", "gens": 2, "out": dir.join("src.png")}),
    )
    .unwrap();
    assert_eq!(w.generation, generation + 2);
    let e = fails(
        wp(
            &mut h,
            json!({"source": "nonesuch", "out": dir.join("x.png")}),
        ),
        ErrorKind::NotFound,
    );
    assert!(
        e.message
            .contains("neither a scene file nor a Quick Start id")
    );
    let e = fails(
        wp(&mut h, json!({"source": scene, "palette": "Bogus"})),
        ErrorKind::NotFound,
    );
    assert!(
        e.message.contains("Palettes: Ember, Tide") && e.message.contains("#103050-#4fc3ff"),
        "{e}"
    );
    // Hex colours work wherever palette names do, as on the rest of the command line.
    for src in [Some(scene.clone()), None] {
        let mut v =
            json!({"size": "40x30", "palette": "#103050-#4fc3ff", "out": dir.join("hex.png")});
        if let Some(s) = &src {
            v["source"] = json!(s);
        }
        wp(&mut h, v).unwrap();
    }
    assert_eq!(h.world().cas[0].palette, "#103050-#4fc3ff");
    // A built-in Quick Start entry; no limits on the command line's context.
    let cli = Ctx::new().with_limits(Limits::NONE);
    let w = WallpaperExport::run(
        &cli,
        &mut Session::new(),
        WallpaperIn {
            source: Some("life".into()),
            size: Some("64x36".into()),
            out: Some(dir.join("life").display().to_string()),
            ..serde_json::from_value(json!({})).unwrap()
        },
        &Quiet,
    )
    .unwrap();
    assert!(
        w.files[0]
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("Life-on-the-square-grid-64x36-")
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn contact_sheets() {
    let dir = scratch("sheets");
    let cx = Ctx::new();
    let png = dir.join("w.png");
    let s = SheetRules::run(
        &cx,
        serde_json::from_value(
            json!({"png": png, "count": 8, "cols": 4, "thumb": 40, "width": 21}),
        )
        .unwrap(),
        &Quiet,
    )
    .unwrap();
    assert_eq!((s.width, s.height), (200, 100));
    assert_eq!(s.text(), format!("sheet: {} (200x100)\n", png.display()));
    assert!(png.exists());
    fails(
        SheetRules::run(
            &cx,
            serde_json::from_value(json!({"png": png, "kind": "bogus"})).unwrap(),
            &Quiet,
        ),
        ErrorKind::Invalid,
    );
    // A palette name in the wrong case is refused, not drawn in Ember.
    let e = fails(
        SheetRules::run(
            &cx,
            serde_json::from_value(json!({"png": png, "count": 2, "palette": "tide"})).unwrap(),
            &Quiet,
        ),
        ErrorKind::Invalid,
    );
    assert!(e.message.contains("no palette named 'tide'"), "{e}");
    let iso = dir.join("iso.png");
    let s = SheetIsohedral::run(
        &cx,
        serde_json::from_value(json!({"png": iso, "thumb": 20, "straight": true})).unwrap(),
        &Quiet,
    )
    .unwrap();
    assert!(iso.exists() && s.width > 0);
    // A sweep folder's runs (no pictures: empty panels under the labels).
    let sw = dir.join("2026-11-10_12-00-00_W");
    std::fs::create_dir_all(&sw).unwrap();
    std::fs::write(sw.join("run.json"), "{}").unwrap();
    std::fs::write(
        sw.join("manifest.json"),
        json!({"rows": [{"index": 0, "rule": "W30", "file": ""},
                        {"index": 1, "rule": "W110", "file": ""}]})
        .to_string(),
    )
    .unwrap();
    let out = dir.join("sweep.png");
    let sweep = |runs: Vec<&str>| {
        SheetSweep::run(
            &cx,
            serde_json::from_value(json!({"folder": sw, "runs": runs, "png": out, "thumb": 40}))
                .unwrap(),
            &Quiet,
        )
    };
    let s = sweep(vec!["W30", "#1"]).unwrap();
    assert!(out.exists() && s.width >= 80);
    fails(sweep(vec![]), ErrorKind::Invalid);
    fails(sweep(vec!["W99"]), ErrorKind::NotFound);
    let _ = std::fs::remove_dir_all(dir);
}
