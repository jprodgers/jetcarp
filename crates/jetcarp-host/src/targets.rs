//! Which surfaces the host's screens need.

/// One surface: a screen, shown somewhere.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Target {
    pub screen: usize,
    pub wallpaper: bool,
    pub monitor: Option<usize>,
}

/// One surface per screen. `screens` holds each screen's monitor and whether it is a wallpaper.
pub fn one_each(screens: &[(Option<usize>, bool)]) -> Vec<Target> {
    screens
        .iter()
        .enumerate()
        .map(|(screen, &(monitor, wallpaper))| Target {
            screen,
            wallpaper,
            monitor,
        })
        .collect()
}

/// Where the system has no compositor to copy a window to other monitors, as on Windows: a
/// wallpaper with no monitor gets a surface on every monitor no other screen claims, the primary
/// first, since the first surface sets the scene's size.
pub fn per_monitor(
    screens: &[(Option<usize>, bool)],
    monitors: usize,
    primary: usize,
) -> Vec<Target> {
    let claimed: Vec<usize> = screens
        .iter()
        .filter(|(_, wallpaper)| *wallpaper)
        .filter_map(|(m, _)| *m)
        .collect();
    let mut order: Vec<usize> = (0..monitors).filter(|m| !claimed.contains(m)).collect();
    order.sort_by_key(|m| *m != primary);
    // Nothing else will show on the primary, so a screen whose monitor has gone can go there.
    let shared = screens
        .iter()
        .any(|&(m, wallpaper)| wallpaper && m.is_none());
    let mut primary_free = !claimed.contains(&primary) && !shared;
    let mut out = vec![];
    let mut filled = false;
    for (screen, &(monitor, wallpaper)) in screens.iter().enumerate() {
        match monitor {
            _ if !wallpaper => out.push(Target {
                screen,
                wallpaper,
                monitor,
            }),
            Some(m) if m < monitors => out.push(Target {
                screen,
                wallpaper,
                monitor: Some(m),
            }),
            // A monitor that has gone: the scene moves to the primary rather than vanishing, unless
            // another screen shows there. Then it waits, unseen, for its monitor to return.
            Some(_) if primary_free => {
                primary_free = false;
                out.push(Target {
                    screen,
                    wallpaper,
                    monitor: Some(primary),
                });
            }
            Some(_) => {}
            None if !filled => {
                filled = true;
                out.extend(order.iter().map(|&m| Target {
                    screen,
                    wallpaper,
                    monitor: Some(m),
                }));
            }
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitors_of(t: &[Target], screen: usize) -> Vec<Option<usize>> {
        t.iter()
            .filter(|x| x.screen == screen)
            .map(|x| x.monitor)
            .collect()
    }

    #[test]
    fn a_shared_wallpaper_fills_every_monitor_primary_first() {
        let t = per_monitor(&[(None, true)], 3, 1);
        assert_eq!(monitors_of(&t, 0), [Some(1), Some(0), Some(2)]);
    }

    #[test]
    fn a_claimed_monitor_is_left_to_its_screen() {
        let t = per_monitor(&[(None, true), (Some(1), true)], 2, 0);
        assert_eq!(monitors_of(&t, 0), [Some(0)]);
        assert_eq!(monitors_of(&t, 1), [Some(1)]);
    }

    #[test]
    fn a_missing_monitor_falls_back_and_windows_stay_single() {
        let t = per_monitor(&[(Some(5), true), (None, false)], 1, 0);
        assert_eq!(monitors_of(&t, 0), [Some(0)]);
        assert_eq!(monitors_of(&t, 1), [None]);
        assert!(!t[1].wallpaper);
    }

    #[test]
    fn a_missing_monitor_does_not_cover_a_screen_still_there() {
        // Two screens on two monitors; monitor 1 is unplugged.
        let t = per_monitor(&[(Some(0), true), (Some(1), true)], 1, 0);
        assert_eq!(monitors_of(&t, 0), [Some(0)]);
        assert_eq!(monitors_of(&t, 1), []);
        // Two screens whose monitors have both gone: only one takes the primary.
        let t = per_monitor(&[(Some(3), true), (Some(4), true)], 1, 0);
        assert_eq!(monitors_of(&t, 0), [Some(0)]);
        assert_eq!(monitors_of(&t, 1), []);
        // A shared wallpaper already fills the primary.
        let t = per_monitor(&[(None, true), (Some(2), true)], 1, 0);
        assert_eq!(monitors_of(&t, 0), [Some(0)]);
        assert_eq!(monitors_of(&t, 1), []);
    }

    #[test]
    fn one_each_keeps_the_order() {
        let t = one_each(&[(Some(1), true), (None, false)]);
        assert_eq!(t.len(), 2);
        assert_eq!((t[1].screen, t[1].wallpaper), (1, false));
    }
}
