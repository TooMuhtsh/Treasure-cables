//! Console et messages selon la plateforme.
//!
//! Sous Windows, l'exécutable publié est une application graphique : aucune
//! console ne s'ouvre au double-clic. Les modes texte se rattachent à la
//! console d'où l'outil a été lancé, l'interface console en ouvre une.

/// Rattache la sortie à la console du terminal appelant (modes texte).
pub fn attach_console() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
        // SAFETY: sans effet si le processus a déjà une console.
        unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    }
}

/// Ouvre une console propre à l'outil (interface plein écran en console).
pub fn own_console() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::{AllocConsole, FreeConsole};
        // SAFETY: se détache d'une éventuelle console puis en crée une.
        unsafe {
            FreeConsole();
            AllocConsole();
        }
    }
}

/// Affiche un message à l'utilisateur, même sans console (boîte de dialogue
/// sous Windows). Bloque jusqu'à sa fermeture.
pub fn message(text: &str) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW};
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
        let (text, title) = (wide(text), wide("Treasure Cables"));
        // SAFETY: chaînes terminées par un zéro.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONINFORMATION,
            )
        };
    }
    #[cfg(not(windows))]
    eprintln!("{text}");
}
