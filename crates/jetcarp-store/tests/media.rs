//! Media (Phase 4 catalog deliverable 6): seamless loops, moving windows, frame counts, the
//! WebP container, idempotent re-rendering, thread-count independence, and content-addressed
//! files: identical renders stored once, a shared file kept when one of its records goes, and
//! `gc`.
//!
//! Set `JETCARP_MEDIA_SAMPLES=DIR` to keep the rendered files for a look.

use jetcarp_core::hunt::separate::{Cache, Census, Obj};
use jetcarp_core::media::{
    self, AloneView, MediaFormat, MediaMode, MediaOptions, MediaStyle, encode,
};
use jetcarp_core::objects::{ObjClass, ObjectOptions, find_objects, place_rows};
use jetcarp_core::pattern::{
    self, DiscoveredBy, PatternClass, PatternRecord, Provenance, record_from_object,
};
use jetcarp_core::render::Image;
use jetcarp_core::*;
use jetcarp_store::media::{MediaTarget, MissingFilter};
use jetcarp_store::{Library, QuickStartRow};

fn temp_lib(name: &str) -> Library {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-media-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Library::open(dir)
}

fn prov() -> Provenance {
    Provenance {
        source: "test".into(),
        jetcarp_version: VERSION.into(),
        date: pattern::now_utc(),
        ..Default::default()
    }
}

/// The record of the one object `rows` makes on a wrapped square grid running `rule`.
fn one(rule: &str, rows: &str) -> PatternRecord {
    let mut w = World::new(TilingSpec::rect("square", 64, 64, Boundary::Wrap), 1).unwrap();
    let g = w.topo.lattice().unwrap().clone();
    w.apply(Command::AddCa {
        ca: CaSpec::new(rule).seed(SeedSpec::Cells {
            cells: place_rows(&g, rows, 20, 20),
        }),
    })
    .unwrap();
    let r = find_objects(&w, &ObjectOptions::default()).unwrap();
    let v: Vec<PatternRecord> = r
        .objects
        .iter()
        .filter(|o| !matches!(o.class, ObjClass::Other { .. }))
        .map(|o| record_from_object(&w, w.cas[0].id, o, prov(), DiscoveredBy::User).unwrap())
        .collect();
    assert_eq!(v.len(), 1, "{rule} {rows}");
    v.into_iter().next().unwrap()
}

fn keep(name: &str, path: &std::path::Path) {
    if let Some(d) = std::env::var_os("JETCARP_MEDIA_SAMPLES") {
        let d = std::path::PathBuf::from(d);
        std::fs::create_dir_all(&d).unwrap();
        if path.is_dir() {
            let to = d.join(name);
            let _ = std::fs::remove_dir_all(&to);
            std::fs::create_dir_all(&to).unwrap();
            for e in std::fs::read_dir(path).unwrap() {
                let e = e.unwrap();
                if e.path().is_file() {
                    std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
                }
            }
        } else {
            std::fs::copy(path, d.join(name)).unwrap();
        }
    }
}

const GLIDER: &str = ".o./..o/ooo";

/// Bounding box and centroid of the pixels of a colour.
fn colour_box(img: &Image, c: [u8; 3]) -> Option<([u32; 4], [f64; 2])> {
    let (mut b, mut sum, mut n) = ([u32::MAX, u32::MAX, 0, 0], [0.0, 0.0], 0.0);
    for y in 0..img.height {
        for x in 0..img.width {
            let o = ((y * img.width + x) * 4) as usize;
            if img.rgba[o..o + 3] == c {
                b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
                sum[0] += x as f64;
                sum[1] += y as f64;
                n += 1.0;
            }
        }
    }
    (n > 0.0).then(|| (b, [sum[0] / n, sum[1] / n]))
}

