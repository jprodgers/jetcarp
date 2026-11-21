//! The setup wizard: the first window a new user sees. It opens on the first launch (when setup
//! has never run), can be put off (**Later**) or answered with *Blank slate*, and opens again
//! from Help → Setup….
//!
//! Its pages, in order, each only when the chosen profile needs it ([`Wizard::pages`]):
//!
//! 1. **Welcome**: what JETCARP is.
//! 2. **Profile**: the five profiles of `setup.apply`, each with what it takes here.
//! 3. **Data folder** (Researcher, Everything): where the library goes, with the free space.
//! 4. **Packs** (profiles that import packs): the packs that came with JETCARP and what is in
//!    them, and (Researcher, Everything) packs from a folder.
//! 5. **Desktop** (Desktop art, Everything): the live wallpaper, login and the screen saver,
//!    all off until turned on.
//! 6. **Summary**, with a box to add JETCARP to the system's apps (ticked when it isn't there
//!    yet, for every profile), then **Progress**: the jobs `setup.apply` started, followed until
//!    they end, with *Continue in the background*.
//!
//! Every catalog and file read runs on a worker thread (`setup.status`, `setup.apply`, the job
//! polls); the UI thread only checks a folder the user typed or picked. What the desktop offers
//! do goes through [`Desktop`], which tests replace.

use crate::desktop_offers::Offer;
use eframe::egui::{self, Color32, RichText};
use jetcarp_api::Ctx;
use jetcarp_api::op::{Op, Quiet};
use jetcarp_api::ops::setup::{
    self, Applied, ApplyIn, DataDirIn, KeySeen, PackEntry, Profile, SetupJob, SetupStatus,
};
use jetcarp_api::ops::system::human_bytes;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

/// The window's key (`crate::popout`).
pub const KEY: &str = "setup";

/// A page of the wizard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Welcome,
    Profile,
    Data,
    Packs,
    Desktop,
    Measure,
    Summary,
    Progress,
}

impl Page {
    fn title(self) -> &'static str {
        match self {
            Page::Welcome => "Welcome",
            Page::Profile => "What for",
            Page::Data => "Data folder",
            Page::Packs => "Packs",
            Page::Desktop => "Desktop",
            Page::Measure => "This computer",
            Page::Summary => "Summary",
            Page::Progress => "Setting up",
        }
    }
}

/// What the desktop offers do: the real desktop ([`RealDesktop`]), or a stand-in in tests.
pub trait Desktop: Send {
    /// It can be offered here; else why not.
    fn available(&self, o: Offer) -> Result<(), String>;
    /// Turn it on.
    fn accept(&mut self, o: Offer) -> Result<String, String>;
    /// The wallpaper, just turned on, shows only after the user logs out and back in once (GNOME
    /// loads JETCARP's extension at login).
    fn wallpaper_waits_for_login(&self) -> bool {
        false
    }
    /// JETCARP is among the system's apps already ([`Offer::Launcher`]).
    fn launcher_installed(&self) -> bool;
}

/// This computer's desktop ([`crate::desktop_offers`]).
pub struct RealDesktop;

impl Desktop for RealDesktop {
    fn available(&self, o: Offer) -> Result<(), String> {
        o.available()
    }
    fn accept(&mut self, o: Offer) -> Result<String, String> {
        o.accept()
    }
    fn launcher_installed(&self) -> bool {
        jetcarp_display::launcher::installed()
    }
    fn wallpaper_waits_for_login(&self) -> bool {
        cfg!(target_os = "linux")
            && jetcarp_display::gnome::state() == jetcarp_display::gnome::ExtensionState::NeedsLogin
    }
}

/// Whether setup ends by saying to log out once: the wallpaper was turned on, and waits for a
/// login to show (asked only then, since asking runs `gdbus`).
fn wants_log_out(done: &[(Offer, Result<String, String>)], waits: impl FnOnce() -> bool) -> bool {
    done.iter()
        .any(|(o, r)| *o == Offer::Wallpaper && r.is_ok())
        && waits()
}

/// Said plainly at the end of setup, when the wallpaper waits for a login (#139).
pub const LOG_OUT_ONCE: &str = "Log out and back in once to see the live wallpaper on your desktop. \
     Until then it shows in a window.";

/// The measuring's progress, for the Progress page.
struct Measuring(Arc<std::sync::Mutex<Option<(f32, String)>>>);

impl jetcarp_api::op::Progress for Measuring {
    fn report(&self, done: u64, total: Option<u64>, msg: &str) {
        if let Ok(mut m) = self.0.lock() {
            let f = total.map_or(0.0, |t| done as f32 / t.max(1) as f32);
            *m = Some((f, msg.to_string()));
        }
    }
}

/// How the wizard makes the operations' context (the app's platform, or a test's folders).
pub type MakeCtx = Arc<dyn Fn() -> Ctx + Send + Sync>;

/// The setup wizard's state.
pub struct Wizard {
    pub open: bool,
    page: Page,
    /// The page whose main button was given the keyboard (#219): as each page shows, its main
    /// button (Next, Start, Close) has it, so Enter goes on and Esc closes the window.
    keyed: Option<Page>,
    pub profile: Option<Profile>,
    /// The data folder chosen (Researcher, Everything), as typed or picked.
    pub data_dir: String,
    /// The folder's check: what it is, and whether that stops setup.
    data_note: Option<(String, bool)>,
    data_free: Option<u64>,
    /// Bundled packs (by id) to import.
    pub bundled_on: BTreeMap<String, bool>,
    /// Folders of packs chosen, and the packs found in them.
    pub extra: Vec<(PathBuf, Vec<PackEntry>)>,
    pub build_on: bool,
    pub minutes: f64,
    /// Measure this computer first (the JETCARP Hardware Score, about a minute): ticked by
    /// default.
    pub measure_on: bool,
    /// What the measuring says while it runs.
    measuring: Arc<std::sync::Mutex<Option<(f32, String)>>>,
    pub offers: BTreeMap<&'static str, bool>,
    /// Add JETCARP to the system's apps (`jetcarp launcher install`), for every profile: ticked
    /// when it isn't there yet.
    pub launcher_on: bool,
    /// JETCARP was among the system's apps when the window last looked: then there is no box to
    /// tick.
    pub launcher_there: bool,
    status: Option<SetupStatus>,
    loading: Option<Receiver<SetupStatus>>,
    applying: Option<Receiver<Result<Applied, String>>>,
    applied: Option<Applied>,
    error: Option<String>,
    jobs: Vec<SetupJob>,
    polling: Option<Receiver<Vec<SetupJob>>>,
    last_poll: f64,
    /// What the desktop offers did.
    pub desktop_done: Vec<(Offer, Result<String, String>)>,
    /// The wallpaper was turned on and shows only after a logout (#139).
    pub log_out_once: bool,
    desktop: Box<dyn Desktop>,
    make_ctx: MakeCtx,
    /// The setup being applied chooses a new data folder.
    data_changing: bool,
    /// The data folder changed: the app opens its library-bound windows again.
    data_changed: bool,
}

fn offer_key(o: Offer) -> &'static str {
    match o {
        Offer::Wallpaper => "wallpaper",
        Offer::Login => "login",
        Offer::Saver => "saver",
        Offer::Launcher => "launcher",
    }
}

