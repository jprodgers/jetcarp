//! A sweep card's picture is the screen's own pixels: a checkerboard of cells `px` pixels across
//! comes out as blocks of exactly `px` pixels.

use jetcarp_core::{Boundary, CaSpec, Scene, SeedSpec, TilingSpec};
use jetcarp_display::crop;
use jetcarp_display::host_settings::{Size, TunedScene, Tuning};
use jetcarp_display::painter::Painter;
use jetcarp_display::profiles::{Profile, Show};
use jetcarp_display::rotation::{Rotation, Slot};
use jetcarp_display::simgpu::SimBackend;
use jetcarp_display::stage::{Stage, no_lookup};
use jetcarp_display::tuning::crop_size;
use std::time::{Duration, Instant};

fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let choice =
        jetcarp_gpu::select::pick(&instance, jetcarp_gpu::select::Purpose::Display, None).ok()?;
    if let Err(e) = &choice.usable {
        eprintln!("skipped: {e}");
        return None;
    }
    pollster::block_on(
        choice
            .adapter
            .request_device(&wgpu::DeviceDescriptor::default()),
    )
    .ok()
}

/// A stage showing a still checkerboard on a `w` by `h` square grid, built at `px` pixels a cell
/// (its own size is 8; a tuning chooses `px`).
fn checkerboard(w: u32, h: u32, px: f32) -> Stage {
    let mut sc = Scene::new(TilingSpec::rect("square", w, h, Boundary::Wrap));
    let cells: Vec<[u32; 2]> = (0..w * h)
        .filter(|i| (i % w + i / w) % 2 == 0)
        .map(|i| [i, 1])
        .collect();
    // Nothing is born and everything survives: the board stays as it is.
    sc.cas = vec![CaSpec::new("B/S012345678").seed(SeedSpec::Cells { cells })];
    let profile = Profile {
        name: "board".into(),
        show: Show::Scene {
            scene: Box::new(sc),
        },
        cell_px: 8.0,
        refill: true,
        speed: 1.0,
        gpu: false,
        ..Default::default()
    };
    let slot = Slot {
        label: "board".into(),
        profile,
        stored: false,
        end: vec![],
        hold_s: 0.0,
        schedule: None,
        n: None,
    };
    let rotation = Rotation {
        name: "test".into(),
        ..Default::default()
    };
    let canvas = [(w as f32 * px) as f64, (h as f32 * px) as f64];
    let mut stage = Stage::new(&rotation, vec![slot], 1, canvas, no_lookup(), 1 << 24).unwrap();
    stage.set_tuned(Some(Tuning {
        rotation: "test".into(),
        canvas: [canvas[0] as u32, canvas[1] as u32],
        scenes: vec![TunedScene {
            scene: "board".into(),
            design_px: 8.0,
            cell_px: px,
            ..Default::default()
        }],
        ..Default::default()
    }));
    let mut sim = SimBackend::unavailable("a test".into());
    let t0 = Instant::now();
    while stage.world().is_none() {
        assert!(t0.elapsed() < Duration::from_secs(20), "never built");
        stage.tick(
            t0.elapsed().as_secs_f64(),
            &mut sim,
            Duration::from_millis(5),
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    stage
}

#[test]
fn a_card_s_picture_has_the_screen_s_pixels() {
    let Some((device, queue)) = gpu() else {
        return;
    };
    let mut painter = Painter::new(&device, crop::FORMAT);
    for px in [1.0f32, 2.0, 4.0] {
        // A board 96 by 60 cells, so the whole canvas is a whole number of cells.
        let stage = checkerboard(96, 60, px);
        let st = stage.status();
        assert_eq!(st.cell_px, px, "built at {px} px");
        assert_eq!(st.grid, Some([96, 60]));
        let canvas = [(96.0 * px) as f64, (60.0 * px) as f64];
        let size = crop_size(canvas);
        let img = crop::render(&device, &queue, &mut painter, &stage, size, 1.0).unwrap();
        assert_eq!([img.width, img.height], size);
        let at = |x: u32, y: u32| {
            let i = ((y * img.width + x) * 4) as usize;
            [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
        };
        // Along a row and down a column the colour changes every `px` pixels, never between.
        let n = px as u32;
        let (mut changes, mut row) = (0, 0);
        for x in 1..img.width {
            if at(x, 0) != at(x - 1, 0) {
                assert_eq!(x % n, 0, "a change inside a cell at x {x}, {px} px");
                changes += 1;
            }
        }
        for y in 1..img.height {
            if at(0, y) != at(0, y - 1) {
                assert_eq!(y % n, 0, "a change inside a cell at y {y}, {px} px");
                row += 1;
            }
        }
        assert_eq!(
            changes,
            (img.width - 1) / n,
            "every cell boundary along the row"
        );
        assert_eq!(
            row,
            (img.height - 1) / n,
            "every cell boundary down the column"
        );
        assert_ne!(at(0, 0), at(n, 0), "two colours");
        let _ = Size::px(px);
    }
}
