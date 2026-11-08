//! The "Close System" key (F4 by default, Change Keybinds): closes the app at
//! once, as a safeguard when it has slowed down or got stuck. It's read here,
//! natively, so it works even when the page itself has stopped answering, but
//! only while one of the app's own windows (or its browser) is in front.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use tauri::AppHandle;

/// Windows virtual-key code of the Close System key (0 = none). F4 = 0x73.
static QUIT_VK: AtomicU32 = AtomicU32::new(0x73);
static CHANGED: AtomicBool = AtomicBool::new(false);
static F4_ELSEWHERE: AtomicBool = AtomicBool::new(false);
static QUIT_ELSEWHERE: AtomicBool = AtomicBool::new(false);
static F4_UNTIL: AtomicU64 = AtomicU64::new(0);
static QUIT_UNTIL: AtomicU64 = AtomicU64::new(0);
const GRACE_MS: u64 = 1500;
const POLL_MS: u64 = 10;
const FRESH_MS: u64 = 500;

fn now_ms() -> u64 {
    static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    T0.get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as u64
}

pub fn held_from_elsewhere() -> bool {
    #[cfg(windows)]
    {
        #[link(name = "user32")]
        extern "system" {
            fn GetAsyncKeyState(vk: i32) -> i16;
        }
        let down = |vk: i32| vk != 0 && (unsafe { GetAsyncKeyState(vk) } as u16) & 0x8000 != 0;
        let quit = QUIT_VK.load(Ordering::SeqCst) as i32;
        let now = now_ms();
        return (down(0x73) && F4_ELSEWHERE.load(Ordering::SeqCst))
            || now < F4_UNTIL.load(Ordering::SeqCst)
            || (down(quit) && QUIT_ELSEWHERE.load(Ordering::SeqCst))
            || now < QUIT_UNTIL.load(Ordering::SeqCst);
    }
    #[cfg(not(windows))]
    false
}

