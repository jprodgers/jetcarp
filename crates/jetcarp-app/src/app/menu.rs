//! The menu bar: every app action, as data ([`MENUS`]) so a test can check that each action and
//! each former button is reachable. Keyboard shortcuts come from the same table ([`Action::keys`]).

use super::*;
use egui::{Key, KeyboardShortcut, Modifiers};

/// Tool windows: each can float in the main window or pop out to its own OS window.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(super) enum ToolWin {
    Scripts,
    Results,
    Library,
    Hunts,
    CatalogTools,
    Ants,
    Sandpile,
    HashLife,
    Palettes,
    AiControl,
    TileEditor,
    CellEditor,
    Shortcuts,
    About,
    Wallpaper,
    Safety,
    Machine,
    Accessibility,
}

impl ToolWin {
    pub const ALL: [ToolWin; 18] = [
        ToolWin::Scripts,
        ToolWin::Results,
        ToolWin::Library,
        ToolWin::Hunts,
        ToolWin::CatalogTools,
        ToolWin::Ants,
        ToolWin::Sandpile,
        ToolWin::HashLife,
        ToolWin::Palettes,
        ToolWin::AiControl,
        ToolWin::TileEditor,
        ToolWin::CellEditor,
        ToolWin::Shortcuts,
        ToolWin::About,
        ToolWin::Wallpaper,
        ToolWin::Safety,
        ToolWin::Machine,
        ToolWin::Accessibility,
    ];

    /// The window's key in `crate::popout` (and the settings file).
    pub fn key(self) -> &'static str {
        match self {
            ToolWin::Scripts => crate::scripts_ui::KEY,
            ToolWin::Results => crate::results::KEY,
            ToolWin::Library => crate::library_ui::KEY,
            ToolWin::Hunts => crate::dash_ui::KEY,
            ToolWin::CatalogTools => crate::catalog_tools_ui::KEY,
            ToolWin::Ants => crate::ants_ui::KEY,
            ToolWin::Sandpile => crate::pile_ui::KEY,
            ToolWin::HashLife => crate::hlview_ui::KEY,
            ToolWin::Palettes => "palettes",
            ToolWin::AiControl => "ai-control",
            ToolWin::TileEditor => crate::editor::KEY,
            ToolWin::CellEditor => crate::cell_editor::KEY,
            ToolWin::Shortcuts => "shortcuts",
            ToolWin::About => "about",
            ToolWin::Wallpaper => super::wallpaper_ui::KEY,
            ToolWin::Safety => "safety",
            ToolWin::Machine => super::machine_ui::KEY,
            ToolWin::Accessibility => "accessibility",
        }
    }

    pub fn from_key(k: &str) -> Option<ToolWin> {
        ToolWin::ALL.into_iter().find(|t| t.key() == k)
    }

    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            ToolWin::Scripts => "Scripts and sweeps…",
            ToolWin::Results => "Sweep results…",
            ToolWin::Library => "Pattern library…",
            ToolWin::Hunts => "Hunts…",
            ToolWin::CatalogTools => "Catalog tools…",
            ToolWin::Ants => "Ants and turmites…",
            ToolWin::Sandpile => "Large sandpile…",
            ToolWin::HashLife => "HashLife view…",
            ToolWin::Palettes => "Palettes…",
            ToolWin::AiControl => "AI control…",
            ToolWin::TileEditor => "Tile editor…",
            ToolWin::CellEditor => "Unit cell editor…",
            ToolWin::Shortcuts => "Keyboard shortcuts…",
            ToolWin::About => "About JETCARP…",
            ToolWin::Safety => "Safety…",
            ToolWin::Wallpaper => "Export wallpaper…",
            ToolWin::Machine => "This computer…",
            ToolWin::Accessibility => "Accessibility…",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            ToolWin::Scripts => "Rule sweeps (click to build) and Rhai scripts",
            ToolWin::Results => "Browse sweep folders: thumbnails, sorting, filters, favourites",
            ToolWin::Library => {
                "The pattern library: browse, curate, paste into this world, promote to the Quick Start"
            }
            ToolWin::Hunts => {
                "Every hunt and search job in the catalog: progress, heat map, finds, ratings"
            }
            ToolWin::CatalogTools => {
                "Collide patterns, export and import records, remove a record, run the evolutionary glider search"
            }
            ToolWin::Ants => {
                "Ant and turmite search: turn strings on unbounded tilings, highways proved exactly, every run in the catalog"
            }
            ToolWin::Sandpile => {
                "A large sandpile on an unbounded grid: N grains settled by doubling, drawn live, written to the piles folder in JETCARP's output folder"
            }
            ToolWin::HashLife => "This world's pattern on the unbounded plane, jumped any distance",
            ToolWin::Palettes => "Edit palettes and apply one to every CA",
            ToolWin::AiControl => {
                "Let an AI model (through `jetcarp mcp --app`) act on this world. Off by default; local only."
            }
            ToolWin::TileEditor => "Open the tile editor on the current tiling",
            ToolWin::CellEditor => {
                "Edit the lattice and all tiles of one cell: mixed tiles, any contact"
            }
            ToolWin::Shortcuts => "Every keyboard shortcut",
            ToolWin::About => "Version and where things are",
            ToolWin::Safety => "The strobe guard: what it does now, and its switches",
            ToolWin::Accessibility => {
                "Reduced motion, the high-contrast theme, light or dark, and the palettes for scenes that name none"
            }
            ToolWin::Wallpaper => {
                "Desktop-sized PNGs: each monitor, the span of all monitors, presets, or any size"
            }
            ToolWin::Machine => {
                "This computer's hardware, its JETCARP Hardware Score, and the limits set from them"
            }
        }
    }
}

/// Everything the menu bar (and the keyboard) can do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Action {
    QuickStart,
    NewEmpty,
    OpenScene,
    SaveScene,
    ExportPng,
    ExportView,
    ExportAnim,
    ExportSheet,
    Quit,
    Reset,
    ClearAll,
    CopyHash,
    CopyRle,
    PasteFromLibrary,
    StopPasting,
    Fit,
    ZoomIn,
    ZoomOut,
    CellOutlines,
    StatusBar,
    AutoHideMenu,
    HidePanes,
    RunPause,
    Step,
    MaxSpeed,
    GpuSim,
    TileJson,
    AddByNotation,
    LoadRuleTable,
    SeedAll,
    RandomAll,
    SweepThisWorld,
    ResetLayout,
    RefreshDisplays,
    /// The main window full screen (F11).
    FullScreen,
    /// The grid on the output windows (Display pane).
    ToggleOutputs,
    Screensaver,
    /// Zoom so one cell edge is the cell size in physical pixels.
    PixelZoom,
    /// Rebuild the tiling to cover the canvas at the cell size.
    FillCanvas,
    KeepAwake,
    IdentifyMonitors,
    /// Switch automatic reseeding on or off (installs a preset when there are no policies).
    AutoReseed,
    /// The setup wizard (`crate::setup_ui`).
    Setup,
    /// Show (and focus) a control pane, wherever it is.
    Pane(&'static str),
    /// Open or close a tool window.
    Tool(ToolWin),
}

/// A `&'static` list of shortcuts (const fn calls are not promoted to statics on their own).
macro_rules! ks {
    ($($k:expr),*) => {{
        const K: &[KeyboardShortcut] = &[$($k),*];
        K
    }};
}

