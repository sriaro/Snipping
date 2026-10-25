use std::{mem::ManuallyDrop, path::Path};
use windows::{
    Win32::{
        Foundation::{ERROR_INSUFFICIENT_BUFFER, HWND, LPARAM, LRESULT, PROPERTYKEY, WPARAM},
        Storage::Packaging::Appx::GetCurrentApplicationUserModelId,
        System::{
            Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0, PropVariantClear},
            Variant::VT_LPWSTR,
        },
        UI::{
            Shell::{
                DefSubclassProc, GetWindowSubclass,
                PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow},
                RemoveWindowSubclass, SHStrDupW, SetWindowSubclass,
            },
            WindowsAndMessaging::WM_NCDESTROY,
        },
    },
    core::{GUID, PCWSTR, PWSTR},
};

const PROPERTY_IDS: [u32; 4] = [2, 4, 3, 5];
const SUBCLASS_ID: usize = 0x534e4950;

fn key(id: u32) -> PROPERTYKEY {
    PROPERTYKEY { fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3), pid: id }
}

fn properties(exe: &Path, app_id: String) -> anyhow::Result<[(u32, String); 4]> {
    anyhow::ensure!(exe.is_absolute(), "taskbar executable path is not absolute");
    let path = exe.to_str().ok_or_else(|| anyhow::anyhow!("taskbar executable path is not Unicode"))?;
    anyhow::ensure!(!path.contains(['\0', '"']), "taskbar executable path is not valid");
    anyhow::ensure!(!app_id.is_empty() && !app_id.contains('\0'), "taskbar app identity is not valid");
    let path = path.strip_prefix(r"\\?\UNC\").map(|rest| format!(r"\\{rest}")).unwrap_or_else(|| path.strip_prefix(r"\\?\").unwrap_or(path).to_string());
    Ok([(2, format!("\"{path}\"")), (4, "Snipping".into()), (3, format!("{path},-1")), (5, app_id)])
}

fn current_app_id() -> anyhow::Result<String> {
    let mut length = 0;
    let status = unsafe { GetCurrentApplicationUserModelId(&mut length, None) };
    if status == ERROR_INSUFFICIENT_BUFFER && (2..=256).contains(&length) {
        let mut value = vec![0u16; length as usize];
        let status = unsafe { GetCurrentApplicationUserModelId(&mut length, Some(PWSTR(value.as_mut_ptr()))) };
        anyhow::ensure!(status.0 == 0 && length as usize <= value.len(), "taskbar app identity read failed: {}", status.0);
        let end = value[..length as usize].iter().position(|&c| c == 0).ok_or_else(|| anyhow::anyhow!("taskbar app identity is not terminated"))?;
        return String::from_utf16(&value[..end]).map_err(Into::into);
    }
    if crate::shell_pkg::has_own_identity() {
        return Ok(crate::shell_pkg::application_user_model_id());
    }
    anyhow::ensure!(!crate::shell_pkg::has_identity(), "taskbar app identity unavailable: {}", status.0);
    Ok("Snipping.Desktop".into())
}

struct SavedValue(PROPVARIANT);

impl Drop for SavedValue {
    fn drop(&mut self) {
        let _ = unsafe { PropVariantClear(&mut self.0) };
    }
}

fn set_string(store: &IPropertyStore, id: u32, value: &str) -> windows::core::Result<()> {
    let text = crate::wide(value);
    let text = unsafe { SHStrDupW(PCWSTR(text.as_ptr())) }?;
    let mut value = PROPVARIANT { Anonymous: PROPVARIANT_0 { Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 { vt: VT_LPWSTR, Anonymous: PROPVARIANT_0_0_0 { pwszVal: text }, ..Default::default() }) } };
    let result = unsafe { store.SetValue(&key(id), &value) };
    unsafe {
        let _ = PropVariantClear(&mut value);
    }
    result
}

fn apply_all<E>(values: &[(u32, String)], mut set: impl FnMut(u32, &str) -> Result<(), E>, mut rollback: impl FnMut()) -> Result<(), E> {
    for (id, value) in values {
        if let Err(error) = set(*id, value) {
            rollback();
            return Err(error);
        }
    }
    Ok(())
}

fn clear(store: &IPropertyStore) {
    let empty = PROPVARIANT::default();
    for id in PROPERTY_IDS {
        let _ = unsafe { store.SetValue(&key(id), &empty) };
    }
}

unsafe extern "system" fn subclass(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM, id: usize, _: usize) -> LRESULT {
    if message == WM_NCDESTROY {
        if let Ok(store) = unsafe { SHGetPropertyStoreForWindow::<IPropertyStore>(hwnd) } {
            clear(&store);
        }
        let _ = unsafe { RemoveWindowSubclass(hwnd, Some(subclass), id) };
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

pub fn attach(hwnd: HWND) -> anyhow::Result<()> {
    let mut existing = 0;
    if unsafe { GetWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID, Some(&mut existing)) }.as_bool() {
        return Ok(());
    }
    let values = properties(&std::env::current_exe()?, current_app_id()?)?;
    let store: IPropertyStore = unsafe { SHGetPropertyStoreForWindow(hwnd)? };
    let mut before = Vec::with_capacity(PROPERTY_IDS.len());
    for id in PROPERTY_IDS {
        before.push((id, SavedValue(unsafe { store.GetValue(&key(id))? })));
    }
    anyhow::ensure!(unsafe { SetWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID, 0) }.as_bool(), "taskbar cleanup hook could not be installed");
    let result = apply_all(
        &values,
        |id, value| set_string(&store, id, value),
        || {
            for (id, value) in &before {
                let _ = unsafe { store.SetValue(&key(*id), &value.0) };
            }
        },
    );
    if result.is_err() {
        let _ = unsafe { RemoveWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID) };
    }
    result.map_err(Into::into)
}
