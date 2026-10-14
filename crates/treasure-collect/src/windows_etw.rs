//! Diagnostic approfondi sous Windows : écoute ETW de la pile USB.
//!
//! Session temps réel sur les fournisseurs Microsoft-Windows-USB-USBHUB3 et
//! Microsoft-Windows-USB-USBXHCI (droits administrateur, ou groupe
//! « Utilisateurs du journal de performances »). Les champs des événements
//! sont lus par nom avec TDH ; l'analyse est dans `deep`.

use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Instant;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, HANDLE,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use windows_sys::Win32::System::Diagnostics::Etw::{
    CONTROLTRACE_HANDLE, CloseTrace, ControlTraceW, EVENT_CONTROL_CODE_CAPTURE_STATE,
    EVENT_CONTROL_CODE_ENABLE_PROVIDER, EVENT_RECORD, EVENT_TRACE_CONTROL_STOP,
    EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES, EVENT_TRACE_REAL_TIME_MODE, EnableTraceEx2,
    OpenTraceW, PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_REAL_TIME, PROCESSTRACE_HANDLE,
    PROPERTY_DATA_DESCRIPTOR, ProcessTrace, StartTraceW, TdhGetProperty, TdhGetPropertySize,
    WNODE_FLAG_TRACED_GUID,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows_sys::core::GUID;

use treasure_model::LinkDiagnostics;

use crate::Error;
use crate::deep::{Analyzer, Provider, UsbEvent};

const SESSION: &str = "TreasureCables-USB";
const HUB3: GUID = GUID::from_u128(0xac52ad17_cc01_4f85_8df5_4dce4333c99b);
const XHCI: GUID = GUID::from_u128(0x30e1d284_5d88_459c_83fd_6345b39b19ec);

/// USBHUB3 : Default, USBError, Enumeration, HWVerifyHost/Hub/Device,
/// Rundown, et le mot-clé non nommé porté par la plupart des erreurs.
const HUB3_KEYWORDS: u64 = 0x1 | 0x2 | 0x400 | 0x1000 | 0x2000 | 0x4000 | 0x8000 | 0x4000_0000_0000;
const XHCI_KEYWORDS: u64 = 0x1 | 0x4000_0000_0000;
const TRACE_LEVEL_VERBOSE: u8 = 5;
const TRACE_LEVEL_INFORMATION: u8 = 4;
const INVALID_PROCESSTRACE_HANDLE: u64 = u64::MAX;

pub fn is_elevated() -> bool {
    let mut token: HANDLE = null_mut();
    // SAFETY: pseudo-poignée du processus courant.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return false;
    }
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned = 0u32;
    // SAFETY: structure de la taille annoncée.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut c_void,
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    // SAFETY: poignée ouverte ci-dessus.
    unsafe { CloseHandle(token) };
    ok != 0 && elevation.TokenIsElevated != 0
}

/// Relance l'outil en administrateur (invite UAC), dans une nouvelle fenêtre.
pub fn relaunch_elevated(arguments: &str) -> Result<(), Error> {
    let exe = std::env::current_exe()?;
    let exe = to_wide(&exe.to_string_lossy());
    let verb = to_wide("runas");
    let args = to_wide(arguments);
    // SAFETY: chaînes terminées par un zéro.
    let result = unsafe {
        ShellExecuteW(
            null_mut(),
            verb.as_ptr(),
            exe.as_ptr(),
            args.as_ptr(),
            null(),
            SW_SHOWNORMAL,
        )
    };
    // Valeur > 32 : succès (convention historique de ShellExecute).
    if result as usize > 32 {
        Ok(())
    } else {
        Err(Error::Io(std::io::Error::other(
            "relance en administrateur refusée ou impossible",
        )))
    }
}

/// Écoute en cours. `stop` renvoie le bilan ; abandonnée, elle s'arrête
/// proprement (la session ETW est toujours fermée).
pub struct DeepCapture {
    session: CONTROLTRACE_HANDLE,
    trace: PROCESSTRACE_HANDLE,
    thread: Option<JoinHandle<()>>,
    analyzer: *mut Mutex<Analyzer>,
    started: Instant,
    stopped: bool,
}

