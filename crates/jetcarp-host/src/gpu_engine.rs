//! Windows' count of this process's GPU time, for *Fit to this computer* on an adapter without
//! GPU timestamps (#72). On Linux the kernel counts each program's GPU time
//! ([`crate::power::process_gpu_time`]); Windows keeps per-process GPU engine counters instead, the
//! "GPU Engine" performance counters that Task Manager's GPU column shows. There is one instance
//! per process, adapter and engine, named like
//! `pid_1234_luid_0x00000000_0x0001195E_phys_0_eng_0_engtype_3D`, and each has a `Utilization
//! Percentage`. This module reads them through PDH (the Performance Data Helper, `pdh.dll`) and
//! sums this process's 3D engines: wgpu hands drawing and compute alike to one direct queue, which
//! runs on the 3D engine, so that is where JETCARP's GPU work shows.
//!
//! `Utilization Percentage` is a rate, which PDH works out between two collections of a query, so
//! a meter is sampled at the start of what it measures ([`EngineMeter::start`], or a
//! [`EngineMeter::share`] whose answer is not needed) and read at the end. Collecting the GPU
//! Engine set lists every process's engines and can take a while, so the tuners collect outside
//! the frames they time, before their processor meter starts and after it stops, so PDH's own work
//! is not counted as JETCARP's.
//!
//! The counter is added with a whole-instance wildcard, `\GPU Engine(*)\Utilization Percentage`,
//! the form `PdhAddCounter` documents for every instance, and this process's 3D engines are picked
//! out here ([`share_of`]), where the matching can be tested on any platform. (Narrowing the
//! wildcard to `pid_1234_*` would not save PDH any work: the counters' provider returns every
//! instance either way.)
//!
//! Where the counters can't be read (counters turned off, a Windows without them, or no 3D engine
//! of this process's yet), [`EngineMeter::start`] or [`EngineMeter::share`] gives None, and the
//! tuner falls back to timing each frame's submission, which it does not judge. Off Windows
//! [`EngineMeter::start`] always gives None.

// The counter, every instance of it, and the PDH query: core's, which reads the GPU's whole
// load with them as JETCARP measures this computer.
#[cfg_attr(not(windows), allow(unused_imports))]
use jetcarp_core::machine::gpu_load::COUNTER_PATH;
#[cfg(windows)]
use jetcarp_core::machine::gpu_load::pdh;

/// This process's share of its GPU engines' time, from Windows' GPU engine counters.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct EngineMeter {
    #[cfg(windows)]
    query: pdh::Query,
    #[cfg(windows)]
    pid: u32,
}

impl EngineMeter {
    /// Opens the counters and takes their first sample. None where they can't be read, and
    /// always off Windows.
    pub(crate) fn start() -> Option<EngineMeter> {
        #[cfg(windows)]
        {
            let query = pdh::Query::open(COUNTER_PATH)?;
            if !query.collect() {
                return None;
            }
            Some(EngineMeter {
                query,
                pid: std::process::id(),
            })
        }
        #[cfg(not(windows))]
        None
    }

    /// This process's share of its 3D engines' time (0 to 1) since the last sample, taking a new
    /// one. None when the counters can't be read, or name no 3D engine of this process's.
    pub(crate) fn share(&mut self) -> Option<f64> {
        #[cfg(windows)]
        {
            if !self.query.collect() {
                return None;
            }
            let values = self.query.values()?;
            share_of(values.iter().map(|(n, v)| (n.as_str(), *v)), self.pid)
        }
        #[cfg(not(windows))]
        None
    }
}

/// The process id and the engine type an instance's name gives: `pid_1234_luid_…_engtype_3D` is
/// process 1234's `3D` engine. None for a name not of that form.
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_instance(name: &str) -> Option<(u32, &str)> {
    jetcarp_core::machine::gpu_load::parse_instance(name).map(|(pid, _, engine)| (pid, engine))
}

