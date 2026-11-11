//! Palettes with backgrounds, view rendering, and wallpapers.

use jetcarp_core::render::{self, Image, Palette, ViewOptions};
use jetcarp_core::wallpaper::{self, Fit, MonitorRect};
use jetcarp_core::*;

fn soup(tiling: TilingSpec, palette: &str, gens: u64) -> World {
    let mut s = Scene::new(tiling);
    s.rng_state = 3;
    let mut ca = CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.35 });
    ca.palette = Some(palette.into());
    s.cas = vec![ca];
    let mut w = World::from_scene(&s).unwrap();
    w.apply(Command::Step { gens }).unwrap();
    w
}

fn px(img: &Image, x: u32, y: u32) -> [u8; 4] {
    let o = ((y * img.width + x) * 4) as usize;
    img.rgba[o..o + 4].try_into().unwrap()
}

fn count(img: &Image, c: [u8; 4]) -> usize {
    img.rgba.chunks(4).filter(|p| *p == c).count()
}

#[test]
fn every_palette_renders_with_its_background() {
    let names: Vec<&str> = render::palette_names().collect();
    assert!(names.len() >= 17, "{} palettes", names.len());
    let mut items = Vec::new();
    for name in &names {
        let p = Palette::builtin(name).unwrap();
        assert!(p.stops.len() >= 4, "{name}");
        let w = soup(
            TilingSpec::rect("square", 120, 80, Boundary::Wrap),
            name,
            500,
        );
        let (bg, stage) = render::backdrop(&w);
        assert_eq!(bg, p.background_rgba(), "{name}");
        assert_eq!(stage, p.stage_rgba(), "{name}");
        // Empty cells take the background; the first and last stops stand out from it.
        let colors = render::cell_colors(&w);
        let empty = w.fields[0].state.iter().position(|&s| s == 0).unwrap();
        assert_eq!(colors[empty], bg, "{name}");
        let dist = |a: [u8; 3], b: [u8; 4]| {
            (0..3)
                .map(|k| (a[k] as i32 - b[k] as i32).abs())
                .sum::<i32>()
        };
        for s in [p.stops[0], *p.stops.last().unwrap()] {
            assert!(
                dist(s, bg) >= 80,
                "{name}: stop {s:?} too close to the background"
            );
        }
        let img = render::render_view(
            &w,
            360,
            240,
            [60.0, 40.0],
            3.0,
            &ViewOptions {
                outline_min_px: 0.0,
                ..Default::default()
            },
        );
        assert_eq!((img.width, img.height), (360, 240));
        assert!(count(&img, bg) > 1000, "{name}: no background");
        items.push((vec![(name.to_string(), [0xe0, 0xe0, 0xe0, 0xff])], img));
    }
    // JETCARP_PALETTE_SHEET=path writes a contact sheet of every palette.
    if let Ok(path) = std::env::var("JETCARP_PALETTE_SHEET") {
        let refs: Vec<_> = items.iter().map(|(l, i)| (l.clone(), i)).collect();
        let sheet = jetcarp_core::sheet::labelled_sheet(&refs, 4);
        render::write_png(std::path::Path::new(&path), &sheet, &[]).unwrap();
    }
    // Default palettes keep JETCARP's slate; scenes keep user palettes' backgrounds.
    let w = soup(
        TilingSpec::rect("square", 20, 20, Boundary::Dead),
        "Ember",
        0,
    );
    assert_eq!(render::backdrop(&w), (render::BACKGROUND, render::STAGE));
    // A palette of the world's own, made and then chosen (a palette must exist to be chosen).
    let mut w = soup(
        TilingSpec::rect("square", 20, 20, Boundary::Dead),
        "Ember",
        0,
    );
    let mut mine = Palette::new("Mine", vec![[10, 10, 10], [90, 90, 90]]);
    mine.background = Some([250, 250, 250]);
    w.apply(Command::SetPalette { palette: mine }).unwrap();
    let id = w.cas[0].id;
    let change = CaChange {
        palette: Some("Mine".into()),
        ..Default::default()
    };
    w.apply(Command::UpdateCa { id, change }).unwrap();
    assert_eq!(render::backdrop(&w).0, [250, 250, 250, 255]);
    let back = World::from_scene(&Scene::from_json(&w.to_scene().to_json()).unwrap()).unwrap();
    assert_eq!(render::backdrop(&back).0, [250, 250, 250, 255]);
}