impl Wizard {
    /// A closed wizard whose operations run with `make_ctx`'s context and whose desktop offers
    /// go to `desktop`.
    pub fn new(make_ctx: MakeCtx, desktop: Box<dyn Desktop>) -> Wizard {
        let launcher_there = desktop.launcher_installed();
        Wizard {
            open: false,
            page: Page::Welcome,
            keyed: None,
            profile: None,
            data_dir: String::new(),
            data_note: None,
            data_free: None,
            bundled_on: BTreeMap::new(),
            extra: vec![],
            build_on: false,
            minutes: 0.0,
            measure_on: true,
            measuring: Default::default(),
            offers: Offer::ALL.iter().map(|o| (offer_key(*o), false)).collect(),
            launcher_on: !launcher_there,
            launcher_there,
            status: None,
            loading: None,
            applying: None,
            applied: None,
            error: None,
            jobs: vec![],
            polling: None,
            last_poll: 0.0,
            desktop_done: vec![],
            log_out_once: false,
            desktop,
            make_ctx,
            data_changing: false,
            data_changed: false,
        }
    }

    /// The app's wizard: the app's platform (the GPU for the build), this computer's desktop.
    pub fn for_app() -> Wizard {
        Wizard::new(
            Arc::new(|| Ctx::new().with_platform(Arc::new(crate::window_job::AppPlatform))),
            Box::new(RealDesktop),
        )
    }

    /// Setup has never run in this settings folder (one small file read).
    pub fn first_run() -> bool {
        !setup::has_run(&Ctx::new())
    }

    /// Open it: on the progress page when setup's jobs have not finished, else at the start (a
    /// setup applied earlier in this session included, so another profile can be chosen).
    pub fn show_again(&mut self) {
        self.open = true;
        self.keyed = None;
        self.refresh();
        // Added or removed since the window last looked.
        self.launcher_there = self.desktop.launcher_installed();
        self.launcher_on = !self.launcher_there;
        let unfinished = self
            .status
            .as_ref()
            .is_some_and(|s| s.jobs.iter().any(|j| j.status != "done"));
        if self.applying.is_none() && !unfinished && self.settled() {
            self.restart();
        }
    }

    /// Back to the start, to choose a profile again. Setup's earlier jobs stay as they are (a
    /// status that arrives with some unfinished shows them again).
    pub fn restart(&mut self) {
        self.applied = None;
        self.error = None;
        self.jobs.clear();
        self.page = Page::Welcome;
    }

    /// Read `setup.status` again, on a worker thread.
    pub fn refresh(&mut self) {
        let (tx, rx) = channel();
        let mk = self.make_ctx.clone();
        std::thread::Builder::new()
            .name("jetcarp-setup-status".into())
            .spawn(move || {
                let _ = tx.send(setup::status(&mk()));
            })
            .ok();
        self.loading = Some(rx);
    }

    /// Take a status (from the worker, or a test's).
    pub fn set_status(&mut self, s: SetupStatus) {
        let unfinished: Vec<SetupJob> = s
            .jobs
            .iter()
            .filter(|j| j.status != "done")
            .cloned()
            .collect();
        if self.data_dir.is_empty() {
            self.data_dir = s.folders.data.display().to_string();
        }
        if self.applied.is_none() && self.applying.is_none() && !unfinished.is_empty() {
            // Setup ran, and some of its jobs have not finished: show them.
            self.jobs = s.jobs.clone();
            self.page = Page::Progress;
        }
        self.status = Some(s);
        if let Some(p) = self.profile {
            self.default_packs(p);
        }
        self.check_data_dir();
    }

    /// The pages for the profile chosen, in order.
    pub fn pages(&self) -> Vec<Page> {
        let mut v = vec![Page::Welcome, Page::Profile];
        if let Some(p) = self.profile {
            if p.chooses_data_dir() {
                v.push(Page::Data);
            }
            if p.packs() != setup::PackChoice::None || p.chooses_packs() {
                v.push(Page::Packs);
            }
            if p.desktop_offers() {
                v.push(Page::Desktop);
            }
        }
        v.push(Page::Measure);
        v.push(Page::Summary);
        v.push(Page::Progress);
        v
    }

    /// Choose a profile: its build, packs and offers as it suggests.
    pub fn choose(&mut self, p: Profile) {
        if self.profile == Some(p) {
            return;
        }
        self.profile = Some(p);
        self.minutes = p.quickstart_minutes();
        self.build_on = self.minutes > 0.0;
        if self.minutes <= 0.0 {
            self.minutes = 10.0;
        }
        for v in self.offers.values_mut() {
            *v = false;
        }
        if !p.chooses_packs() {
            self.extra.clear();
        }
        if !p.chooses_data_dir()
            && let Some(s) = &self.status
        {
            self.data_dir = s.folders.data.display().to_string();
        }
        self.default_packs(p);
        self.check_data_dir();
    }

    fn default_packs(&mut self, p: Profile) {
        self.bundled_on.clear();
        if let Some(s) = &self.status {
            for e in setup::packs_for(p, &s.bundled) {
                self.bundled_on.insert(e.id(), !e.imported);
            }
        }
    }

    /// The page shown.
    pub fn page(&self) -> Page {
        self.page
    }

    /// Show this page (tests, and the offscreen pictures).
    pub fn go_to(&mut self, p: Page) {
        self.page = p;
    }

    /// Setup applied (its jobs may still run).
    pub fn applied(&self) -> Option<&Applied> {
        self.applied.as_ref()
    }

    /// The data folder's check (what it is, and whether that stops setup), and its free space.
    pub fn data_check(&self) -> (Option<&(String, bool)>, Option<u64>) {
        (self.data_note.as_ref(), self.data_free)
    }

    /// The page can be left forwards.
    pub fn can_next(&self) -> bool {
        match self.page {
            Page::Profile => self.profile.is_some(),
            Page::Data => !self.data_note.as_ref().is_some_and(|n| n.1),
            Page::Progress => false,
            _ => true,
        }
    }

    /// The next page (from the summary: apply).
    pub fn next(&mut self) {
        if !self.can_next() {
            return;
        }
        if self.page == Page::Summary {
            self.apply();
            return;
        }
        let pages = self.pages();
        if let Some(i) = pages.iter().position(|p| *p == self.page) {
            self.page = pages[(i + 1).min(pages.len() - 1)];
        }
    }

    /// The page before (none from the progress page).
    pub fn back(&mut self) {
        if self.page == Page::Progress {
            return;
        }
        let pages = self.pages();
        if let Some(i) = pages.iter().position(|p| *p == self.page) {
            self.page = pages[i.saturating_sub(1)];
        }
    }

    /// Put setup off: close, recording nothing (it opens again at the next launch).
    pub fn later(&mut self) {
        self.open = false;
    }

    /// Close (setup's jobs carry on in the background).
    pub fn close(&mut self) {
        self.open = false;
    }

    /// The data folder changed since this was last asked.
    pub fn take_data_changed(&mut self) -> bool {
        std::mem::take(&mut self.data_changed)
    }

    fn current_data(&self) -> Option<PathBuf> {
        self.status.as_ref().map(|s| s.folders.data.clone())
    }

    /// The data folder setup would choose (none: keep the current one).
    fn chosen_data_dir(&self) -> Option<String> {
        let p = self.profile?;
        if !p.chooses_data_dir() {
            return None;
        }
        let d = self.data_dir.trim();
        if d.is_empty() {
            return None;
        }
        let same = self
            .current_data()
            .is_some_and(|c| Path::new(d) == c.as_path());
        (!same).then(|| d.to_string())
    }