#[test]
fn glider_gif_loops_seamlessly_in_a_moving_window() {
    let lib = temp_lib("glider");
    let rec = one("B3/S23", GLIDER);
    assert_eq!(rec.class, PatternClass::Spaceship);
    lib.add(rec.clone()).unwrap();
    let o = MediaOptions::default();
    let r = jetcarp_store::media::render(&lib, &MediaTarget::Pattern(rec.id.clone()), &o).unwrap();
    assert!(r.written);
    assert_eq!(r.mode, MediaMode::Alone);
    let anim = r.animation().unwrap();
    assert_eq!(anim.kind, "gif", "a glider is small: GIF");
    assert_eq!(anim.render["window"], "moving");
    keep("glider.gif", &anim.abs_path);
    let frames = encode::decode_gif(&std::fs::read(&anim.abs_path).unwrap()).unwrap();
    assert_eq!(frames.len(), 4, "one period");
    // The frame after the last, from the world's own stepper, is the first frame exactly.
    let view = AloneView::new(&rec, 1, &o.style).unwrap();
    let mut w = view.world_with(&rec.phase(3).unwrap()).unwrap();
    w.apply(Command::Step { gens: 1 }).unwrap();
    let next = view.live(&w);
    assert_eq!(next, rec.phase(4).unwrap(), "the glider moved one period");
    let after = view.frame(&next, 4).unwrap();
    assert_eq!(
        after.rgba, frames[0].rgba,
        "frame 4 equals frame 0 pixel for pixel"
    );
    // The moving window keeps it centred: every frame shows all five cells, well inside the
    // picture, near the middle.
    let live = o.style.binary[0];
    for (t, f) in frames.iter().enumerate() {
        let (b, c) = colour_box(f, live).expect("glider visible");
        let m = view.cell_px as u32;
        assert!(
            b[0] >= m && b[1] >= m && b[2] + m < f.width && b[3] + m < f.height,
            "frame {t}: {b:?} in {}x{}",
            f.width,
            f.height
        );
        let (cx, cy) = (f.width as f64 / 2.0, f.height as f64 / 2.0);
        assert!(
            (c[0] - cx).abs() < view.cell_px * 1.01 && (c[1] - cy).abs() < view.cell_px * 1.01,
            "frame {t}: centroid {c:?}, centre ({cx}, {cy})"
        );
    }
    // The poster and thumbnail exist and are PNGs of the first frame.
    let poster =
        encode::decode_png(&std::fs::read(&r.file("poster").unwrap().abs_path).unwrap()).unwrap();
    assert_eq!(poster.rgba, frames[0].rgba);
    let thumb = r.file("thumb").unwrap();
    assert!(thumb.width <= o.style.thumb_side && thumb.height <= o.style.thumb_side);
    // One PNG per step, content-addressed, listed in order in the frame list.
    let fr = r.file("frames").unwrap();
    assert_eq!(fr.frames, 4);
    assert_eq!(fr.frame_files.len(), 4);
    assert!(fr.frame_files.iter().all(|f| f.exists()));
    assert_eq!(fr.abs_path.extension().unwrap(), "txt");
    let list = std::fs::read_to_string(&fr.abs_path).unwrap();
    assert!(list.starts_with("jetcarp-frames 1\n"), "{list}");
    assert_eq!(media::content_hash(list.as_bytes()), fr.hash);
    // Every file is named by its hash.
    for f in r
        .rows
        .iter()
        .map(|x| x.abs_path.clone())
        .chain(fr.frame_files.clone())
    {
        let name = f.file_stem().unwrap().to_string_lossy().into_owned();
        assert_eq!(name, media::content_hash(&std::fs::read(&f).unwrap()));
        assert_eq!(
            f.parent().unwrap().file_name().unwrap().to_string_lossy(),
            &name[..2]
        );
    }
}