const fn key(k: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::NONE, k)
}
const fn ctrl(k: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::COMMAND, k)
}
/// Ctrl+Shift (⌘⇧ on macOS) and a key.
const fn ctrl_shift(k: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), k)
}
/// ⌃⌘ and a key: macOS's own full-screen shortcut is ⌃⌘F. It never matches elsewhere.
const fn ctrl_cmd(k: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::CTRL.plus(Modifiers::MAC_CMD), k)
}

impl Action {
    /// Every action without a parameter, then every pane and tool window.
    pub fn all() -> Vec<Action> {
        use Action::*;
        let mut v = vec![
            QuickStart,
            NewEmpty,
            OpenScene,
            SaveScene,
            ExportPng,
            ExportView,
            ExportAnim,
            ExportSheet,
            Quit,
            Reset,
            ClearAll,
            CopyHash,
            CopyRle,
            PasteFromLibrary,
            StopPasting,
            Fit,
            ZoomIn,
            ZoomOut,
            CellOutlines,
            StatusBar,
            AutoHideMenu,
            HidePanes,
            RunPause,
            Step,
            MaxSpeed,
            GpuSim,
            TileJson,
            AddByNotation,
            LoadRuleTable,
            SeedAll,
            RandomAll,
            SweepThisWorld,
            ResetLayout,
            RefreshDisplays,
            FullScreen,
            ToggleOutputs,
            Screensaver,
            PixelZoom,
            FillCanvas,
            KeepAwake,
            IdentifyMonitors,
            AutoReseed,
            Setup,
        ];
        v.extend(panes::PANES.iter().map(|p| Pane(p.id)));
        v.extend(ToolWin::ALL.map(Tool));
        v
    }

    pub fn label(self) -> String {
        use Action::*;
        match self {
            QuickStart => "Quick Start".into(),
            NewEmpty => "New empty world".into(),
            OpenScene => "Open scene…".into(),
            SaveScene => "Save scene…".into(),
            ExportPng => "Export the world as PNG…".into(),
            ExportView => "Export the view as PNG…".into(),
            ExportAnim => "Export animation (GIF or WebP)…".into(),
            ExportSheet => "Export contact sheet…".into(),
            Quit => "Quit".into(),
            Reset => "Reset".into(),
            ClearAll => "Clear every CA".into(),
            CopyHash => "Copy state hash".into(),
            CopyRle => "Copy as RLE".into(),
            PasteFromLibrary => "Paste a library pattern…".into(),
            StopPasting => "Stop pasting".into(),
            Fit => "Fit".into(),
            ZoomIn => "Zoom in".into(),
            ZoomOut => "Zoom out".into(),
            CellOutlines => "Cell outlines".into(),
            StatusBar => "Status bar".into(),
            AutoHideMenu => "Auto-hide menu bar".into(),
            HidePanes => "Hide all panes".into(),
            RunPause => "Run / Pause".into(),
            Step => "Step".into(),
            MaxSpeed => "Max speed".into(),
            GpuSim => "GPU simulation".into(),
            TileJson => "Paste tile JSON…".into(),
            AddByNotation => "Add by notation…".into(),
            LoadRuleTable => "Load rule table…".into(),
            SeedAll => "Seed every CA".into(),
            RandomAll => "Random cells in every CA".into(),
            SweepThisWorld => "Sweep this world…".into(),
            ResetLayout => "Reset layout".into(),
            RefreshDisplays => "Refresh".into(),
            FullScreen => "Full screen".into(),
            ToggleOutputs => "Show on outputs".into(),
            Screensaver => "Screensaver mode".into(),
            PixelZoom => "Pixel-exact zoom".into(),
            FillCanvas => "Fill the canvas".into(),
            KeepAwake => "Keep awake".into(),
            IdentifyMonitors => "Identify monitors".into(),
            AutoReseed => "Auto reseed".into(),
            Setup => "Setup…".into(),
            Pane("screensaver") => "Screensaver settings…".into(),
            Pane("desktop") => "Desktop background settings…".into(),
            Pane(id) => panes::pane_def(id).map(|d| d.title).unwrap_or(id).into(),
            Tool(t) => t.label().into(),
        }
    }

    pub fn hint(self) -> &'static str {
        use Action::*;
        match self {
            QuickStart => "Back to the Quick Start list",
            NewEmpty => "A square grid with no CAs",
            ExportPng => {
                "The whole tiling, framed to fit, whatever the view shows (Export size sets its scale); large images are written in strips"
            }
            ExportView => "What the grid shows now, at the screen's resolution",
            ExportAnim => "The view now and the generations after it, as an animated GIF or WebP",
            ExportSheet => "A labelled PNG of the first elementary CA's rule and the ones after it",
            Reset => "Clear and reseed every CA",
            CopyHash => "Copy the world's state hash",
            CopyRle => {
                "The pattern as RLE, the format Golly and LifeViewer read, for the ConwayLife forums: B/S rules on the square and hexagon grids"
            }
            PasteFromLibrary => "Pick a pattern in the library, then click cells in the view",
            CellOutlines => "Draw tile outlines when tiles are large enough",
            AutoHideMenu => {
                "Hide the menu bar; it shows when the pointer touches the top edge, Alt is held, or F10 opens a menu"
            }
            HidePanes => "Let the grid fill the window (F9 shows the panes again)",
            GpuSim => {
                "Follow the shared setting, on, off: step on the GPU; results are identical to the CPU"
            }
            TileJson => "Load a tile from the prototype's JSON (in the Tiling pane)",
            SeedAll => "Apply each CA's own seed",
            RandomAll => "Add random live cells (30%) to every CA that takes them",
            SweepThisWorld => {
                "Open the sweep builder set up from this world: its tiling, size, CAs and seeds"
            }
            ResetLayout => "Dock every pane where it starts and close every popped-out window",
            FullScreen => "The main window full screen (Display pane: which monitor)",
            ToggleOutputs => {
                "The grid on chrome-less full-screen windows on the monitors chosen in the Display pane"
            }
            Screensaver => {
                "Hide every control and run the chosen profile or rotation on the outputs (or full screen); Esc leaves"
            }
            Pane("screensaver") => {
                "Screensaver profiles (scene, palette, reseeding, cells, speed), rotations, and options"
            }
            Pane("desktop") => {
                "The background host: the desktop background, its screens and monitors, the lock screen, and fitting it to this computer"
            }
            PixelZoom => "One cell edge is the Display pane's cell size in physical pixels",
            FillCanvas => "Rebuild this tiling to cover the canvas at the cell size",
            KeepAwake => "No screen blanking, locking, or suspend while JETCARP is open",
            IdentifyMonitors => "Show each monitor's number on it for three seconds",
            AutoReseed => {
                "Run the automatic reseed policies (the Reseed pane edits them); with none, install the Screensaver preset"
            }
            Setup => {
                "Choose what JETCARP sets up: packs, Quick Start entries of your own, the desktop"
            }
            Tool(t) => t.hint(),
            _ => "",
        }
    }

    /// Shortcuts here; the first is shown in the menu. Bare keys only act when no text field has
    /// keyboard focus.
    pub fn keys(self) -> &'static [KeyboardShortcut] {
        self.keys_for(cfg!(target_os = "macos"))
    }

    /// Shortcuts on macOS (`mac`) or on Windows and Linux, for the book's table of both.
    pub fn keys_for(self, mac: bool) -> &'static [KeyboardShortcut] {
        use Action::*;
        match self {
            // ⌘H hides the app on macOS, so Quick Start is Ctrl+Shift+H (⌘⇧H) everywhere, Ctrl+H
            // still working where the system leaves it alone (#91).
            QuickStart => ks!(ctrl_shift(Key::H), ctrl(Key::H)),
            NewEmpty => ks!(ctrl(Key::N)),
            OpenScene => ks!(ctrl(Key::O)),
            SaveScene => ks!(ctrl(Key::S)),
            ExportPng => ks!(ctrl(Key::E)),
            Quit => ks!(ctrl(Key::Q)),
            Reset => ks!(key(Key::R)),
            StopPasting => ks!(key(Key::Escape)),
            Fit => ks!(key(Key::F)),
            ZoomIn => ks!(key(Key::Plus), key(Key::Equals)),
            ZoomOut => ks!(key(Key::Minus)),
            // Tab moves the keyboard between controls (#219); F10 opens the menus.
            HidePanes => ks!(key(Key::F9)),
            RunPause => ks!(key(Key::Space)),
            Step => ks!(key(Key::S), key(Key::ArrowRight)),
            AutoHideMenu => ks!(ctrl(Key::F10)),
            // F11 shows the desktop on macOS; its own full-screen key is ⌃⌘F (#91).
            FullScreen => {
                if mac {
                    ks!(ctrl_cmd(Key::F), key(Key::F11))
                } else {
                    ks!(key(Key::F11))
                }
            }
            Screensaver => ks!(key(Key::F12)),
            PixelZoom => ks!(ctrl(Key::Num1)),
            Tool(ToolWin::Shortcuts) => ks!(key(Key::F1)),
            _ => &[],
        }
    }
}

