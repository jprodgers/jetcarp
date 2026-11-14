//! Adapter choice and the self-test on this machine's adapters. Skipped (with a note) when there
//! is no usable adapter; `JETCARP_GPU=software` runs them on a software adapter (CI's lavapipe).

use jetcarp_gpu::select::{self, Policy, Purpose, Want};

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
            eprintln!("note: skipping the adapter tests: {e}");
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
    if policy.want == Want::Software {
        assert_eq!(
            c.info.device_type,
            wgpu::DeviceType::Cpu,
            "asked for software"
        );
    } else {
        assert_ne!(
            c.info.device_type,
            wgpu::DeviceType::Cpu,
            "a software adapter only when asked for"
        );
    }
    // The second time comes from the cache.
    let again = select::self_test(&c.adapter);
    assert!(again.cached && again.passed);
}

#[test]
fn a_display_always_gets_an_adapter_to_draw_with() {
    let all = adapters();
    if all.is_empty() {
        eprintln!("note: no graphics adapter");
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
        eprintln!("note: no usable adapter");
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