#[test]
fn oscillator_gif_has_period_frames() {
    let lib = temp_lib("osc");
    // Blinker (p2) and pulsar (p3).
    let blinker = one("B3/S23", "ooo");
    let pulsar = one(
        "B3/S23",
        "..ooo...ooo../\
         ............./\
         o....o.o....o/\
         o....o.o....o/\
         o....o.o....o/\
         ..ooo...ooo../\
         ............./\
         ..ooo...ooo../\
         o....o.o....o/\
         o....o.o....o/\
         o....o.o....o/\
         ............./\
         ..ooo...ooo..",
    );
    let block = one("B3/S23", "oo/oo");
    for (name, rec, p) in [
        ("blinker", &blinker, 2),
        ("pulsar", &pulsar, 3),
        ("block", &block, 1),
    ] {
        assert_eq!(rec.period, p, "{name}");
        lib.add(rec.clone()).unwrap();
        let r = jetcarp_store::media::render(
            &lib,
            &MediaTarget::Pattern(rec.id.clone()),
            &MediaOptions::default(),
        )
        .unwrap();
        let a = r.animation().unwrap();
        assert_eq!(a.kind, "gif");
        keep(&format!("{name}.gif"), &a.abs_path);
        let frames = encode::decode_gif(&std::fs::read(&a.abs_path).unwrap()).unwrap();
        assert_eq!(
            frames.len() as u64,
            p,
            "{name}: exactly one period of frames"
        );
        assert_eq!(a.frames as u64, p);
        // It loops: stepping the last phase gives the first.
        let view = AloneView::new(rec, 1, &MediaStyle::default()).unwrap();
        let mut w = view.world_with(&rec.phase(p - 1).unwrap()).unwrap();
        w.apply(Command::Step { gens: 1 }).unwrap();
        assert_eq!(
            view.frame(&view.live(&w), p).unwrap().rgba,
            frames[0].rgba,
            "{name}"
        );
    }
}

/// Bill Gosper's glider gun (as in `tests/hunt.rs`).
const GOSPER: &str = "........................o.........../\
......................o.o.........../\
............oo......oo............oo/\
...........o...o....oo............oo/\
oo........o.....o...oo............../\
oo........o...o.oo....o.o.........../\
..........o.....o.......o.........../\
...........o...o..................../\
............oo......................";

fn gosper() -> PatternRecord {
    let c = Census::new("B3/S23", &TilingSpec::new("square"), None, 64).unwrap();
    let parts = c.census(
        &mut Cache::default(),
        &jetcarp_core::objects::rows_to_cells(GOSPER),
    );
    let mut rec = parts
        .iter()
        .find_map(|p| match &p.obj {
            Obj::Pattern(f) if f.class == PatternClass::Gun => Some(f.record.clone()),
            _ => None,
        })
        .expect("the gun");
    rec.verified = pattern::verify(&rec, 4).unwrap();
    rec
}

#[test]
fn gun_gif_shows_its_stream_and_loops() {
    let lib = temp_lib("gun");
    let rec = gosper();
    lib.add(rec.clone()).unwrap();
    let o = MediaOptions::default();
    let r = jetcarp_store::media::render(&lib, &MediaTarget::Pattern(rec.id.clone()), &o).unwrap();
    let a = r.animation().unwrap();
    keep("gosper.gif", &a.abs_path);
    keep("gosper-poster.png", &r.file("poster").unwrap().abs_path);
    assert_eq!(a.render["window"], "stream");
    assert_eq!(a.frames, 30, "one gun period");
    let anim = media::pattern_animation(
        &rec,
        &MediaOptions {
            periods: 2,
            ..o.clone()
        },
    )
    .unwrap();
    // Two periods: the second repeats the first exactly, so one period loops.
    assert_eq!(anim.frames.len(), 60);
    for t in 0..30 {
        assert_eq!(anim.frames[t].rgba, anim.frames[t + 30].rgba, "frame {t}");
    }
    // The stream: at least four gliders' worth of live pixels beyond the core.
    let frames = encode::decode_gif(&std::fs::read(&a.abs_path).unwrap()).unwrap();
    let live = o.style.binary[0];
    let n = frames[0]
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[..3] == live)
        .count() as f64
        / (anim.cell_px * anim.cell_px);
    assert!(n >= 36.0 + 4.0 * 5.0, "{n} live cells drawn");
}

