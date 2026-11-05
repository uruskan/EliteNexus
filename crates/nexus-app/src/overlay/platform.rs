//! What the overlay asks the desktop: whether the game (or Elite Nexus) is the window in front, which monitor the game
//! is on, whether a see-through window can work here, and, on Windows, for a window that never takes the focus from the
//! game, has the island's shape, or (see-through) shows nothing of its own.
//!
//! Windows reads the foreground window's class; X11 reads `_NET_ACTIVE_WINDOW` through libX11, loaded at run time
//! (winit loads the same library), so the binary links nothing new. Wayland and macOS can't tell, so the overlay
//! stays visible there.

/// The game's window class on Windows.
#[cfg(windows)]
const GAME_CLASS: &str = "FrontierDevelopmentsAppWinClass";

/// Asks the desktop about windows, keeping what it found between calls.
#[derive(Default)]
pub struct Desktop {
    /// The overlay's own window, once found.
    #[cfg(windows)]
    overlay: Option<isize>,
    /// The window [`Desktop::see_through`] last cleared.
    #[cfg(windows)]
    cleared: Option<isize>,
    /// The X connection: None until first tried, then whether it opened.
    #[cfg(target_os = "linux")]
    x11: Option<Option<x11::Conn>>,
}

impl Desktop {
    /// Is the game, or a window of Elite Nexus, the one in front? None when the desktop can't say (Wayland, macOS,
    /// or between two windows).
    pub fn game_in_front(&mut self) -> Option<bool> {
        #[cfg(windows)]
        {
            win::game_in_front()
        }
        #[cfg(target_os = "linux")]
        {
            if std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()) {
                return None;
            }
            self.x11.get_or_insert_with(x11::Conn::open).as_ref()?.game_in_front()
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            None
        }
    }

    /// The game's monitor, or the primary one without the game: left, top, width and height in physical pixels.
    /// Windows only; elsewhere the overlay goes on the monitor it opened on.
    pub fn game_monitor(&self) -> Option<[i32; 4]> {
        #[cfg(windows)]
        {
            win::game_monitor()
        }
        #[cfg(not(windows))]
        {
            None
        }
    }

    /// Windows: the overlay window (found by `title` among this thread's windows) never becomes the active window while
    /// `locked`, so the game keeps the focus and joystick input; unlocked it may, to be dragged. winit rewrites the
    /// window's extended styles whenever it changes it, so this is applied again whenever they've been reset.
    pub fn keep_from_focus(&mut self, title: &str, locked: bool) {
        #[cfg(windows)]
        {
            let hwnd = match self.overlay {
                Some(h) => h,
                None => match win::find(title) {
                    Some(h) => *self.overlay.insert(h),
                    None => return,
                },
            };
            if !win::set_no_activate(hwnd, locked) {
                self.overlay = None;
            }
        }
        #[cfg(not(windows))]
        let _ = (title, locked);
    }

    /// Windows: the overlay window (found by `title` among this thread's windows), for [`set_shape`]. None elsewhere.
    pub fn window(&mut self, title: &str) -> Option<isize> {
        #[cfg(windows)]
        {
            if self.overlay.is_none() {
                self.overlay = win::find(title);
            }
            self.overlay
        }
        #[cfg(not(windows))]
        {
            let _ = title;
            None
        }
    }

    /// Windows, see-through: `window` (from [`Desktop::window`]) shows nothing of its own, no background, frame or
    /// shadow, so only what the island draws is there (see [`win::see_through`]). Once per window; it lasts as long as
    /// the window does.
    pub fn see_through(&mut self, window: isize) {
        #[cfg(windows)]
        if self.cleared != Some(window) && win::see_through(window) {
            self.cleared = Some(window);
        }
        #[cfg(not(windows))]
        let _ = window;
    }

    /// The overlay window was closed.
    pub fn forget_window(&mut self) {
        #[cfg(windows)]
        {
            self.overlay = None;
            self.cleared = None;
        }
    }
}

/// Linux: whether something composes windows, so one can be see-through. A Wayland session always does, also for
/// Elite Nexus run through XWayland; X11 only while a compositing manager runs (it owns the `_NET_WM_CM_S<screen>`
/// selection). True when it can't be told (no X display, no libX11): only a desktop known not to compose rules
/// see-through out.
#[cfg(target_os = "linux")]
pub fn composited() -> bool {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()) || std::env::var("XDG_SESSION_TYPE").is_ok_and(|s| s.eq_ignore_ascii_case("wayland"));
    wayland || x11::Conn::open().is_none_or(|c| c.composited())
}

