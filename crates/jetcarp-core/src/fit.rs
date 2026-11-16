//! Whether *Fit to this computer* is in this release: the one switch every front end checks
//! (#112).
//!
//! Fit sweeps each scene of the desktop background through a ladder of cell sizes, measures each,
//! and lets the user choose one (`jetcarp_display::tuning`, the host's `tune` and `core::sweep`,
//! the app's *Desktop background* pane). As built for 0.4.2's release candidates, every scene
//! through every size at about 2 s a size, a fit took 45 s to 12 minutes, against a target of 10 s
//! (30 at most). So 0.4.2 ships without it, and a fast fit returns in 0.4.3 (#113), built on the
//! same code. While [`FIT`] is false:
//!
//! - the app's *Desktop background* pane has no *Fit to this computer…*, limits or cards;
//! - the host applies no size an earlier Fit saved, so each scene shows at its own size, as in
//!   0.4.1, and leaves what was saved in `host.json` as it is, for 0.4.3; it sweeps nothing when
//!   the monitors change, refuses every sweep however it is asked, and says nothing of fitting in
//!   its status;
//! - `jetcarp host tune` is hidden from the help, and refuses;
//! - the MCP tool `host_control` has no `tune` action, and refuses one.

/// *Fit to this computer* is in this release.
pub const FIT: bool = false;

/// What every front end says when asked for Fit while it is switched off.
pub const HELD_BACK: &str = "Fit to this computer isn't in JETCARP 0.4.2: it returns, faster, in \
                             0.4.3. Each scene shows at its own size.";

/// Ok while Fit is in this release; else the refusal, [`HELD_BACK`].
pub fn available() -> Result<(), String> {
    if FIT {
        Ok(())
    } else {
        Err(HELD_BACK.to_string())
    }
}