    /// Check the folder typed or picked (`setup.set_data_dir` without recording anything).
    pub fn check_data_dir(&mut self) {
        let d = self.data_dir.trim().to_string();
        if d.is_empty() || self.profile.is_none_or(|p| !p.chooses_data_dir()) {
            self.data_note = None;
            self.data_free = None;
            return;
        }
        self.data_free = setup::free_space(Path::new(&d));
        let cx = (self.make_ctx)();
        self.data_note = match setup::SetupSetDataDir::run(
            &cx,
            DataDirIn {
                path: Some(d),
                dry_run: true,
            },
            &Quiet,
        ) {
            Ok(o) if o.has_library => Some((
                "This folder holds a JETCARP library already; JETCARP will use it.".into(),
                false,
            )),
            Ok(o) => Some((o.notes.join(" "), false)).filter(|n| !n.0.is_empty()),
            Err(e) => Some((
                match &e.hint {
                    Some(h) => format!("{}. {}.", e.message, capitalise(h)),
                    None => e.message.clone(),
                },
                true,
            )),
        };
    }

    /// Packs found in a folder the user picked (none found: a note instead).
    pub fn add_pack_folder(&mut self, dir: PathBuf) {
        if self.extra.iter().any(|(d, _)| *d == dir) {
            return;
        }
        let found = setup::packs_in(&dir, None);
        if found.is_empty() {
            self.error = Some(format!(
                "{} holds no packs (a pack store has a jetcarp-store.json file, an export a pack.json)",
                dir.display()
            ));
            return;
        }
        self.error = None;
        self.extra.push((dir, found));
    }

    /// What `setup.apply` is asked to do.
    pub fn apply_input(&self) -> Option<ApplyIn> {
        let p = self.profile?;
        Some(ApplyIn {
            profile: p.name().into(),
            data_dir: self.chosen_data_dir(),
            quickstart_minutes: Some(if self.build_on { self.minutes } else { 0.0 }),
            packs: self
                .extra
                .iter()
                .map(|(d, _)| d.display().to_string())
                .collect(),
            bundled: Some(
                self.bundled_on
                    .iter()
                    .filter(|(_, on)| **on)
                    .map(|(id, _)| id.clone())
                    .collect(),
            ),
            // As JETCARP's GPU setting says.
            gpu: None,
            background: true,
            measure: Some(self.measure_on),
            ..Default::default()
        })
    }

    /// The offers turned on, of those this system has.
    pub fn offers_on(&self) -> Vec<Offer> {
        if !self.profile.is_some_and(|p| p.desktop_offers()) {
            return vec![];
        }
        Offer::ALL
            .into_iter()
            .filter(|o| self.offers.get(offer_key(*o)) == Some(&true))
            .filter(|o| self.desktop.available(*o).is_ok())
            .collect()
    }

    /// What applying does on the desktop, in order: the Desktop page's offers turned on, then
    /// adding JETCARP to the system's apps when that is ticked and it isn't there.
    pub fn desktop_steps(&self) -> Vec<Offer> {
        let mut v = self.offers_on();
        if self.launcher_on && !self.launcher_there {
            v.push(Offer::Launcher);
        }
        v
    }

    /// Does [`Wizard::desktop_steps`], each result kept for the progress page.
    fn run_desktop_steps(&mut self) {
        self.desktop_done.clear();
        for o in self.desktop_steps() {
            let r = self.desktop.accept(o);
            if o == Offer::Launcher && r.is_ok() {
                self.launcher_there = true;
                self.launcher_on = false;
            }
            self.desktop_done.push((o, r));
        }
    }

    /// Apply: the desktop offers turned on, then `setup.apply` on a worker thread; on to the
    /// progress page.
    pub fn apply(&mut self) {
        let Some(input) = self.apply_input() else {
            return;
        };
        self.run_desktop_steps();
        self.log_out_once = wants_log_out(&self.desktop_done, || {
            self.desktop.wallpaper_waits_for_login()
        });
        self.data_changing = input.data_dir.is_some();
        let (tx, rx) = channel();
        let mk = self.make_ctx.clone();
        let measuring = self.measuring.clone();
        std::thread::Builder::new()
            .name("jetcarp-setup-apply".into())
            .spawn(move || {
                // Only the measuring reports progress (the jobs start in the background).
                let p = Measuring(measuring.clone());
                let r = setup::SetupApply::run(&mk(), input, &p).map_err(|e| e.to_string());
                if let Ok(mut m) = measuring.lock() {
                    *m = None;
                }
                let _ = tx.send(r);
            })
            .ok();
        self.applying = Some(rx);
        self.applied = None;
        self.error = None;
        self.jobs.clear();
        self.page = Page::Progress;
    }

    /// Carry on a job that stopped (`jobs.resume` in the background).
    fn resume(&mut self, job: i64, _kind: &str) {
        let mk = self.make_ctx.clone();
        let lib = self.library_path();
        std::thread::Builder::new()
            .name("jetcarp-setup-resume".into())
            .spawn(move || {
                let cx = mk().with_library(lib);
                let a = jetcarp_api::ops::jobs::ResumeIn {
                    job,
                    background: true,
                    // As the job and JETCARP's GPU setting say.
                    gpu: None,
                    budget: None,
                    threads: None,
                    detach: false,
                    files: Default::default(),
                };
                let _ = jetcarp_api::ops::jobs::JobsResume::run(&cx, a, &Quiet);
            })
            .ok();
    }

    fn library_path(&self) -> Option<PathBuf> {
        match (&self.applied, &self.status) {
            (Some(a), _) => Some(PathBuf::from(&a.library)),
            (None, Some(s)) => Some(PathBuf::from(&s.library)),
            _ => None,
        }
    }

    /// Take what the workers sent; poll the jobs about once a second.
    pub fn poll(&mut self, now: f64) {
        if let Some(rx) = &self.loading
            && let Ok(s) = rx.try_recv()
        {
            self.loading = None;
            self.set_status(s);
        }
        if let Some(rx) = &self.applying
            && let Ok(r) = rx.try_recv()
        {
            self.applying = None;
            match r {
                Ok(a) => {
                    self.data_changed |= std::mem::take(&mut self.data_changing);
                    self.jobs = a.jobs.clone();
                    self.applied = Some(a);
                    self.refresh();
                }
                Err(e) => self.error = Some(e),
            }
        }
        if let Some(rx) = &self.polling
            && let Ok(v) = rx.try_recv()
        {
            self.polling = None;
            if !v.is_empty() {
                self.jobs = v;
            }
        }
        let watching = self.page == Page::Progress && !self.jobs.is_empty();
        if watching && self.polling.is_none() && now - self.last_poll >= 1.0 {
            self.last_poll = now;
            let ids: Vec<i64> = self.jobs.iter().map(|j| j.job).collect();
            let lib = self.library_path();
            let mk = self.make_ctx.clone();
            let (tx, rx) = channel();
            std::thread::Builder::new()
                .name("jetcarp-setup-jobs".into())
                .spawn(move || {
                    let cx = mk().with_library(lib.clone());
                    let l = cx.library();
                    let v: Vec<SetupJob> = ids
                        .iter()
                        .filter_map(|j| setup::setup_job(&cx, &l, *j))
                        .collect();
                    let _ = tx.send(v);
                })
                .ok();
            self.polling = Some(rx);
        }
    }