/// Windows: cuts `window` (from [`Desktop::window`]) to `parts`, rounded rectangles in its client area's physical
/// pixels (left, top, right, bottom, corner radius), so with see-through off it has the island's shape; no parts give
/// it back its whole rectangle. True once Windows has taken it. Elsewhere the window stays a rectangle (false).
pub fn set_shape(window: isize, parts: &[[i32; 5]]) -> bool {
    #[cfg(windows)]
    {
        win::set_shape(window, parts)
    }
    #[cfg(not(windows))]
    {
        let _ = (window, parts);
        false
    }
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMNCRP_DISABLED, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_NCRENDERING_POLICY};
    use windows_sys::Win32::Graphics::Gdi::{
        ClientToScreen, CombineRgn, CreateRoundRectRgn, DeleteObject, FillRect, GetMonitorInfoW, GetStockObject, GetUpdateRect, GetWindowDC, MonitorFromPoint, MonitorFromWindow, ReleaseDC,
        SetWindowRgn, BLACK_BRUSH, HRGN, MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, RGN_OR,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, DefWindowProcW, EnumThreadWindows, FindWindowW, GetClassNameW, GetForegroundWindow, GetPropW, GetWindowLongW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
        IsWindow, RemovePropW, SetPropW, SetWindowLongW, SetWindowPos, GWLP_WNDPROC, GWL_EXSTYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WM_ERASEBKGND,
        WM_NCACTIVATE, WM_NCDESTROY, WM_NCPAINT, WM_PAINT, WNDPROC, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn game_in_front() -> Option<bool> {
        // SAFETY: queries about the foreground window; the buffers outlive the calls.
        unsafe {
            let fg = GetForegroundWindow();
            if fg.is_null() {
                // Between two windows (Alt+Tab): no answer.
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(fg, &mut pid);
            if pid == GetCurrentProcessId() {
                return Some(true);
            }
            let mut buf = [0u16; 64];
            let n = GetClassNameW(fg, buf.as_mut_ptr(), buf.len() as i32);
            Some(n > 0 && String::from_utf16_lossy(&buf[..n as usize]) == super::GAME_CLASS)
        }
    }

    pub fn game_monitor() -> Option<[i32; 4]> {
        let class = wide(super::GAME_CLASS);
        let empty = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, rcMonitor: empty, rcWork: empty, dwFlags: 0 };
        // SAFETY: the class name is NUL-terminated and outlives the call; MONITORINFO says its own size.
        unsafe {
            let game = FindWindowW(class.as_ptr(), std::ptr::null());
            let monitor = if game.is_null() { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) } else { MonitorFromWindow(game, MONITOR_DEFAULTTONEAREST) };
            if monitor.is_null() || GetMonitorInfoW(monitor, &mut info) == 0 {
                return None;
            }
        }
        let r = info.rcMonitor;
        Some([r.left, r.top, r.right - r.left, r.bottom - r.top])
    }

    struct Search {
        title: Vec<u16>,
        found: HWND,
    }

    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> windows_sys::core::BOOL {
        // SAFETY: `lparam` is the `Search` that `find` passes, alive for the whole enumeration.
        let search = unsafe { &mut *(lparam as *mut Search) };
        let mut buf = [0u16; 128];
        // SAFETY: the buffer outlives the call and its length is passed.
        let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        if n > 0 && buf[..n as usize] == search.title[..search.title.len() - 1] {
            search.found = hwnd;
            return 0;
        }
        1
    }

    /// The overlay window: winit creates every window on this (the event loop's) thread.
    pub fn find(title: &str) -> Option<isize> {
        let mut search = Search { title: wide(title), found: std::ptr::null_mut() };
        // SAFETY: `visit` uses `search` only during the call.
        unsafe { EnumThreadWindows(GetCurrentThreadId(), Some(visit), &mut search as *mut Search as LPARAM) };
        (!search.found.is_null()).then_some(search.found as isize)
    }

    /// Gives the window the shape of `parts` (rounded rectangles: left, top, right, bottom, corner radius, in its client
    /// area's pixels), or its whole rectangle back for none. False when the window is gone or Windows refused the shape.
    pub fn set_shape(hwnd: isize, parts: &[[i32; 5]]) -> bool {
        let hwnd = hwnd as HWND;
        // SAFETY: the handle is checked first; the rectangle and point are locals. Each region made here is either
        // combined into the first and deleted, or handed to SetWindowRgn, after which the system owns it (deleted here
        // only if that call fails).
        unsafe {
            if IsWindow(hwnd) == 0 {
                return false;
            }
            // A window's shape is in window coordinates, and a borderless window keeps a pixel of frame at its top
            // (winit): the parts move by where the client area starts, so that frame stays outside the shape.
            let mut frame = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            let mut client = POINT { x: 0, y: 0 };
            let (dx, dy) = if GetWindowRect(hwnd, &mut frame) != 0 && ClientToScreen(hwnd, &mut client) != 0 { (client.x - frame.left, client.y - frame.top) } else { (0, 0) };
            let mut region: HRGN = std::ptr::null_mut();
            for &[left, top, right, bottom, radius] in parts {
                // Right and bottom are exclusive; the ellipse is the corner's diameter.
                let part = CreateRoundRectRgn(left + dx, top + dy, right + dx + 1, bottom + dy + 1, 2 * radius, 2 * radius);
                if part.is_null() {
                    continue;
                }
                if region.is_null() {
                    region = part;
                } else {
                    CombineRgn(region, region, part, RGN_OR);
                    DeleteObject(part);
                }
            }
            // No redraw: the overlay presents its frame for this shape right after.
            if SetWindowRgn(hwnd, region, 0) == 0 {
                if !region.is_null() {
                    DeleteObject(region);
                }
                return false;
            }
        }
        true
    }

    /// Sets or clears WS_EX_NOACTIVATE (and keeps WS_EX_TOOLWINDOW, which leaves it out of Alt+Tab). False when the
    /// window is gone.
    pub fn set_no_activate(hwnd: isize, on: bool) -> bool {
        let hwnd = hwnd as HWND;
        // SAFETY: the handle is checked first; only extended style bits change, and SetWindowPos neither moves,
        // sizes, reorders nor activates the window.
        unsafe {
            if IsWindow(hwnd) == 0 {
                return false;
            }
            let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            let want = (if on { style | WS_EX_NOACTIVATE } else { style & !WS_EX_NOACTIVATE }) | WS_EX_TOOLWINDOW;
            if want != style {
                SetWindowLongW(hwnd, GWL_EXSTYLE, want as i32);
                SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
            }
        }
        true
    }

    /// Where a see-through window keeps the window procedure it had (winit's) while [`clear_proc`] stands in front.
    const PREVIOUS: windows_sys::core::PCWSTR = windows_sys::w!("EliteNexus.SeeThrough");

    /// See-through: the window shows nothing of its own, so only what the overlay draws is there.
    ///
    /// The island is drawn through DirectComposition, over a surface Windows keeps for every window not made without
    /// one (winit can, but eframe doesn't ask it to). Nothing paints that surface, and Windows shows it white, so the
    /// island sat on a white rectangle. GDI leaves what it paints with no opacity, and a transparent window's surface is
    /// composed by its opacity: painted black, it shows nothing. [`clear_proc`] paints it again whenever Windows remakes
    /// or redraws it (a new size, click-through on or off, shown again). Under it Windows draws the frame winit keeps on
    /// a borderless window, for its shadow: once nothing covers it, its shadow, border and close button show. That frame
    /// is switched off here, and its border too on Windows 11. False when the window is gone.
    pub fn see_through(hwnd: isize) -> bool {
        let hwnd = hwnd as HWND;
        // SAFETY: the handle is checked first: a window of this thread (winit makes every window on it), not another
        // that got a gone window's handle. The procedure set is a plain function that lives as long as the program;
        // the one it replaces is kept with the window and put back as the window is destroyed. The attribute values are
        // locals that outlive the calls.
        unsafe {
            if IsWindow(hwnd) == 0 || GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) != GetCurrentThreadId() {
                return false;
            }
            if GetPropW(hwnd, PREVIOUS).is_null() {
                let previous = window_proc(hwnd);
                if previous == 0 || SetPropW(hwnd, PREVIOUS, previous as HANDLE) == 0 {
                    return false;
                }
                set_window_proc(hwnd, clear_proc as *const () as isize);
            }
            // Turning the frame off repaints it, which `clear_proc` turns into nothing. Windows 10 has no border to
            // turn off and refuses the second attribute.
            let disabled = DWMNCRP_DISABLED;
            DwmSetWindowAttribute(hwnd, DWMWA_NCRENDERING_POLICY as u32, (&raw const disabled).cast(), size_of_val(&disabled) as u32);
            let none = DWMWA_COLOR_NONE;
            DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR as u32, (&raw const none).cast(), size_of_val(&none) as u32);
            clear(hwnd);
        }
        true
    }

    /// Paints the whole window, its frame included, black: nothing, for a see-through window ([`see_through`]).
    unsafe fn clear(hwnd: HWND) {
        // SAFETY: the caller's window; the DC is released here, and a stock brush is never freed.
        unsafe {
            let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            if GetWindowRect(hwnd, &mut r) == 0 {
                return;
            }
            let dc = GetWindowDC(hwnd);
            if dc.is_null() {
                return;
            }
            FillRect(dc, &RECT { left: 0, top: 0, right: r.right - r.left, bottom: r.bottom - r.top }, GetStockObject(BLACK_BRUSH));
            ReleaseDC(hwnd, dc);
        }
    }

    /// A see-through window's procedure, in front of winit's: whatever Windows would paint of the window itself is
    /// painted nothing instead ([`clear`]); every message still goes on to winit except those paints.
    unsafe extern "system" fn clear_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // SAFETY: Windows calls this on the window's thread with its own handle. `see_through` kept the previous
        // procedure with the window (a window procedure or nothing, both pointer-sized), and it's put back before the
        // window's last message goes on to it.
        unsafe {
            let previous = std::mem::transmute::<HANDLE, WNDPROC>(GetPropW(hwnd, PREVIOUS));
            if previous.is_none() {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            match msg {
                WM_ERASEBKGND => {
                    clear(hwnd);
                    return 1;
                }
                WM_NCPAINT => {
                    clear(hwnd);
                    return 0;
                }
                // Activated or not, the frame isn't drawn (-1 says so, unless a visual style draws it anyway: then it's
                // painted over); winit still hears of it.
                WM_NCACTIVATE => {
                    let result = CallWindowProcW(previous, hwnd, msg, wparam, -1);
                    clear(hwnd);
                    return result;
                }
                // Part of the window needs painting (not just a frame of the island's): in case Windows remade the
                // surface without asking for it to be erased.
                WM_PAINT if GetUpdateRect(hwnd, std::ptr::null_mut(), 0) != 0 => clear(hwnd),
                WM_NCDESTROY => {
                    set_window_proc(hwnd, GetPropW(hwnd, PREVIOUS) as isize);
                    RemovePropW(hwnd, PREVIOUS);
                }
                _ => {}
            }
            CallWindowProcW(previous, hwnd, msg, wparam, lparam)
        }
    }

    /// A window's procedure. Pointer-sized: the functions for that exist on 64-bit Windows only.
    unsafe fn window_proc(hwnd: HWND) -> isize {
        #[cfg(target_pointer_width = "64")]
        {
            // SAFETY: as the caller's.
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_WNDPROC) }
        }
        #[cfg(not(target_pointer_width = "64"))]
        {
            // SAFETY: as the caller's.
            unsafe { GetWindowLongW(hwnd, GWLP_WNDPROC) as isize }
        }
    }

    /// Gives a window another procedure.
    unsafe fn set_window_proc(hwnd: HWND, proc: isize) {
        #[cfg(target_pointer_width = "64")]
        // SAFETY: as the caller's.
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(hwnd, GWLP_WNDPROC, proc);
        }
        #[cfg(not(target_pointer_width = "64"))]
        // SAFETY: as the caller's.
        unsafe {
            SetWindowLongW(hwnd, GWLP_WNDPROC, proc as i32);
        }
    }
}