/// Process `pid`'s share of its 3D engines' time (0 to 1) from the counters' instances, each
/// `(name, utilization percentage)`: the sum of its 3D engines (one an adapter it uses), at most
/// the whole. None when no instance is one of its 3D engines, so a meter that sees nothing of this
/// process's is not mistaken for an idle GPU.
#[cfg_attr(not(windows), allow(dead_code))]
fn share_of<'a>(instances: impl IntoIterator<Item = (&'a str, f64)>, pid: u32) -> Option<f64> {
    let mut sum = None;
    for (name, percent) in instances {
        // Instance names are as the driver gives them; Task Manager's tools lower-case them.
        let ours = parse_instance(name)
            .is_some_and(|(p, engine)| p == pid && engine.eq_ignore_ascii_case("3D"));
        if ours && percent.is_finite() {
            *sum.get_or_insert(0.0) += percent.max(0.0);
        }
    }
    sum.map(|s: f64| (s / 100.0).min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_counter_is_every_gpu_engine_s_utilization() {
        assert_eq!(COUNTER_PATH, r"\GPU Engine(*)\Utilization Percentage");
    }

    #[test]
    fn instance_names_give_their_process_and_engine() {
        assert_eq!(
            parse_instance("pid_53616_luid_0x00000000_0x0001195E_phys_0_eng_0_engtype_3D"),
            Some((53616, "3D"))
        );
        assert_eq!(
            parse_instance("pid_7_luid_0x00000000_0x00011C92_phys_0_eng_3_engtype_VideoDecode"),
            Some((7, "VideoDecode"))
        );
        // As PowerShell's Get-Counter shows them.
        assert_eq!(
            parse_instance("pid_7_luid_0x00000000_0x0001195e_phys_0_eng_0_engtype_3d"),
            Some((7, "3d"))
        );
        assert_eq!(parse_instance("_Total"), None);
        assert_eq!(parse_instance("pid_x_luid_0_engtype_3D"), None);
        assert_eq!(parse_instance("pid_7_luid_0x0_phys_0_eng_0"), None);
    }

    /// The Acer's host on its Intel UHD (pid 53616): its 3D engine, beside its copy engine, the
    /// same engine of another process whose id starts the same, and the app's (#72).
    #[test]
    fn a_process_s_share_is_the_sum_of_its_3d_engines() {
        let uhd = "luid_0x00000000_0x0001195E_phys_0";
        let rtx = "luid_0x00000000_0x00011C92_phys_0";
        let names = [
            format!("pid_53616_{uhd}_eng_0_engtype_3D"),
            format!("pid_53616_{uhd}_eng_1_engtype_Copy"),
            format!("pid_5361_{uhd}_eng_0_engtype_3D"),
            format!("pid_536160_{uhd}_eng_0_engtype_3D"),
            format!("pid_53616_{rtx}_eng_0_engtype_3D"),
        ];
        let values = [19.5, 40.0, 70.0, 80.0, 0.5];
        let all = || names.iter().map(String::as_str).zip(values);
        let share = share_of(all(), 53616).unwrap();
        assert!((share - 0.20).abs() < 1e-9, "{share}");
        assert_eq!(share_of(all(), 5361), Some(0.70));
        assert_eq!(share_of(all(), 42), None, "no engine of its own");
        // Idle, but counted: zero, not unknown.
        assert_eq!(share_of([(names[0].as_str(), 0.0)], 53616), Some(0.0));
        // At most the whole, and nothing from a value that isn't one.
        let busy = [(names[0].as_str(), 100.0), (names[4].as_str(), 60.0)];
        assert_eq!(share_of(busy, 53616), Some(1.0));
        assert_eq!(share_of([(names[0].as_str(), f64::NAN)], 53616), None);
    }

    /// On Windows the counters are read without error, whatever the machine has (a runner with no
    /// GPU may have no counters, or no engine of this process's, so no share); elsewhere there are
    /// none.
    #[test]
    fn reading_the_counters_is_not_an_error() {
        if let Some(mut m) = EngineMeter::start() {
            let _ = m.share();
            std::thread::sleep(std::time::Duration::from_millis(50));
            if let Some(s) = m.share() {
                assert!((0.0..=1.0).contains(&s), "{s}");
            }
        }
        #[cfg(windows)]
        if let Some(q) = pdh::Query::open(COUNTER_PATH)
            && q.collect()
            && q.collect()
            && let Some(values) = q.values()
        {
            // Every instance's name and value, over two samples (none, with no GPU engines).
            assert!(values.iter().all(|(n, v)| !n.is_empty() && v.is_finite()));
        }
        if cfg!(not(windows)) {
            assert!(EngineMeter::start().is_none());
        }
    }
}