/// Menu entries that are widgets rather than single actions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Custom {
    ExportSize,
    Jump,
    Speed,
    Family,
    Boundary,
    TilingCatalog,
    AddFromCatalog,
    SeedEach,
    PaneList,
    ToolList,
    Displays,
    ReseedNow,
    /// GPU simulation: automatic, on, off.
    Gpu,
}

pub(super) enum Item {
    A(Action),
    C(Custom),
    Sep,
}

use Action as A_;
use Item::{A, C, Sep};

/// The menu bar, left to right.
pub(super) const MENUS: &[(&str, &[Item])] = &[
    (
        "File",
        &[
            A(A_::QuickStart),
            A(A_::NewEmpty),
            Sep,
            A(A_::OpenScene),
            A(A_::SaveScene),
            Sep,
            A(A_::ExportPng),
            C(Custom::ExportSize),
            A(A_::ExportView),
            A(A_::ExportAnim),
            A(A_::ExportSheet),
            A(A_::Tool(ToolWin::Wallpaper)),
            Sep,
            A(A_::Quit),
        ],
    ),
    (
        "Edit",
        &[
            A(A_::Reset),
            A(A_::ClearAll),
            Sep,
            A(A_::CopyHash),
            A(A_::CopyRle),
            A(A_::PasteFromLibrary),
            A(A_::StopPasting),
        ],
    ),
    (
        "View",
        &[
            A(A_::Fit),
            A(A_::ZoomIn),
            A(A_::ZoomOut),
            A(A_::CellOutlines),
            Sep,
            A(A_::StatusBar),
            A(A_::AutoHideMenu),
            A(A_::HidePanes),
            Sep,
            A(A_::PixelZoom),
            A(A_::FillCanvas),
            Sep,
            A(A_::FullScreen),
            A(A_::ToggleOutputs),
            A(A_::Screensaver),
            A(A_::Pane("screensaver")),
            A(A_::Pane("desktop")),
            A(A_::KeepAwake),
            A(A_::Pane("display")),
        ],
    ),
    (
        "Run",
        &[
            A(A_::RunPause),
            A(A_::Step),
            C(Custom::Jump),
            Sep,
            C(Custom::Speed),
            A(A_::MaxSpeed),
            C(Custom::Gpu),
        ],
    ),
    (
        "Tiling",
        &[
            A(A_::Pane("tiling")),
            C(Custom::Family),
            C(Custom::Boundary),
            C(Custom::TilingCatalog),
            Sep,
            A(A_::Tool(ToolWin::TileEditor)),
            A(A_::Tool(ToolWin::CellEditor)),
            A(A_::TileJson),
        ],
    ),
    (
        "CA",
        &[
            C(Custom::AddFromCatalog),
            A(A_::AddByNotation),
            A(A_::LoadRuleTable),
            Sep,
            A(A_::Pane("cas")),
            A(A_::Pane("sequence")),
            A(A_::Tool(ToolWin::Palettes)),
        ],
    ),
    (
        "Seed",
        &[
            A(A_::SeedAll),
            A(A_::RandomAll),
            C(Custom::SeedEach),
            Sep,
            A(A_::ClearAll),
            A(A_::Reset),
            Sep,
            A(A_::Pane("reseed")),
            C(Custom::ReseedNow),
            A(A_::AutoReseed),
        ],
    ),
    (
        "Tools",
        &[
            A(A_::Tool(ToolWin::Scripts)),
            A(A_::SweepThisWorld),
            A(A_::Tool(ToolWin::Results)),
            Sep,
            A(A_::Tool(ToolWin::Library)),
            A(A_::Tool(ToolWin::Hunts)),
            A(A_::Tool(ToolWin::CatalogTools)),
            Sep,
            A(A_::Tool(ToolWin::Ants)),
            A(A_::Tool(ToolWin::Sandpile)),
            A(A_::Tool(ToolWin::HashLife)),
            A(A_::Pane("objects")),
            Sep,
            A(A_::Tool(ToolWin::AiControl)),
            A(A_::Tool(ToolWin::Machine)),
        ],
    ),
    (
        "Window",
        &[
            C(Custom::PaneList),
            Sep,
            C(Custom::ToolList),
            Sep,
            A(A_::HidePanes),
            A(A_::ResetLayout),
            Sep,
            C(Custom::Displays),
        ],
    ),
    (
        "Settings",
        &[
            A(A_::Tool(ToolWin::Safety)),
            A(A_::Tool(ToolWin::Accessibility)),
        ],
    ),
    (
        "Help",
        &[
            A(A_::Setup),
            Sep,
            A(A_::Tool(ToolWin::Shortcuts)),
            A(A_::Tool(ToolWin::About)),
        ],
    ),
];

/// Actions a custom entry holds (the coverage test; where the keyboard page says an action is).
fn custom_actions(c: Custom) -> Vec<Action> {
    match c {
        Custom::PaneList => panes::PANES.iter().map(|p| Action::Pane(p.id)).collect(),
        Custom::ToolList => ToolWin::ALL.map(Action::Tool).to_vec(),
        Custom::Displays => vec![Action::RefreshDisplays, Action::IdentifyMonitors],
        Custom::Gpu => vec![Action::GpuSim],
        _ => vec![],
    }
}

/// The menu an action is in: the first that has it as an item of its own, else the first that
/// lists it (the Window menu lists every tool window).
pub(super) fn menu_of(a: Action) -> Option<&'static str> {
    let has = |own: bool| {
        MENUS.iter().find_map(|(title, items)| {
            items
                .iter()
                .any(|it| match it {
                    A(x) => *x == a,
                    C(c) => !own && custom_actions(*c).contains(&a),
                    Sep => false,
                })
                .then_some(*title)
        })
    };
    has(true).or_else(|| has(false))
}

