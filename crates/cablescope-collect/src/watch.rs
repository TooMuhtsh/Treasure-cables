//! Détection des branchements et débranchements (appareils USB et écrans),
//! pour rafraîchir l'analyse sans intervention.
//!
//! Windows prévient l'application (notifications du gestionnaire de
//! périphériques) ; sous Linux, la liste des appareils de sysfs est relue
//! chaque seconde. Le veilleur ne fait que compter les changements : à
//! l'appelant d'attendre que l'appareil ait fini de se présenter avant de
//! relancer une analyse.

use std::sync::atomic::{AtomicU64, Ordering};

/// Changements signalés depuis le démarrage du programme.
static CHANGES: AtomicU64 = AtomicU64::new(0);

pub struct ChangeWatcher {
    #[cfg(windows)]
    handles: Vec<windows_sys::Win32::Devices::DeviceAndDriverInstallation::HCMNOTIFICATION>,
    #[cfg(target_os = "linux")]
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl ChangeWatcher {
    /// Nombre de changements signalés jusqu'ici (il ne fait que croître).
    pub fn count(&self) -> u64 {
        CHANGES.load(Ordering::Relaxed)
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        CM_NOTIFY_ACTION, CM_NOTIFY_EVENT_DATA, CM_NOTIFY_FILTER,
        CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE, CM_Register_Notification,
        CM_Unregister_Notification, CR_SUCCESS, HCMNOTIFICATION,
    };
    use windows_sys::Win32::Devices::Display::GUID_DEVINTERFACE_MONITOR;
    use windows_sys::Win32::Devices::Usb::{
        GUID_DEVINTERFACE_USB_DEVICE, GUID_DEVINTERFACE_USB_HUB,
    };

    /// Appelé par le système, sur un fil à lui, à chaque arrivée ou départ.
    unsafe extern "system" fn on_change(
        _notify: HCMNOTIFICATION,
        _context: *const core::ffi::c_void,
        _action: CM_NOTIFY_ACTION,
        _data: *const CM_NOTIFY_EVENT_DATA,
        _size: u32,
    ) -> u32 {
        CHANGES.fetch_add(1, Ordering::Relaxed);
        0
    }

    impl ChangeWatcher {
        pub fn start() -> Option<Self> {
            let mut handles = Vec::new();
            for class in [
                GUID_DEVINTERFACE_USB_DEVICE,
                GUID_DEVINTERFACE_USB_HUB,
                GUID_DEVINTERFACE_MONITOR,
            ] {
                let mut filter = CM_NOTIFY_FILTER {
                    cbSize: size_of::<CM_NOTIFY_FILTER>() as u32,
                    FilterType: CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE,
                    ..Default::default()
                };
                filter.u.DeviceInterface.ClassGuid = class;
                let mut handle: HCMNOTIFICATION = std::ptr::null_mut();
                // SAFETY: filtre initialisé, rappel sans contexte.
                let status = unsafe {
                    CM_Register_Notification(
                        &filter,
                        std::ptr::null(),
                        Some(on_change),
                        &mut handle,
                    )
                };
                if status == CR_SUCCESS {
                    handles.push(handle);
                }
            }
            (!handles.is_empty()).then_some(ChangeWatcher { handles })
        }
    }

    impl Drop for ChangeWatcher {
        fn drop(&mut self) {
            for handle in self.handles.drain(..) {
                // SAFETY: inscription obtenue de CM_Register_Notification.
                unsafe { CM_Unregister_Notification(handle) };
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    /// Appareils USB et état des sorties vidéo, tels que vus par sysfs.
    fn snapshot() -> Vec<String> {
        let mut items: Vec<String> = std::fs::read_dir("/sys/bus/usb/devices")
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        for entry in std::fs::read_dir("/sys/class/drm")
            .into_iter()
            .flatten()
            .flatten()
        {
            if let Ok(status) = std::fs::read_to_string(entry.path().join("status")) {
                items.push(format!(
                    "{}={}",
                    entry.file_name().to_string_lossy(),
                    status.trim()
                ));
            }
        }
        items.sort();
        items
    }

    impl ChangeWatcher {
        pub fn start() -> Option<Self> {
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            std::thread::Builder::new()
                .name("veille-usb".into())
                .spawn(move || {
                    let mut last = snapshot();
                    while !flag.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_secs(1));
                        let now = snapshot();
                        if now != last {
                            CHANGES.fetch_add(1, Ordering::Relaxed);
                            last = now;
                        }
                    }
                })
                .ok()?;
            Some(ChangeWatcher { stop })
        }
    }

    impl Drop for ChangeWatcher {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
impl ChangeWatcher {
    pub fn start() -> Option<Self> {
        None
    }
}
