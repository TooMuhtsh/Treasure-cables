//! Écrans actifs sous Windows (API DisplayConfig, sans droits).
//!
//! Windows indique le type de liaison (DisplayPort, DisplayPort tunnelisé en
//! USB4, HDMI...), le mode actuel et le mode natif annoncé par l'écran. Un
//! écran externe sous son mode natif est un indice de câble, d'adaptateur ou
//! de dock trop limité. Le débit du lien DisplayPort lui-même n'est exposé que
//! par les API des fabricants de GPU (non utilisées ici).

use std::mem::size_of;
use std::ptr::null_mut;

use windows_sys::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_PREFERRED_MODE,
    DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE,
    DISPLAYCONFIG_MODE_INFO_TYPE_TARGET, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_RATIONAL,
    DISPLAYCONFIG_TARGET_DEVICE_NAME, DISPLAYCONFIG_TARGET_PREFERRED_MODE,
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS,
    QueryDisplayConfig,
};

use treasure_model::{Display, DisplayConnection};

pub fn displays() -> Vec<Display> {
    let Some((paths, modes)) = query_active_paths() else {
        return Vec::new();
    };
    let source = |p: &DISPLAYCONFIG_PATH_INFO| {
        let a = p.sourceInfo.adapterId;
        (a.LowPart, a.HighPart, p.sourceInfo.id)
    };
    paths
        .iter()
        .map(|path| Display {
            mirrored: paths.iter().filter(|p| source(p) == source(path)).count() > 1,
            ..display(path, &modes)
        })
        .collect()
}

fn query_active_paths() -> Option<(Vec<DISPLAYCONFIG_PATH_INFO>, Vec<DISPLAYCONFIG_MODE_INFO>)> {
    // La configuration peut changer entre les deux appels : on réessaie.
    for _ in 0..3 {
        let (mut path_count, mut mode_count) = (0u32, 0u32);
        // SAFETY: pointeurs vers des compteurs locaux.
        if unsafe {
            GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
        } != 0
        {
            return None;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
        // SAFETY: tableaux dimensionnés d'après GetDisplayConfigBufferSizes.
        let r = unsafe {
            QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut path_count,
                paths.as_mut_ptr(),
                &mut mode_count,
                modes.as_mut_ptr(),
                null_mut(),
            )
        };
        if r == 0 {
            paths.truncate(path_count as usize);
            modes.truncate(mode_count as usize);
            return Some((paths, modes));
        }
    }
    None
}

fn display(path: &DISPLAYCONFIG_PATH_INFO, modes: &[DISPLAYCONFIG_MODE_INFO]) -> Display {
    let target = &path.targetInfo;
    let mut display = Display {
        connection: Some(connection(target.outputTechnology)),
        refresh_hz: hz(target.refreshRate),
        ..Default::default()
    };

    // Mode du signal envoyé à l'écran, sinon mode du bureau.
    // SAFETY: sans QDC_VIRTUAL_MODE_AWARE, modeInfoIdx est un simple index.
    let target_index = unsafe { target.Anonymous.modeInfoIdx } as usize;
    let source_index = unsafe { path.sourceInfo.Anonymous.modeInfoIdx } as usize;
    if let Some(mode) = modes.get(target_index)
        && mode.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_TARGET
    {
        // SAFETY: infoType indique le membre actif de l'union.
        let signal = unsafe { mode.Anonymous.targetMode.targetVideoSignalInfo };
        display.width = Some(signal.activeSize.cx);
        display.height = Some(signal.activeSize.cy);
        display.refresh_hz = hz(signal.vSyncFreq).or(display.refresh_hz);
    } else if let Some(mode) = modes.get(source_index)
        && mode.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE
    {
        // SAFETY: infoType indique le membre actif de l'union.
        let source = unsafe { mode.Anonymous.sourceMode };
        display.width = Some(source.width);
        display.height = Some(source.height);
    }

    let mut name = DISPLAYCONFIG_TARGET_DEVICE_NAME {
        header: header(
            DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
            size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>(),
            path,
        ),
        ..Default::default()
    };
    // SAFETY: l'en-tête décrit la taille exacte de la structure.
    if unsafe { DisplayConfigGetDeviceInfo(&mut name.header) } == 0 {
        let text = String::from_utf16_lossy(&name.monitorFriendlyDeviceName);
        let text = text.trim_end_matches('\0').trim();
        if !text.is_empty() {
            display.name = Some(text.to_owned());
        }
    }

    let mut preferred = DISPLAYCONFIG_TARGET_PREFERRED_MODE {
        header: header(
            DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_PREFERRED_MODE,
            size_of::<DISPLAYCONFIG_TARGET_PREFERRED_MODE>(),
            path,
        ),
        ..Default::default()
    };
    // SAFETY: l'en-tête décrit la taille exacte de la structure.
    if unsafe { DisplayConfigGetDeviceInfo(&mut preferred.header) } == 0 {
        display.native_width = Some(preferred.width).filter(|w| *w > 0);
        display.native_height = Some(preferred.height).filter(|h| *h > 0);
        display.native_refresh_hz = hz(preferred.targetMode.targetVideoSignalInfo.vSyncFreq);
    }
    display
}

fn header(
    kind: i32,
    size: usize,
    path: &DISPLAYCONFIG_PATH_INFO,
) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
    DISPLAYCONFIG_DEVICE_INFO_HEADER {
        r#type: kind,
        size: size as u32,
        adapterId: path.targetInfo.adapterId,
        id: path.targetInfo.id,
    }
}

fn hz(rate: DISPLAYCONFIG_RATIONAL) -> Option<f64> {
    (rate.Denominator != 0 && rate.Numerator != 0)
        .then(|| f64::from(rate.Numerator) / f64::from(rate.Denominator))
}

/// Valeurs de DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY.
fn connection(technology: i32) -> DisplayConnection {
    match technology {
        i32::MIN | 6 | 11 | 13 => DisplayConnection::Internal, // interne, LVDS, eDP, UDI intégré
        10 => DisplayConnection::DisplayPort,
        18 => DisplayConnection::DisplayPortUsbTunnel,
        5 => DisplayConnection::Hdmi,
        4 => DisplayConnection::Dvi,
        0 => DisplayConnection::Vga,
        15 => DisplayConnection::Wireless,
        _ => DisplayConnection::Other,
    }
}
