#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(windows)]
static SWITCHED_TO_EXTERNAL: AtomicBool = AtomicBool::new(false);
#[cfg(windows)]
static EXIT_FSE_ON_QUIT: AtomicBool = AtomicBool::new(false);

pub fn switch_to_tv() -> Result<(), String> {
    #[cfg(windows)]
    {
        EXIT_FSE_ON_QUIT.store(true, Ordering::Relaxed);
        windows::switch_display("/external")?;
        SWITCHED_TO_EXTERNAL.store(true, Ordering::Relaxed);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("Display switching is only supported on Windows.".into())
    }
}

pub fn on_exit() {
    #[cfg(windows)]
    {
        if EXIT_FSE_ON_QUIT.swap(false, Ordering::Relaxed) {
            if let Err(e) = windows::exit_fse_if_active() {
                crate::warn_log!("Could not exit Windows Full Screen Experience: {e}");
            }
        }
        if SWITCHED_TO_EXTERNAL.swap(false, Ordering::Relaxed) {
            if let Err(e) = windows::switch_display("/internal") {
                crate::warn_log!("Could not restore the PC display: {e}");
            }
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::{ffi::OsStr, process::Command};

    type Bool = i32;
    type Handle = isize;

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryW(name: *const u16) -> Handle;
        fn GetProcAddress(module: Handle, name: *const u8) -> *mut std::ffi::c_void;
        fn FreeLibrary(module: Handle) -> Bool;
    }

    type IsFseActive = unsafe extern "system" fn() -> Bool;
    type SetFseActive = unsafe extern "system" fn(Bool) -> i32;

    pub fn switch_display(mode: &str) -> Result<(), String> {
        use std::os::windows::process::CommandExt;

        let system_root = std::env::var_os("WINDIR").unwrap_or_else(|| OsStr::new(r"C:\Windows").to_os_string());
        let switcher = std::path::Path::new(&system_root).join("System32").join("DisplaySwitch.exe");
        let status = Command::new(switcher)
            .arg(mode)
            .creation_flags(0x0800_0000)
            .status()
            .map_err(|e| format!("Could not start Windows display switching: {e}"))?;
        if !status.success() {
            return Err(format!("Windows display switching exited with status {status}."));
        }
        Ok(())
    }

    pub fn exit_fse_if_active() -> Result<(), String> {
        let api_name: Vec<u16> = "api-ms-win-gaming-experience-l1-1-0.dll"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            let module = LoadLibraryW(api_name.as_ptr());
            if module == 0 {
                return Ok(());
            }
            let is_active = GetProcAddress(module, b"IsGamingFullScreenExperienceActive\0".as_ptr());
            let set_active = GetProcAddress(module, b"SetGamingFullScreenExperience\0".as_ptr());
            if is_active.is_null() || set_active.is_null() {
                FreeLibrary(module);
                return Ok(());
            }
            let is_active: IsFseActive = std::mem::transmute(is_active);
            let set_active: SetFseActive = std::mem::transmute(set_active);
            if is_active() == 0 {
                FreeLibrary(module);
                return Ok(());
            }
            let result = set_active(0);
            if result < 0 {
                FreeLibrary(module);
                return Err(format!("Windows rejected the request to exit FSE (HRESULT 0x{:08X}).", result as u32));
            }
            for _ in 0..50 {
                if is_active() == 0 {
                    FreeLibrary(module);
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            FreeLibrary(module);
            return Err("Windows did not leave FSE after the exit request.".into());
        }
    }
}