/// Set the key (from Change Keybinds; 0 turns it off, e.g. while the
/// keybinds menu is waiting for a key).
#[tauri::command(async)]
pub fn set_quit_key(vk: u32) {
    if QUIT_VK.swap(vk, Ordering::SeqCst) != vk {
        CHANGED.store(true, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct KeyWatch {
    was: bool,
    elsewhere: bool,
    until: u64,
}
impl KeyWatch {
    fn update(&mut self, held: bool, front: bool, fresh: bool, now: u64) -> bool {
        let new = held && !self.was;
        if new {
            self.elsewhere = !front || fresh;
        }
        if self.elsewhere || (!front && now < self.until) {
            self.until = now + GRACE_MS;
        }
        if !held {
            self.elsewhere = false;
        }
        self.was = held;
        if new && !self.elsewhere {
            self.until = 0;
        }
        new && !self.elsewhere
    }
    #[cfg(test)]
    fn from_elsewhere(&self, now: u64) -> bool {
        self.elsewhere || now < self.until
    }
}

/// Close everything now: the pointer is let go and the browser closed (given
/// a moment at most), then the process ends.
pub fn quit_now(app: &AppHandle) {
    crate::log::line("INFO", "system", "Close System key pressed: closing");
    let a = app.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        crate::cursorlock::release_now();
        crate::browser::on_app_exit(&a);
        crate::maybe_cleanup_temp(&a);
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(std::time::Duration::from_millis(1500));
    std::process::exit(0);
}

/// Watch the key (Windows).
pub fn start(app: AppHandle) {
    #[cfg(windows)]
    std::thread::spawn(move || {
        #[link(name = "user32")]
        extern "system" {
            fn GetAsyncKeyState(vk: i32) -> i16;
            fn GetForegroundWindow() -> isize;
            fn GetWindowThreadProcessId(h: isize, pid: *mut u32) -> u32;
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GetCurrentProcessId() -> u32;
        }
        let me = unsafe { GetCurrentProcessId() };
        let ours = || {
            let fg = unsafe { GetForegroundWindow() };
            if fg == 0 {
                return false;
            }
            let mut pid = 0u32;
            unsafe {
                GetWindowThreadProcessId(fg, &mut pid);
            }
            pid == me || fg == crate::browser::browser_hwnd()
        };
        let down = |vk: i32| vk != 0 && (unsafe { GetAsyncKeyState(vk) } as u16) & 0x8000 != 0;
        let mut f4 = KeyWatch {
            was: down(0x73),
            ..Default::default()
        };
        let mut quit = KeyWatch {
            was: true,
            ..Default::default()
        };
        let (mut was_front, mut front_at) = (false, 0u64);
        loop {
            std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
            let front = ours();
            let now = now_ms();
            if front && !was_front {
                front_at = now;
            }
            was_front = front;
            let fresh = front && now.saturating_sub(front_at) < FRESH_MS;
            f4.update(down(0x73), front, fresh, now);
            F4_ELSEWHERE.store(f4.elsewhere, Ordering::SeqCst);
            F4_UNTIL.store(f4.until, Ordering::SeqCst);
            if CHANGED.swap(false, Ordering::SeqCst) {
                quit = KeyWatch {
                    was: true,
                    ..Default::default()
                };
            }
            let vk = QUIT_VK.load(Ordering::SeqCst) as i32;
            if vk == 0 {
                quit.was = true;
                QUIT_ELSEWHERE.store(false, Ordering::SeqCst);
                QUIT_UNTIL.store(0, Ordering::SeqCst);
                continue;
            }
            let fire = quit.update(down(vk), front, fresh, now);
            QUIT_ELSEWHERE.store(quit.elsewhere, Ordering::SeqCst);
            QUIT_UNTIL.store(quit.until, Ordering::SeqCst);
            if fire {
                quit_now(&app);
            }
        }
    });
    #[cfg(not(windows))]
    let _ = app;
}

#[cfg(test)]
mod tests {
    use super::{KeyWatch, GRACE_MS};

    #[test]
    fn key_held_while_another_app_is_front_does_not_quit_on_return() {
        let mut key = KeyWatch::default();
        assert!(!key.update(true, false, false, 100));
        assert!(!key.update(true, true, false, 200));
        assert!(key.from_elsewhere(200));
        assert!(!key.update(false, true, false, 300));
        assert!(key.from_elsewhere(300 + GRACE_MS - 1));
        assert!(!key.from_elsewhere(300 + GRACE_MS));
    }

    #[test]
    fn new_app_press_counts_after_an_external_press_is_released() {
        let mut key = KeyWatch::default();
        assert!(!key.update(true, false, false, 0));
        assert!(!key.update(false, true, false, 100));
        assert!(key.update(true, true, false, 200));
        assert!(!key.from_elsewhere(200));
    }

    #[test]
    fn immediate_game_exit_press_is_treated_as_external() {
        let mut key = KeyWatch::default();
        assert!(!key.update(true, true, true, 0));
        assert!(key.from_elsewhere(0));
        assert!(!key.update(false, true, false, 600));
        assert!(key.from_elsewhere(600 + GRACE_MS - 1));
        assert!(!key.from_elsewhere(600 + GRACE_MS));
        assert!(key.update(true, true, false, 5000));
    }

    #[test]
    fn a_key_released_in_the_game_stays_suppressed_when_the_game_closes() {
        let mut key = KeyWatch::default();
        assert!(!key.update(true, false, false, 0));
        assert!(!key.update(false, false, false, 60));
        assert!(!key.update(false, false, false, 1400));
        assert!(!key.update(false, true, false, 2000));
        assert!(key.from_elsewhere(2000));
        assert!(!key.from_elsewhere(2900));
        assert!(key.update(true, true, false, 5000));
    }

    #[test]
    fn a_key_held_when_rebound_only_counts_after_release() {
        let mut key = KeyWatch {
            was: true,
            ..Default::default()
        };
        assert!(!key.update(true, true, false, 0));
        assert!(!key.update(false, true, false, 10));
        assert!(key.update(true, true, false, 20));
    }
}