#[cfg(target_os = "linux")]
mod x11 {
    use std::ffi::{c_char, c_ulong, CStr};
    use x11_dl::xlib;

    /// A connection to the X server (libX11 loaded at run time).
    pub struct Conn {
        lib: xlib::Xlib,
        display: *mut xlib::Display,
        active: xlib::Atom,
        pid: xlib::Atom,
    }

    impl Conn {
        pub fn open() -> Option<Conn> {
            let lib = xlib::Xlib::open().ok()?;
            // SAFETY: a NULL name opens $DISPLAY; the atom names are NUL-terminated literals.
            unsafe {
                let display = (lib.XOpenDisplay)(std::ptr::null());
                if display.is_null() {
                    return None;
                }
                let active = (lib.XInternAtom)(display, c"_NET_ACTIVE_WINDOW".as_ptr(), xlib::False);
                let pid = (lib.XInternAtom)(display, c"_NET_WM_PID".as_ptr(), xlib::False);
                Some(Conn { lib, display, active, pid })
            }
        }

        /// The window in front is the game's (by WM_CLASS) or Elite Nexus's (by its process). None when the window
        /// manager doesn't publish the active window.
        pub fn game_in_front(&self) -> Option<bool> {
            // SAFETY: the display is open while `self` lives; what Xlib returns is freed with XFree. An error (a
            // window closing meanwhile) goes to winit's error handler, which only logs it.
            unsafe {
                let root = (self.lib.XDefaultRootWindow)(self.display);
                let win = self.property(root, self.active, xlib::XA_WINDOW)?;
                if win == 0 {
                    return None;
                }
                if self.property(win, self.pid, xlib::XA_CARDINAL) == Some(std::process::id() as c_ulong) {
                    return Some(true);
                }
                let mut hint = xlib::XClassHint { res_name: std::ptr::null_mut(), res_class: std::ptr::null_mut() };
                if (self.lib.XGetClassHint)(self.display, win, &mut hint) == 0 {
                    return Some(false);
                }
                let text = |p: *mut c_char| if p.is_null() { String::new() } else { CStr::from_ptr(p).to_string_lossy().to_ascii_lowercase() };
                let (name, class) = (text(hint.res_name), text(hint.res_class));
                for p in [hint.res_name, hint.res_class] {
                    if !p.is_null() {
                        (self.lib.XFree)(p.cast());
                    }
                }
                Some(is_game(&name) || is_game(&class))
            }
        }

