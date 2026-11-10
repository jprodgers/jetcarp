//! Windows: a window under the desktop icons, and one inside the Screen Saver dialog's preview.
//! Both are made directly with Win32, since winit's style handling would fight the re-parenting.
//!
//! Two desktop layouts exist, found by looking, not by version number:
//! - classic: message 0x052C moves the icons into a top-level WorkerW and makes a second, empty
//!   WorkerW behind it; our window becomes a child of that one.
//! - raised (24H2 and later): the icons (SHELLDLL_DefView) stay a child of Progman and the WorkerW
//!   is also a child of Progman. Our window becomes a layered child of Progman, between them.
//!
//! None of this is documented by Microsoft. The handles are looked up every time, because the
//! WorkerW changes between runs. `spikes/desktop-surface` is where this was proven.

use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::RemoteDesktop::{
    WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTS_SESSIONSTATE_LOCK, WTSFreeMemory,
    WTSINFOEXW, WTSQuerySessionInformationW, WTSSessionInfoEx,
};
use windows_sys::Win32::System::StationsAndDesktops::{CloseDesktop, OpenInputDesktop};
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use winit::raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle,
};

/// `DESKTOP_SWITCHDESKTOP`: the access that fails while the secure desktop is showing.
const SWITCH_DESKTOP: u32 = 0x0100;
const ALREADY_REGISTERED: u32 = 1410;

/// A monitor, in physical pixels on the virtual screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn class_of(h: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(h, buf.as_mut_ptr(), 128) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn progman() -> Result<HWND, String> {
    let h = unsafe { FindWindowW(wide("Progman").as_ptr(), null()) };
    if h.is_null() {
        Err("no Progman window".into())
    } else {
        Ok(h)
    }
}

/// The direct children of `parent`, front to back.
fn children(parent: HWND) -> Vec<HWND> {
    let mut out = vec![];
    let mut after: HWND = null_mut();
    loop {
        after = unsafe { FindWindowExW(parent, after, null(), null()) };
        if after.is_null() {
            return out;
        }
        out.push(after);
    }
}

fn top_level_workers() -> Vec<HWND> {
    let class = wide("WorkerW");
    let mut out = vec![];
    let mut after: HWND = null_mut();
    loop {
        after = unsafe { FindWindowExW(null_mut(), after, class.as_ptr(), null()) };
        if after.is_null() {
            return out;
        }
        out.push(after);
    }
}

enum Layout {
    /// A child of this top-level WorkerW.
    Classic { worker: HWND },
    /// A layered child of Progman, under these icons.
    Raised { icons: HWND },
}

fn find_child(parent: HWND, class: &str) -> Option<HWND> {
    children(parent).into_iter().find(|c| class_of(*c) == class)
}

fn detect(progman: HWND) -> Option<Layout> {
    if let Some(icons) = find_child(progman, "SHELLDLL_DefView") {
        return find_child(progman, "WorkerW").map(|_| Layout::Raised { icons });
    }
    let workers = top_level_workers();
    let with_icons = workers
        .iter()
        .position(|w| find_child(*w, "SHELLDLL_DefView").is_some())?;
    // The empty WorkerW follows the one holding the icons in z-order.
    let after = unsafe { GetWindow(workers[with_icons], GW_HWNDNEXT) };
    if !after.is_null() && class_of(after) == "WorkerW" {
        return Some(Layout::Classic { worker: after });
    }
    workers
        .iter()
        .find(|w| find_child(**w, "SHELLDLL_DefView").is_none())
        .map(|w| Layout::Classic { worker: *w })
}

fn spawn_message(progman: HWND, wparam: usize, lparam: isize) {
    let mut result = 0usize;
    unsafe {
        SendMessageTimeoutW(
            progman,
            0x052C,
            wparam,
            lparam,
            SMTO_NORMAL,
            1000,
            &mut result,
        );
    }
}

fn layout(progman: HWND) -> Result<Layout, String> {
    if let Some(l) = detect(progman) {
        return Ok(l);
    }
    spawn_message(progman, 0xD, 1);
    if let Some(l) = detect(progman) {
        return Ok(l);
    }
    spawn_message(progman, 0, 0);
    detect(progman).ok_or_else(|| "no usable desktop layout was found".to_string())
}

unsafe extern "system" fn wnd_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match m {
        // Nothing here may steal focus or paint over the frames.
        WM_ERASEBKGND => 1,
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        _ => unsafe { DefWindowProcW(h, m, w, l) },
    }
}

