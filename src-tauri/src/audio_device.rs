use std::ffi::c_void;

#[cfg(windows)]
pub fn cycle_default_output() -> Result<String, String> {
    win::cycle_default_output()
}

#[cfg(not(windows))]
pub fn cycle_default_output() -> Result<String, String> {
    Err("Output device switching is only supported on Windows.".into())
}

#[cfg(windows)]
#[allow(non_snake_case)]
mod win {
    use super::*;
    use std::{ptr, slice};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    const CLSID_MM_DEVICE_ENUMERATOR: Guid = Guid {
        data1: 0xBCDE0395,
        data2: 0xE52F,
        data3: 0x467C,
        data4: [0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E],
    };
    const IID_MM_DEVICE_ENUMERATOR: Guid = Guid {
        data1: 0xA95664D2,
        data2: 0x9614,
        data3: 0x4F35,
        data4: [0xA7, 0x46, 0xDE, 0x8D, 0xB6, 0x36, 0x17, 0xE6],
    };
    const CLSID_POLICY_CONFIG: Guid = Guid {
        data1: 0x870AF99C,
        data2: 0x171D,
        data3: 0x4F9E,
        data4: [0xAF, 0x0D, 0xE6, 0x3D, 0xF4, 0x0C, 0x2B, 0xC9],
    };
    const IID_POLICY_CONFIG: Guid = Guid {
        data1: 0xF8679F50,
        data2: 0x850A,
        data3: 0x41CF,
        data4: [0x9C, 0x72, 0x43, 0x0F, 0x29, 0x02, 0x90, 0xC8],
    };
    const CLSCTX_ALL: u32 = 23;
    const COINIT_APARTMENTTHREADED: u32 = 2;
    const E_RENDER: i32 = 0;
    const DEVICE_STATE_ACTIVE: u32 = 1;
    const STGM_READ: u32 = 0;

    #[repr(C)]
    struct PropertyKey {
        fmtid: Guid,
        pid: u32,
    }

    #[repr(C)]
    struct PropVariant {
        vt: u16,
        reserved: [u16; 3],
        data: [usize; 2],
    }

    struct ComPtr(*mut c_void);

    impl ComPtr {
        fn new(raw: *mut c_void) -> Result<Self, String> {
            if raw.is_null() {
                Err("Windows returned an empty audio interface.".into())
            } else {
                Ok(Self(raw))
            }
        }
    }

    impl Drop for ComPtr {
        fn drop(&mut self) {
            unsafe {
                let release: unsafe extern "system" fn(*mut c_void) -> u32 = method(self.0, 2);
                release(self.0);
            }
        }
    }

    struct ComApartment;

    impl ComApartment {
        fn initialize() -> Result<Self, String> {
            let hr = unsafe { CoInitializeEx(ptr::null_mut(), COINIT_APARTMENTTHREADED) };
            if hr < 0 {
                Err(format!(
                    "Could not initialize Windows audio services (HRESULT 0x{:08X}).",
                    hr as u32
                ))
            } else {
                Ok(Self)
            }
        }
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }

    unsafe fn method<T: Copy>(object: *mut c_void, slot: usize) -> T {
        let vtable = *(object as *mut *mut *mut c_void);
        std::mem::transmute_copy(&*vtable.add(slot))
    }

    fn check(hr: i32, action: &str) -> Result<(), String> {
        if hr < 0 {
            Err(format!("{action} failed (HRESULT 0x{:08X}).", hr as u32))
        } else {
            Ok(())
        }
    }

    unsafe fn create(clsid: &Guid, iid: &Guid) -> Result<ComPtr, String> {
        let mut raw = ptr::null_mut();
        let hr = CoCreateInstance(clsid, ptr::null_mut(), CLSCTX_ALL, iid, &mut raw);
        check(hr, "Creating the Windows audio interface")?;
        ComPtr::new(raw)
    }

    unsafe fn device_id(device: *mut c_void) -> Result<Vec<u16>, String> {
        let mut raw = ptr::null_mut();
        let get_id: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> i32 =
            method(device, 5);
        check(get_id(device, &mut raw), "Reading an output device ID")?;
        if raw.is_null() {
            return Err("Windows returned an empty output device ID.".into());
        }
        let mut len = 0;
        while *raw.add(len) != 0 {
            len += 1;
        }
        let id = slice::from_raw_parts(raw, len).to_vec();
        CoTaskMemFree(raw as *mut c_void);
        Ok(id)
    }

