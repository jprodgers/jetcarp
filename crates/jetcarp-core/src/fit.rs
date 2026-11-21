//! Whether *Fit to this computer* is in this release: the one switch every front end checks
//! (#112).
//!
//! Fit finds the finest size each desktop background scene can show at within the processor and
//! GPU limits (`jetcarp_display::tuning`, the host's `tune` and `core::sweep`, the app's *Desktop
//! background* pane). As built for 0.4.2's release candidates, every scene through every size at
//! about 2 s a size, a fit took 45 s to 12 minutes, so 0.4.2 shipped without it. 0.4.3 brings it
//! back fast (#113): one scene a screen, every screen at once, a binary search of about five
//! sizes of a second each, about ten seconds in all. While [`FIT`] is false:
//!
//! - the app's *Desktop background* pane has no *Fit to this computer…*, limits or cards;
//! - the host applies no size an earlier Fit saved, so each scene shows at its own size, and
//!   leaves what was saved in `host.json` as it is; it fits nothing when the monitors change,
//!   refuses every fit however it is asked, and says nothing of fitting in its status;
//! - `jetcarp host tune` is hidden from the help, and refuses;
//! - the MCP tool `host_control` has no `tune` action, and refuses one.

/// *Fit to this computer* is in this release.
pub const FIT: bool = true;

/// What every front end says when asked for Fit while it is switched off.
pub const HELD_BACK: &str = "Fit to this computer isn't in this release of JETCARP. Each scene \
                             shows at its own size.";

/// Ok while Fit is in this release; else the refusal, [`HELD_BACK`].
pub fn available() -> Result<(), String> {
    if FIT {
        Ok(())
    } else {
        Err(HELD_BACK.to_string())
    }
}