fn class_name() -> Result<Vec<u16>, String> {
    let class = wide("JetcarpDesktopSurface");
    unsafe {
        let mut wc: WNDCLASSEXW = std::mem::zeroed();
        wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(wnd_proc);
        wc.hInstance = GetModuleHandleW(null());
        wc.lpszClassName = class.as_ptr();
        if RegisterClassExW(&wc) == 0 && GetLastError() != ALREADY_REGISTERED {
            return Err(format!("RegisterClassExW: {}", GetLastError()));
        }
    }
    Ok(class)
}

/// Puts `hwnd` under the icons, over `rect`.
fn place(hwnd: HWND, progman: HWND, layout: &Layout, rect: Rect) -> Result<(), String> {
    unsafe {
        let (parent, insert_after, layered) = match layout {
            Layout::Classic { worker } => (*worker, HWND_TOP, false),
            Layout::Raised { icons } => (progman, *icons, true),
        };
        if layered {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (ex | WS_EX_LAYERED) as isize);
            if SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA) == 0 {
                return Err(format!("SetLayeredWindowAttributes: {}", GetLastError()));
            }
        }
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        SetWindowLongPtrW(hwnd, GWL_STYLE, ((style & !WS_POPUP) | WS_CHILD) as isize);
        if SetParent(hwnd, parent).is_null() {
            return Err(format!("SetParent: {}", GetLastError()));
        }
        // The monitor's top-left in the parent's coordinates.
        let mut origin = POINT {
            x: rect.x,
            y: rect.y,
        };
        ScreenToClient(parent, &mut origin);
        let placed = SetWindowPos(
            hwnd,
            insert_after,
            origin.x,
            origin.y,
            rect.w as i32,
            rect.h as i32,
            SWP_NOACTIVATE | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
        );
        if placed == 0 {
            return Err(format!("SetWindowPos: {}", GetLastError()));
        }
    }
    Ok(())
}

pub struct DesktopWindow {
    hwnd: HWND,
    /// The window this one lives inside.
    parent: HWND,
    pub size: (u32, u32),
    pub scale: f64,
    /// The monitor it covers (None in a preview).
    pub rect: Option<Rect>,
    pub layout: &'static str,
}

impl DesktopWindow {
    /// A window under the desktop icons, over `rect`.
    pub fn under_icons(rect: Rect, scale: f64) -> Result<DesktopWindow, String> {
        let progman = progman()?;
        let layout = layout(progman)?;
        let class = class_name()?;
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                class.as_ptr(),
                wide("JETCARP wallpaper").as_ptr(),
                WS_POPUP,
                0,
                0,
                100,
                100,
                null_mut(),
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            )
        };
        if hwnd.is_null() {
            return Err(format!("CreateWindowExW: {}", unsafe { GetLastError() }));
        }
        if let Err(e) = place(hwnd, progman, &layout, rect) {
            unsafe { DestroyWindow(hwnd) };
            return Err(e);
        }
        let parent = unsafe { GetParent(hwnd) };
        Ok(DesktopWindow {
            hwnd,
            parent,
            size: (rect.w, rect.h),
            scale,
            rect: Some(rect),
            layout: match layout {
                Layout::Classic { .. } => "classic",
                Layout::Raised { .. } => "raised",
            },
        })
    }

    /// A window filling the Screen Saver dialog's preview.
    pub fn in_preview(parent: isize) -> Result<DesktopWindow, String> {
        let parent = parent as HWND;
        if unsafe { IsWindow(parent) } == 0 {
            return Err("the preview window has gone".into());
        }
        let mut r: RECT = unsafe { std::mem::zeroed() };
        unsafe { GetClientRect(parent, &mut r) };
        let (w, h) = ((r.right - r.left).max(1), (r.bottom - r.top).max(1));
        let class = class_name()?;
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                wide("JETCARP preview").as_ptr(),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                w,
                h,
                parent,
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            )
        };
        if hwnd.is_null() {
            return Err(format!("CreateWindowExW: {}", unsafe { GetLastError() }));
        }
        Ok(DesktopWindow {
            hwnd,
            parent,
            size: (w as u32, h as u32),
            scale: 1.0,
            rect: None,
            layout: "preview",
        })
    }

    /// Explorer can raise its own background over this window, after a monitor is plugged in for
    /// one. Puts it back directly under the icons if so. Only the raised layout has siblings.
    pub fn keep_under_icons(&self) {
        if self.layout != "raised" {
            return;
        }
        unsafe {
            let mut above = GetWindow(self.hwnd, GW_HWNDPREV);
            while !above.is_null() && class_of(above) == "JetcarpDesktopSurface" {
                above = GetWindow(above, GW_HWNDPREV);
            }
            if !above.is_null() && class_of(above) == "SHELLDLL_DefView" {
                return;
            }
            if let Some(icons) = find_child(self.parent, "SHELLDLL_DefView") {
                SetWindowPos(
                    self.hwnd,
                    icons,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
    }

    /// False once the window or what it lives in has gone (Explorer restarted, dialog closed).
    pub fn alive(&self) -> bool {
        unsafe { IsWindow(self.hwnd) != 0 && IsWindow(self.parent) != 0 }
    }

    pub fn surface(&self, instance: &wgpu::Instance) -> Result<wgpu::Surface<'static>, String> {
        let mut handle = Win32WindowHandle::new(
            std::num::NonZeroIsize::new(self.hwnd as isize).ok_or("null window")?,
        );
        handle.hinstance =
            std::num::NonZeroIsize::new(unsafe { GetModuleHandleW(null()) } as isize);
        unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(RawDisplayHandle::Windows(WindowsDisplayHandle::new())),
                raw_window_handle: RawWindowHandle::Win32(handle),
            })
        }
        .map_err(|e| e.to_string())
    }
}