impl DeepCapture {
    pub fn start(buses: HashMap<(u32, u32, u32), String>) -> Result<DeepCapture, Error> {
        let mut properties = properties_buffer();
        let name = to_wide(SESSION);
        let mut session = CONTROLTRACE_HANDLE { Value: 0 };
        // SAFETY: tampon aligné et dimensionné (propriétés + nom de session).
        let mut status =
            unsafe { StartTraceW(&mut session, name.as_ptr(), properties_ptr(&mut properties)) };
        if status == ERROR_ALREADY_EXISTS {
            // Session restée ouverte après un arrêt brutal : on la ferme.
            stop_session(CONTROLTRACE_HANDLE { Value: 0 }, Some(&name));
            properties = properties_buffer();
            // SAFETY: idem.
            status = unsafe {
                StartTraceW(&mut session, name.as_ptr(), properties_ptr(&mut properties))
            };
        }
        match status {
            0 => {}
            ERROR_ACCESS_DENIED => {
                return Err(Error::Unsupported(
                    "droits administrateur nécessaires pour écouter la pile USB",
                ));
            }
            code => {
                return Err(Error::Io(std::io::Error::other(format!(
                    "impossible de démarrer l'écoute ETW (code {code})"
                ))));
            }
        }

        let enable = |guid: &GUID, level: u8, keywords: u64| {
            // SAFETY: session ouverte, GUID valide.
            unsafe {
                EnableTraceEx2(
                    session,
                    guid,
                    EVENT_CONTROL_CODE_ENABLE_PROVIDER,
                    level,
                    keywords,
                    0,
                    0,
                    null(),
                )
            }
        };
        enable(&HUB3, TRACE_LEVEL_VERBOSE, HUB3_KEYWORDS);
        enable(&XHCI, TRACE_LEVEL_INFORMATION, XHCI_KEYWORDS);

        let analyzer = Box::into_raw(Box::new(Mutex::new(Analyzer::new(buses))));
        let mut logger_name = to_wide(SESSION);
        // SAFETY: structure entièrement initialisée à zéro, puis renseignée.
        let mut logfile: EVENT_TRACE_LOGFILEW = unsafe { std::mem::zeroed() };
        logfile.LoggerName = logger_name.as_mut_ptr();
        logfile.Anonymous1.ProcessTraceMode =
            PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
        logfile.Anonymous2.EventRecordCallback = Some(on_event);
        logfile.Context = analyzer as *mut c_void;
        // SAFETY: structure valide pendant l'appel.
        let trace = unsafe { OpenTraceW(&mut logfile) };
        if trace.Value == INVALID_PROCESSTRACE_HANDLE {
            stop_session(session, None);
            // SAFETY: récupère la boîte créée ci-dessus, inutilisée.
            drop(unsafe { Box::from_raw(analyzer) });
            return Err(Error::Io(std::io::Error::other(
                "impossible d'ouvrir l'écoute ETW",
            )));
        }
        let handle = trace.Value;
        let thread = std::thread::spawn(move || {
            let trace = PROCESSTRACE_HANDLE { Value: handle };
            // SAFETY: bloque jusqu'à l'arrêt de la session.
            unsafe { ProcessTrace(&trace, 1, null(), null()) };
        });

        // Une fois l'écoute branchée : demande l'état courant (rundown),
        // qui situe chaque appareil déjà connecté.
        std::thread::sleep(std::time::Duration::from_millis(300));
        // SAFETY: session ouverte, GUID valide.
        unsafe {
            EnableTraceEx2(
                session,
                &HUB3,
                EVENT_CONTROL_CODE_CAPTURE_STATE,
                TRACE_LEVEL_VERBOSE,
                HUB3_KEYWORDS,
                0,
                0,
                null(),
            )
        };

        Ok(DeepCapture {
            session,
            trace,
            thread: Some(thread),
            analyzer,
            started: Instant::now(),
            stopped: false,
        })
    }

    /// Événements USB pertinents reçus jusqu'ici.
    pub fn events(&self) -> u32 {
        // SAFETY: l'analyseur vit jusqu'à `stop`.
        unsafe { &*self.analyzer }
            .lock()
            .map(|a| a.events())
            .unwrap_or(0)
    }

    pub fn elapsed_s(&self) -> u32 {
        self.started.elapsed().as_secs() as u32
    }

    pub fn stop(mut self) -> LinkDiagnostics {
        let duration = self.elapsed_s();
        self.shutdown();
        // SAFETY: le fil de lecture est terminé, plus aucun rappel possible.
        let analyzer = unsafe { Box::from_raw(self.analyzer) };
        self.analyzer = null_mut();
        analyzer
            .into_inner()
            .map(|a| a.finish(duration))
            .unwrap_or_default()
    }

    fn shutdown(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        stop_session(self.session, None);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        // SAFETY: ProcessTrace est terminé.
        unsafe { CloseTrace(self.trace) };
    }
}

impl Drop for DeepCapture {
    fn drop(&mut self) {
        self.shutdown();
        if !self.analyzer.is_null() {
            // SAFETY: plus aucun rappel après `shutdown`.
            drop(unsafe { Box::from_raw(self.analyzer) });
        }
    }
}