#[test]
fn soup_webp_decodes_with_its_frame_count_and_size() {
    let lib = temp_lib("soup");
    let mut sc = Scene::new(TilingSpec::rect("square", 160, 120, Boundary::Wrap));
    sc.name = "Life soup".into();
    sc.cas = vec![CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.35 })];
    let row = QuickStartRow::for_scene("soup-test", "scene", sc.clone());
    lib.put_quickstart(&row).unwrap();
    let o = MediaOptions {
        gens: Some(60),
        ..Default::default()
    };
    let r = jetcarp_store::media::render(&lib, &MediaTarget::QuickStart("soup-test".into()), &o)
        .unwrap();
    assert_eq!(r.mode, MediaMode::Seed);
    let a = r.animation().unwrap();
    assert_eq!(a.kind, "webp", "a whole soup is large: WebP");
    keep("life-soup.webp", &a.abs_path);
    let frames = encode::decode_webp(&std::fs::read(&a.abs_path).unwrap()).unwrap();
    assert_eq!(frames.len(), 61, "generations 0 to 60");
    // 160 x 120 cells at 3 pixels per cell (512 / 160, whole pixels).
    assert_eq!((frames[0].width, frames[0].height), (480, 360));
    assert_eq!((a.width, a.height), (480, 360));
    // The decoded frames are the PNG frames.
    let f10 =
        encode::decode_png(&std::fs::read(&r.file("frames").unwrap().frame_files[10]).unwrap())
            .unwrap();
    assert_eq!(f10.rgba, frames[10].rgba);
    // The poster is the last frame.
    let poster =
        encode::decode_png(&std::fs::read(&r.file("poster").unwrap().abs_path).unwrap()).unwrap();
    assert_eq!(poster.rgba, frames[60].rgba);
    // The same scene as a GIF on request.
    let g = media::scene_animation(&sc, &o).unwrap();
    let (f, bytes) = g.encode(MediaFormat::Gif, &o.style).unwrap();
    assert_eq!(f, MediaFormat::Gif);
    assert_eq!(encode::decode_gif(&bytes).unwrap().len(), 61);
}