    /// Every job setup started has ended (or stopped).
    pub fn settled(&self) -> bool {
        self.applying.is_none() && self.jobs.iter().all(SetupJob::settled)
    }

    // ------------------------------------------------------------------------------------- UI

    /// The window (every frame; nothing when closed).
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let now = ctx.input(|i| i.time);
        self.poll(now);
        if self.loading.is_some() || self.applying.is_some() || !self.settled() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        let mut open = self.open;
        let screen = ctx.content_rect();
        let size = [
            660.0f32.min(screen.width() - 20.0),
            600.0f32.min(screen.height() - 40.0),
        ];
        crate::popout::tool_window(
            ctx,
            crate::popout::Spec::new(KEY, "Set up JETCARP")
                .size(size[0], size[1])
                .configure(move |w| {
                    w.default_pos(screen.center() - egui::vec2(size[0], size[1]) / 2.0)
                        .default_size(size)
                        .min_size([480.0, 420.0])
                        .collapsible(false)
                        .resizable(true)
                }),
            &mut open,
            |ui| self.contents(ui),
        );
        // The window's own close button puts setup off, as Later does.
        if !open {
            self.open = false;
        }
    }

    /// The wizard's contents: the steps, the page, and its buttons.
    pub fn contents(&mut self, ui: &mut egui::Ui) {
        self.steps_line(ui);
        ui.add_space(6.0);
        let bottom = 40.0;
        let h = (ui.available_height() - bottom).max(120.0);
        egui::ScrollArea::vertical()
            .max_height(h)
            .min_scrolled_height(h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_height(h - 8.0);
                match self.page {
                    Page::Welcome => self.welcome_ui(ui),
                    Page::Profile => self.profile_ui(ui),
                    Page::Data => self.data_ui(ui),
                    Page::Packs => self.packs_ui(ui),
                    Page::Desktop => self.desktop_ui(ui),
                    Page::Measure => self.measure_ui(ui),
                    Page::Summary => self.summary_ui(ui),
                    Page::Progress => self.progress_ui(ui),
                }
                if let Some(e) = &self.error {
                    ui.add_space(6.0);
                    ui.colored_label(crate::theme::ink(ui, Color32::LIGHT_RED), e);
                }
            });
        ui.separator();
        self.buttons(ui);
    }

    /// `Welcome · What for · Packs · Summary · Setting up`, the page shown bold.
    fn steps_line(&self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let pages = self.pages();
            for (i, p) in pages.iter().enumerate() {
                if i > 0 {
                    ui.label(RichText::new("·").weak());
                }
                let t = RichText::new(p.title());
                ui.label(if *p == self.page {
                    t.strong()
                } else {
                    t.weak()
                });
            }
        });
    }

    fn buttons(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            match self.page {
                Page::Welcome | Page::Profile => {
                    if ui
                        .button("Later")
                        .on_hover_text(
                            "Close this and set up next time JETCARP opens (Help → Setup… opens it any time)",
                        )
                        .clicked()
                    {
                        self.later();
                    }
                }
                Page::Progress => {
                    if self.settled()
                        && ui
                            .button("Set up again")
                            .on_hover_text(
                                "Back to the start, to choose another profile or more packs",
                            )
                            .clicked()
                    {
                        self.restart();
                    }
                }
                _ => {}
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                match self.page {
                    Page::Progress => {
                        if self.settled() {
                            let b = ui.button(RichText::new("Close").strong());
                            self.key_main(&b);
                            if b.clicked() {
                                self.close();
                            }
                        } else {
                            let b = ui
                                .button(RichText::new("Continue in the background").strong())
                                .on_hover_text(
                                    "Close this window; the jobs carry on, and Tools → Hunts… shows them",
                                );
                            self.key_main(&b);
                            if b.clicked() {
                                self.close();
                            }
                        }
                    }
                    Page::Summary => {
                        let nothing = self
                            .apply_input()
                            .is_some_and(|a| self.would_run_nothing(&a));
                        let label = if nothing { "Finish" } else { "Start" };
                        let b = ui.button(RichText::new(label).strong());
                        self.key_main(&b);
                        if b.clicked() {
                            self.next();
                        }
                        if ui.button("Back").clicked() {
                            self.back();
                        }
                    }
                    _ => {
                        let b = ui.add_enabled(
                            self.can_next(),
                            egui::Button::new(RichText::new("Next").strong()),
                        );
                        self.key_main(&b);
                        if b.clicked() {
                            self.next();
                        }
                        if self.page != Page::Welcome && ui.button("Back").clicked() {
                            self.back();
                        }
                    }
                }
            });
        });
    }

    /// As a page shows, give its main button the keyboard, once (#219): as soon as it can be
    /// pressed (Next is greyed out while setup reads where things stand, and on the profile page
    /// until a profile is chosen), unless the keyboard is already on a control of this window,
    /// such as the profile just chosen.
    fn key_main(&mut self, b: &egui::Response) {
        if self.keyed == Some(self.page) || !b.enabled() {
            return;
        }
        let here = b.ctx.memory(|m| m.focused()).is_some_and(|f| {
            b.ctx
                .viewport(|v| v.this_pass.widgets.get(f).map(|w| w.layer_id))
                == Some(b.layer_id)
        });
        if !here {
            b.request_focus();
        }
        self.keyed = Some(self.page);
    }

    /// Applying `a` imports and builds nothing (the summary's button says Finish).
    pub fn would_run_nothing(&self, a: &ApplyIn) -> bool {
        a.quickstart_minutes.unwrap_or(0.0) <= 0.0
            && a.packs.is_empty()
            && a.bundled.as_ref().is_none_or(|b| b.is_empty())
    }

    fn welcome_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading(RichText::new("Welcome to JETCARP").size(26.0).strong());
        ui.add_space(10.0);
        ui.label(
            RichText::new(
                "JETCARP runs cellular automata, grids of cells that live and die by simple rules, \
                 on almost any tiling of the plane: squares and hexagons, Penrose tilings, the hat, \
                 and over a thousand more.",
            )
            .size(15.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "You can watch them, hunt for patterns that move or repeat, and run them as a \
                 living wallpaper or screen saver.",
            )
            .size(15.0),
        );
        ui.add_space(14.0);
        ui.label(
            "A few questions set JETCARP up for what you want to do. Nothing is downloaded: \
             everything comes with JETCARP or stays on this computer.",
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new("You can change your answers later: Help → Setup… opens this again.")
                .weak(),
        );
    }

    fn profile_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("What would you like JETCARP for?");
        ui.add_space(6.0);
        let mut chosen = None;
        for p in Profile::ALL {
            let selected = self.profile == Some(p);
            let cost = self
                .status
                .as_ref()
                .and_then(|s| s.info(p))
                .map(|i| capitalise(&i.cost));
            let frame = egui::Frame::group(ui.style()).fill(if selected {
                ui.visuals().selection.bg_fill.gamma_multiply(0.35)
            } else {
                Color32::TRANSPARENT
            });
            let r = frame
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        if ui.radio(selected, "").clicked() {
                            chosen = Some(p);
                        }
                        ui.vertical(|ui| {
                            ui.label(RichText::new(p.title()).strong().size(15.0));
                            ui.label(p.summary());
                            ui.label(
                                RichText::new(cost.clone().unwrap_or_else(|| "…".into()))
                                    .small()
                                    .weak(),
                            );
                        });
                    });
                })
                .response
                .interact(egui::Sense::click());
            if r.clicked() {
                chosen = Some(p);
            }
            ui.add_space(2.0);
        }
        if let Some(p) = chosen {
            self.choose(p);
        }
        if let Some(s) = &self.status
            && let Some(r) = &s.record
        {
            ui.add_space(4.0);
            let title = Profile::parse(&r.profile)
                .map(|p| p.title())
                .unwrap_or(r.profile.as_str());
            ui.label(
                RichText::new(format!(
                    "Setup last ran on {} with {title}. Choosing again adds to what is there; it never deletes anything.",
                    short_date(&r.date)
                ))
                .small()
                .weak(),
            );
        }
    }

    fn data_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Where should JETCARP keep its data?");
        ui.add_space(4.0);
        ui.label(
            "The library, its pictures and your searches go in one folder. A researcher's \
             library can grow to many gigabytes, so pick a disk with room to spare.",
        );
        ui.add_space(8.0);
        let mut changed = false;
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 190.0).max(160.0);
            changed |= ui
                .add(egui::TextEdit::singleline(&mut self.data_dir).desired_width(w))
                .lost_focus();
            let clicked = ui.button("Choose…").clicked();
            let start = self.data_dir.trim().to_string();
            if let Some(d) =
                crate::file_dialog::on_click(ui.ctx(), "setup.data_dir", clicked, move || {
                    rfd::FileDialog::new().set_directory(start).pick_folder()
                })
            {
                self.data_dir = d.display().to_string();
                changed = true;
            }
            if ui
                .button("Default")
                .on_hover_text("JETCARP's usual data folder")
                .clicked()
            {
                self.data_dir = jetcarp_core::paths::default_data_dir()
                    .display()
                    .to_string();
                changed = true;
            }
        });
        if changed {
            self.check_data_dir();
        }
        ui.add_space(4.0);
        if let Some(b) = self.data_free {
            ui.label(format!("{} free on that disk.", human_bytes(b)));
        }
        if let Some((n, err)) = &self.data_note {
            ui.colored_label(
                crate::theme::ink(
                    ui,
                    if *err {
                        Color32::LIGHT_RED
                    } else {
                        ui.visuals().text_color()
                    },
                ),
                n,
            );
        }
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "The folder must be empty, or hold a JETCARP data folder already. Setup never \
                 moves a library. The choice is kept in paths.json in JETCARP's settings folder.",
            )
            .small()
            .weak(),
        );
    }

    fn pack_row(ui: &mut egui::Ui, e: &PackEntry, on: Option<&mut bool>) {
        ui.horizontal(|ui| {
            match on {
                Some(on) if !e.imported => {
                    ui.checkbox(on, "");
                }
                Some(_) => {
                    ui.add_enabled(false, egui::Checkbox::new(&mut true, ""));
                }
                None => {
                    ui.label(RichText::new("•").strong());
                }
            }
            ui.vertical(|ui| {
                let mut t = e.name();
                if e.starter && !t.to_ascii_lowercase().contains("starter") {
                    t.push_str("  (the starter pack)");
                }
                ui.label(RichText::new(t).strong());
                ui.label(RichText::new(e.contents()).small());
                if !e.description.is_empty() {
                    ui.label(RichText::new(&e.description).weak());
                }
                if let Some(w) = e.signed_by() {
                    ui.label(RichText::new(capitalise(&w)).small())
                        .on_hover_text(format!("The publisher's key: {}", e.publisher));
                    if !e.signed {
                        ui.colored_label(
                            crate::theme::ink(ui, Color32::LIGHT_RED),
                            RichText::new("Setup imports only packs their publisher signed.")
                                .small(),
                        );
                    }
                }
                // The packs that came with JETCARP are under their own heading already.
                if let Some((words, new)) = e
                    .key_seen()
                    .filter(|k| *k != KeySeen::Shipped)
                    .map(key_line)
                {
                    key_label(ui, &words, new);
                }
                if e.imported {
                    ui.label(RichText::new("Already in your library.").small().weak());
                }
            });
        });
    }

    fn packs_ui(&mut self, ui: &mut egui::Ui) {
        let Some(p) = self.profile else { return };
        ui.heading("Packs");
        ui.add_space(4.0);
        ui.label(
            "A pack is a collection of patterns, Quick Start entries and search results that \
             someone has published. Importing one checks every file and signature, then adds it \
             to your library.",
        );
        ui.add_space(8.0);
        let bundled: Vec<PackEntry> = self
            .status
            .as_ref()
            .map(|s| {
                s.bundled
                    .iter()
                    .filter(|e| e.error.is_none())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let shown: Vec<&PackEntry> = match p.packs() {
            setup::PackChoice::All => bundled.iter().collect(),
            setup::PackChoice::Starter => bundled.iter().filter(|e| e.starter).collect(),
            setup::PackChoice::None => vec![],
        };
        if self.status.is_none() {
            ui.label(RichText::new("Looking for the packs that came with JETCARP…").weak());
        } else if shown.is_empty() {
            ui.label(
                "No packs came with this copy of JETCARP (as when it is built with cargo). The \
                 built-in examples are in the Quick Start all the same, and the jetcarp pack \
                 import command imports a pack later.",
            );
        } else {
            ui.label(RichText::new("Came with JETCARP").strong());
            for e in shown {
                let id = e.id();
                let mut on = self.bundled_on.get(&id).copied().unwrap_or(false);
                Self::pack_row(ui, e, Some(&mut on));
                if !e.imported {
                    self.bundled_on.insert(id, on);
                }
            }
        }
        if p.chooses_packs() {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("From your own folders").strong());
                let clicked = ui
                    .button("Add packs from a folder…")
                    .on_hover_text("A pack store, an export, or a folder that holds several")
                    .clicked();
                if let Some(d) =
                    crate::file_dialog::on_click(ui.ctx(), "setup.pack_folder", clicked, || {
                        rfd::FileDialog::new().pick_folder()
                    })
                {
                    self.add_pack_folder(d);
                }
            });
            let mut drop = None;
            for (i, (d, found)) in self.extra.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(d.display().to_string()).monospace().small());
                    if ui
                        .small_button("Remove")
                        .on_hover_text("Leave these packs out")
                        .clicked()
                    {
                        drop = Some(i);
                    }
                });
                for e in found {
                    ui.indent(("extra-pack", i, e.id()), |ui| match &e.error {
                        Some(err) => {
                            ui.colored_label(
                                crate::theme::ink(ui, Color32::LIGHT_RED),
                                format!("{}: {err}", e.id()),
                            );
                        }
                        None => Self::pack_row(ui, e, None),
                    });
                }
            }
            if let Some(i) = drop {
                self.extra.remove(i);
            }
            if self.extra.is_empty() {
                ui.label(
                    RichText::new(
                        "None yet. Large packs can also be imported later, with the jetcarp pack import command.",
                    )
                    .small()
                    .weak(),
                );
            }
        }
    }

    fn desktop_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Use JETCARP on your desktop?");
        ui.add_space(4.0);
        ui.label(
            "These change your desktop, so each stays off unless you tick it. You can turn them \
             off again in the Desktop background pane (View → Desktop background settings…), or \
             with the jetcarp host command.",
        );
        ui.add_space(8.0);
        for o in Offer::ALL {
            let avail = self.desktop.available(o);
            let key = offer_key(o);
            let mut on = self.offers.get(key).copied().unwrap_or(false);
            let login_needs_wallpaper =
                o == Offer::Login && !self.offers.get("wallpaper").copied().unwrap_or(false);
            ui.add_enabled_ui(avail.is_ok() && !login_needs_wallpaper, |ui| {
                ui.checkbox(&mut on, RichText::new(o.title()).strong());
                ui.indent(("offer", key), |ui| {
                    ui.label(RichText::new(o.detail()).small());
                    match &avail {
                        Err(why) => {
                            ui.label(RichText::new(capitalise(why)).small().weak());
                        }
                        Ok(()) => {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("The same as").small().weak());
                                ui.label(RichText::new(o.commands()).monospace().size(10.5).weak());
                            });
                        }
                    }
                });
            });
            if login_needs_wallpaper {
                on = false;
            }
            self.offers.insert(key, on && avail.is_ok());
            ui.add_space(4.0);
        }
    }

    /// Measure this computer: about a minute, ticked by default, skippable.
    fn measure_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Measure this computer");
        ui.add_space(6.0);
        ui.label(
            "JETCARP can measure this computer with about a minute of its own work: Life soups on \
             five tilings, one core, the GPU, memory and the disk, each answer checked. Nothing \
             is drawn but a progress bar, and nothing is sent anywhere. Its limits then fit it, \
             and it gets a JETCARP Hardware Score, kept in the settings folder.",
        );
        ui.add_space(6.0);
        ui.checkbox(
            &mut self.measure_on,
            "Measure this computer first (about a minute)",
        );
        ui.add_space(6.0);
        let l = jetcarp_core::machine::current();
        ui.label(
            RichText::new(format!(
                "Without measuring, the limits come from the memory alone: {} reserved for \
                 JETCARP, worlds of up to {}. Tools → This computer… measures it later and \
                 changes any limit.",
                jetcarp_core::machine::bytes_text(l.jetcarp_memory()),
                jetcarp_core::machine::bytes_text(l.world_bytes())
            ))
            .weak(),
        );
    }

    fn summary_ui(&mut self, ui: &mut egui::Ui) {
        let Some(p) = self.profile else { return };
        ui.heading(format!("Ready to set up JETCARP for {}", p.title()));
        ui.add_space(6.0);
        let status = self.status.clone();
        let data = if p.chooses_data_dir() {
            self.data_dir.clone()
        } else {
            status
                .as_ref()
                .map(|s| s.folders.data.display().to_string())
                .unwrap_or_default()
        };
        let free = if p.chooses_data_dir() {
            self.data_free
        } else {
            status.as_ref().and_then(|s| s.free_bytes)
        };
        egui::Grid::new("setup-summary")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label(RichText::new("Data").strong());
                ui.vertical(|ui| {
                    ui.label(RichText::new(&data).monospace());
                    if let Some(b) = free {
                        ui.label(RichText::new(format!("{} free", human_bytes(b))).small().weak());
                    }
                });
                ui.end_row();
                ui.label(RichText::new("Packs").strong());
                ui.vertical(|ui| {
                    let mut any = false;
                    if let Some(s) = &status {
                        for e in &s.bundled {
                            if self.bundled_on.get(&e.id()) == Some(&true) {
                                ui.label(format!("{} ({})", e.name(), e.contents()));
                                any = true;
                            }
                        }
                    }
                    for (_, found) in &self.extra {
                        for e in found.iter().filter(|e| e.error.is_none()) {
                            ui.label(format!("{} ({})", e.name(), e.contents()));
                            any = true;
                        }
                    }
                    if !any {
                        ui.label(RichText::new("None: the built-in examples only.").weak());
                    }
                });
                ui.end_row();
                ui.label(RichText::new("This computer").strong());
                ui.checkbox(&mut self.measure_on, "Measured first (about a minute)");
                ui.end_row();
                // For every profile, once: JETCARP opens from its icon like the other apps.
                if !self.launcher_there {
                    ui.label(RichText::new("Your apps").strong());
                    ui.checkbox(&mut self.launcher_on, Offer::Launcher.title())
                        .on_hover_text(Offer::Launcher.detail());
                    ui.end_row();
                }
                ui.label(RichText::new("Quick Start").strong());
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.build_on, "Search for");
                        ui.add_enabled(
                            self.build_on,
                            egui::DragValue::new(&mut self.minutes)
                                .range(1.0..=600.0)
                                .speed(0.5)
                                .fixed_decimals(0)
                                .suffix(" min"),
                        );
                        ui.label("and add the best finds to the Quick Start");
                    });
                    let gpu = status.as_ref().map(|s| &s.gpu);
                    let note = match gpu {
                        Some(g) if g.usable => format!("On the GPU: {}.", g.name),
                        Some(_) => "On the CPU: no GPU that JETCARP can use was found, so it finds less in the time.".into(),
                        None => String::new(),
                    };
                    let mut words = vec![note];
                    if self.build_on {
                        words.push(
                            "Every computer's search finds different things.".into(),
                        );
                    }
                    ui.label(RichText::new(words.join(" ")).small().weak());
                });
                ui.end_row();
                if p.desktop_offers() {
                    ui.label(RichText::new("Desktop").strong());
                    ui.vertical(|ui| {
                        let on = self.offers_on();
                        if on.is_empty() {
                            ui.label(RichText::new("Nothing changes on the desktop.").weak());
                        }
                        for o in on {
                            ui.label(o.title());
                        }
                    });
                    ui.end_row();
                }
            });
        ui.add_space(10.0);
        let pack_bytes: u64 = status
            .as_ref()
            .map(|s| {
                s.bundled
                    .iter()
                    .filter(|e| self.bundled_on.get(&e.id()) == Some(&true))
                    .map(|e| e.bytes)
                    .sum::<u64>()
            })
            .unwrap_or(0)
            + self
                .extra
                .iter()
                .flat_map(|(_, f)| f.iter().map(|e| e.bytes))
                .sum::<u64>();
        let mins = if self.build_on { self.minutes } else { 0.0 };
        let (bytes, minutes) = setup::estimate(pack_bytes, mins);
        ui.label(capitalise(&setup::cost_text(bytes, minutes, false)) + ".");
        if bytes > 0 || minutes > 0.0 {
            ui.label(
                RichText::new(
                    "It runs in the background: you can use JETCARP meanwhile, and the Hunts \
                     window (Tools → Hunts…) shows the jobs too.",
                )
                .small()
                .weak(),
            );
        }
    }

    fn progress_ui(&mut self, ui: &mut egui::Ui) {
        let title = self
            .profile
            .or_else(|| self.status.as_ref().and_then(|s| s.profile()))
            .map(|p| p.title())
            .unwrap_or("JETCARP");
        let done = self.settled() && self.applying.is_none();
        ui.heading(if done && self.error.is_none() {
            format!("{title}: all set")
        } else {
            format!("Setting up for {title}")
        });
        ui.add_space(6.0);
        if self.applying.is_some() {
            let m = self.measuring.lock().ok().and_then(|m| m.clone());
            match m {
                // A plain bar, nothing animated (the benchmark draws nothing).
                Some((f, what)) => {
                    ui.label("Measuring this computer (about a minute)…");
                    ui.add(egui::ProgressBar::new(f).text(what));
                }
                None => {
                    ui.horizontal(|ui| {
                        crate::a11y::spinner(ui);
                        ui.label("Starting…");
                    });
                }
            }
        }
        for (o, r) in &self.desktop_done {
            match r {
                Ok(m) => ui.label(format!("{}: {}.", o.title(), m)),
                Err(e) => ui.colored_label(
                    crate::theme::ink(ui, Color32::LIGHT_RED),
                    format!("{}: {e}", o.title()),
                ),
            };
        }
        if self.log_out_once {
            ui.add_space(6.0);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(LOG_OUT_ONCE).strong().size(15.0));
            });
        }
        let mut resume = None;
        for j in &self.jobs {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let what = if j.kind == "explore" {
                    "Building your Quick Start".to_string()
                } else {
                    format!(
                        "Importing {}",
                        j.label.strip_prefix("setup: ").unwrap_or(&j.label)
                    )
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(what).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (word, colour) = match (j.status.as_str(), j.stale) {
                            ("done", _) => ("done", Color32::LIGHT_GREEN),
                            ("failed", _) => ("failed", Color32::LIGHT_RED),
                            (_, true) => ("cut off", Color32::from_rgb(255, 170, 90)),
                            ("paused", _) => ("paused", Color32::from_rgb(255, 170, 90)),
                            _ => ("running", ui.visuals().text_color()),
                        };
                        if j.settled() && j.status != "done" && ui.small_button("Resume").clicked()
                        {
                            resume = Some((j.job, j.kind.clone()));
                        }
                        ui.label(RichText::new(word).color(crate::theme::ink(ui, colour)));
                    });
                });
                let frac = j.fraction.unwrap_or(0.0) as f32;
                let mut bar = egui::ProgressBar::new(frac);
                if !j.settled() {
                    bar = bar.animate(crate::a11y::animate_bars(ui.ctx()));
                }
                ui.add(bar.desired_height(8.0));
                if j.kind == "explore" && j.done == 0 && !j.settled() {
                    ui.label(
                        RichText::new(
                            "Starting the search: its first results take a minute or so to arrive.",
                        )
                        .small(),
                    );
                }
                // An import's line ends with who signed the pack; a key new to the library is
                // said again below it, where the eye goes.
                ui.label(RichText::new(&j.line).small().weak());
                if j.key.as_deref().and_then(KeySeen::parse) == Some(KeySeen::New) {
                    let (words, new) = key_line(KeySeen::New);
                    key_label(ui, &words, new);
                }
            });
        }
        if let Some((job, kind)) = resume {
            self.resume(job, &kind);
        }
        if let Some(a) = &self.applied {
            for n in &a.notes {
                ui.label(RichText::new(n).small().weak());
            }
            if a.jobs.is_empty() && self.error.is_none() {
                ui.label(if a.steps.iter().any(|s| s.kind == "skip") {
                    "Nothing needed importing or building: the built-in examples are in the Quick Start."
                } else {
                    "Done."
                });
            }
        }
        ui.add_space(8.0);
        ui.label(
            RichText::new(if done {
                "New entries are in the Quick Start. Set up again, here or from Help → Setup…, to \
                 choose another profile."
            } else {
                "You can close this window: the jobs carry on in the background, a stopped one \
                 resumes from Tools → Hunts…, and new entries appear in the Quick Start as they \
                 arrive."
            })
            .small()
            .weak(),
        );
    }
}