/// Rappel ETW, appelé sur le fil de `ProcessTrace`.
unsafe extern "system" fn on_event(record: *mut EVENT_RECORD) {
    // SAFETY: pointeur fourni par ETW pour la durée du rappel.
    let Some(record) = (unsafe { record.as_ref() }) else {
        return;
    };
    let provider = if same_guid(&record.EventHeader.ProviderId, &HUB3) {
        Provider::Hub3
    } else if same_guid(&record.EventHeader.ProviderId, &XHCI) {
        Provider::Xhci
    } else {
        return;
    };
    let id = record.EventHeader.EventDescriptor.Id;
    if !crate::deep::is_relevant(provider, id) {
        return;
    }
    let depth = u32_prop(record, "fid_PortPathDepth").unwrap_or(0).min(8);
    let event = UsbEvent {
        id,
        usb_device: pointer_prop(record, "fid_UsbDevice"),
        hub_device: pointer_prop(record, "fid_HubDevice"),
        port: u32_prop(record, "fid_PortNumber"),
        nt_status: u32_prop(record, "fid_NtStatus"),
        vendor_id: u16_prop(record, "fid_idVendor"),
        product_id: u16_prop(record, "fid_idProduct"),
        port_path: (0..depth)
            .filter_map(|i| property(record, "fid_PortPath", i).and_then(|b| le_u32(&b)))
            .collect(),
        pci: match (
            u32_prop(record, "fid_PciBus"),
            u32_prop(record, "fid_PciDevice"),
            u32_prop(record, "fid_PciFunction"),
        ) {
            (Some(b), Some(d), Some(f)) => Some((b, d, f)),
            _ => None,
        },
    };
    // SAFETY: contexte fourni à OpenTraceW, valide jusqu'à `stop`.
    let analyzer = unsafe { &*(record.UserContext as *const Mutex<Analyzer>) };
    if let Ok(mut analyzer) = analyzer.lock() {
        analyzer.feed(provider, &event);
    }
}

/// Lit un champ par son nom (élément `index` d'un tableau, ou `u32::MAX`).
fn property(record: &EVENT_RECORD, name: &str, index: u32) -> Option<Vec<u8>> {
    let wide = to_wide(name);
    let descriptor = PROPERTY_DATA_DESCRIPTOR {
        PropertyName: wide.as_ptr() as u64,
        ArrayIndex: index,
        Reserved: 0,
    };
    let mut size = 0u32;
    // SAFETY: enregistrement valide, descripteur pointant vers `wide`.
    if unsafe { TdhGetPropertySize(record, 0, null(), 1, &descriptor, &mut size) } != 0 || size == 0
    {
        return None;
    }
    let mut buffer = vec![0u8; size as usize];
    // SAFETY: tampon de `size` octets.
    if unsafe { TdhGetProperty(record, 0, null(), 1, &descriptor, size, buffer.as_mut_ptr()) } != 0
    {
        return None;
    }
    Some(buffer)
}

fn le_u32(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?))
}

fn u32_prop(record: &EVENT_RECORD, name: &str) -> Option<u32> {
    le_u32(&property(record, name, u32::MAX)?)
}

fn u16_prop(record: &EVENT_RECORD, name: &str) -> Option<u16> {
    let b = property(record, name, u32::MAX)?;
    Some(u16::from_le_bytes(b.get(..2)?.try_into().ok()?))
}

fn pointer_prop(record: &EVENT_RECORD, name: &str) -> Option<u64> {
    let b = property(record, name, u32::MAX)?;
    match b.len() {
        8 => Some(u64::from_le_bytes(b[..8].try_into().ok()?)),
        4 => le_u32(&b).map(u64::from),
        _ => None,
    }
}

fn same_guid(a: &GUID, b: &GUID) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}

/// Propriétés de session suivies de son nom (exigence d'ETW), dans un
/// tampon aligné sur 8 octets.
fn properties_buffer() -> Vec<u64> {
    let bytes = size_of::<EVENT_TRACE_PROPERTIES>() + 1024;
    let mut buffer = vec![0u64; bytes.div_ceil(8)];
    let props = buffer.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES;
    // SAFETY: tampon assez grand et aligné pour la structure.
    unsafe {
        (*props).Wnode.BufferSize = (buffer.len() * 8) as u32;
        (*props).Wnode.Flags = WNODE_FLAG_TRACED_GUID;
        (*props).Wnode.ClientContext = 1; // horloge QPC
        (*props).LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
        (*props).FlushTimer = 1; // livraison au plus toutes les secondes
        (*props).LoggerNameOffset = size_of::<EVENT_TRACE_PROPERTIES>() as u32;
    }
    buffer
}

fn properties_ptr(buffer: &mut [u64]) -> *mut EVENT_TRACE_PROPERTIES {
    buffer.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES
}

fn stop_session(session: CONTROLTRACE_HANDLE, name: Option<&[u16]>) {
    let mut properties = properties_buffer();
    // SAFETY: tampon dimensionné ; la session est désignée par sa poignée ou son nom.
    unsafe {
        ControlTraceW(
            session,
            name.map_or(null(), |n| n.as_ptr()),
            properties_ptr(&mut properties),
            EVENT_TRACE_CONTROL_STOP,
        )
    };
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
