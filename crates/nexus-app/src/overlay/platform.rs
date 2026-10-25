//! What the overlay asks the desktop: whether the game (or Elite Nexus) is the window in front, which monitor the game
//! is on, and, on Windows, for a window that never takes the focus from the game.
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

    /// The overlay window was closed.
    pub fn forget_window(&mut self) {
        #[cfg(windows)]
        {
            self.overlay = None;
        }
    }
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY};
    use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, FindWindowW, GetClassNameW, GetForegroundWindow, GetWindowLongW, GetWindowTextW, GetWindowThreadProcessId, IsWindow, SetWindowLongW, SetWindowPos,
        GWL_EXSTYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
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