#[test]
fn unchanged_inputs_skip_and_a_style_change_rerenders() {
    let lib = temp_lib("idem");
    let rec = one("B3/S23", GLIDER);
    lib.add(rec.clone()).unwrap();
    let t = MediaTarget::Pattern(rec.id.clone());
    let o = MediaOptions::default();
    let a = jetcarp_store::media::render(&lib, &t, &o).unwrap();
    assert!(a.written);
    let gif = a.animation().unwrap().abs_path.clone();
    let before = std::fs::metadata(&gif).unwrap().modified().unwrap();
    let b = jetcarp_store::media::render(&lib, &t, &o).unwrap();
    assert!(!b.written, "same inputs and style: skipped");
    assert_eq!(std::fs::metadata(&gif).unwrap().modified().unwrap(), before);
    assert_eq!(b.rows.len(), 4, "frames, gif, poster, thumb");
    // Curation changes do not change the pictures.
    lib.update(
        &rec.id,
        &jetcarp_store::Update {
            rating: Some(4),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!jetcarp_store::media::render(&lib, &t, &o).unwrap().written);
    // A style change marks it stale.
    let mut o2 = o.clone();
    o2.style.binary = vec![[0x60, 0xd0, 0xff]];
    let c = jetcarp_store::media::render(&lib, &t, &o2).unwrap();
    assert!(c.written, "style changed: rendered again");
    assert_ne!(c.rows[0].key, a.rows[0].key);
    // New bytes, a new file; the old one stays until `gc` (nothing names it now).
    let gif2 = c.animation().unwrap().abs_path.clone();
    assert_ne!(gif2, gif);
    assert!(gif.exists());
    let frames = encode::decode_gif(&std::fs::read(&gif2).unwrap()).unwrap();
    assert!(colour_box(&frames[0], [0x60, 0xd0, 0xff]).is_some());
    // A missing-media batch now finds nothing to do for it.
    let r = jetcarp_store::media::render_missing(
        &lib,
        &MissingFilter {
            patterns: true,
            ..Default::default()
        },
        &o2,
    )
    .unwrap();
    assert_eq!((r.written(), r.skipped()), (0, 1));
    // Both modes coexist.
    let seed = jetcarp_store::media::render(
        &lib,
        &t,
        &MediaOptions {
            mode: MediaMode::Seed,
            gens: Some(20),
            ..o2.clone()
        },
    )
    .unwrap();
    assert!(seed.written && seed.rows.iter().all(|r| r.from_seed));
    assert_eq!(
        jetcarp_store::media::stored(&lib, "pattern", &rec.id)
            .unwrap()
            .len(),
        8
    );
    assert!(
        jetcarp_store::media::thumbnail(&lib, "pattern", &rec.id)
            .unwrap()
            .is_some()
    );
}

#[test]
fn batch_is_identical_at_any_thread_count() {
    let recs = [
        one("B3/S23", GLIDER),
        one("B3/S23", "ooo"),
        one("B3/S23", "oo/oo"),
        one("B3/S23", "o..o./....o/o...o/.oooo"),
    ];
    let mut outs = Vec::new();
    for threads in [1, 4] {
        let lib = temp_lib(&format!("threads{threads}"));
        for r in &recs {
            lib.add(r.clone()).unwrap();
        }
        let mut sc = Scene::new(TilingSpec::rect("hex", 48, 40, Boundary::Wrap));
        sc.cas = vec![CaSpec::new("B2/S34").seed(SeedSpec::Random { density: 0.3 })];
        lib.put_quickstart(&QuickStartRow::for_scene("hex-soup", "scene", sc))
            .unwrap();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let rep = pool
            .install(|| {
                jetcarp_store::media::render_missing(
                    &lib,
                    &MissingFilter {
                        patterns: true,
                        quickstart: true,
                        ..Default::default()
                    },
                    &MediaOptions {
                        gens: Some(30),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert!(rep.failed.is_empty(), "{:?}", rep.failed);
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        let root = lib.folder().join("media");
        let mut stack = vec![root.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    files.push((
                        p.strip_prefix(&root).unwrap().display().to_string(),
                        std::fs::read(&p).unwrap(),
                    ));
                }
            }
        }
        files.sort();
        outs.push(files);
    }
    assert!(outs[0].len() > 20, "{} files", outs[0].len());
    assert_eq!(outs[0].len(), outs[1].len());
    for (a, b) in outs[0].iter().zip(&outs[1]) {
        assert_eq!(a.0, b.0);
        assert!(a.1 == b.1, "{} differs between 1 and 4 threads", a.0);
    }
}

/// A palette override colours the media from a built-in palette and is part of the style hash
/// (so stored media in another palette render again); the house style stays as it was.
#[test]
fn a_palette_override_recolours_and_marks_media_stale() {
    let house = MediaStyle::default();
    // The house style's JSON (and so its hash and every render key) has no palette field.
    assert!(!serde_json::to_string(&house).unwrap().contains("palette"));
    assert_eq!(MediaStyle::with_palette(None).unwrap(), house);
    assert_eq!(
        house.hash(),
        HOUSE_STYLE_HASH,
        "the house style's hash changed"
    );
    let paper = MediaStyle::with_palette(Some("Paper")).unwrap();
    assert_ne!(paper.hash(), house.hash());
    assert_ne!(
        MediaStyle::with_palette(Some("Candy")).unwrap().hash(),
        paper.hash()
    );
    let p = jetcarp_core::render::Palette::builtin("Paper").unwrap();
    let bg = p.background_rgba();
    assert_eq!(paper.background, [bg[0], bg[1], bg[2]]);
    assert_eq!(paper.binary, vec![p.stops[0]]);
    assert_eq!(paper.palette.as_deref(), Some("Paper"));
    let e = MediaStyle::with_palette(Some("Nope")).unwrap_err();
    assert!(e.contains("Nope") && e.contains("Glacier"), "{e}");

    let lib = temp_lib("palette");
    let rec = one("B3/S23", GLIDER);
    lib.add(rec.clone()).unwrap();
    let t = MediaTarget::Pattern(rec.id.clone());
    let o = MediaOptions::default();
    let a = jetcarp_store::media::render(&lib, &t, &o).unwrap();
    assert!(a.written);
    let po = MediaOptions {
        style: paper.clone(),
        ..Default::default()
    };
    let b = jetcarp_store::media::render(&lib, &t, &po).unwrap();
    assert!(b.written, "another palette: rendered again");
    assert_ne!(b.rows[0].key, a.rows[0].key);
    assert_eq!(b.rows[0].render["palette"], "Paper");
    assert!(a.rows[0].render.get("palette").is_none());
    let poster =
        encode::decode_png(&std::fs::read(&b.file("poster").unwrap().abs_path).unwrap()).unwrap();
    assert_eq!(
        &poster.rgba[..3],
        &paper.background[..],
        "the corner is empty"
    );
    assert!(
        colour_box(&poster, p.stops[0]).is_some(),
        "live cells in the first stop"
    );
    assert!(colour_box(&poster, house.binary[0]).is_none());
    assert!(
        !jetcarp_store::media::render(&lib, &t, &po).unwrap().written,
        "same palette: skipped"
    );
    assert!(
        jetcarp_store::media::render(&lib, &t, &o).unwrap().written,
        "back to the house style"
    );
    // In memory: the same frames as the stored animation, no files or rows written.
    let anim = jetcarp_store::media::target_animation(&lib, &t, &po).unwrap();
    assert_eq!(anim.poster_image().rgba, poster.rgba);
    assert_eq!(
        jetcarp_store::media::stored(&lib, "pattern", &rec.id)
            .unwrap()
            .iter()
            .filter(|r| r.render.get("palette").is_some())
            .count(),
        0,
        "target_animation stores nothing"
    );
}

/// `MediaStyle::default().hash()` (pinned: a change re-renders every stored media file). 16 hex
/// digits of SHA-256 of the style's JSON.
const HOUSE_STYLE_HASH: &str = "86fdb8c4566d9a22";

/// Every file in the library's media folder, relative to it.
fn media_files(lib: &Library) -> Vec<String> {
    let root = lib.folder().join("media");
    let mut out = Vec::new();
    for d in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        for f in std::fs::read_dir(d.path()).into_iter().flatten().flatten() {
            out.push(format!(
                "{}/{}",
                d.file_name().to_string_lossy(),
                f.file_name().to_string_lossy()
            ));
        }
    }
    out.sort();
    out
}

#[test]
fn identical_renders_share_files_and_gc_keeps_what_is_named() {
    use jetcarp_store::media::{self as m, gc, remove};
    let lib = temp_lib("dedupe");
    // Two Quick Start entries showing the same scene, and a blinker whose two phases repeat.
    let mut sc = Scene::new(TilingSpec::rect("square", 24, 24, Boundary::Wrap));
    sc.cas = vec![CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.3 })];
    for id in ["twin-a", "twin-b"] {
        lib.put_quickstart(&QuickStartRow::for_scene(id, "scene", sc.clone()))
            .unwrap();
    }
    let blinker = one("B3/S23", "ooo");
    lib.add(blinker.clone()).unwrap();
    let o = MediaOptions {
        gens: Some(6),
        ..Default::default()
    };
    let a = m::render(&lib, &MediaTarget::QuickStart("twin-a".into()), &o).unwrap();
    let b = m::render(&lib, &MediaTarget::QuickStart("twin-b".into()), &o).unwrap();
    let hashes = |r: &jetcarp_store::media::Rendered| -> Vec<(String, String)> {
        r.rows
            .iter()
            .map(|x| (x.kind.clone(), x.hash.clone()))
            .collect()
    };
    assert_eq!(hashes(&a), hashes(&b), "the same render, the same files");
    // The soup's frames, their list, the animation, the poster and the thumbnail, once each
    // (a file that two of them share, such as the poster and the last frame, once too).
    let named: std::collections::HashSet<std::path::PathBuf> = a
        .rows
        .iter()
        .map(|x| x.abs_path.clone())
        .chain(a.file("frames").unwrap().frame_files.clone())
        .collect();
    assert_eq!(media_files(&lib).len(), named.len());
    // The blinker shown for two periods: four frames, two files.
    let bl = m::render(
        &lib,
        &MediaTarget::Pattern(blinker.id.clone()),
        &MediaOptions {
            periods: 2,
            ..Default::default()
        },
    )
    .unwrap();
    let fr = bl.file("frames").unwrap();
    assert_eq!(fr.frame_files.len(), 4);
    assert_eq!(fr.frame_files[0], fr.frame_files[2]);
    assert_eq!(fr.frame_files[1], fr.frame_files[3]);
    assert_ne!(fr.frame_files[0], fr.frame_files[1]);
    // Nothing to collect: every file is named.
    let all = media_files(&lib);
    let r = gc(&lib, false, 0).unwrap();
    assert_eq!((r.deleted, r.temp_files, r.missing), (0, 0, 0));
    assert_eq!(r.kept as usize, all.len());
    assert_eq!(media_files(&lib), all);
    // One twin goes: its rows only; the other still names every file, so gc keeps them all.
    lib.remove_quickstart("twin-a").unwrap();
    assert!(m::stored(&lib, "quickstart", "twin-a").unwrap().is_empty());
    let r = gc(&lib, false, 0).unwrap();
    assert_eq!(r.deleted, 0, "{:?}", r.files);
    assert_eq!(media_files(&lib), all);
    assert!(
        m::export(
            &lib,
            "quickstart",
            "twin-b",
            None,
            &lib.folder().join("out-b")
        )
        .is_ok()
    );
    // The other goes too: its files (not the blinker's) are no longer named. A dry run lists
    // them; files newer than the age limit are left; then they are deleted, with a leftover
    // temporary file.
    assert_eq!(remove(&lib, "quickstart", "twin-b", None).unwrap(), 4);
    let tmp = lib
        .folder()
        .join("media")
        .join("00")
        .join(".abc.png.1-1.tmp");
    std::fs::create_dir_all(tmp.parent().unwrap()).unwrap();
    std::fs::write(&tmp, b"half").unwrap();
    let dry = gc(&lib, true, 0).unwrap();
    assert!(dry.deleted > 0 && dry.dry_run);
    assert_eq!(
        media_files(&lib).len(),
        all.len() + 1,
        "a dry run deletes nothing"
    );
    let young = gc(&lib, false, 3600).unwrap();
    assert_eq!((young.deleted, young.temp_files), (0, 0));
    assert!(young.too_new > 0);
    let done = gc(&lib, false, 0).unwrap();
    assert_eq!(done.deleted, dry.deleted);
    assert_eq!(done.temp_files, 1);
    assert!(!tmp.exists());
    // The blinker's files are all still there, and the twins' are gone.
    for row in m::stored(&lib, "pattern", &blinker.id).unwrap() {
        assert!(row.abs_path.exists());
        assert!(row.frame_files.iter().all(|f| f.exists()));
    }
    assert!(!a.animation().unwrap().abs_path.exists());
    assert!(
        !lib.folder().join("media").join("00").exists(),
        "empty folders go"
    );
    // A readable copy.
    let out = lib.folder().join("readable");
    let files = m::export(&lib, "pattern", &blinker.id, Some(false), &out).unwrap();
    assert!(out.join("anim.gif").is_file() && out.join("poster.png").is_file());
    assert!(out.join("frames").join("0003.png").is_file());
    assert_eq!(files.len(), 4 + 3);
    assert!(m::export(&lib, "pattern", &blinker.id, Some(true), &out).is_err());
}