/// Every action reachable from the menu bar.
#[cfg(test)]
fn menu_actions() -> Vec<Action> {
    let mut v = vec![];
    for (_, items) in MENUS {
        for it in *items {
            match it {
                A(a) => v.push(*a),
                C(c) => v.extend(custom_actions(*c)),
                Sep => {}
            }
        }
    }
    v
}

impl JetcarpApp {
    pub(super) fn tool_open(&self, t: ToolWin) -> bool {
        match t {
            ToolWin::Scripts => self.scripts.open,
            ToolWin::Results => self.results.open,
            ToolWin::Library => self.library.open,
            ToolWin::Hunts => self.dash.open,
            ToolWin::CatalogTools => self.catalog_tools.open,
            ToolWin::Ants => self.ants.open,
            ToolWin::Sandpile => self.pile.open,
            ToolWin::HashLife => self.hlview.open,
            ToolWin::Palettes => self.pal_open,
            ToolWin::AiControl => self.ai.window_open,
            ToolWin::TileEditor => self.editor.open,
            ToolWin::CellEditor => self.cell_editor.open,
            ToolWin::Shortcuts => self.shell.shortcuts_open,
            ToolWin::About => self.shell.about_open,
            ToolWin::Wallpaper => self.wallpaper.open,
            ToolWin::Safety => self.strobe.window_open,
            ToolWin::Machine => self.machine.open,
            ToolWin::Accessibility => self.a11y.window_open,
        }
    }

    /// Open or close a tool window, loading what it shows when it opens.
    pub(super) fn set_tool_open(&mut self, t: ToolWin, on: bool) {
        if !on {
            match t {
                ToolWin::Scripts => self.scripts.open = false,
                ToolWin::Results => self.results.open = false,
                ToolWin::Library => self.library.open = false,
                ToolWin::Hunts => self.dash.open = false,
                ToolWin::CatalogTools => self.catalog_tools.open = false,
                ToolWin::Ants => self.ants.open = false,
                ToolWin::Sandpile => self.pile.open = false,
                ToolWin::HashLife => self.hlview.open = false,
                ToolWin::Palettes => self.pal_open = false,
                ToolWin::AiControl => self.ai.window_open = false,
                ToolWin::TileEditor => self.editor.open = false,
                ToolWin::CellEditor => self.cell_editor.open = false,
                ToolWin::Shortcuts => self.shell.shortcuts_open = false,
                ToolWin::About => self.shell.about_open = false,
                ToolWin::Wallpaper => self.wallpaper.open = false,
                ToolWin::Safety => self.strobe.window_open = false,
                ToolWin::Machine => self.machine.open = false,
                ToolWin::Accessibility => self.a11y.window_open = false,
            }
            return;
        }
        match t {
            ToolWin::Scripts => self.scripts.open = true,
            ToolWin::Results => self.results.open = true,
            ToolWin::Library => {
                self.library.open = true;
                self.library.reload();
            }
            ToolWin::Hunts => self.dash.open = true,
            ToolWin::CatalogTools => self.catalog_tools.open = true,
            ToolWin::Ants => self.ants.open = true,
            ToolWin::Sandpile => self.pile.open = true,
            ToolWin::HashLife => {
                self.playing = false;
                if let Err(e) = self.hlview.load(&self.world) {
                    self.message = Some((e, true));
                }
            }
            ToolWin::Palettes => {
                self.pal_open = true;
                if self.pal_stops.is_empty() {
                    self.pick_palette("Ember");
                }
            }
            ToolWin::AiControl => self.ai.window_open = true,
            ToolWin::TileEditor => {
                self.editor.load_spec(&self.world.topo.spec);
                self.editor.open = true;
            }
            ToolWin::CellEditor => {
                let spec = &self.world.topo.spec;
                self.message = self.cell_editor.load_spec(spec).err().map(|e| (e, true));
                self.cell_editor.open = true;
            }
            ToolWin::Shortcuts => self.shell.shortcuts_open = true,
            ToolWin::About => self.shell.about_open = true,
            ToolWin::Wallpaper => self.wallpaper.open = true,
            ToolWin::Safety => self.strobe.window_open = true,
            ToolWin::Machine => {
                self.machine.open = true;
                self.machine.reload();
            }
            ToolWin::Accessibility => self.a11y.window_open = true,
        }
    }

    fn sheet_ca(&self) -> Option<&world::CaInstance> {
        self.world
            .cas
            .iter()
            .find(|c| jetcarp_core::sheet::SheetKind::of_rule(&c.rule).is_some())
    }

    fn aperiodic_family(&self) -> bool {
        jetcarp_core::families::family(&self.t_family)
            .is_some_and(|f| f.kind == FamilyKind::Aperiodic)
    }

    pub(super) fn action_enabled(&self, a: Action) -> bool {
        use Action::*;
        let sim = self.screen == Screen::Sim;
        match a {
            SaveScene | ExportPng | ExportView | ExportAnim | Reset | ClearAll | CopyHash
            | CopyRle | Fit | ZoomIn | ZoomOut | RunPause | Step | SeedAll | RandomAll
            | SweepThisWorld | AddByNotation | LoadRuleTable | TileJson | Pane(_) | PixelZoom
            | FillCanvas | AutoReseed => sim,
            ExportSheet => sim && self.sheet_ca().is_some(),
            StopPasting => self.paste.is_some(),
            GpuSim => self.sim.available(),
            Tool(
                ToolWin::TileEditor | ToolWin::HashLife | ToolWin::Palettes | ToolWin::Wallpaper,
            ) => sim,
            Tool(ToolWin::CellEditor) => sim && !self.aperiodic_family(),
            HidePanes => sim,
            _ => true,
        }
    }

    /// Some: a toggle, checked or not.
    pub(super) fn action_checked(&self, a: Action) -> Option<bool> {
        use Action::*;
        Some(match a {
            RunPause => self.playing,
            MaxSpeed => self.max_speed,
            GpuSim => self.sim.mode != jetcarp_gpu::select::Mode::Off,
            CellOutlines => self.shell.cell_outlines,
            StatusBar => self.shell.status_bar,
            AutoHideMenu => self.shell.menu_autohide,
            HidePanes => self.shell.panes_hidden,
            FullScreen => self.shell.display.mode == super::display::OutputMode::FullScreen,
            ToggleOutputs => self.shell.display.mode == super::display::OutputMode::Outputs,
            Screensaver => self.saver_active(),
            KeepAwake => self.shell.display.keep_awake,
            AutoReseed => self.auto_reseed_on(),
            Tool(t) => self.tool_open(t),
            _ => return None,
        })
    }

    /// Do an action (from the menu or a shortcut).
    /// The app's own GPU choice: follow the shared setting, on or off (saved in app.json).
    pub(super) fn set_gpu_choice(&mut self, own: jetcarp_gpu::select::Own) {
        self.shell.gpu = own;
        self.sim.mode = crate::simgpu::app_policy(own).mode;
    }

