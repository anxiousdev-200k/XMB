//! Getting the app back in front, and knowing when a game has ended.
//!
//! * `bring_to_front`: after a Windows folder / file window closes, or when a
//!   game ends, the app's window becomes the foreground window again, so the
//!   controller keeps working without touching the mouse or keyboard.
//!   (Windows only lets the foreground app hand the foreground over; the usual
//!   way round it is used: a tap of Alt, and attaching to the foreground
//!   window's input for the call.)
//! * Game mode: while a game runs, `in_game()` is true. `watch_game` follows
//!   the game: the
//!   process that was started, any program running from the game's folder,
//!   and the window that comes to the front after the launch. When all of
//!   them are gone, the app is told ("game-exited") and brought back.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
#[cfg(windows)]
use tauri::Emitter;

static IN_GAME: AtomicBool = AtomicBool::new(false);
static XMB_DURING_GAME: AtomicBool = AtomicBool::new(false);
static GAME_SUSPENDED: AtomicBool = AtomicBool::new(false);
static WAKE_GRACE_UNTIL: AtomicU64 = AtomicU64::new(0);
static WATCH_ID: AtomicU64 = AtomicU64::new(0);
static ACTIVE_GAME_PIDS: Mutex<Vec<(u32, String)>> = Mutex::new(Vec::new());
static TRACKED_GAME_PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());
/// The page inside the app's window has the keyboard / controller focus
/// (told by the page itself on focus / blur).
static PAGE_FOCUSED: AtomicBool = AtomicBool::new(true);
fn now_ms() -> u64 {
    static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    T0.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
}
#[tauri::command]
pub fn page_focus(focused: bool) { PAGE_FOCUSED.store(focused, Ordering::Relaxed); }

