//! Adapter choice and the self-test on this machine's adapters. Skipped (with a note) when there is
//! no usable adapter (failed under `JETCARP_REQUIRE_GPU=1`); `JETCARP_GPU=software` runs them on a
//! software adapter (CI's lavapipe).

use jetcarp_gpu::select::{self, Policy, Purpose, Want};

/// Windows' software renderer, which the GPU simulation never uses unless the tests allow it
/// (`JETCARP_ALLOW_WARP=1`, #321).
fn warp_refused(info: &wgpu::AdapterInfo) -> bool {
    info.name.contains("Microsoft Basic Render Driver") && !select::warp_allowed()
}

/// A software adapter the GPU simulation would use when asked (not WARP, unless allowed).
fn usable_software(all: &[wgpu::Adapter]) -> bool {
    all.iter().any(|a| {
        let i = a.get_info();
        i.device_type == wgpu::DeviceType::Cpu && !warp_refused(&i)
    })
}

fn adapters() -> Vec<wgpu::Adapter> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()))
}

#[test]
fn the_compute_choice_passes_the_self_test() {
    let policy = Policy::load();
    let c = match select::pick_from(&adapters(), &policy, Purpose::Compute, None) {
        Ok(c) => c,
        Err(e) => {
            jetcarp_gpu::no_gpu("the adapter tests", &e);
            return;
        }
    };
    let t = c.test.clone().expect("a compute choice runs the self-test");
    eprintln!(
        "{} ({}): {} in {} ms{}",
        jetcarp_gpu::describe(&c.info),
        c.why,
        t.note,
        t.millis,
        if t.cached { ", cached" } else { "" }
    );
    assert!(t.passed, "{}", t.note);
    assert!(c.usable.is_ok());
    if policy.want == Want::Software && usable_software(&adapters()) {
        assert_eq!(
            c.info.device_type,
            wgpu::DeviceType::Cpu,
            "asked for software"
        );
    } else if policy.want == Want::Software {
        // No software adapter it would use (macOS has none; Windows has only WARP): the best
        // adapter there is, saying so.
        assert!(c.why.contains("no software adapter"), "{}", c.why);
    } else {
        assert_ne!(
            c.info.device_type,
            wgpu::DeviceType::Cpu,
            "a software adapter only when asked for"
        );
    }
    // The second time comes from memory: the same result, still saying where it came from
    // (kept from before only when it was read from the settings folder).
    let again = select::self_test(&c.adapter);
    assert!(again.passed);
    assert_eq!((again.millis, again.cached), (t.millis, t.cached));
}

#[test]
fn a_display_always_gets_an_adapter_to_draw_with() {
    let all = adapters();
    if all.is_empty() {
        jetcarp_gpu::no_gpu("the display adapter test", &"no graphics adapter");
        return;
    }
    let policy = Policy::load();
    let c = select::pick_from(&all, &policy, Purpose::Display, None).unwrap();
    eprintln!("display: {} ({})", jetcarp_gpu::describe(&c.info), c.why);
    // Even with the GPU off, the adapter is chosen: only the simulation is left out.
    let off = Policy {
        mode: select::Mode::Off,
        ..policy
    };
    let off = select::pick_from(&all, &off, Purpose::Display, None).unwrap();
    assert_eq!(off.info, c.info);
}

#[test]
fn software_adapters_need_asking_for() {
    let all = adapters();
    let soft: Vec<_> = all
        .iter()
        .filter(|a| a.get_info().device_type == wgpu::DeviceType::Cpu)
        .collect();
    if soft.is_empty() {
        eprintln!("note: no software adapter here");
        return;
    }
    let auto = select::pick_from(&all, &Policy::from_env("auto"), Purpose::Compute, None);
    if let Ok(c) = &auto {
        assert_ne!(c.info.device_type, wgpu::DeviceType::Cpu);
    }
    if !usable_software(&all) {
        // Windows' WARP only (as on CI's Windows runners): never used, even when asked, and the
        // refusal says why when there is no hardware adapter either.
        if let Err(e) =
            select::pick_from(&all, &Policy::from_env("software"), Purpose::Compute, None)
        {
            assert!(e.contains("WARP"), "{e}");
        }
        return;
    }
    let c = select::pick_from(&all, &Policy::from_env("software"), Purpose::Compute, None)
        .expect("a software adapter when asked for");
    assert_eq!(c.info.device_type, wgpu::DeviceType::Cpu);
    assert!(c.why.contains("software"), "{}", c.why);
    let named = soft[0].get_info().name;
    let c = select::pick_from(&all, &Policy::from_env(&named), Purpose::Compute, None).unwrap();
    assert_eq!(c.info.name, named);
}

#[test]
fn a_missing_adapter_falls_back_with_a_note() {
    let all = adapters();
    let Ok(best) = select::pick_from(&all, &Policy::from_env("auto"), Purpose::Compute, None)
    else {
        // The automatic choice takes no software adapter, so this needs a hardware one, which
        // CI's lavapipe is not: a note, even under JETCARP_REQUIRE_GPU=1.
        eprintln!("note: skipping the fallback test: no usable hardware adapter");
        return;
    };
    let c = select::pick_from(
        &all,
        &Policy::from_env("NoSuchAdapter 9000"),
        Purpose::Compute,
        None,
    )
    .unwrap();
    assert_eq!(c.info, best.info);
    assert!(c.why.contains("no NoSuchAdapter 9000 adapter"), "{}", c.why);
}