    /// Run → GPU adapter (#59): which adapter the shared setting names (`gpu_adapter` in
    /// `gpu.json`), so a laptop with two GPUs can move JETCARP to the other one from the app. The
    /// app picks its adapter when it starts, so it moves at its next start; the command line, the
    /// MCP server and the background host follow at theirs.
    fn gpu_adapter_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("GPU adapter", |ui| {
            let now = crate::simgpu::shared_adapter();
            let found = self
                .gpu_adapters
                .get_or_insert_with(jetcarp_gpu::select::adapter_names)
                .clone();
            let mut entries: Vec<(Option<String>, String)> = vec![
                (None, "Automatic: the best one for each use".into()),
                (Some("discrete".into()), "Discrete".into()),
                (Some("integrated".into()), "Integrated".into()),
            ];
            for (name, kind) in &found {
                entries.push((Some(name.clone()), format!("{name} ({kind})")));
            }
            let same = |a: &Option<String>, b: &Option<String>| {
                a.as_deref().map(str::to_ascii_lowercase)
                    == b.as_deref().map(str::to_ascii_lowercase)
            };
            // A name saved on another computer, or for an adapter that has gone, stays visible.
            if now.is_some() && !entries.iter().any(|(v, _)| same(v, &now)) {
                let n = now.clone().unwrap_or_default();
                entries.push((now.clone(), format!("{n} (saved; not found here)")));
            }
            let mut picked = None;
            for (i, (value, label)) in entries.iter().enumerate() {
                if i == 3 {
                    ui.separator();
                }
                if ui.radio(same(value, &now), label.as_str()).clicked() {
                    picked = Some(value.clone());
                    ui.close();
                }
            }
            ui.label(
                RichText::new(
                    "Shared with the command line, the MCP server and the background host. The \
                     app moves to it when it next starts; hover over GPU simulation for the \
                     adapter it uses now.",
                )
                .small()
                .weak(),
            );
            if let Ok(v) = std::env::var("JETCARP_GPU") {
                ui.colored_label(
                    crate::theme::ink(ui, egui::Color32::LIGHT_RED),
                    format!("JETCARP_GPU={v} is set, and overrides this"),
                );
            }
            if let Some(value) = picked {
                let what = value.clone().unwrap_or_else(|| "automatic".into());
                self.message = Some(match crate::simgpu::save_shared_adapter(value) {
                    Ok(()) => (
                        format!(
                            "GPU adapter saved ({what}): the app moves to it when it next starts"
                        ),
                        false,
                    ),
                    Err(e) => (format!("GPU adapter not saved: {e}"), true),
                });
            }
        });
    }

    /// The shared GPU setting every part of JETCARP follows unless it has a choice of its own.
    pub(super) fn set_shared_gpu(&mut self, m: jetcarp_gpu::select::Mode) {
        match crate::simgpu::save_shared(m) {
            Ok(()) => self.sim.mode = crate::simgpu::app_policy(self.shell.gpu).mode,
            Err(e) => self.message = Some((format!("GPU setting not saved: {e}"), true)),
        }
    }

    pub(super) fn run_action(&mut self, ctx: &egui::Context, a: Action) {
        use Action::*;
        if !self.action_enabled(a) {
            return;
        }
        match a {
            QuickStart => {
                self.playing = false;
                self.screen = Screen::Start;
                self.refresh_finds();
            }
            NewEmpty => self.empty_world(),
            OpenScene => self.open_scene(),
            SaveScene => {
                self.commit_rule_edit(ctx);
                self.save_scene()
            }
            ExportPng => self.export_png(),
            ExportView => self.export_view(),
            ExportAnim => self.anim.open = true,
            ExportSheet => {
                if let Some(ca) = self.sheet_ca()
                    && let Some((kind, n)) = jetcarp_core::sheet::SheetKind::of_rule(&ca.rule)
                {
                    let random = matches!(ca.seed, SeedSpec::TopRandom { .. });
                    self.pending_sheet = Some((kind, n, ca.palette.clone(), random));
                }
            }
            Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Reset => {
                self.exec(Command::Reset);
            }
            ClearAll => {
                for id in self.world.cas.iter().map(|c| c.id).collect::<Vec<_>>() {
                    self.exec(Command::Clear { id });
                }
            }
            CopyHash => ctx.copy_text(self.world.hash_hex()),
            CopyRle => {
                let note = format!(
                    "Copied from JETCARP {} at generation {}; state hash {}",
                    env!("CARGO_PKG_VERSION"),
                    self.world.generation,
                    self.world.hash_hex()
                );
                self.message = Some(match jetcarp_api::rle::of_world(&self.world, &[note]) {
                    Ok(text) => {
                        ctx.copy_text(text);
                        (
                            "Copied the pattern as RLE, for Golly, LifeViewer and the ConwayLife forums."
                                .into(),
                            false,
                        )
                    }
                    Err(e) => (e, true),
                });
            }
            PasteFromLibrary => {
                self.set_tool_open(ToolWin::Library, true);
                self.message = Some((
                    "Pick a pattern in the library and press Paste, then click cells in the view."
                        .into(),
                    false,
                ));
            }
            StopPasting => {
                self.paste = None;
                self.message = None;
            }
            Fit => self.fit_pending = true,
            ZoomIn => self.zoom_by(1.25),
            ZoomOut => self.zoom_by(0.8),
            CellOutlines => self.shell.cell_outlines = !self.shell.cell_outlines,
            StatusBar => self.shell.status_bar = !self.shell.status_bar,
            AutoHideMenu => self.shell.menu_autohide = !self.shell.menu_autohide,
            HidePanes => self.shell.panes_hidden = !self.shell.panes_hidden,
            RunPause => self.playing = !self.playing,
            Step => {
                self.playing = false;
                self.exec(Command::Step { gens: 1 });
            }
            MaxSpeed => self.max_speed = !self.max_speed,
            GpuSim => self.set_gpu_choice(crate::simgpu::next_choice(self.shell.gpu)),
            TileJson | Pane(_) | AddByNotation => {
                let id = match a {
                    TileJson => "tiling",
                    AddByNotation => "add",
                    Pane(id) => id,
                    _ => unreachable!(),
                };
                self.shell.panes_hidden = false;
                self.shell.layout.show_pane(id);
            }
            LoadRuleTable => self.load_table(),
            SeedAll => {
                let seeds: Vec<_> = self
                    .world
                    .cas
                    .iter()
                    .map(|c| (c.id, c.seed.clone()))
                    .collect();
                for (id, seed) in seeds {
                    self.exec(Command::Seed { id, seed });
                }
            }
            RandomAll => {
                let ids: Vec<_> = self
                    .world
                    .cas
                    .iter()
                    .filter(|c| takes_random(&c.rule))
                    .map(|c| c.id)
                    .collect();
                for id in ids {
                    self.exec(Command::Seed {
                        id,
                        seed: SeedSpec::Random { density: 0.3 },
                    });
                }
            }
            SweepThisWorld => self.scripts.sweep_from_world(&self.world),
            ResetLayout => self.reset_layout(ctx),
            RefreshDisplays => self.shell.monitors.refresh(),
            FullScreen => self.toggle_fullscreen(),
            ToggleOutputs => self.toggle_outputs(),
            Screensaver => self.saver_toggle(ctx),
            PixelZoom => self.pixel_zoom(self.shell.display.cell_px as f64),
            FillCanvas => self.fill_canvas(),
            KeepAwake => self.shell.display.keep_awake = !self.shell.display.keep_awake,
            IdentifyMonitors => self.identify_monitors(ctx),
            AutoReseed => self.toggle_auto_reseed(),
            Setup => self.setup.show_again(),
            Tool(t) => {
                let on = !self.tool_open(t);
                self.set_tool_open(t, on);
            }
        }
    }

    /// Keyboard shortcuts, from the action table. A key with no Ctrl is typing while a text field
    /// has the keyboard; Space, Enter, Tab and the arrows belong to whatever control has it (they
    /// press it, or move in it or from it). The other single keys (R, F, S, + and −) and the
    /// function keys act while a button or check box has the keyboard, and Esc always stops
    /// pasting.
    pub(super) fn shortcuts(&mut self, ctx: &egui::Context) {
        // macOS's Minimise: the app has no Window menu for the system's shortcut to act on (#98).
        #[cfg(target_os = "macos")]
        mac_minimise_shortcut(ctx);
        let typing = ctx.text_edit_focused();
        let focused = ctx.memory(|m| m.focused().is_some());
        for a in Action::all() {
            for k in a.keys() {
                let bare = k.modifiers.is_none()
                    && k.logical_key != Key::Escape
                    && !function_key(k.logical_key);
                if bare && (typing || (focused && super::keyboard::widget_key(k.logical_key))) {
                    continue;
                }
                if !self.action_enabled(a) {
                    continue;
                }
                if ctx.input_mut(|i| i.consume_shortcut(k)) {
                    self.run_action(ctx, a);
                    break;
                }
            }
        }
    }

    /// The menu bar's menus.
    pub(super) fn menu_bar_contents(&mut self, ui: &mut egui::Ui) {
        let mut buttons = vec![];
        for (n, (title, items)) in MENUS.iter().enumerate() {
            let r = ui.menu_button(*title, |ui| {
                ui.set_min_width(220.0);
                for it in *items {
                    match it {
                        A(a) => self.menu_item(ui, *a),
                        C(c) => self.custom_item(ui, *c),
                        Sep => {
                            ui.separator();
                        }
                    }
                }
            });
            let popup = egui::Popup::default_response_id(&r.response);
            buttons.push((r.response.id, popup));
            // F10: the keyboard on File, its menu open (↓ goes into it).
            if n == 0 && std::mem::take(&mut self.kb.menu_request) {
                r.response.request_focus();
                egui::Popup::open_id(ui.ctx(), popup);
            }
            // `JETCARP_MENU=<title>` (screenshots): keep that menu open.
            if self.shell.open_menu.as_deref() == Some(*title) {
                egui::Popup::open_id(ui.ctx(), popup);
            }
        }
        self.kb.menu_buttons = buttons;
    }

    fn menu_item(&mut self, ui: &mut egui::Ui, a: Action) {
        let enabled = self.action_enabled(a);
        let mut label = a.label();
        if let Action::Tool(ToolWin::Scripts) = a
            && self.scripts.is_running()
        {
            label.push_str(" ●");
        }
        if let Action::Tool(ToolWin::Hunts) = a
            && self.dash.busy()
        {
            label.push_str(" ●");
        }
        if let Action::Tool(ToolWin::AiControl) = a
            && self.ai_port().is_some()
        {
            label = format!("AI control: on ({})…", self.ai_port().unwrap_or(0));
        }
        if a == Action::ExportPng {
            let (ew, eh, _) = render::render_size(&self.world.topo, &self.export_options());
            label = format!("Export the world as PNG ({ew} × {eh})…");
        }
        let checked = self.action_checked(a);
        let mut b = match checked {
            Some(c) => egui::Button::new((check_atom(c), label)),
            None => egui::Button::new(label),
        };
        if let Some(k) = a.keys().first() {
            b = b.shortcut_text(ui.ctx().format_shortcut(k));
        }
        let mut r = ui.add_enabled(enabled, b);
        if a == Action::GpuSim {
            r = r.on_hover_text(self.sim.hint());
        } else if !a.hint().is_empty() {
            r = r.on_hover_text(a.hint());
        }
        if r.clicked() {
            let ctx = ui.ctx().clone();
            self.run_action(&ctx, a);
            // Toggles keep the menu open; everything else closes it.
            if checked.is_none() || matches!(a, Action::Tool(_)) {
                ui.close();
            }
        }
    }

    fn custom_item(&mut self, ui: &mut egui::Ui, c: Custom) {
        let sim = self.screen == Screen::Sim;
        match c {
            Custom::ExportSize => {
                ui.horizontal(|ui| {
                    ui.label("    Export size");
                    ui.add(
                        egui::DragValue::new(&mut self.export_px)
                            .range(0.0..=64.0)
                            .speed(0.1)
                            .custom_formatter(|v, _| {
                                if v <= 0.0 {
                                    "fit".into()
                                } else {
                                    format!("{v:.0} px/tile")
                                }
                            }),
                    )
                    .on_hover_text("Pixels per tile, or 'fit' for 2400 pixels on the long side");
                });
            }
            Custom::Jump => {
                let mut jump = None;
                ui.add_enabled_ui(sim, |ui| {
                    // The menu holds a text field (the generations), so a click inside it must
                    // not close it, or the number can't be typed (#88); its buttons close it.
                    use egui::containers::menu;
                    let cfg = menu::MenuConfig::new()
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
                    let body = |ui: &mut egui::Ui| {
                        jump = crate::jump_ui::menu(ui, &self.world, &mut self.jump_gens);
                        if jump.is_some() {
                            ui.close();
                        }
                    };
                    if menu::is_in_menu(ui) {
                        menu::SubMenuButton::new("Jump").config(cfg).ui(ui, body);
                    } else {
                        menu::MenuButton::new("Jump").config(cfg).ui(ui, body);
                    }
                });
                if let Some(j) = jump {
                    self.do_jump(j);
                }
            }
            Custom::Speed => {
                ui.horizontal(|ui| {
                    ui.label("    Speed");
                    ui.add_enabled(
                        !self.max_speed,
                        egui::Slider::new(&mut self.speed, 1.0..=240.0)
                            .logarithmic(true)
                            .suffix(" gen/s"),
                    );
                });
            }
            Custom::Family => {
                ui.add_enabled_ui(sim, |ui| {
                    ui.menu_button("Family", |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(560.0)
                            .show(ui, |ui| {
                                let mut group = "";
                                for f in FAMILIES {
                                    if f.group != group {
                                        group = f.group;
                                        ui.label(RichText::new(group).weak().small());
                                    }
                                    if ui
                                        .selectable_label(
                                            self.t_family == f.id,
                                            format!("{}  ({})", f.name, f.sym),
                                        )
                                        .clicked()
                                    {
                                        if self.t_family != f.id {
                                            self.pick_family(f.id);
                                        }
                                        ui.close();
                                    }
                                }
                            });
                    });
                });
            }
            Custom::Gpu => {
                use jetcarp_gpu::select::Own;
                let hint = self.sim.hint();
                let shared = crate::simgpu::mode_label(crate::simgpu::shared_mode());
                ui.add_enabled_ui(self.sim.available(), |ui| {
                    ui.menu_button("GPU simulation", |ui| {
                        for (o, label) in [
                            (Own::Follow, format!("Follow the shared setting ({shared})")),
                            (Own::On, "On".into()),
                            (Own::Off, "Off".into()),
                        ] {
                            if ui.radio(self.shell.gpu == o, label).clicked() {
                                self.set_gpu_choice(o);
                                ui.close();
                            }
                        }
                        ui.label(
                            RichText::new(
                                "The shared setting is in the Desktop background pane; the app, \
                                 the background host and the command line follow it unless they \
                                 have a choice of their own.",
                            )
                            .small()
                            .weak(),
                        );
                    })
                    .response
                    .on_hover_text(hint);
                });
                self.gpu_adapter_menu(ui);
            }
            Custom::Boundary => {
                ui.add_enabled_ui(sim && !self.aperiodic_family(), |ui| {
                    ui.menu_button("Edges", |ui| {
                        for (b, label) in [
                            (Boundary::Dead, "Dead"),
                            (Boundary::Wrap, "Wrap"),
                            (Boundary::Live, "Live"),
                        ] {
                            if ui.radio(self.t_boundary == b, label).clicked() {
                                self.t_boundary = b;
                                self.apply_tiling_ui(true);
                                ui.close();
                            }
                        }
                    });
                });
            }
            Custom::TilingCatalog => {
                ui.add_enabled_ui(sim, |ui| {
                    ui.menu_button("Tiling catalog", |ui| {
                        ui.set_min_width(360.0);
                        if self.catalog_picker_ui(ui) {
                            ui.close();
                        }
                    });
                });
            }
            Custom::AddFromCatalog => {
                ui.add_enabled_ui(sim, |ui| {
                    ui.menu_button("Add from catalog", |ui| {
                        if let Some(i) = rule_menu(ui, None) {
                            let spec = CaSpec::new(catalog()[i].notation);
                            if let Some(a) = self.exec(Command::AddCa { ca: spec }) {
                                self.draw_target = a.new_id;
                            }
                            ui.close();
                        }
                    });
                });
            }
            Custom::SeedEach => {
                ui.add_enabled_ui(sim && !self.world.cas.is_empty(), |ui| {
                    ui.menu_button("Each CA", |ui| {
                        let cas: Vec<_> = self
                            .world
                            .cas
                            .iter()
                            .map(|c| (c.id, c.name.clone(), c.seed.clone(), takes_random(&c.rule)))
                            .collect();
                        for (id, name, seed, random) in cas {
                            ui.menu_button(name, |ui| {
                                if ui
                                    .button("Seed")
                                    .on_hover_text(format!("{seed:?}"))
                                    .clicked()
                                {
                                    self.exec(Command::Seed {
                                        id,
                                        seed: seed.clone(),
                                    });
                                }
                                if random && ui.button("Random (30%)").clicked() {
                                    self.exec(Command::Seed {
                                        id,
                                        seed: SeedSpec::Random { density: 0.3 },
                                    });
                                }
                                if ui.button("Clear").clicked() {
                                    self.exec(Command::Clear { id });
                                }
                            });
                        }
                    });
                });
            }
            Custom::PaneList => {
                ui.label(RichText::new("Panes").weak().small());
                for def in panes::PANES {
                    let place = self.shell.layout.place(def.id);
                    ui.menu_button(format!("{}  ({})", def.title, place.label()), |ui| {
                        for p in panes::Place::ALL {
                            if ui.radio(place == p, p.label()).clicked() {
                                self.shell.layout.set_place(def.id, p);
                                if p != panes::Place::Hidden {
                                    self.shell.panes_hidden = false;
                                }
                                ui.close();
                            }
                        }
                    });
                }
            }
            Custom::ToolList => {
                ui.label(RichText::new("Tool windows").weak().small());
                let ctx = ui.ctx().clone();
                for t in ToolWin::ALL {
                    let open = self.tool_open(t);
                    let popped = crate::popout::is_popped(&ctx, t.key());
                    let state = match (open, popped) {
                        (false, _) => "closed",
                        (true, false) => "open",
                        (true, true) => "own window",
                    };
                    let title = t.label().trim_end_matches('…');
                    ui.menu_button(format!("{title}  ({state})"), |ui| {
                        let enabled = self.action_enabled(Action::Tool(t));
                        let mut o = open;
                        if ui.add_enabled(enabled, egui::Checkbox::new(&mut o, "Open")).changed() {
                            self.set_tool_open(t, o);
                        }
                        let mut p = popped;
                        if ui
                            .checkbox(&mut p, "In its own window")
                            .on_hover_text("Pop out to an OS window (any monitor); off: float in the main window")
                            .changed()
                        {
                            crate::popout::set_popped(&ctx, t.key(), p);
                        }
                    });
                }
            }
            Custom::ReseedNow => self.reseed_now_menu(ui),
            Custom::Displays => {
                ui.menu_button("Displays", |ui| {
                    let list = self.shell.monitors.list();
                    if list.is_empty() {
                        ui.label(RichText::new("No monitors reported (yet).").weak());
                    }
                    for m in list {
                        let mut t = format!("{}: {}", m.index, m.describe());
                        if m.current {
                            t.push_str("  (this window)");
                        }
                        ui.label(RichText::new(t).monospace());
                    }
                    ui.separator();
                    self.menu_item(ui, Action::RefreshDisplays);
                    self.menu_item(ui, Action::IdentifyMonitors);
                    self.menu_item(ui, Action::Pane("display"));
                });
            }
        }
    }

    /// Run one of the Jump menu's choices.
    pub(super) fn do_jump(&mut self, j: crate::jump_ui::JumpAction) {
        match j {
            crate::jump_ui::JumpAction::InPlace(n) => {
                self.playing = false;
                self.exec(Command::Jump { gens: n });
            }
            crate::jump_ui::JumpAction::Plane(n) => {
                match crate::jump_ui::plane_jump(&self.world, n) {
                    Ok(w) => {
                        let name = format!("{} +{n} (HashLife plane)", self.scene_name);
                        let desc = format!(
                            "The pattern of “{}” {n} generations later on the unbounded \
                         plane (edges ignored), shown in the same frame.",
                            self.scene_name
                        );
                        self.show_world(w, name, desc);
                    }
                    Err(e) => self.message = Some((e, true)),
                }
            }
            crate::jump_ui::JumpAction::View => {
                self.playing = false;
                if let Err(e) = self.hlview.load(&self.world) {
                    self.message = Some((e, true));
                }
            }
        }
    }

    /// The keyboard shortcuts window.
    pub(super) fn shortcuts_window(&mut self, ctx: &egui::Context) {
        let mut open = self.shell.shortcuts_open;
        crate::popout::tool_window(
            ctx,
            crate::popout::Spec::new(ToolWin::Shortcuts.key(), "Keyboard shortcuts")
                .size(560.0, 560.0),
            &mut open,
            |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.label(
                        RichText::new(
                            "Keys without Ctrl are typing while a text field has the keyboard; \
                         Space, Enter and the arrows belong to the control that has it.",
                        )
                        .small()
                        .weak(),
                    );
                    let mac = cfg!(target_os = "macos");
                    // The same table as `jetcarp shortcuts` and the book's keyboard page.
                    for (n, g) in super::keyboard::table().iter().enumerate() {
                        ui.add_space(6.0);
                        ui.label(RichText::new(&g.name).strong());
                        egui::Grid::new(("shortcut-grid", n))
                            .striped(true)
                            .show(ui, |ui| {
                                for s in &g.keys {
                                    let keys = if mac { &s.mac } else { &s.keys };
                                    if keys.is_empty() {
                                        continue;
                                    }
                                    ui.label(RichText::new(keys).monospace());
                                    ui.add(egui::Label::new(&s.what).wrap());
                                    ui.end_row();
                                }
                            });
                    }
                    ui.add_space(6.0);
                    egui::Grid::new("shortcut-mouse")
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label(RichText::new("Right drag").monospace());
                            ui.label("Pan (any tool); scroll zooms");
                            ui.end_row();
                        });
                });
            },
        );
        self.shell.shortcuts_open = open;
    }

    pub(super) fn about_window(&mut self, ctx: &egui::Context) {
        let mut open = self.shell.about_open;
        let settings = self.shell.store.path().map(|p| p.display().to_string());
        let out = self.out_root.display().to_string();
        crate::popout::tool_window(
            ctx,
            crate::popout::Spec::new(ToolWin::About.key(), "About JETCARP").size(420.0, 200.0),
            &mut open,
            |ui| {
                ui.heading("JETCARP");
                ui.label(format!(
                    "Cellular automata research tool, version {VERSION}."
                ));
                ui.add_space(6.0);
                egui::Grid::new("about-grid").show(ui, |ui| {
                    ui.label("Settings");
                    ui.label(
                        RichText::new(
                            settings
                                .as_deref()
                                .unwrap_or("not saved (screenshot or test run)"),
                        )
                        .monospace(),
                    );
                    ui.end_row();
                    ui.label("Output folder");
                    ui.label(RichText::new(&out).monospace());
                    ui.end_row();
                });
            },
        );
        self.shell.about_open = open;
    }
}

