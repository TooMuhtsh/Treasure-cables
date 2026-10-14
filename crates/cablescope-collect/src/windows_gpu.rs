//! Lien DisplayPort réellement négocié, lu auprès des pilotes graphiques.
//!
//! Windows n'expose ni le débit par voie ni le nombre de voies d'un lien
//! DisplayPort. Les bibliothèques installées avec les pilotes, si présentes,
//! le font sans droits administrateur :
//!
//! - Intel : IGCL (`ControlLib.dll`), lecture AUX des registres DPCD de l'écran
//!   (0x001-0x002 : maximum de l'écran ; 0x100-0x101 : lien entraîné), et type
//!   de sortie (USB-C, Thunderbolt, USB4) ;
//! - NVIDIA : NVAPI (`nvapi64.dll`), `NvAPI_GetDisplayPortInfo`.
//!
//! Les DLL sont chargées dynamiquement : absentes, on passe simplement.

use std::collections::HashMap;
use std::ffi::{CString, c_void};
use std::mem::size_of;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};

use cablescope_model::{DisplayTransport, DpLink};

struct Library(HMODULE);

impl Library {
    /// Charge une DLL uniquement depuis System32 (pas depuis le dossier courant).
    fn load(name: &str) -> Option<Library> {
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        // SAFETY: nom terminé par un zéro.
        let module =
            unsafe { LoadLibraryExW(wide.as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
        (!module.is_null()).then_some(Library(module))
    }

    /// SAFETY: `T` doit être le type de pointeur de fonction exact du symbole.
    unsafe fn symbol<T: Copy>(&self, name: &str) -> Option<T> {
        let name = CString::new(name).ok()?;
        // SAFETY: module chargé, nom terminé par un zéro.
        let address = unsafe { GetProcAddress(self.0, name.as_ptr() as *const u8) }?;
        // SAFETY: l'appelant garantit la signature.
        Some(unsafe { std::mem::transmute_copy(&address) })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: module chargé par LoadLibraryExW.
        unsafe { FreeLibrary(self.0) };
    }
}

/// Codes DPCD de débit par voie (LINK_BW_SET) -> Mb/s.
fn dpcd_rate_mbps(code: u8) -> Option<u32> {
    Some(match code {
        0x06 => 1620,
        0x0a => 2700,
        0x14 => 5400,
        0x1e => 8100,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Intel IGCL (en-tête igcl_api.h, version 1.1)

#[repr(C)]
#[derive(Default)]
struct CtlInitArgs {
    size: u32,
    version: u8,
    app_version: u32,
    flags: u32,
    supported_version: u32,
    application_uid: [u8; 16],
}

#[repr(C)]
#[derive(Default)]
struct CtlDisplayTiming {
    size: u32,
    version: u8,
    pixel_clock: u64,
    h_active: u32,
    v_active: u32,
    h_total: u32,
    v_total: u32,
    h_blank: u32,
    v_blank: u32,
    h_sync: u32,
    v_sync: u32,
    refresh_rate: f32,
    signal_standard: u32,
    vic_id: u8,
}

/// `ctl_os_display_encoder_identifier_t` : union d'un u32 et d'un
/// `ctl_generic_void_datatype_t` (pointeur + u32).
#[repr(C)]
#[derive(Default)]
struct CtlEncoderId {
    windows_encoder_id: u32,
    _rest: u32,
    _size: u64,
}

#[repr(C)]
#[derive(Default)]
struct CtlDisplayProperties {
    size: u32,
    version: u8,
    os_display_encoder_handle: CtlEncoderId,
    output_type: u32,
    attached_display_mux_type: u32,
    protocol_converter_output: u32,
    supported_spec: [u8; 3],
    supported_output_bpc_flags: u32,
    protocol_converter_type: u32,
    display_config_flags: u32,
    feature_enabled_flags: u32,
    feature_supported_flags: u32,
    advanced_feature_enabled_flags: u32,
    advanced_feature_supported_flags: u32,
    display_timing_info: CtlDisplayTiming,
    reserved_fields: [u32; 16],
}

#[repr(C)]
struct CtlAuxAccessArgs {
    size: u32,
    version: u8,
    op_type: u32,
    flags: u32,
    address: u32,
    rad: u64,
    port_id: u32,
    data_size: u32,
    data: [u8; 132],
}

// Tailles calculées d'après igcl_api.h (alignement MSVC x64).
const _: () = {
    assert!(size_of::<CtlInitArgs>() == 36);
    assert!(size_of::<CtlDisplayTiming>() == 64);
    assert!(size_of::<CtlEncoderId>() == 16);
    assert!(size_of::<CtlDisplayProperties>() == 200);
    assert!(size_of::<CtlAuxAccessArgs>() == 176);
};

const CTL_IMPL_VERSION: u32 = (1 << 16) | 1;
const CTL_RESULT_SUCCESS: u32 = 0;
const CTL_DISPLAY_OUTPUT_TYPES_DISPLAYPORT: u32 = 1;
const CTL_DISPLAY_CONFIG_FLAG_DISPLAY_ACTIVE: u32 = 1;
const CTL_OPERATION_TYPE_READ: u32 = 1;
const CTL_AUX_FLAG_NATIVE_AUX: u32 = 1;

type CtlInit = unsafe extern "C" fn(*mut CtlInitArgs, *mut *mut c_void) -> u32;
type CtlClose = unsafe extern "C" fn(*mut c_void) -> u32;
type CtlEnumerate = unsafe extern "C" fn(*mut c_void, *mut u32, *mut *mut c_void) -> u32;
type CtlGetDisplayProperties = unsafe extern "C" fn(*mut c_void, *mut CtlDisplayProperties) -> u32;
type CtlAuxAccess = unsafe extern "C" fn(*mut c_void, *mut CtlAuxAccessArgs) -> u32;

/// Sortie d'affichage Intel active, indexée par identifiant d'encodeur
/// Windows (identifiant de cible de QueryDisplayConfig).
pub struct IntelOutput {
    pub transport: Option<DisplayTransport>,
    pub link: Option<DpLink>,
}

pub fn intel_outputs() -> HashMap<u32, IntelOutput> {
    let mut outputs = HashMap::new();
    let Some(lib) = Library::load("ControlLib.dll") else {
        return outputs;
    };
    // SAFETY: signatures conformes à igcl_api.h.
    let (
        Some(init),
        Some(close),
        Some(enum_devices),
        Some(enum_outputs),
        Some(get_props),
        Some(aux),
    ) = (unsafe {
        (
            lib.symbol::<CtlInit>("ctlInit"),
            lib.symbol::<CtlClose>("ctlClose"),
            lib.symbol::<CtlEnumerate>("ctlEnumerateDevices"),
            lib.symbol::<CtlEnumerate>("ctlEnumerateDisplayOutputs"),
            lib.symbol::<CtlGetDisplayProperties>("ctlGetDisplayProperties"),
            lib.symbol::<CtlAuxAccess>("ctlAUXAccess"),
        )
    })
    else {
        return outputs;
    };

    let mut args = CtlInitArgs {
        size: size_of::<CtlInitArgs>() as u32,
        app_version: CTL_IMPL_VERSION,
        ..Default::default()
    };
    let mut api = null_mut();
    // SAFETY: structure dimensionnée selon l'en-tête.
    if unsafe { init(&mut args, &mut api) } != CTL_RESULT_SUCCESS || api.is_null() {
        return outputs;
    }

    for device in enumerate(enum_devices, api) {
        for output in enumerate(enum_outputs, device) {
            let mut props = CtlDisplayProperties {
                size: size_of::<CtlDisplayProperties>() as u32,
                ..Default::default()
            };
            props.display_timing_info.size = size_of::<CtlDisplayTiming>() as u32;
            // SAFETY: poignée renvoyée par l'énumération, structure dimensionnée.
            if unsafe { get_props(output, &mut props) } != CTL_RESULT_SUCCESS
                || props.display_config_flags & CTL_DISPLAY_CONFIG_FLAG_DISPLAY_ACTIVE == 0
            {
                continue;
            }
            let transport = match props.attached_display_mux_type {
                0 => Some(DisplayTransport::Native),
                1 => Some(DisplayTransport::Thunderbolt),
                2 => Some(DisplayTransport::UsbC),
                3 => Some(DisplayTransport::Usb4),
                _ => None,
            };
            let link = (props.output_type == CTL_DISPLAY_OUTPUT_TYPES_DISPLAYPORT)
                .then(|| intel_dp_link(aux, output))
                .flatten();
            outputs.insert(
                props.os_display_encoder_handle.windows_encoder_id,
                IntelOutput { transport, link },
            );
        }
    }
    // SAFETY: poignée obtenue par ctlInit.
    unsafe { close(api) };
    outputs
}

fn enumerate(function: CtlEnumerate, parent: *mut c_void) -> Vec<*mut c_void> {
    let mut count = 0u32;
    // SAFETY: premier appel pour le nombre d'éléments.
    if unsafe { function(parent, &mut count, null_mut()) } != CTL_RESULT_SUCCESS || count == 0 {
        return Vec::new();
    }
    let mut handles = vec![null_mut(); count as usize];
    // SAFETY: tableau de `count` poignées.
    if unsafe { function(parent, &mut count, handles.as_mut_ptr()) } != CTL_RESULT_SUCCESS {
        return Vec::new();
    }
    handles.truncate(count as usize);
    handles
}

/// Lecture AUX native des registres DPCD (lecture seule, sans droits).
fn read_dpcd(aux: CtlAuxAccess, output: *mut c_void, address: u32, len: u32) -> Option<Vec<u8>> {
    let mut args = CtlAuxAccessArgs {
        size: size_of::<CtlAuxAccessArgs>() as u32,
        version: 0,
        op_type: CTL_OPERATION_TYPE_READ,
        flags: CTL_AUX_FLAG_NATIVE_AUX,
        address,
        rad: 0,
        port_id: 0,
        data_size: len,
        data: [0; 132],
    };
    // SAFETY: structure dimensionnée selon l'en-tête.
    if unsafe { aux(output, &mut args) } != CTL_RESULT_SUCCESS {
        return None;
    }
    Some(args.data[..len.min(132) as usize].to_vec())
}

fn intel_dp_link(aux: CtlAuxAccess, output: *mut c_void) -> Option<DpLink> {
    let caps = read_dpcd(aux, output, 0x000, 3)?;
    let trained = read_dpcd(aux, output, 0x100, 2)?;
    Some(DpLink {
        // 0 : débit choisi par table (eDP) ou lien UHBR, non décodé ici.
        lane_rate_mbps: dpcd_rate_mbps(trained[0])?,
        lanes: trained[1] & 0x1f,
        max_lane_rate_mbps: dpcd_rate_mbps(caps[1]),
        max_lanes: Some(caps[2] & 0x1f).filter(|l| *l > 0),
        source: "Intel IGCL (registres DPCD de l'écran)".to_owned(),
    })
    .filter(|l| l.lanes > 0)
}

// ---------------------------------------------------------------------------
// NVIDIA NVAPI (en-têtes nvapi.h et nvapi_interface.h)

const NVAPI_INITIALIZE: u32 = 0x0150_e828;
const NVAPI_GET_DISPLAY_PORT_INFO: u32 = 0xc64f_f367;
const NVAPI_DISP_GET_DISPLAY_ID_BY_DISPLAY_NAME: u32 = 0xae45_7190;

/// `NV_DISPLAY_PORT_INFO_V1` : dix champs de 32 bits puis un mot de drapeaux.
#[repr(C)]
#[derive(Default)]
struct NvDisplayPortInfo {
    version: u32,
    dpcd_ver: u32,
    max_link_rate: u32,
    max_lane_count: u32,
    cur_link_rate: u32,
    cur_lane_count: u32,
    color_format: u32,
    dynamic_range: u32,
    colorimetry: u32,
    bpc: u32,
    flags: u32,
}

const _: () = assert!(size_of::<NvDisplayPortInfo>() == 44);

type NvQueryInterface = unsafe extern "C" fn(u32) -> *mut c_void;
type NvInitialize = unsafe extern "C" fn() -> i32;
type NvGetDisplayIdByName = unsafe extern "C" fn(*const u8, *mut u32) -> i32;
type NvGetDisplayPortInfo = unsafe extern "C" fn(*mut c_void, u32, *mut NvDisplayPortInfo) -> i32;

pub struct Nvidia {
    _lib: Library,
    display_id: NvGetDisplayIdByName,
    port_info: NvGetDisplayPortInfo,
}

impl Nvidia {
    pub fn load() -> Option<Nvidia> {
        let lib = Library::load("nvapi64.dll")?;
        // SAFETY: point d'entrée unique de NVAPI.
        let query: NvQueryInterface = unsafe { lib.symbol("nvapi_QueryInterface") }?;
        let get = |id: u32| {
            // SAFETY: identifiants de nvapi_interface.h.
            let p = unsafe { query(id) };
            (!p.is_null()).then_some(p)
        };
        // SAFETY: pointeurs de fonction aux signatures de nvapi.h.
        let (initialize, display_id, port_info) = unsafe {
            (
                std::mem::transmute::<*mut c_void, NvInitialize>(get(NVAPI_INITIALIZE)?),
                std::mem::transmute::<*mut c_void, NvGetDisplayIdByName>(get(
                    NVAPI_DISP_GET_DISPLAY_ID_BY_DISPLAY_NAME,
                )?),
                std::mem::transmute::<*mut c_void, NvGetDisplayPortInfo>(get(
                    NVAPI_GET_DISPLAY_PORT_INFO,
                )?),
            )
        };
        // SAFETY: sans argument.
        if unsafe { initialize() } != 0 {
            return None;
        }
        Some(Nvidia {
            _lib: lib,
            display_id,
            port_info,
        })
    }

    /// `gdi_name` : nom GDI de la source ("\\.\DISPLAY2").
    pub fn dp_link(&self, gdi_name: &str) -> Option<DpLink> {
        let name = CString::new(gdi_name).ok()?;
        let mut id = 0u32;
        // SAFETY: nom terminé par un zéro.
        if unsafe { (self.display_id)(name.as_ptr() as *const u8, &mut id) } != 0 {
            return None;
        }
        let mut info = NvDisplayPortInfo {
            version: size_of::<NvDisplayPortInfo>() as u32 | (1 << 16),
            ..Default::default()
        };
        // SAFETY: structure V1 versionnée.
        if unsafe { (self.port_info)(null_mut(), id, &mut info) } != 0 || info.flags & 1 == 0 {
            return None; // erreur, ou écran non piloté en DisplayPort (isDp)
        }
        let rate = |code: u32| u8::try_from(code).ok().and_then(dpcd_rate_mbps);
        Some(DpLink {
            lane_rate_mbps: rate(info.cur_link_rate)?,
            lanes: u8::try_from(info.cur_lane_count).ok().filter(|l| *l > 0)?,
            max_lane_rate_mbps: rate(info.max_link_rate),
            max_lanes: u8::try_from(info.max_lane_count).ok().filter(|l| *l > 0),
            source: "NVIDIA NVAPI".to_owned(),
        })
    }
}
