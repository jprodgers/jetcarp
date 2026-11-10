//! The tray icon on Windows and macOS. On Linux, GNOME's top-bar menu comes with the Shell
//! extension (PLAN.md wave 5).

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// What the user asked for from the tray.
pub enum Do {
    Next,
    Previous,
    Pause(bool),
    Wallpaper(bool),
    Rotation(String),
    OpenApp,
    RunAtLogin(bool),
    Quit,
}

pub struct Tray {
    _icon: TrayIcon,
    now: MenuItem,
    next: MenuItem,
    previous: MenuItem,
    pause: CheckMenuItem,
    wallpaper: CheckMenuItem,
    rotations: Vec<(String, CheckMenuItem)>,
    open: MenuItem,
    login: CheckMenuItem,
    quit: MenuItem,
}

/// A small square of live and dead cells, as RGBA.
fn icon() -> Result<Icon, String> {
    const SIZE: u32 = 32;
    const CELL: u32 = 8;
    // A glider, in a 4 by 4 grid.
    const LIVE: [(u32, u32); 5] = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let live = LIVE.contains(&(x / CELL, y / CELL));
            let edge = x % CELL == 0 || y % CELL == 0;
            let v = if edge {
                40
            } else if live {
                235
            } else {
                70
            };
            rgba.extend_from_slice(&[v, v, v, 255]);
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE).map_err(|e| e.to_string())
}

impl Tray {
    /// Must be called once the event loop is running.
    pub fn new(names: &[String]) -> Result<Tray, String> {
        let now = MenuItem::new("Starting…", false, None);
        let next = MenuItem::new("Next", true, None);
        let previous = MenuItem::new("Previous", true, None);
        let pause = CheckMenuItem::new("Paused", true, false, None);
        let wallpaper = CheckMenuItem::new("Show on the desktop", true, false, None);
        let submenu = Submenu::new("Rotation", true);
        let mut rotations = Vec::with_capacity(names.len());
        for n in names {
            let item = CheckMenuItem::new(n, true, false, None);
            submenu.append(&item).map_err(|e| e.to_string())?;
            rotations.push((n.clone(), item));
        }
        let open = MenuItem::new("Open JETCARP", true, None);
        let login = CheckMenuItem::new("Start at login", true, false, None);
        let quit = MenuItem::new("Quit", true, None);
        let menu = Menu::new();
        menu.append_items(&[
            &now,
            &PredefinedMenuItem::separator(),
            &next,
            &previous,
            &pause,
            &wallpaper,
            &submenu,
            &PredefinedMenuItem::separator(),
            &open,
            &login,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(|e| e.to_string())?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("JETCARP")
            .with_icon(icon()?)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Tray {
            _icon: icon,
            now,
            next,
            previous,
            pause,
            wallpaper,
            rotations,
            open,
            login,
            quit,
        })
    }

    /// Menu clicks since the last call.
    pub fn poll(&self) -> Vec<Do> {
        let mut out = vec![];
        while let Ok(e) = MenuEvent::receiver().try_recv() {
            let id = e.id();
            if id == self.next.id() {
                out.push(Do::Next);
            } else if id == self.previous.id() {
                out.push(Do::Previous);
            } else if id == self.pause.id() {
                out.push(Do::Pause(self.pause.is_checked()));
            } else if id == self.wallpaper.id() {
                out.push(Do::Wallpaper(self.wallpaper.is_checked()));
            } else if id == self.open.id() {
                out.push(Do::OpenApp);
            } else if id == self.login.id() {
                out.push(Do::RunAtLogin(self.login.is_checked()));
            } else if id == self.quit.id() {
                out.push(Do::Quit);
            } else if let Some((n, _)) = self.rotations.iter().find(|(_, i)| i.id() == id) {
                out.push(Do::Rotation(n.clone()));
            }
        }
        out
    }

    /// Makes the menu show the host's state.
    pub fn refresh(&self, label: &str, rotation: &str, paused: bool, wallpaper: bool, login: bool) {
        self.now.set_text(if label.is_empty() {
            "Starting…"
        } else {
            label
        });
        self.pause.set_checked(paused);
        self.wallpaper.set_checked(wallpaper);
        self.login.set_checked(login);
        for (n, item) in &self.rotations {
            item.set_checked(n == rotation);
        }
    }
}