/// F1 to F12: never typing, so they act whatever has the keyboard.
fn function_key(k: Key) -> bool {
    use Key::*;
    matches!(
        k,
        F1 | F2 | F3 | F4 | F5 | F6 | F7 | F8 | F9 | F10 | F11 | F12
    )
}

/// A small check box drawn in a menu item.
fn check_atom(checked: bool) -> egui::Atom<'static> {
    egui::Atom::paint(egui::vec2(12.0, 12.0), move |ui, args| {
        let r = args.rect.shrink(1.0);
        let p = ui.painter();
        p.rect_stroke(
            r,
            2.0,
            egui::Stroke::new(1.0, args.fallback_text_color.gamma_multiply(0.7)),
            egui::StrokeKind::Inside,
        );
        if checked {
            let s = egui::Stroke::new(1.8, args.fallback_text_color);
            let pts = [
                r.left_top() + egui::vec2(r.width() * 0.2, r.height() * 0.5),
                r.left_top() + egui::vec2(r.width() * 0.42, r.height() * 0.75),
                r.left_top() + egui::vec2(r.width() * 0.82, r.height() * 0.22),
            ];
            p.line(pts.to_vec(), s);
        }
    })
}

/// ⌘M on macOS: minimise the window through AppKit, as its yellow button does, never through
/// egui's `ViewportCommand::Minimized`, which left egui counting the window as hidden after a
/// restore from the Dock, so it was never drawn again (#131; see `macwin`).
#[cfg(target_os = "macos")]
fn mac_minimise_shortcut(ctx: &egui::Context) {
    if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::MAC_CMD, Key::M))) {
        crate::macwin::miniaturize();
    }
}