/// What the library knows of a pack's key, as the setup window says it, and whether to draw
/// the eye to it (a key never seen in this library).
fn key_line(k: KeySeen) -> (String, bool) {
    (format!("{}.", capitalise(k.words())), k == KeySeen::New)
}

/// [`key_line`]'s words: a new key in the colour the window uses for what needs a look, with
/// what it means on hover; any other plainly.
fn key_label(ui: &mut egui::Ui, words: &str, new: bool) {
    if new {
        ui.label(
            RichText::new(words)
                .small()
                .strong()
                .color(crate::theme::ink(ui, Color32::from_rgb(255, 170, 90))),
        )
        .on_hover_text(
            "No pack signed by this key has been imported into this library before. If you \
             expected a pack from someone whose packs you already have, check that its key is \
             theirs.",
        );
    } else {
        ui.label(RichText::new(words).small().weak());
    }
}

/// The first letter in capitals.
fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// `2026-11-12T10:20:30Z` as `2026-11-12`.
fn short_date(d: &str) -> &str {
    d.get(..10).unwrap_or(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offscreen::Offscreen;

    /// What the window says of a pack's key: a key never seen in this library draws the eye;
    /// the others are said plainly.
    #[test]
    fn a_key_never_seen_draws_the_eye() {
        assert_eq!(
            key_line(KeySeen::New),
            ("New key: never seen in this library.".to_string(), true)
        );
        assert_eq!(
            key_line(KeySeen::Known),
            (
                "Known key: this library has imported packs it signed.".to_string(),
                false
            )
        );
        assert_eq!(key_line(KeySeen::Own).0, "Your own publisher key.");
        assert_eq!(key_line(KeySeen::Shipped).0, "The pack came with JETCARP.");
    }

    /// Frames of the wizard's window until it settles; the last one saved as `<dir>/<name>.png`.
    fn shot(o: &mut Offscreen, w: &mut Wizard, dir: &Path, name: &str) {
        let mut px = vec![];
        for _ in 0..8 {
            px = o.frame(0.05, |ctx| w.show(ctx));
        }
        let p = dir.join(format!("{name}.png"));
        o.save(&px, &p).unwrap();
        eprintln!("wrote {}", p.display());
    }

    /// The app's fonts (Hack after the proportional font, for arrows and dots).
    fn app_fonts(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();
        if let Some(p) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            p.push("Hack".to_owned());
        }
        ctx.set_fonts(fonts);
    }

    /// What the wizard's pictures need: their folder (`JETCARP_OFFSCREEN_OUT`), a device of the
    /// window's size with the app's fonts, a context maker, and the status with the folders
    /// shown replaced by an example user's, so no local path is in a picture. `None` without
    /// `JETCARP_OFFSCREEN_OUT` or a GPU.
    fn wizard_pictures() -> Option<(PathBuf, Offscreen, MakeCtx, SetupStatus)> {
        let dir = std::env::var_os("JETCARP_OFFSCREEN_OUT").map(PathBuf::from)?;
        std::fs::create_dir_all(&dir).unwrap();
        for v in [
            "JETCARP_CONFIG_DIR",
            "JETCARP_LIBRARY",
            "JETCARP_BUNDLED_PACKS",
        ] {
            assert!(std::env::var_os(v).is_some(), "set {v}");
        }
        let Some(o) = Offscreen::new(690, 640) else {
            return None;
        };
        app_fonts(&o.ctx);
        let mk: MakeCtx =
            Arc::new(|| Ctx::new().with_platform(Arc::new(crate::window_job::AppPlatform)));
        let mut status = setup::status(&mk());
        status.folders.data = PathBuf::from("/home/ada/.local/share/jetcarp");
        status.library = "/home/ada/.local/share/jetcarp/library.sqlite".into();
        Some((dir, o, mk, status))
    }

    /// Setup ends by saying to log out once only when the wallpaper was turned on and waits for
    /// a login (#139).
    #[test]
    fn setup_says_to_log_out_once_when_the_wallpaper_waits() {
        let ok = |o| (o, Ok::<String, String>("done".into()));
        assert!(wants_log_out(&[ok(Offer::Wallpaper)], || true));
        assert!(!wants_log_out(&[ok(Offer::Wallpaper)], || false));
        assert!(!wants_log_out(&[ok(Offer::Saver)], || true));
        assert!(!wants_log_out(
            &[(Offer::Wallpaper, Err("refused".into()))],
            || true
        ));
        assert!(LOG_OUT_ONCE.starts_with("Log out and back in once"));
    }

    /// A desktop that records what it is asked to do, with JETCARP among its apps or not.
    struct StandIn {
        there: bool,
        asked: Arc<std::sync::Mutex<Vec<Offer>>>,
    }

    impl Desktop for StandIn {
        fn available(&self, _: Offer) -> Result<(), String> {
            Ok(())
        }
        fn accept(&mut self, o: Offer) -> Result<String, String> {
            self.asked.lock().unwrap().push(o);
            Ok("done".into())
        }
        fn launcher_installed(&self) -> bool {
            self.there
        }
    }

    /// Setup's summary offers to add JETCARP to the system's apps, for every profile, ticked
    /// when it isn't there yet; applying does it once, after the desktop offers, and reports it
    /// with them (#105). When it is there already, there is nothing to tick and nothing is done.
    #[test]
    fn setup_adds_jetcarp_to_the_apps_when_it_is_not_there() {
        let mk: MakeCtx = Arc::new(Ctx::new);
        for p in [
            Profile::DesktopArt,
            Profile::Explorer,
            Profile::Researcher,
            Profile::Everything,
            Profile::BlankSlate,
        ] {
            let asked = Arc::new(std::sync::Mutex::new(vec![]));
            let desk = StandIn {
                there: false,
                asked: asked.clone(),
            };
            let mut w = Wizard::new(mk.clone(), Box::new(desk));
            w.choose(p);
            assert!(!w.launcher_there && w.launcher_on, "{p:?}: ticked");
            if p.desktop_offers() {
                w.offers.insert("wallpaper", true);
            }
            let steps = w.desktop_steps();
            assert_eq!(steps.last(), Some(&Offer::Launcher), "{p:?}: {steps:?}");
            assert_eq!(steps.len(), if p.desktop_offers() { 2 } else { 1 });
            w.run_desktop_steps();
            assert_eq!(*asked.lock().unwrap(), steps);
            assert_eq!(w.desktop_done.last().map(|d| d.0), Some(Offer::Launcher));
            // Unticked: not done.
            w.launcher_on = false;
            assert!(!w.desktop_steps().contains(&Offer::Launcher));
        }
        let asked = Arc::new(std::sync::Mutex::new(vec![]));
        let desk = StandIn {
            there: true,
            asked: asked.clone(),
        };
        let mut w = Wizard::new(mk, Box::new(desk));
        w.choose(Profile::Explorer);
        assert!(
            w.launcher_there && !w.launcher_on,
            "already there: no box, not ticked"
        );
        w.launcher_on = true;
        assert!(
            w.desktop_steps().is_empty(),
            "never done again when it is there"
        );
        w.run_desktop_steps();
        assert!(asked.lock().unwrap().is_empty());
    }

    /// A wizard open at its first page, with this status.
    fn fresh_wizard(mk: &MakeCtx, status: &SetupStatus) -> Wizard {
        let mut w = Wizard::new(mk.clone(), Box::new(RealDesktop));
        w.set_status(status.clone());
        w.open = true;
        w
    }

    /// Every page of the setup wizard before it starts, drawn headlessly as PNGs in
    /// `JETCARP_OFFSCREEN_OUT` (the book's first-run page uses them). Its folders come from the
    /// environment, which must point at scratch folders: `JETCARP_CONFIG_DIR`,
    /// `JETCARP_DATA_DIR`, `JETCARP_LIBRARY`, `JETCARP_OUT`, `JETCARP_BUNDLED_PACKS` (the packs
    /// shown as having come with JETCARP), and `JETCARP_OFFSCREEN_PACKS` (a folder of packs added
    /// on the Researcher's pack page). [`offscreen_setup_wizard_running`] draws the pages after.
    #[test]
    #[ignore]
    fn offscreen_setup_wizard() {
        let Some((dir, mut o, mk, status)) = wizard_pictures() else {
            return;
        };
        let mut w = fresh_wizard(&mk, &status);
        shot(&mut o, &mut w, &dir, "setup-1-welcome");
        w.next();
        w.choose(Profile::Explorer);
        shot(&mut o, &mut w, &dir, "setup-2-profile");
        w.choose(Profile::Researcher);
        w.data_dir = "/media/ada/Big Disk/jetcarp".into();
        w.check_data_dir();
        // Its notes here are about this run's JETCARP_ variables, which a user has not set.
        w.data_note = None;
        w.go_to(Page::Data);
        shot(&mut o, &mut w, &dir, "setup-3-data");
        if let Some(p) = std::env::var_os("JETCARP_OFFSCREEN_PACKS") {
            w.add_pack_folder(PathBuf::from(p));
            if let Some(e) = w.extra.first_mut() {
                e.0 = PathBuf::from("/media/ada/Big Disk/packs");
            }
        }
        w.go_to(Page::Packs);
        shot(&mut o, &mut w, &dir, "setup-4-packs-researcher");
        w.choose(Profile::Explorer);
        w.go_to(Page::Packs);
        shot(&mut o, &mut w, &dir, "setup-4-packs");
        w.choose(Profile::DesktopArt);
        w.offers.insert("wallpaper", true);
        w.go_to(Page::Desktop);
        shot(&mut o, &mut w, &dir, "setup-5-desktop");
        w.choose(Profile::Explorer);
        // The limits it shows come from the test's reference machine (16 GB), not this one.
        w.go_to(Page::Measure);
        shot(&mut o, &mut w, &dir, "setup-5b-measure");
        // As on a computer where JETCARP is not among the apps yet, whatever this one has.
        w.launcher_there = false;
        w.launcher_on = true;
        w.go_to(Page::Summary);
        shot(&mut o, &mut w, &dir, "setup-6-summary");
    }

    /// The setup wizard's last pages, as [`offscreen_setup_wizard`] draws the first: a real
    /// Explorer setup with a one-minute Quick Start build in the scratch folders (an explore
    /// search on the GPU when there is one), running, then done.
    #[test]
    #[ignore]
    fn offscreen_setup_wizard_running() {
        let Some((dir, mut o, mk, status)) = wizard_pictures() else {
            return;
        };
        let mut w = fresh_wizard(&mk, &status);
        w.choose(Profile::Explorer);
        w.minutes = 1.0;
        // The build's pages, without the minute of measuring first, and without adding JETCARP
        // to this computer's apps.
        w.measure_on = false;
        w.launcher_on = false;
        w.go_to(Page::Summary);
        w.next();
        let t = std::time::Instant::now();
        while t.elapsed().as_secs() < 25 {
            o.frame(0.25, |ctx| w.show(ctx));
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        shot(&mut o, &mut w, &dir, "setup-7-progress");
        while !(w.applied().is_some() && w.settled()) && t.elapsed().as_secs() < 600 {
            o.frame(0.5, |ctx| w.show(ctx));
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        // A few more polls, so the page shows the jobs' last state.
        let t2 = std::time::Instant::now();
        while t2.elapsed().as_secs() < 3 {
            o.frame(0.5, |ctx| w.show(ctx));
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        shot(&mut o, &mut w, &dir, "setup-8-done");
        let a = w.applied().expect("applied");
        eprintln!("setup applied: {:?}", a.jobs);
        assert!(w.settled());
    }
}