#[cfg_attr(not(windows), allow(dead_code))]
pub fn in_game() -> bool { IN_GAME.load(Ordering::Relaxed) }
pub fn xmb_during_game() -> bool { XMB_DURING_GAME.load(Ordering::Relaxed) }
pub fn set_in_game(on: bool) {
    IN_GAME.store(on, Ordering::Relaxed);
    if on {
        if let Ok(mut pids) = ACTIVE_GAME_PIDS.lock() { pids.clear(); }
        if let Ok(mut pids) = TRACKED_GAME_PIDS.lock() { pids.clear(); }
        GAME_SUSPENDED.store(false, Ordering::Relaxed);
        WAKE_GRACE_UNTIL.store(0, Ordering::Relaxed);
        XMB_DURING_GAME.store(false, Ordering::Relaxed);
    } else {
        XMB_DURING_GAME.store(false, Ordering::Relaxed);
        GAME_SUSPENDED.store(false, Ordering::Relaxed);
        WAKE_GRACE_UNTIL.store(0, Ordering::Relaxed);
        if let Ok(mut pids) = ACTIVE_GAME_PIDS.lock() { pids.clear(); }
        if let Ok(mut pids) = TRACKED_GAME_PIDS.lock() { pids.clear(); }
        WATCH_ID.fetch_add(1, Ordering::Relaxed);   // any watcher stops
    }
}
pub fn set_game_suspended(suspended: bool) {
    GAME_SUSPENDED.store(suspended, Ordering::Relaxed);
    if !suspended && in_game() {
        WAKE_GRACE_UNTIL.store(now_ms().saturating_add(60_000), Ordering::Relaxed);
    }
}
pub fn resume_game() -> Result<(), String> {
    #[cfg(windows)]
    {
        XMB_DURING_GAME.store(false, Ordering::Relaxed);
        let mut process_ids = TRACKED_GAME_PIDS.lock().map_err(|e| e.to_string())?.clone();
        let safe_pids = ACTIVE_GAME_PIDS.lock().map_err(|e| e.to_string())?.clone();
        for (pid, _) in safe_pids {
            if !process_ids.contains(&pid) { process_ids.push(pid); }
        }
        if process_ids.is_empty() {
            return Err("No tracked game process is still running.".into());
        }
        let mut found_window = false;
        let mut focused = false;
        for _ in 0..20 {
            if let Some(window) = win::window_for_pids(&process_ids) {
                found_window = true;
                win::force_foreground(window);
                if win::foreground() == window {
                    focused = true;
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        if !found_window {
            return Err("No visible window for the tracked game was found.".into());
        }
        if !focused && !process_ids.contains(&win::foreground_pid().unwrap_or_default()) {
            return Err("Windows did not allow focus to return to the game.".into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    Err("Resuming tracked games is only supported on Windows.".into())
}

pub fn quit_running_game() -> Result<(), String> {
    #[cfg(windows)]
    {
        let pids = ACTIVE_GAME_PIDS.lock().map_err(|e| e.to_string())?.clone();
        if pids.is_empty() { return Err("No tracked game process is available to quit.".into()); }
        return win::terminate_process_tree(&pids);
    }
    #[cfg(not(windows))]
    Err("Quitting tracked games is only supported on Windows.".into())
}

/// Make the app the foreground window again (and focus its page), so the
/// controller works on it at once. Windows hands the foreground back to the
/// app a moment after a folder / file window closes (or may give it to the
/// program behind), so it's done now and again a few times shortly after,
/// until the app really is in front.
pub fn bring_to_front(app: &AppHandle) {
    // always from the app's own window thread (it owns the window)
    { let a = app.clone(); let _ = app.run_on_main_thread(move || front_now(&a)); }
    let app = app.clone();
    std::thread::spawn(move || {
        for wait in [120u64, 250, 450, 800] {
            std::thread::sleep(std::time::Duration::from_millis(wait));
            let a = app.clone();
            let _ = app.run_on_main_thread(move || front_now(&a));
        }
    });
}
fn front_now(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_minimized().unwrap_or(false) { let _ = w.unminimize(); }
        #[cfg(windows)]
        if let Ok(h) = w.hwnd() { win::force_foreground(h.0 as isize); }
        let _ = w.set_focus();
        // the page inside the window gets the keyboard / controller focus too
        if let Some(v) = app.get_webview("main") { let _ = v.set_focus(); }
    }
}

/// Follow a started game (see the module notes). `pid`: the process started,
/// when known; `dir`: the folder the game runs from, when known.
pub fn watch_game(app: AppHandle, pid: Option<u32>, dir: Option<PathBuf>) {
    set_in_game(true);
    let id = WATCH_ID.fetch_add(1, Ordering::Relaxed) + 1;
    std::thread::spawn(move || {
        #[cfg(windows)]
        {
            let dir = dir.filter(|d| win::usable_dir(d));
            let started = std::time::Instant::now();
            let mut tracked: Vec<u32> = pid.into_iter().collect();
            if let Ok(mut pids) = TRACKED_GAME_PIDS.lock() { *pids = tracked.clone(); }
            let mut killable: Vec<(u32, String)> = tracked.iter().filter_map(|&p| {
                win::image_path(p).map(|path| (p, path))
            }).collect();
            if let Ok(mut active) = ACTIVE_GAME_PIDS.lock() { *active = killable.clone(); }
            let mut seen = false;
            let mut gone = 0;
            let mut foreground_tracked = false;
            let mut last_scan = std::time::Instant::now();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(200));
                if WATCH_ID.load(Ordering::Relaxed) != id || !in_game() { return; }   // back in the app some other way
                if GAME_SUSPENDED.load(Ordering::Relaxed) { continue; }
                // Track the first foreground game process during launch, not
                // unrelated apps the user may switch to later.
                let foreground = win::foreground_pid();
                if !foreground_tracked && started.elapsed() < std::time::Duration::from_secs(30) {
                    if let Some(fp) = foreground {
                        if fp != std::process::id() && !win::is_shell_process(fp) {
                            if !tracked.contains(&fp) { tracked.push(fp); }
                            if let Ok(mut pids) = TRACKED_GAME_PIDS.lock() { *pids = tracked.clone(); }
                            if !killable.iter().any(|(p, _)| *p == fp) {
                                if let Some(path) = win::image_path(fp) { killable.push((fp, path)); }
                                if let Ok(mut active) = ACTIVE_GAME_PIDS.lock() { *active = killable.clone(); }
                            }
                            seen = true;
                            foreground_tracked = true;
                        }
                    }
                }
                if !seen && tracked.iter().any(|&p| win::alive(p)) { seen = true; }
                if seen && foreground == Some(std::process::id()) && !XMB_DURING_GAME.load(Ordering::Relaxed) {
                    if let Some(window) = win::window_for_pids(&tracked) { win::force_foreground(window); }
                }
                if last_scan.elapsed() < std::time::Duration::from_millis(1500) { continue; }
                last_scan = std::time::Instant::now();
                if let Some(d) = dir.as_ref() {
                    for p in win::running_from(d) {
                        if !tracked.contains(&p) { tracked.push(p); }
                        if !killable.iter().any(|(pid, _)| *pid == p) {
                            if let Some(path) = win::image_path(p) { killable.push((p, path)); }
                        }
                    }
                    if let Ok(mut pids) = TRACKED_GAME_PIDS.lock() { *pids = tracked.clone(); }
                    if let Ok(mut active) = ACTIVE_GAME_PIDS.lock() { *active = killable.clone(); }
                }
                let alive = tracked.iter().any(|&p| win::alive(p));
                if alive { seen = true; gone = 0; continue; }
                if now_ms() < WAKE_GRACE_UNTIL.load(Ordering::Relaxed) { continue; }
                if !seen {
                    // nothing to follow (yet): give up after a while, the app comes
                    // back when the user returns to it
                    if started.elapsed() > std::time::Duration::from_secs(240) { return; }
                    continue;
                }
                gone += 1;
                if gone >= 2 {
                    if GAME_SUSPENDED.load(Ordering::Relaxed) {
                        gone = 0;
                        continue;
                    }
                    crate::log::line(
                        "WARN",
                        "game",
                        &format!("Game watcher confirmed tracked processes exited: {tracked:?}."),
                    );
                    set_in_game(false);
                    bring_to_front(&app);
                    let _ = app.emit("game-exited", ());
                    return;
                }
            }
        }
        #[cfg(not(windows))]
        { let _ = (app, pid, dir, id); }
    });
}

/// The controller always gets the app back. Windows sometimes doesn't hand
/// the foreground (or the page's focus) back to the app after the Internet
/// Browser, a file window or a game closes, and a page without the focus gets
/// no controller input, so the app looked stuck until the mouse was used. So
/// while the app (or its page) isn't the one in front, the controller is
/// watched here, and a press brings the app back to the front with its page
/// focused. A running game is left entirely to its own input until its process
/// watcher reports that it has exited.
pub fn start_pad_watch(app: AppHandle) {
    #[cfg(windows)]
    std::thread::spawn(move || {
        use std::time::Duration;
        let mut reader = crate::gamepad::Reader::new();
        let mut prev: u32 = 0;
        let mut game_guide_prev = false;
        let mut game_chord_prev = false;
        let mut stick_was = false;
        let mut last_front = 0u64;
        loop {
            std::thread::sleep(Duration::from_millis(50));
            if crate::browser::session_active() || crate::browser::picker_active() { prev = u32::MAX; continue; }
            if in_game() {
                let Some(p) = reader.poll() else {
                    game_guide_prev = false;
                    game_chord_prev = false;
                    continue;
                };
                let chord = p.buttons & (crate::gamepad::B_SELECT | crate::gamepad::B_START)
                    == (crate::gamepad::B_SELECT | crate::gamepad::B_START);
                let guide_pressed = p.guide && !game_guide_prev;
                let chord_pressed = chord && !game_chord_prev;
                game_guide_prev = p.guide;
                game_chord_prev = chord;
                if !XMB_DURING_GAME.load(Ordering::Relaxed) && (guide_pressed || chord_pressed) {
                    XMB_DURING_GAME.store(true, Ordering::Relaxed);
                    let event = if chord_pressed { "game-quick-menu" } else { "game-home" };
                    let _ = app.emit(event, ());
                    bring_to_front(&app);
                }
                prev = u32::MAX;
                continue;
            }
            game_guide_prev = false;
            game_chord_prev = false;
            let Some(w) = app.get_webview_window("main") else { continue };
            let Ok(h) = w.hwnd() else { continue };
            let h = h.0 as isize;
            let fg = win::foreground();
            let fg_pid = win::foreground_pid().unwrap_or(0);
            let ours = fg == h || fg_pid == std::process::id();
            if ours && PAGE_FOCUSED.load(Ordering::Relaxed) {
                // all is well: look again in a moment (nothing is read)
                prev = u32::MAX;
                std::thread::sleep(Duration::from_millis(150));
                continue;
            }
            let Some(p) = reader.poll() else { prev = 0; continue };
            // (buttons already held when the watch started don't count)
            if prev == u32::MAX { prev = p.buttons; stick_was = true; }
            let stick = p.lx.abs() > 0.6 || p.ly.abs() > 0.6;
            let pressed = (p.buttons & !prev) != 0 || (stick && !stick_was);
            prev = p.buttons; stick_was = stick;
            if !pressed { continue; }
            let t = now_ms();
            if t.saturating_sub(last_front) > 500 { last_front = t; bring_to_front(&app); }
        }
    });
    #[cfg(not(windows))]
    { let _ = (app, pid, dir, id); }
}

#[cfg(windows)]
#[allow(non_snake_case, clippy::upper_case_acronyms)]
mod win {
    use std::path::Path;
    type HWND = isize;
    type BOOL = i32;
    type HANDLE = isize;
    type LPARAM = isize;

    #[repr(C)]
    struct PROCESSENTRY32W {
        dwSize: u32, cntUsage: u32, th32ProcessID: u32, th32DefaultHeapID: usize, th32ModuleID: u32,
        cntThreads: u32, th32ParentProcessID: u32, pcPriClassBase: i32, dwFlags: u32, szExeFile: [u16; 260],
    }

    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(cb: extern "system" fn(HWND, LPARAM) -> BOOL, data: LPARAM) -> BOOL;
        fn GetWindowThreadProcessId(h: HWND, pid: *mut u32) -> u32;
        fn GetForegroundWindow() -> HWND;
        fn IsWindowVisible(h: HWND) -> BOOL;
        fn SetFocus(h: HWND) -> HWND;
        fn SwitchToThisWindow(h: HWND, alt_tab: BOOL);
        fn SetForegroundWindow(h: HWND) -> BOOL;
        fn BringWindowToTop(h: HWND) -> BOOL;
        fn ShowWindow(h: HWND, cmd: i32) -> BOOL;
        fn IsIconic(h: HWND) -> BOOL;
        fn AttachThreadInput(a: u32, b: u32, attach: BOOL) -> BOOL;
        fn keybd_event(vk: u8, scan: u8, flags: u32, extra: usize);
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentThreadId() -> u32;
        fn OpenProcess(access: u32, inherit: BOOL, pid: u32) -> HANDLE;
        fn CloseHandle(h: HANDLE) -> BOOL;
        fn WaitForSingleObject(h: HANDLE, ms: u32) -> u32;
        fn QueryFullProcessImageNameW(h: HANDLE, flags: u32, buf: *mut u16, len: *mut u32) -> BOOL;
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> HANDLE;
        fn Process32FirstW(snap: HANDLE, e: *mut PROCESSENTRY32W) -> BOOL;
        fn Process32NextW(snap: HANDLE, e: *mut PROCESSENTRY32W) -> BOOL;
    }
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const WAIT_TIMEOUT: u32 = 0x102;
    const TH32CS_SNAPPROCESS: u32 = 0x2;
    const VK_MENU: u8 = 0x12;
    const KEYEVENTF_KEYUP: u32 = 0x2;
    const SW_RESTORE: i32 = 9;

    pub fn force_foreground(h: HWND) {
        if h == 0 { return; }
        unsafe {
            if IsIconic(h) != 0 { ShowWindow(h, SW_RESTORE); }
            if GetForegroundWindow() == h { return; }
            let fg = GetForegroundWindow();
            let me = GetCurrentThreadId();
            let fg_thread = if fg != 0 { GetWindowThreadProcessId(fg, std::ptr::null_mut()) } else { 0 };
            let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, 1) != 0;
            // a tap of Alt lets this process take the foreground
            keybd_event(VK_MENU, 0, 0, 0);
            keybd_event(VK_MENU, 0, KEYEVENTF_KEYUP, 0);
            BringWindowToTop(h);
            SetForegroundWindow(h);
            SetFocus(h);
            if attached { AttachThreadInput(me, fg_thread, 0); }
            if GetForegroundWindow() != h { SwitchToThisWindow(h, 1); }
        }
    }

    pub fn foreground() -> HWND { unsafe { GetForegroundWindow() } }

    pub fn foreground_pid() -> Option<u32> {
        let h = unsafe { GetForegroundWindow() };
        if h == 0 { return None; }
        let mut p = 0u32;
        unsafe { GetWindowThreadProcessId(h, &mut p) };
        (p != 0).then_some(p)
    }

    struct WindowFinder { pid: u32, hwnd: HWND }

    extern "system" fn find_visible_window(hwnd: HWND, data: LPARAM) -> BOOL {
        unsafe {
            let finder = &mut *(data as *mut WindowFinder);
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == finder.pid && IsWindowVisible(hwnd) != 0 {
                finder.hwnd = hwnd;
                0
            } else {
                1
            }
        }
    }

    pub fn window_for_pids(pids: &[u32]) -> Option<HWND> {
        for &pid in pids.iter().rev() {
            let mut finder = WindowFinder { pid, hwnd: 0 };
            unsafe { EnumWindows(find_visible_window, &mut finder as *mut WindowFinder as LPARAM); }
            if finder.hwnd != 0 { return Some(finder.hwnd); }
        }
        None
    }

    pub fn alive(pid: u32) -> bool {
        unsafe {
            let h = OpenProcess(SYNCHRONIZE, 0, pid);
            if h == 0 { return false; }
            let r = WaitForSingleObject(h, 0);
            CloseHandle(h);
            r == WAIT_TIMEOUT
        }
    }

    pub fn image_path(pid: u32) -> Option<String> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h == 0 { return None; }
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) != 0;
            CloseHandle(h);
            ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
        }
    }

    /// Programs that are part of Windows or of a store client, never "the game".
    pub fn is_shell_process(pid: u32) -> bool {
        let Some(p) = image_path(pid) else { return true };
        let n = Path::new(&p).file_name().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
        const SHELL: &[&str] = &["explorer.exe", "cmd.exe", "conhost.exe", "searchhost.exe", "searchapp.exe", "shellexperiencehost.exe",
            "startmenuexperiencehost.exe", "applicationframehost.exe", "taskmgr.exe", "steam.exe", "steamwebhelper.exe",
            "epicgameslauncher.exe", "galaxyclient.exe", "eadesktop.exe", "origin.exe", "ubisoftconnect.exe", "upc.exe",
            "battle.net.exe", "lockapp.exe", "textinputhost.exe", "msedgewebview2.exe"];
        SHELL.contains(&n.as_str())
    }

    /// A folder specific enough to say "the game is running from here".
    pub fn usable_dir(d: &Path) -> bool {
        let s = d.display().to_string().to_lowercase();
        d.components().count() >= 3 && !s.contains("\\windows\\") && !s.ends_with("\\windows")
    }

    pub fn running_from(dir: &Path) -> Vec<u32> {
        let want = dir.display().to_string().to_lowercase();
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == -1 || snap == 0 { return Vec::new(); }
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut found = Vec::new();
            let mut ok = Process32FirstW(snap, &mut e) != 0;
            while ok {
                if e.th32ProcessID != 0 && e.th32ProcessID != std::process::id() {
                    if let Some(p) = image_path(e.th32ProcessID) {
                        if p.to_lowercase().starts_with(&want) { found.push(e.th32ProcessID); }
                    }
                }
                ok = Process32NextW(snap, &mut e) != 0;
            }
            CloseHandle(snap);
            found
        }
    }

    pub fn terminate_process_tree(processes: &[(u32, String)]) -> Result<(), String> {
        use std::{os::windows::process::CommandExt, process::{Command, Stdio}, time::{Duration, Instant}};

        let matches_identity = |(pid, expected): &(u32, String)| {
            alive(*pid) && image_path(*pid).map(|p| p.eq_ignore_ascii_case(expected)).unwrap_or(false)
        };
        let active: Vec<(u32, String)> = processes.iter().filter(|p| matches_identity(p)).cloned().collect();
        if active.is_empty() {
            if !processes.iter().any(|(pid, _)| alive(*pid)) { return Ok(()); }
            return Err("The tracked game process is no longer available or its identity changed.".into());
        }
        for (pid, expected) in &active {
            if !matches_identity(&(*pid, expected.clone())) { continue; }
            let pid_arg = pid.to_string();
            Command::new("taskkill.exe")
                .args(["/PID", pid_arg.as_str(), "/T"])
                .creation_flags(0x0800_0000)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|e| format!("Could not request the game to close: {e}"))?;
        }
        let graceful_deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < graceful_deadline && active.iter().any(matches_identity) {
            std::thread::sleep(Duration::from_millis(100));
        }
        for (pid, expected) in &active {
            if !matches_identity(&(*pid, expected.clone())) { continue; }
            let pid_arg = pid.to_string();
            Command::new("taskkill.exe")
                .args(["/PID", pid_arg.as_str(), "/T", "/F"])
                .creation_flags(0x0800_0000)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|e| format!("Could not force-close the game: {e}"))?;
        }
        let force_deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < force_deadline && active.iter().any(matches_identity) {
            std::thread::sleep(Duration::from_millis(100));
        }
        let remaining: Vec<String> = active.iter().filter(|p| matches_identity(p)).map(|(pid, _)| pid.to_string()).collect();
        if remaining.is_empty() { Ok(()) }
        else { Err(format!("The game process did not close (PID {}).", remaining.join(", "))) }
    }
}