/// Whether "Random" seeding makes sense for a rule (not ants, rows, sandpiles, wires, JvN).
pub(super) fn takes_random(rule: &Rule) -> bool {
    !rule.is_agent()
        && !matches!(
            rule,
            Rule::Wolfram(_) | Rule::Sandpile | Rule::WireWorld | Rule::VonNeumann
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_is_in_a_menu() {
        let reachable = menu_actions();
        for a in Action::all() {
            assert!(
                reachable.contains(&a),
                "{a:?} ({}) is not in any menu",
                a.label()
            );
        }
    }

    /// Every button of the old top bar and side panel, and where it went.
    #[test]
    fn former_buttons_are_reachable() {
        use Action::*;
        let former: &[(&str, Action)] = &[
            ("< Quick Start", QuickStart),
            ("Open…", OpenScene),
            ("Save scene…", SaveScene),
            ("Palettes…", Tool(ToolWin::Palettes)),
            ("Export PNG…", ExportPng),
            ("AI control…", Tool(ToolWin::AiControl)),
            ("Scripts…", Tool(ToolWin::Scripts)),
            ("Sweep this world…", SweepThisWorld),
            ("Results…", Tool(ToolWin::Results)),
            ("Library…", Tool(ToolWin::Library)),
            ("Hunts…", Tool(ToolWin::Hunts)),
            ("Catalog tools…", Tool(ToolWin::CatalogTools)),
            ("Ants…", Tool(ToolWin::Ants)),
            ("Sandpile…", Tool(ToolWin::Sandpile)),
            ("Run / Pause", RunPause),
            ("Step", Step),
            ("Reset", Reset),
            ("Fit", Fit),
            ("Max", MaxSpeed),
            ("GPU simulation", GpuSim),
            ("Edit tile…", Tool(ToolWin::TileEditor)),
            ("Edit unit cell…", Tool(ToolWin::CellEditor)),
            ("Load rule table…", LoadRuleTable),
            ("Contact sheet…", ExportSheet),
            ("HashLife view… (Jump menu)", Tool(ToolWin::HashLife)),
            ("Tiling section", Pane("tiling")),
            ("Cellular automata section", Pane("cas")),
            ("Sequence section", Pane("sequence")),
            ("Add a CA section", Pane("add")),
            ("Mouse section", Pane("draw")),
            ("Objects section", Pane("objects")),
            ("Command log section", Pane("log")),
        ];
        let reachable = menu_actions();
        for (old, a) in former {
            assert!(
                reachable.contains(a),
                "former button {old:?} ({a:?}) is not in a menu"
            );
        }
        // The widgets that are not single actions.
        let customs: Vec<Custom> = MENUS
            .iter()
            .flat_map(|(_, items)| items.iter())
            .filter_map(|i| match i {
                C(c) => Some(*c),
                _ => None,
            })
            .collect();
        for c in [
            Custom::Jump,
            Custom::Speed,
            Custom::ExportSize,
            Custom::AddFromCatalog,
        ] {
            assert!(customs.contains(&c), "{c:?} missing");
        }
    }

    /// ⌘M is taken, and minimises without egui's `Minimized` command, which froze the window
    /// after a restore from the Dock (#131).
    #[cfg(target_os = "macos")]
    #[test]
    fn cmd_m_minimises_through_appkit_not_egui() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::M,
                physical_key: Some(Key::M),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::MAC_CMD,
            }],
            ..Default::default()
        };
        let mut taken = false;
        let mut out = ctx.run_ui(input, |ui| {
            mac_minimise_shortcut(ui.ctx());
            taken = !ui.ctx().input(|i| {
                i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Key { key: Key::M, .. }))
            });
        });
        assert!(taken, "⌘M was not taken by the shortcut");
        for (id, v) in &out.viewport_output {
            assert!(
                !v.commands
                    .iter()
                    .any(|c| matches!(c, egui::ViewportCommand::Minimized(_))),
                "⌘M sent egui's Minimized command to {id:?}"
            );
        }
        // No renderer takes the fonts' texture here; a debug build checks it was (epaint).
        out.textures_delta.clear();
    }

    #[test]
    fn shortcuts_are_unique_and_tool_keys_distinct() {
        // No two actions share a key. Shift is left out, as egui's matching ignores an extra Shift,
        // so one action may have Ctrl+Shift+H and Ctrl+H (#91).
        let mut seen = std::collections::HashMap::new();
        for a in Action::all() {
            for k in a.keys() {
                let was = seen.insert((k.modifiers.command, k.modifiers.mac_cmd, k.logical_key), a);
                assert!(was.is_none_or(|b| b == a), "{a:?} reuses {k:?}");
            }
        }
        let mut keys = std::collections::HashSet::new();
        for t in ToolWin::ALL {
            assert!(keys.insert(t.key()), "{t:?}");
            assert_eq!(ToolWin::from_key(t.key()), Some(t));
        }
        // Panes and tool windows share the pop-out key space (`JETCARP_POPOUT`, viewport ids).
        for p in panes::PANES {
            assert!(keys.insert(p.id), "pane {} has a tool window's key", p.id);
        }
        assert!(keys.insert(panes::GRID));
    }
}