#[test]
fn views_have_exact_sizes_and_repeat_seamlessly() {
    let w = soup(
        TilingSpec::rect("square", 50, 40, Boundary::Wrap),
        "Paper",
        30,
    );
    let stage = render::backdrop(&w).1;
    for (wd, ht) in [(1, 1), (37, 101), (640, 360), (1000, 20)] {
        let img = render::render_view(&w, wd, ht, [25.0, 20.0], 2.5, &ViewOptions::default());
        assert_eq!(
            (img.width, img.height, img.rgba.len()),
            (wd, ht, (wd * ht * 4) as usize)
        );
    }
    // Without repeat, a view larger than the grid shows the stage; with repeat it does not.
    let plain = render::render_view(&w, 400, 300, [25.0, 20.0], 3.0, &ViewOptions::default());
    assert!(count(&plain, stage) > 1000);
    let rep = ViewOptions {
        repeat: true,
        ..Default::default()
    };
    let tiled = render::render_view(&w, 400, 300, [25.0, 20.0], 3.0, &rep);
    assert_eq!(count(&tiled, stage), 0);
    // One grid width (150 px at 3 px per cell) to the right is the same picture.
    for y in [0, 77, 299] {
        for x in [0, 33, 249] {
            assert_eq!(px(&tiled, x, y), px(&tiled, x + 150, y), "({x}, {y})");
        }
    }
    // A view of the middle matches the plain render of the grid at the same scale.
    let full = render::render_view(&w, 150, 120, [25.0, 20.0], 3.0, &ViewOptions::default());
    let mid = render::render_view(&w, 150, 120, [25.0, 20.0], 3.0, &rep);
    assert_eq!(full.rgba, mid.rgba);
}

#[test]
fn wallpaper_fits_and_spans() {
    let tilings = [
        TilingSpec::rect("square", 160, 90, Boundary::Dead),
        TilingSpec::rect("hex", 90, 60, Boundary::Dead),
        TilingSpec::rect("trihex", 40, 40, Boundary::Dead),
        TilingSpec::rect("snubsq", 30, 30, Boundary::Wrap),
        TilingSpec::new("hat").with_size(SizeSpec::Level(4)),
    ];
    for t in tilings {
        let what = t.family.clone();
        let w = soup(t, "Glacier", 20);
        let stage = render::backdrop(&w).1;
        for (wd, ht) in [(640, 360), (300, 500)] {
            let cover = wallpaper::wallpaper(&w, wd, ht, &Fit::Cover, 0.0).unwrap();
            assert_eq!((cover.width, cover.height), (wd, ht), "{what}");
            assert_eq!(
                count(&cover, stage),
                0,
                "{what}: stage in a {wd}x{ht} cover"
            );
            let contain = wallpaper::wallpaper(&w, wd, ht, &Fit::Contain, 0.0).unwrap();
            assert_eq!((contain.width, contain.height), (wd, ht));
        }
        let rep = wallpaper::wallpaper(&w, 200, 100, &Fit::Repeat { cell_px: 3.0 }, 0.0);
        assert_eq!(rep.is_ok(), w.topo.wraps(), "{what}: repeat");
    }
    // A span of two monitors: the per-monitor cuts tile back into the span.
    let w = soup(
        TilingSpec::rect("square", 200, 100, Boundary::Wrap),
        "Neon",
        40,
    );
    let mons = wallpaper::parse_monitors("256x144+0+0,192x108+256+18").unwrap();
    let (_, _, sw, sh) = wallpaper::span_box(&mons);
    assert_eq!((sw, sh), (448, 144));
    let span = wallpaper::wallpaper(&w, sw, sh, &Fit::Cover, 0.0).unwrap();
    let parts = wallpaper::split(&span, &mons);
    assert_eq!((parts[0].width, parts[0].height), (256, 144));
    assert_eq!((parts[1].width, parts[1].height), (192, 108));
    for (m, p) in mons.iter().zip(&parts) {
        let MonitorRect { x, y, .. } = *m;
        for yy in (0..p.height).step_by(7) {
            for xx in (0..p.width).step_by(5) {
                assert_eq!(px(p, xx, yy), px(&span, x as u32 + xx, y as u32 + yy));
            }
        }
    }
    // A fresh grid sized for the image fills it at about the cell size asked for.
    let spec = wallpaper::fresh_tiling(&TilingSpec::new("square"), 1920, 1080, 4.0);
    assert_eq!(spec.size, Some(SizeSpec::Rect { w: 480, h: 270 }));
    let spec = wallpaper::fresh_tiling(&TilingSpec::new("hex"), 1920, 1080, 6.0);
    assert!(matches!(spec.size, Some(SizeSpec::Rect { .. })));
}

#[test]
fn a_wide_span_renders() {
    // 7680 x 2160 (two 4K monitors side by side) in memory.
    let spec = wallpaper::fresh_tiling(
        &TilingSpec::new("square").with_boundary(Boundary::Wrap),
        7680,
        2160,
        3.0,
    );
    let mut s = Scene::new(spec);
    s.cas = vec![CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.3 })];
    let w = World::from_scene(&s).unwrap();
    let img = wallpaper::wallpaper(&w, 7680, 2160, &Fit::Cover, 0.0).unwrap();
    assert_eq!((img.width, img.height), (7680, 2160));
    assert_eq!(count(&img, render::backdrop(&w).1), 0);
}
