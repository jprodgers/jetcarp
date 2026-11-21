//! The benchmark behind the JETCARP Hardware Score on the GPU, as CI runs it: its answers only,
//! never its speed. The soups on the explore batch stepper and the worlds of the crossover on the
//! world stepper must give the stored answers, the same as the processor's. Skipped when no GPU
//! adapter is available, failed under `JETCARP_REQUIRE_GPU=1`.

use jetcarp_core::World;
use jetcarp_core::machine::bench::{self, Gpu, Options, WorldStepper};
use jetcarp_gpu::GpuSim;
use jetcarp_gpu::batch::BatchSim;

struct Sim(GpuSim);

impl WorldStepper for Sim {
    fn step(&mut self, w: &mut World, gens: u64) -> Result<(), String> {
        let m = self.0.step_mixed(w, gens)?;
        match m.fallback {
            Some(why) if m.cpu_gens > 0 => Err(format!("it stepped on the processor: {why}")),
            _ => Ok(()),
        }
    }
}

#[test]
fn the_benchmark_gives_its_answers_on_the_gpu() {
    let batch = match BatchSim::headless() {
        Ok(b) => b,
        Err(e) => return jetcarp_gpu::no_gpu("the benchmark on the GPU", &e),
    };
    let sim = match GpuSim::headless() {
        Ok(s) => s,
        Err(e) => return jetcarp_gpu::no_gpu("the benchmark on the GPU", &e),
    };
    let name = sim.name().to_string();
    eprintln!("benchmark answers on {name}");
    let gpus = vec![Gpu {
        name: name.clone(),
        batch: Some(Box::new(batch)),
        world: Some(Box::new(Sim(sim))),
    }];
    let opts = Options {
        quick: true,
        ..Default::default()
    };
    let s = bench::run(&opts, gpus, &|_, _| {}, &|| false).unwrap();
    assert!(s.problems.is_empty(), "{:?}", s.problems);
    let on_gpu: Vec<&str> = s
        .parts
        .iter()
        .filter(|p| p.device == name)
        .map(|p| p.id.as_str())
        .collect();
    assert_eq!(
        on_gpu,
        [
            "soups-hat",
            "soups-spectre",
            "soups-penrose-rhomb",
            "soups-rhombitrihex",
            "soups-k2-017",
            "gpu-crossover"
        ]
    );
    assert!(
        s.parts
            .iter()
            .filter(|p| p.device == name)
            .all(|p| p.correct == Some(true))
    );
}
