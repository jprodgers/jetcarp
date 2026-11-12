//! The setup wizard's state machine (`jetcarp_app::setup_ui`): its pages for each profile,
//! forwards and back, Later, the choices it hands `setup.apply`, the desktop offers (through a
//! stand-in desktop), and Blank slate applied and recorded. Every folder is a scratch folder.

use jetcarp_api::Ctx;
use jetcarp_api::ops::setup::{self, Profile};
use jetcarp_app::desktop_offers::Offer;
use jetcarp_app::setup_ui::{Desktop, MakeCtx, Page, Wizard};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A desktop that records what it was asked to do, with no screen saver list.
#[derive(Default, Clone)]
struct Mock(Arc<Mutex<Vec<Offer>>>);

impl Desktop for Mock {
    fn available(&self, o: Offer) -> Result<(), String> {
        if o == Offer::Saver {
            Err("no screen saver list here".into())
        } else {
            Ok(())
        }
    }
    fn accept(&mut self, o: Offer) -> Result<String, String> {
        self.0.lock().unwrap().push(o);
        Ok("done".into())
    }
}

fn scratch(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("setup-wizard")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A wizard on scratch folders (settings, library, output, no bundled packs), its status read.
fn wizard(dir: &Path, mock: Mock) -> Wizard {
    let d = dir.to_path_buf();
    let mk: MakeCtx = Arc::new(move || {
        Ctx::new()
            .with_settings_dir(Some(d.join("settings")))
            .with_library(Some(d.join("data/library.sqlite")))
            .with_out_root(Some(d.join("out")))
            .with_bundled_packs(Some(d.join("bundled")))
    });
    let mut w = Wizard::new(mk.clone(), Box::new(mock));
    w.set_status(setup::status(&mk()));
    w
}

#[test]
fn pages_follow_the_profile() {
    let d = scratch("pages");
    let mut w = wizard(&d, Mock::default());
    use Page::{Data, Desktop, Packs, Progress, Summary, Welcome};
    let pf = Page::Profile;
    assert_eq!(w.pages(), [Welcome, pf, Summary, Progress]);
    let want = [
        (
            Profile::DesktopArt,
            vec![Welcome, pf, Desktop, Summary, Progress],
        ),
        (
            Profile::Explorer,
            vec![Welcome, pf, Packs, Summary, Progress],
        ),
        (
            Profile::Researcher,
            vec![Welcome, pf, Data, Packs, Summary, Progress],
        ),
        (
            Profile::Everything,
            vec![Welcome, pf, Data, Packs, Desktop, Summary, Progress],
        ),
        (Profile::BlankSlate, vec![Welcome, pf, Summary, Progress]),
    ];
    for (p, pages) in want {
        w.choose(p);
        assert_eq!(w.pages(), pages, "{p:?}");
    }
}

#[test]
fn forward_back_and_later() {
    let d = scratch("walk");
    let mock = Mock::default();
    let mut w = wizard(&d, mock.clone());
    w.open = true;
    assert_eq!(w.page(), Page::Welcome);
    w.back();
    assert_eq!(w.page(), Page::Welcome, "nothing before the welcome");
    w.next();
    assert_eq!(w.page(), Page::Profile);
    // No profile chosen: no way on.
    assert!(!w.can_next());
    w.next();
    assert_eq!(w.page(), Page::Profile);
    w.choose(Profile::Everything);
    w.next();
    assert_eq!(w.page(), Page::Data);
    w.next();
    assert_eq!(w.page(), Page::Packs);
    w.next();
    assert_eq!(w.page(), Page::Desktop);
    w.back();
    w.back();
    assert_eq!(w.page(), Page::Data);
    // A profile with fewer pages: back from the summary skips what it does not ask.
    w.back();
    w.choose(Profile::Explorer);
    w.next();
    assert_eq!(w.page(), Page::Packs);
    w.next();
    assert_eq!(w.page(), Page::Summary);
    w.back();
    assert_eq!(w.page(), Page::Packs);
    // Later closes and records nothing.
    w.later();
    assert!(!w.open);
    assert!(w.applied().is_none());
    assert!(!d.join("settings").join(setup::RECORD_FILE).exists());
    assert!(mock.0.lock().unwrap().is_empty());
}

#[test]
fn the_choices_become_setup_apply_input() {
    let d = scratch("input");
    let mut w = wizard(&d, Mock::default());
    assert!(w.apply_input().is_none(), "no profile, nothing to apply");
    w.choose(Profile::Explorer);
    let a = w.apply_input().unwrap();
    assert_eq!(a.profile, "explorer");
    assert_eq!(a.quickstart_minutes, Some(10.0));
    assert!(a.background);
    assert_eq!(a.data_dir, None, "Explorer keeps the default folder");
    // The build is optional for Explorer; the minutes are adjustable.
    w.build_on = false;
    assert_eq!(w.apply_input().unwrap().quickstart_minutes, Some(0.0));
    w.build_on = true;
    w.minutes = 3.0;
    assert_eq!(w.apply_input().unwrap().quickstart_minutes, Some(3.0));
    // Researcher: a data folder of its own, checked as it is chosen.
    w.choose(Profile::Researcher);
    assert_eq!(w.minutes, 30.0);
    let big = d.join("big disk");
    w.data_dir = big.display().to_string();
    w.check_data_dir();
    assert!(w.data_check().1.is_some(), "free space shown");
    assert_eq!(w.apply_input().unwrap().data_dir, Some(w.data_dir.clone()));
    // A folder that is not empty and holds no library stops the page.
    let full = d.join("full");
    std::fs::create_dir_all(&full).unwrap();
    std::fs::write(full.join("holiday.jpg"), b"x").unwrap();
    w.data_dir = full.display().to_string();
    w.check_data_dir();
    w.go_to(Page::Data);
    assert!(!w.can_next());
    let (note, _) = w.data_check();
    assert!(note.unwrap().0.contains("not empty"), "{note:?}");
    w.data_dir = big.display().to_string();
    w.check_data_dir();
    assert!(w.can_next());
    // Blank slate: nothing to import or build.
    w.choose(Profile::BlankSlate);
    let a = w.apply_input().unwrap();
    assert!(w.would_run_nothing(&a));
}

#[test]
fn desktop_offers_are_off_until_ticked_and_only_ticked_ones_run() {
    let d = scratch("offers");
    let mock = Mock::default();
    let mut w = wizard(&d, mock.clone());
    w.choose(Profile::DesktopArt);
    assert!(w.offers_on().is_empty(), "nothing on by default");
    w.offers.insert("wallpaper", true);
    // The saver is not available on the stand-in: ticking it does nothing.
    w.offers.insert("saver", true);
    assert_eq!(w.offers_on(), [Offer::Wallpaper]);
    w.apply();
    assert_eq!(*mock.0.lock().unwrap(), [Offer::Wallpaper]);
    assert_eq!(w.page(), Page::Progress);
    // Profiles without offers never run them.
    let mock2 = Mock::default();
    let mut w2 = wizard(&scratch("offers2"), mock2.clone());
    w2.choose(Profile::DesktopArt);
    w2.offers.insert("wallpaper", true);
    w2.choose(Profile::Explorer);
    assert!(w2.offers_on().is_empty());
    assert!(mock2.0.lock().unwrap().is_empty());
}

#[test]
fn applying_blank_slate_records_it_and_shows_it_done() {
    let d = scratch("blank");
    let mut w = wizard(&d, Mock::default());
    w.open = true;
    w.next();
    w.choose(Profile::BlankSlate);
    w.next();
    assert_eq!(w.page(), Page::Summary);
    w.next();
    assert_eq!(w.page(), Page::Progress);
    let t = std::time::Instant::now();
    while w.applied().is_none() && t.elapsed().as_secs() < 30 {
        w.poll(0.0);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let a = w.applied().expect("applied");
    assert_eq!(a.profile, "blank-slate");
    assert!(a.jobs.is_empty());
    assert!(w.settled());
    let rec = setup::record(&d.join("settings")).unwrap();
    assert_eq!(rec.profile, "blank-slate");
    // Back is gone on the progress page.
    w.back();
    assert_eq!(w.page(), Page::Progress);
}
