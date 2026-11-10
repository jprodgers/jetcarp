//! Debug aid: `JETCARP_FRAMETIME=1` records frame times while a scene runs and prints a summary when
//! the app closes (with `JETCARP_SCREENSHOT`, when the screenshot is taken). Used to measure the
//! app on large patches: frame interval, time spent stepping, and time spent colouring cells.

use std::time::Instant;

#[derive(Default)]
pub struct FrameStats {
    last: Option<Instant>,
    start: Option<(Instant, u64)>,
    end: Option<(Instant, u64)>,
    frame: Vec<f64>,
    step: Vec<f64>,
    color: Vec<f64>,
    colored: Vec<usize>,
    /// Accumulated this frame.
    cur_step: f64,
    cur_color: f64,
    cur_colored: usize,
}

impl FrameStats {
    pub fn from_env() -> Option<FrameStats> {
        std::env::var("JETCARP_FRAMETIME")
            .ok()
            .map(|_| FrameStats::default())
    }

    /// Call once per frame at its start, with the world's generation.
    pub fn frame(&mut self, generation: u64) {
        let now = Instant::now();
        if let Some(t) = self.last {
            self.frame.push((now - t).as_secs_f64() * 1e3);
            self.step.push(self.cur_step);
            self.color.push(self.cur_color);
            self.colored.push(self.cur_colored);
        }
        if self.start.is_none() {
            self.start = Some((now, generation));
        }
        self.end = Some((now, generation));
        self.last = Some(now);
        self.cur_step = 0.0;
        self.cur_color = 0.0;
        self.cur_colored = 0;
    }

    /// Forget frames so far (a build finished; measure the running scene only).
    pub fn restart(&mut self) {
        *self = FrameStats::default();
    }

    pub fn add_step(&mut self, ms: f64) {
        self.cur_step += ms;
    }

    pub fn add_color(&mut self, ms: f64, cells: usize) {
        self.cur_color += ms;
        self.cur_colored += cells;
    }

    pub fn report(&self) -> String {
        let n = self.frame.len();
        if n == 0 {
            return "frametime: no frames".into();
        }
        let med = |v: &[f64]| {
            let mut s = v.to_vec();
            s.sort_by(|a, b| a.total_cmp(b));
            s[s.len() / 2]
        };
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        let (gens, secs) = match (self.start, self.end) {
            (Some((t0, g0)), Some((t1, g1))) => (g1 - g0, (t1 - t0).as_secs_f64()),
            _ => (0, 0.0),
        };
        let colored = self.colored.iter().sum::<usize>() as f64 / n as f64;
        format!(
            "frametime: {n} frames, frame median {:.1} ms mean {:.1} ms ({:.1} fps); step mean {:.1} ms; \
             colour mean {:.1} ms ({:.0} cells per frame); {gens} generations in {secs:.1} s ({:.1} gens/s)",
            med(&self.frame),
            mean(&self.frame),
            1e3 / mean(&self.frame),
            mean(&self.step),
            mean(&self.color),
            colored,
            gens as f64 / secs.max(1e-9),
        )
    }
}