        /// A compositing manager runs on the default screen: it owns the `_NET_WM_CM_S<screen>` selection. True when
        /// that can't be asked.
        pub fn composited(&self) -> bool {
            // SAFETY: the display is open while `self` lives; the atom's name is a NUL-terminated local that outlives
            // the call.
            unsafe {
                let screen = (self.lib.XDefaultScreen)(self.display);
                let Ok(name) = std::ffi::CString::new(format!("_NET_WM_CM_S{screen}")) else { return true };
                let atom = (self.lib.XInternAtom)(self.display, name.as_ptr(), xlib::False);
                atom == 0 || (self.lib.XGetSelectionOwner)(self.display, atom) != 0
            }
        }

        /// The first item of a 32-bit window property (a window id, a number).
        unsafe fn property(&self, win: c_ulong, prop: xlib::Atom, kind: xlib::Atom) -> Option<c_ulong> {
            let (mut actual, mut format, mut items, mut after, mut data) = (0, 0, 0, 0, std::ptr::null_mut());
            // SAFETY: every out-pointer points at a local; the data is freed below.
            unsafe {
                let status = (self.lib.XGetWindowProperty)(self.display, win, prop, 0, 1, xlib::False, kind, &mut actual, &mut format, &mut items, &mut after, &mut data);
                let value = (status == 0 && !data.is_null() && items > 0 && format == 32).then(|| *(data as *const c_ulong));
                if !data.is_null() {
                    (self.lib.XFree)(data.cast());
                }
                value
            }
        }
    }

    impl Drop for Conn {
        fn drop(&mut self) {
            // SAFETY: opened in `open`, closed once.
            unsafe { (self.lib.XCloseDisplay)(self.display) };
        }
    }

    /// Elite Dangerous under Proton (`steam_app_359320`) or Wine (`elitedangerous64.exe`).
    fn is_game(s: &str) -> bool {
        s.contains("elitedangerous") || s == "steam_app_359320"
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn the_game_by_its_window_class() {
            assert!(super::is_game("steam_app_359320"));
            assert!(super::is_game("elitedangerous64.exe"));
            assert!(!super::is_game("steam"));
        }
    }
}