    unsafe fn device_name(device: *mut c_void, id: &[u16]) -> String {
        let mut store_raw = ptr::null_mut();
        let open_store: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32 =
            method(device, 4);
        if open_store(device, STGM_READ, &mut store_raw) < 0 || store_raw.is_null() {
            return String::from_utf16_lossy(id);
        }
        let store = ComPtr(store_raw);
        let key = PropertyKey {
            fmtid: Guid {
                data1: 0xA45C254E,
                data2: 0xDF1C,
                data3: 0x4EFD,
                data4: [0x80, 0x20, 0x67, 0xD1, 0x46, 0xA8, 0x50, 0xE0],
            },
            pid: 14,
        };
        let mut value = PropVariant {
            vt: 0,
            reserved: [0; 3],
            data: [0; 2],
        };
        let get_value: unsafe extern "system" fn(
            *mut c_void,
            *const PropertyKey,
            *mut PropVariant,
        ) -> i32 = method(store.0, 5);
        if get_value(store.0, &key, &mut value) < 0 || value.vt != 31 {
            PropVariantClear(&mut value);
            return String::from_utf16_lossy(id);
        }
        let name_ptr = value.data[0] as *const u16;
        let mut len = 0;
        while !name_ptr.is_null() && *name_ptr.add(len) != 0 {
            len += 1;
        }
        let name = if name_ptr.is_null() {
            String::from_utf16_lossy(id)
        } else {
            String::from_utf16_lossy(slice::from_raw_parts(name_ptr, len))
        };
        PropVariantClear(&mut value);
        name
    }

    pub fn cycle_default_output() -> Result<String, String> {
        let _apartment = ComApartment::initialize()?;
        unsafe {
            let enumerator = create(&CLSID_MM_DEVICE_ENUMERATOR, &IID_MM_DEVICE_ENUMERATOR)?;
            let mut collection_raw = ptr::null_mut();
            let enumerate: unsafe extern "system" fn(
                *mut c_void,
                i32,
                u32,
                *mut *mut c_void,
            ) -> i32 = method(enumerator.0, 3);
            check(
                enumerate(
                    enumerator.0,
                    E_RENDER,
                    DEVICE_STATE_ACTIVE,
                    &mut collection_raw,
                ),
                "Enumerating active output devices",
            )?;
            let collection = ComPtr::new(collection_raw)?;
            let mut count = 0u32;
            let get_count: unsafe extern "system" fn(*mut c_void, *mut u32) -> i32 =
                method(collection.0, 3);
            check(
                get_count(collection.0, &mut count),
                "Counting output devices",
            )?;
            if count == 0 {
                return Err("Windows has no active output devices.".into());
            }

            let mut current_raw = ptr::null_mut();
            let get_default: unsafe extern "system" fn(
                *mut c_void,
                i32,
                i32,
                *mut *mut c_void,
            ) -> i32 = method(enumerator.0, 4);
            let current_id = if get_default(enumerator.0, E_RENDER, 1, &mut current_raw) >= 0
                && !current_raw.is_null()
            {
                let current = ComPtr(current_raw);
                device_id(current.0).ok()
            } else {
                None
            };

            let get_item: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32 =
                method(collection.0, 4);
            let mut devices = Vec::with_capacity(count as usize);
            for index in 0..count {
                let mut device_raw = ptr::null_mut();
                check(
                    get_item(collection.0, index, &mut device_raw),
                    "Reading an output device",
                )?;
                let device = ComPtr::new(device_raw)?;
                let id = device_id(device.0)?;
                let name = device_name(device.0, &id);
                devices.push((id, name));
            }
            let current_index = current_id
                .as_ref()
                .and_then(|id| devices.iter().position(|(candidate, _)| candidate == id));
            let next_index = current_index.map_or(0, |i| (i + 1) % devices.len());
            let (next_id, next_name) = &devices[next_index];
            let policy = create(&CLSID_POLICY_CONFIG, &IID_POLICY_CONFIG)?;
            // PolicyConfig is the Windows COM interface that accepts default-endpoint changes.
            let set_default: unsafe extern "system" fn(*mut c_void, *const u16, i32) -> i32 =
                method(policy.0, 13);
            for role in 0..3 {
                check(
                    set_default(policy.0, next_id.as_ptr(), role),
                    "Switching the default output device",
                )?;
            }
            Ok(next_name.clone())
        }
    }

    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> i32;
        fn CoUninitialize();
        fn CoCreateInstance(
            clsid: *const Guid,
            outer: *mut c_void,
            context: u32,
            iid: *const Guid,
            object: *mut *mut c_void,
        ) -> i32;
        fn CoTaskMemFree(memory: *mut c_void);
        fn PropVariantClear(value: *mut PropVariant) -> i32;
    }
}