impl Drop for DesktopWindow {
    fn drop(&mut self) {
        unsafe {
            if IsWindow(self.hwnd) != 0 {
                DestroyWindow(self.hwnd);
            }
        }
    }
}

/// Whether a screen saver is running. JETCARP's own draws one window per monitor, so no single window
/// fills them all.
fn saver_running() -> bool {
    let mut running: windows_sys::core::BOOL = 0;
    let got = unsafe {
        SystemParametersInfoW(
            SPI_GETSCREENSAVERRUNNING,
            0,
            (&mut running as *mut windows_sys::core::BOOL).cast(),
            0,
        )
    };
    got != 0 && running != 0
}

/// Whether the wallpaper can't be seen: a screen saver is running, or the focused window fills
/// every one of `monitors`.
pub fn covered(monitors: &[Rect]) -> bool {
    if saver_running() {
        return true;
    }
    unsafe {
        let top = GetForegroundWindow();
        if top.is_null() || matches!(class_of(top).as_str(), "Progman" | "WorkerW") {
            return false;
        }
        let mut r: RECT = std::mem::zeroed();
        if GetWindowRect(top, &mut r) == 0 {
            return false;
        }
        monitors.iter().all(|m| {
            r.left <= m.x
                && r.top <= m.y
                && r.right >= m.x + m.w as i32
                && r.bottom >= m.y + m.h as i32
        })
    }
}

/// Whether Windows reports this session as locked. The lock curtain is an ordinary window on the
/// normal desktop, so the secure desktop alone only catches the password box.
fn session_locked() -> bool {
    unsafe {
        let mut info = null_mut();
        let mut bytes = 0;
        let got = WTSQuerySessionInformationW(
            WTS_CURRENT_SERVER_HANDLE,
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut info,
            &mut bytes,
        );
        if got == 0 || info.is_null() {
            return false;
        }
        let session = &*(info as *const WTSINFOEXW);
        let locked = session.Level == 1
            && session.Data.WTSInfoExLevel1.SessionFlags as u32 == WTS_SESSIONSTATE_LOCK;
        WTSFreeMemory(info.cast());
        locked
    }
}

/// Whether the screen is taken: the session is locked, or the secure desktop is up (a UAC prompt).
pub fn locked() -> bool {
    if session_locked() {
        return true;
    }
    unsafe {
        let d = OpenInputDesktop(0, 0, SWITCH_DESKTOP);
        if d.is_null() {
            return true;
        }
        CloseDesktop(d);
        false
    }
}

/// Puts the desktop's own picture back by setting the current wallpaper to itself. Nothing is
/// saved to the user's settings.
pub fn restore_desktop() {
    unsafe {
        let mut path = [0u16; 1024];
        let got = SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            path.len() as u32,
            path.as_mut_ptr().cast(),
            0,
        );
        if got != 0 && path[0] != 0 {
            SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, path.as_mut_ptr().cast(), 0);
        }
    }
}

/// Tells the user something when there is no console to say it on.
pub fn message(text: &str) {
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(text).as_ptr(),
            wide("JETCARP").as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}
