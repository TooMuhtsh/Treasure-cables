//! Collecte sous Windows, sans droits administrateur.
//!
//! Même démarche que l'exemple USBView de Microsoft : on ouvre chaque
//! contrôleur hôte, son hub racine, puis chaque hub, et on interroge chaque
//! port avec les IOCTL des hubs (toutes en `FILE_ANY_ACCESS`) :
//!
//! - `IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX` : descripteur d'appareil
//!   (en cache), vitesse USB 1/2, état de la connexion ;
//! - `..._EX_V2` : lien SuperSpeed(Plus) actif, et capacités de l'appareil ;
//! - `..._SUPERSPEEDPLUS_INFORMATION` : débit par voie et nombre de voies ;
//! - `IOCTL_USB_GET_PORT_CONNECTOR_PROPERTIES` : port USB-C, port compagnon ;
//! - `IOCTL_USB_GET_DESCRIPTOR_FROM_NODE_CONNECTION` : BOS (capacités,
//!   Billboard), configuration, chaînes — sans changer le pilote de l'appareil.
//!
//! Les propriétés PnP (cfgmgr32) complètent : nom signalé par le bus,
//! code de problème, état d'alimentation.
//!
//! Voir docs/recherche-sources-windows.md.

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::mem::{offset_of, size_of};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CM_GETIDLIST_FILTER_ENUMERATOR,
    CM_GETIDLIST_FILTER_PRESENT, CM_Get_DevNode_PropertyW, CM_Get_DevNode_Status,
    CM_Get_Device_ID_List_SizeW, CM_Get_Device_ID_ListW, CM_Get_Device_Interface_List_SizeW,
    CM_Get_Device_Interface_ListW, CM_Get_Device_Interface_PropertyW, CM_LOCATE_DEVNODE_NORMAL,
    CM_Locate_DevNodeW, CR_SUCCESS, DN_HAS_PROBLEM,
};
use windows_sys::Win32::Devices::Properties::{
    DEVPKEY_Device_Address, DEVPKEY_Device_BusNumber, DEVPKEY_Device_BusReportedDeviceDesc,
    DEVPKEY_Device_CompatibleIds, DEVPKEY_Device_DeviceDesc, DEVPKEY_Device_Driver,
    DEVPKEY_Device_FriendlyName, DEVPKEY_Device_InstanceId, DEVPKEY_Device_PowerData,
    DEVPKEY_Device_Service,
};
use windows_sys::Win32::Devices::Usb::{
    GUID_DEVINTERFACE_USB_HOST_CONTROLLER, IOCTL_USB_GET_DESCRIPTOR_FROM_NODE_CONNECTION,
    IOCTL_USB_GET_HUB_INFORMATION_EX, IOCTL_USB_GET_NODE_CONNECTION_DRIVERKEY_NAME,
    IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX, IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX_V2,
    IOCTL_USB_GET_NODE_CONNECTION_NAME, IOCTL_USB_GET_NODE_INFORMATION,
    IOCTL_USB_GET_PORT_CONNECTOR_PROPERTIES, IOCTL_USB_GET_ROOT_HUB_NAME, USB_DESCRIPTOR_REQUEST,
    USB_HUB_INFORMATION_EX, USB_NODE_CONNECTION_INFORMATION_EX,
    USB_NODE_CONNECTION_INFORMATION_EX_V2, USB_NODE_INFORMATION, USB_PIPE_INFO,
    USB_PORT_CONNECTOR_PROPERTIES,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, DEVPROPKEY, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};

use treasure_model::{
    Bcd, Bus, ConnectionProblem, Connector, Device, Lanes, PortInfo, Report, Source, Sourced,
    Speed, Usb4Router,
};

use crate::deep::child_location;
use crate::{Error, bos, empty_report};

/// Absent de windows-sys : `CTL_CODE(FILE_DEVICE_USB, 289, METHOD_BUFFERED, FILE_ANY_ACCESS)`.
const IOCTL_USB_GET_NODE_CONNECTION_SUPERSPEEDPLUS_INFORMATION: u32 = 0x0022_0484;

// USB_PROTOCOLS
const USB110: u32 = 0x01;
const USB200: u32 = 0x02;
const USB300: u32 = 0x04;

// USB_NODE_CONNECTION_INFORMATION_EX_V2_FLAGS
const OPERATING_AT_SUPERSPEED: u32 = 0x01;
const SUPERSPEED_CAPABLE: u32 = 0x02;
const OPERATING_AT_SUPERSPEED_PLUS: u32 = 0x04;
const SUPERSPEED_PLUS_CAPABLE: u32 = 0x08;

// USB_PORT_PROPERTIES
const PORT_IS_USER_CONNECTABLE: u32 = 0x01;
const PORT_CONNECTOR_IS_TYPE_C: u32 = 0x08;

// USB_CONNECTION_STATUS
const NO_DEVICE_CONNECTED: i32 = 0;
const DEVICE_CONNECTED: i32 = 1;

/// Code 43 du Gestionnaire de périphériques (CM_PROB_FAILED_POST_START).
const CM_PROB_FAILED_POST_START: u32 = 43;
/// `DEVICE_POWER_STATE::PowerDeviceD0`.
const POWER_DEVICE_D0: i32 = 1;

const DESCRIPTOR_DEVICE: u8 = 0x01;
const DESCRIPTOR_CONFIGURATION: u8 = 0x02;
const DESCRIPTOR_STRING: u8 = 0x03;
const DESCRIPTOR_BOS: u8 = 0x0f;

// Disposition des structures (#pragma pack(1) dans usbioctl.h) : on lit les
// tampons octet par octet, ces assertions garantissent les offsets utilisés.
const EX_SPEED: usize = 23;
const EX_IS_HUB: usize = 24;
const EX_STATUS: usize = 31;
const _: () = {
    assert!(offset_of!(USB_NODE_CONNECTION_INFORMATION_EX, Speed) == EX_SPEED);
    assert!(offset_of!(USB_NODE_CONNECTION_INFORMATION_EX, DeviceIsHub) == EX_IS_HUB);
    assert!(offset_of!(USB_NODE_CONNECTION_INFORMATION_EX, ConnectionStatus) == EX_STATUS);
    assert!(size_of::<USB_NODE_CONNECTION_INFORMATION_EX_V2>() == 16);
    assert!(offset_of!(USB_PORT_CONNECTOR_PROPERTIES, CompanionHubSymbolicLinkName) == 16);
    assert!(offset_of!(USB_DESCRIPTOR_REQUEST, Data) == 12);
};

/// `light` : pas de lecture de descripteurs ni d'écrans (surveillance répétée).
pub fn collect(light: bool) -> Result<Report, Error> {
    let mut report = empty_report(os_version());
    let devnodes = DevnodeIndex::load();

    let controllers = interface_list(&GUID_DEVINTERFACE_USB_HOST_CONTROLLER);
    if controllers.is_empty() {
        report.notes.push("Aucun contrôleur USB trouvé.".to_owned());
    }
    let mut roots = Vec::new();
    for (i, path) in controllers.iter().enumerate() {
        let bus = (i + 1).to_string();
        let controller =
            interface_devnode(path).and_then(|d| devnode_string(d, &DEVPKEY_Device_DeviceDesc));
        let Some(hc) = Handle::open(path) else {
            report.notes.push(format!(
                "Contrôleur {} inaccessible.",
                controller.as_deref().unwrap_or(path)
            ));
            continue;
        };
        let Some(root_name) = root_hub_name(&hc) else {
            report.notes.push(format!(
                "Hub racine introuvable pour {}.",
                controller.as_deref().unwrap_or(path)
            ));
            continue;
        };
        roots.push((bus, controller, device_path(&root_name)));
    }

    let mut walker = Walker {
        devnodes: &devnodes,
        notes: &mut report.notes,
        hub_locations: roots
            .iter()
            .map(|(bus, _, path)| (path.to_lowercase(), bus.clone()))
            .collect(),
        recorded_ports: HashSet::new(),
        read_descriptors: !light,
    };
    let mut buses = Vec::new();
    for (bus, controller, root_path) in roots {
        let (ports, children) = walker.hub_ports(&root_path, &bus);
        buses.push(Bus {
            id: format!("usb{bus}"),
            controller,
            root_hub: Device {
                location: format!("usb{bus}"),
                product: Some("Hub racine".to_owned()),
                is_hub: true,
                ports,
                children,
                ..Default::default()
            },
        });
    }
    report.buses = buses;
    if !light {
        report.displays = crate::windows_display::displays();
        report.usb4_routers = usb4_routers();
    }
    report.notes.dedup();
    Ok(report)
}

struct Walker<'a> {
    devnodes: &'a DevnodeIndex,
    notes: &'a mut Vec<String>,
    /// Chemin de hub (minuscules) -> emplacement ("2", "2-3"...).
    hub_locations: HashMap<String, String>,
    /// Ports déjà comptés, pour ne compter qu'une fois un connecteur dont
    /// les moitiés USB 2 et USB 3 sont sur deux ports (voire deux contrôleurs).
    recorded_ports: HashSet<String>,
    /// Faux en collecte légère : on évite tout échange avec les appareils.
    read_descriptors: bool,
}

impl Walker<'_> {
    /// Interroge chaque port d'un hub et renvoie les appareils branchés.
    fn hub_ports(&mut self, hub_path: &str, location: &str) -> (Vec<PortInfo>, Vec<Device>) {
        let Some(hub) = Handle::open(hub_path) else {
            self.notes.push(format!("Hub inaccessible : {hub_path}"));
            return (Vec::new(), Vec::new());
        };
        self.hub_locations
            .insert(hub_path.to_lowercase(), location.to_owned());
        let count = port_count(&hub);
        let mut ports = Vec::new();
        let mut devices = Vec::new();
        for port in 1..=count {
            let here = child_location(location, port);
            let connector = port_connector(&hub, port);
            // Un connecteur USB 3 apparaît comme deux ports (USB 2 et USB 3) :
            // on ne compte la paire qu'une fois, sur le premier rencontré.
            let duplicate = connector
                .as_ref()
                .and_then(|c| self.companion_location(c))
                .is_some_and(|c| self.recorded_ports.contains(&c));
            let device = self.port_device(&hub, port, &here, connector.as_ref());
            let info = match &device {
                Some(d) => d.port.clone(),
                None => Some(self.port_info(
                    port,
                    &here,
                    connection_info_v2(&hub, port),
                    connector.as_ref(),
                    None,
                )),
            };
            if let Some(info) = info
                && !duplicate
            {
                ports.push(info);
                self.recorded_ports.insert(here);
            }
            devices.extend(device);
        }
        (ports, devices)
    }

    fn port_device(
        &mut self,
        hub: &Handle,
        port: u32,
        location: &str,
        connector: Option<&ConnectorInfo>,
    ) -> Option<Device> {
        let ex = connection_info_ex(hub, port)?;
        let status = i32::from_le_bytes(ex[EX_STATUS..EX_STATUS + 4].try_into().ok()?);
        if status == NO_DEVICE_CONNECTED {
            return None;
        }
        let descriptor = &ex[4..22];
        let descriptor_valid = descriptor[1] == DESCRIPTOR_DEVICE;
        let usb_version =
            descriptor_valid.then(|| Bcd(u16::from_le_bytes([descriptor[2], descriptor[3]])));
        let is_hub = ex[EX_IS_HUB] != 0;

        let v2 = connection_info_v2(hub, port);
        let flags = v2.map(|v| v.flags).unwrap_or(0);

        // Vitesse réelle : le champ Speed ne dépasse jamais High Speed,
        // les drapeaux V2 disent si le lien est SuperSpeed(Plus).
        let mut lanes = None;
        let speed = if flags & OPERATING_AT_SUPERSPEED_PLUS != 0 {
            let ssp = superspeed_plus_info(hub, port);
            lanes = ssp.map(|s| Lanes {
                rx: s.rx_lanes,
                tx: s.tx_lanes,
            });
            Some(ssp.and_then(|s| s.speed()).unwrap_or(Speed::Gen2x1))
        } else if flags & OPERATING_AT_SUPERSPEED != 0 {
            Some(Speed::Gen1x1)
        } else {
            match ex[EX_SPEED] {
                0 => Some(Speed::Low),
                1 => Some(Speed::Full),
                2 => Some(Speed::High),
                3 => Some(Speed::Gen1x1),
                _ => None,
            }
        }
        .filter(|_| status == DEVICE_CONNECTED)
        .map(Sourced::negotiated);

        // Capacités annoncées par l'appareil.
        let mut max_speed = if flags & SUPERSPEED_PLUS_CAPABLE != 0 {
            Some(Sourced::declared(Speed::Gen2x1))
        } else if flags & SUPERSPEED_CAPABLE != 0 {
            Some(Sourced::declared(Speed::Gen1x1))
        } else {
            None
        };

        let devnode = driver_key_name(hub, port).and_then(|k| self.devnodes.get(&k));
        let powered = devnode.is_none_or(|d| d.powered);

        let mut device = Device {
            location: location.to_owned(),
            vendor_id: descriptor_valid.then(|| u16::from_le_bytes([descriptor[8], descriptor[9]])),
            product_id: descriptor_valid
                .then(|| u16::from_le_bytes([descriptor[10], descriptor[11]])),
            product: devnode.and_then(|d| d.description.clone()),
            serial: devnode.and_then(|d| d.serial.clone()),
            device_class: descriptor_valid.then_some(descriptor[4]),
            usb_version,
            lanes,
            is_hub,
            problem: connection_problem(status).or_else(|| devnode.and_then(|d| d.problem)),
            ..Default::default()
        };

        // Lectures de descripteurs : de vrais échanges USB, uniquement pour
        // un appareil connecté et réveillé (comme USBView).
        if self.read_descriptors && status == DEVICE_CONNECTED && descriptor_valid && powered {
            if let Some(config) = get_descriptor(hub, port, DESCRIPTOR_CONFIGURATION, 0, 0, 9)
                && config.len() >= 9
            {
                let unit = if speed.is_some_and(|s| s.value.is_superspeed()) {
                    8
                } else {
                    2
                };
                device.max_power_ma = Some(u32::from(config[8]) * unit);
            }
            if usb_version.is_some_and(|v| v.0 > 0x0200)
                && let Some(info) = read_bos(hub, port)
            {
                if let Some(bos_max) = info.max_speed
                    && max_speed.is_none_or(|m| m.value < bos_max)
                {
                    max_speed = Some(Sourced::declared(bos_max));
                }
                device.alt_modes = info.alt_modes;
            }
            let lang = string_language(hub, port);
            if let Some(lang) = lang {
                device.manufacturer = get_string(hub, port, descriptor[14], lang);
                if device.product.is_none() {
                    device.product = get_string(hub, port, descriptor[15], lang);
                }
            }
        }

        // La vitesse actuelle est une borne basse de la capacité réelle.
        device.max_speed = match (max_speed, speed) {
            (Some(m), Some(s)) if s.value > m.value => Some(Sourced {
                value: s.value,
                source: Source::Negotiated,
            }),
            (None, Some(s)) if s.value.is_superspeed() => Some(Sourced {
                value: s.value,
                source: Source::Negotiated,
            }),
            (m, _) => m,
        };
        device.speed = speed;
        device.port = Some(self.port_info(port, location, v2, connector, speed));

        if is_hub && let Some(name) = connection_name(hub, port) {
            (device.ports, device.children) = self.hub_ports(&device_path(&name), location);
        }
        Some(device)
    }

    /// Emplacement du port compagnon ("1-2"), si son hub est connu.
    fn companion_location(&self, connector: &ConnectorInfo) -> Option<String> {
        if connector.companion_port == 0 {
            return None;
        }
        let hub = connector.companion_hub.as_deref()?.to_lowercase();
        let location = self.hub_locations.get(&hub)?;
        Some(child_location(
            location,
            u32::from(connector.companion_port),
        ))
    }

    fn port_info(
        &mut self,
        port: u32,
        location: &str,
        v2: Option<V2Info>,
        connector: Option<&ConnectorInfo>,
        speed: Option<Sourced<Speed>>,
    ) -> PortInfo {
        // Capacité du port : ses protocoles, ceux de son compagnon (l'autre
        // moitié USB 2 / USB 3 du même connecteur), et la vitesse obtenue.
        let mut protocols = v2.map(|v| v.protocols).unwrap_or(0);
        let mut companion = None;
        if let Some(c) = connector
            && c.companion_port != 0
        {
            companion = self
                .companion_location(c)
                .or_else(|| Some(format!("port {}", c.companion_port)));
            if let Some(companion_hub) = c.companion_hub.as_deref().and_then(Handle::open)
                && let Some(cv2) = connection_info_v2(&companion_hub, u32::from(c.companion_port))
            {
                protocols |= cv2.protocols;
            }
        }
        let declared = if protocols & USB300 != 0 {
            // Au moins 5 Gb/s ; le SuperSpeedPlus du port n'est pas exposé.
            Some(Sourced::inferred(Speed::Gen1x1))
        } else if protocols & USB200 != 0 {
            Some(Sourced::declared(Speed::High))
        } else if protocols & USB110 != 0 {
            Some(Sourced::declared(Speed::Full))
        } else {
            None
        };
        let max_speed = match (declared, speed) {
            (Some(d), Some(s)) if s.value > d.value => Some(s),
            (d, _) => d,
        };
        let connector_kind = connector.map(|c| {
            if c.properties & PORT_CONNECTOR_IS_TYPE_C != 0 {
                Connector::TypeC
            } else if c.properties & PORT_IS_USER_CONNECTABLE == 0 {
                Connector::Internal
            } else {
                Connector::Unknown
            }
        });
        PortInfo {
            location: Some(location.to_owned()),
            number: u8::try_from(port).ok(),
            connector: connector_kind,
            max_speed,
            companion,
        }
    }
}

fn connection_problem(status: i32) -> Option<ConnectionProblem> {
    Some(match status {
        2 => ConnectionProblem::EnumerationFailed,
        3 => ConnectionProblem::GeneralFailure,
        4 => ConnectionProblem::Overcurrent,
        5 => ConnectionProblem::NotEnoughPower,
        6 => ConnectionProblem::NotEnoughBandwidth,
        7 => ConnectionProblem::HubNestedTooDeeply,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// IOCTL des hubs

struct Handle(HANDLE);

impl Handle {
    fn open(path: &str) -> Option<Handle> {
        let wide = to_wide(path);
        // SAFETY: chemin terminé par un zéro, autres paramètres constants.
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_WRITE,
                FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                0,
                null_mut(),
            )
        };
        (handle != INVALID_HANDLE_VALUE && !handle.is_null()).then_some(Handle(handle))
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: poignée ouverte par CreateFileW et fermée une seule fois.
        unsafe { CloseHandle(self.0) };
    }
}

/// DeviceIoControl avec le même tampon en entrée et en sortie (convention
/// des IOCTL USB). Renvoie le tampon tronqué à la taille écrite.
fn ioctl(handle: &Handle, code: u32, mut buffer: Vec<u8>) -> Option<Vec<u8>> {
    let mut returned = 0u32;
    let len = u32::try_from(buffer.len()).ok()?;
    // SAFETY: le tampon vit pendant l'appel ; tailles exactes passées.
    let ok = unsafe {
        DeviceIoControl(
            handle.0,
            code,
            buffer.as_ptr() as *const c_void,
            len,
            buffer.as_mut_ptr() as *mut c_void,
            len,
            &mut returned,
            null_mut(),
        )
    };
    if ok == 0 {
        return None;
    }
    buffer.truncate(returned as usize);
    Some(buffer)
}

/// IOCTL renvoyant un nom de taille variable : un premier appel donne
/// `ActualLength` (à `length_offset`), le second lit le nom (à `length_offset + 4`).
fn ioctl_name(handle: &Handle, code: u32, header: &[u8], length_offset: usize) -> Option<String> {
    let mut first = header.to_vec();
    first.resize(length_offset + 4 + 2, 0);
    let first = ioctl(handle, code, first)?;
    let actual = u32::from_le_bytes(
        first
            .get(length_offset..length_offset + 4)?
            .try_into()
            .ok()?,
    );
    let actual = (actual as usize).max(length_offset + 6);
    let mut second = header.to_vec();
    second.resize(actual, 0);
    let second = ioctl(handle, code, second)?;
    Some(from_wide_bytes(second.get(length_offset + 4..)?))
}

fn root_hub_name(controller: &Handle) -> Option<String> {
    ioctl_name(controller, IOCTL_USB_GET_ROOT_HUB_NAME, &[], 0)
}

fn connection_name(hub: &Handle, port: u32) -> Option<String> {
    ioctl_name(
        hub,
        IOCTL_USB_GET_NODE_CONNECTION_NAME,
        &port.to_le_bytes(),
        4,
    )
}

fn driver_key_name(hub: &Handle, port: u32) -> Option<String> {
    ioctl_name(
        hub,
        IOCTL_USB_GET_NODE_CONNECTION_DRIVERKEY_NAME,
        &port.to_le_bytes(),
        4,
    )
}

fn port_count(hub: &Handle) -> u32 {
    // Hub USB 3 racine : HighestPortNumber couvre les ports USB 2 et USB 3.
    let ex = ioctl(
        hub,
        IOCTL_USB_GET_HUB_INFORMATION_EX,
        vec![0; size_of::<USB_HUB_INFORMATION_EX>()],
    )
    .filter(|b| b.len() >= 6)
    .map(|b| u32::from(u16::from_le_bytes([b[4], b[5]])));
    // USB_NODE_INFORMATION : NodeType (4 octets) puis le descripteur de hub,
    // dont bNumberOfPorts est le 3e octet.
    let legacy = ioctl(
        hub,
        IOCTL_USB_GET_NODE_INFORMATION,
        vec![0; size_of::<USB_NODE_INFORMATION>()],
    )
    .filter(|b| b.len() >= 7)
    .map(|b| u32::from(b[6]));
    ex.into_iter().chain(legacy).max().unwrap_or(0)
}

fn connection_info_ex(hub: &Handle, port: u32) -> Option<Vec<u8>> {
    let mut buffer = vec![
        0u8;
        size_of::<USB_NODE_CONNECTION_INFORMATION_EX>()
            + 30 * size_of::<USB_PIPE_INFO>()
    ];
    buffer[..4].copy_from_slice(&port.to_le_bytes());
    ioctl(hub, IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX, buffer)
        .filter(|b| b.len() > EX_STATUS + 4)
}

#[derive(Clone, Copy)]
struct V2Info {
    protocols: u32,
    flags: u32,
}

fn connection_info_v2(hub: &Handle, port: u32) -> Option<V2Info> {
    let mut buffer = vec![0u8; 16];
    buffer[0..4].copy_from_slice(&port.to_le_bytes());
    buffer[4..8].copy_from_slice(&16u32.to_le_bytes());
    // L'appelant doit annoncer les protocoles qu'il comprend.
    buffer[8..12].copy_from_slice(&(USB110 | USB200 | USB300).to_le_bytes());
    let b = ioctl(hub, IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX_V2, buffer)?;
    Some(V2Info {
        protocols: u32::from_le_bytes(b.get(8..12)?.try_into().ok()?),
        flags: u32::from_le_bytes(b.get(12..16)?.try_into().ok()?),
    })
}

#[derive(Clone, Copy)]
struct SuperSpeedPlusInfo {
    rx_speed: u32,
    rx_lanes: u8,
    tx_lanes: u8,
}

impl SuperSpeedPlusInfo {
    fn speed(&self) -> Option<Speed> {
        let exponent = (self.rx_speed >> 4) & 0b11;
        let mantissa = u64::from(self.rx_speed >> 16);
        let per_lane = mantissa * 1000u64.pow(exponent);
        let two_lanes = self.rx_lanes >= 2;
        Some(match per_lane {
            b if b >= 10_000_000_000 && two_lanes => Speed::Gen2x2,
            b if b >= 10_000_000_000 => Speed::Gen2x1,
            b if b >= 5_000_000_000 && two_lanes => Speed::Gen1x2,
            b if b >= 5_000_000_000 => Speed::Gen1x1,
            _ => return None,
        })
    }
}

fn superspeed_plus_info(hub: &Handle, port: u32) -> Option<SuperSpeedPlusInfo> {
    let mut buffer = vec![0u8; 24];
    buffer[0..4].copy_from_slice(&port.to_le_bytes());
    buffer[4..8].copy_from_slice(&24u32.to_le_bytes());
    let b = ioctl(
        hub,
        IOCTL_USB_GET_NODE_CONNECTION_SUPERSPEEDPLUS_INFORMATION,
        buffer,
    )?;
    let word = |o: usize| {
        b.get(o..o + 4)
            .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
    };
    // Les nombres de voies sont comptés à partir de zéro.
    Some(SuperSpeedPlusInfo {
        rx_speed: word(8)?,
        rx_lanes: u8::try_from(word(12)? + 1).ok()?,
        tx_lanes: u8::try_from(word(20)? + 1).ok()?,
    })
}

struct ConnectorInfo {
    properties: u32,
    companion_port: u16,
    companion_hub: Option<String>,
}

fn port_connector(hub: &Handle, port: u32) -> Option<ConnectorInfo> {
    let header = port.to_le_bytes();
    let mut first = header.to_vec();
    first.resize(size_of::<USB_PORT_CONNECTOR_PROPERTIES>(), 0);
    let first = ioctl(hub, IOCTL_USB_GET_PORT_CONNECTOR_PROPERTIES, first)?;
    let actual = u32::from_le_bytes(first.get(4..8)?.try_into().ok()?) as usize;
    let mut second = header.to_vec();
    second.resize(actual.max(first.len()), 0);
    let b = ioctl(hub, IOCTL_USB_GET_PORT_CONNECTOR_PROPERTIES, second)?;
    let name = b.get(16..).map(from_wide_bytes).filter(|n| !n.is_empty());
    Some(ConnectorInfo {
        properties: u32::from_le_bytes(b.get(8..12)?.try_into().ok()?),
        companion_port: u16::from_le_bytes(b.get(14..16)?.try_into().ok()?),
        companion_hub: name.map(|n| device_path(&n)),
    })
}

/// Requête GET_DESCRIPTOR standard transmise par le hub (aucun pilote requis).
fn get_descriptor(
    hub: &Handle,
    port: u32,
    kind: u8,
    index: u8,
    language: u16,
    length: u16,
) -> Option<Vec<u8>> {
    let mut buffer = vec![0u8; 12 + usize::from(length)];
    buffer[0..4].copy_from_slice(&port.to_le_bytes());
    buffer[4] = 0x80; // bmRequest : appareil vers hôte
    buffer[5] = 0x06; // GET_DESCRIPTOR
    buffer[6..8].copy_from_slice(&(u16::from(kind) << 8 | u16::from(index)).to_le_bytes());
    buffer[8..10].copy_from_slice(&language.to_le_bytes());
    buffer[10..12].copy_from_slice(&length.to_le_bytes());
    let b = ioctl(hub, IOCTL_USB_GET_DESCRIPTOR_FROM_NODE_CONNECTION, buffer)?;
    let data = b.get(12..)?.to_vec();
    (!data.is_empty()).then_some(data)
}

fn read_bos(hub: &Handle, port: u32) -> Option<bos::BosInfo> {
    let header = get_descriptor(hub, port, DESCRIPTOR_BOS, 0, 0, 5)?;
    let total = u16::from_le_bytes([*header.get(2)?, *header.get(3)?]).clamp(5, 4095);
    let full = get_descriptor(hub, port, DESCRIPTOR_BOS, 0, 0, total)?;
    bos::parse(&full)
}

fn string_language(hub: &Handle, port: u32) -> Option<u16> {
    let langs = get_descriptor(hub, port, DESCRIPTOR_STRING, 0, 0, 255)?;
    (langs.len() >= 4).then(|| u16::from_le_bytes([langs[2], langs[3]]))
}

fn get_string(hub: &Handle, port: u32, index: u8, language: u16) -> Option<String> {
    if index == 0 {
        return None;
    }
    let data = get_descriptor(hub, port, DESCRIPTOR_STRING, index, language, 255)?;
    let len = usize::from(*data.first()?).min(data.len());
    let text = from_wide_bytes(data.get(2..len)?);
    let text = text.trim().to_owned();
    (!text.is_empty()).then_some(text)
}

// ---------------------------------------------------------------------------
// Configuration Manager (cfgmgr32)

struct DevnodeInfo {
    description: Option<String>,
    serial: Option<String>,
    problem: Option<ConnectionProblem>,
    powered: bool,
}

/// Nœuds PnP du bus USB, indexés par clé de pilote (ce que renvoie
/// IOCTL_USB_GET_NODE_CONNECTION_DRIVERKEY_NAME).
struct DevnodeIndex(HashMap<String, DevnodeInfo>);

impl DevnodeIndex {
    fn load() -> Self {
        let mut map = HashMap::new();
        for id in device_id_list(Some("USB")) {
            let Some(devinst) = locate_devnode(&id) else {
                continue;
            };
            let Some(driver) = devnode_string(devinst, &DEVPKEY_Device_Driver) else {
                continue;
            };
            let description = devnode_string(devinst, &DEVPKEY_Device_BusReportedDeviceDesc)
                .or_else(|| devnode_string(devinst, &DEVPKEY_Device_DeviceDesc));
            // L'identifiant d'instance se termine par le numéro de série quand
            // l'appareil en a un (sinon Windows y met un chemin avec des "&").
            let serial = id
                .rsplit('\\')
                .next()
                .filter(|s| !s.contains('&'))
                .map(str::to_owned);
            let problem = devnode_problem(devinst)
                .filter(|p| *p == CM_PROB_FAILED_POST_START)
                .map(|_| ConnectionProblem::GeneralFailure);
            let powered = devnode_property(devinst, &DEVPKEY_Device_PowerData)
                .and_then(|b| Some(i32::from_le_bytes(b.get(4..8)?.try_into().ok()?)))
                .is_none_or(|state| state == POWER_DEVICE_D0);
            map.insert(
                driver.to_lowercase(),
                DevnodeInfo {
                    description,
                    serial,
                    problem,
                    powered,
                },
            );
        }
        DevnodeIndex(map)
    }

    fn get(&self, driver_key: &str) -> Option<&DevnodeInfo> {
        self.0.get(&driver_key.to_lowercase())
    }
}

fn interface_list(guid: &windows_sys::core::GUID) -> Vec<String> {
    let mut len = 0u32;
    // SAFETY: pointeurs valides ; la taille est relue avant l'allocation.
    if unsafe {
        CM_Get_Device_Interface_List_SizeW(
            &mut len,
            guid,
            null(),
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
        )
    } != CR_SUCCESS
    {
        return Vec::new();
    }
    let mut buffer = vec![0u16; len as usize];
    // SAFETY: tampon de `len` caractères.
    if unsafe {
        CM_Get_Device_Interface_ListW(
            guid,
            null(),
            buffer.as_mut_ptr(),
            len,
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
        )
    } != CR_SUCCESS
    {
        return Vec::new();
    }
    split_multi_sz(&buffer)
}

/// Routeurs du gestionnaire de connexion USB4 de Microsoft (TB4/TB5, USB4).
/// Les anciens contrôleurs Thunderbolt gérés par le firmware Intel n'y
/// figurent pas.
fn usb4_routers() -> Vec<Usb4Router> {
    let mut routers = Vec::new();
    for id in device_id_list(None) {
        let Some(devinst) = locate_devnode(&id) else {
            continue;
        };
        let Some(service) = devnode_string(devinst, &DEVPKEY_Device_Service) else {
            continue;
        };
        let host = match service.to_ascii_lowercase().as_str() {
            "usb4hostrouter" => true,
            "usb4devicerouter" => false,
            _ => continue,
        };
        let name = devnode_string(devinst, &DEVPKEY_Device_BusReportedDeviceDesc)
            .or_else(|| devnode_string(devinst, &DEVPKEY_Device_FriendlyName))
            .or_else(|| devnode_string(devinst, &DEVPKEY_Device_DeviceDesc))
            .unwrap_or(id);
        routers.push(Usb4Router { name, host });
    }
    routers.sort_by_key(|r| !r.host);
    routers
}

fn device_id_list(enumerator: Option<&str>) -> Vec<String> {
    let filter = enumerator.map(to_wide);
    let filter_ptr = filter.as_ref().map_or(null(), |f| f.as_ptr());
    let flags = if filter.is_some() {
        CM_GETIDLIST_FILTER_ENUMERATOR | CM_GETIDLIST_FILTER_PRESENT
    } else {
        CM_GETIDLIST_FILTER_PRESENT
    };
    let mut len = 0u32;
    // SAFETY: filtre terminé par un zéro.
    if unsafe { CM_Get_Device_ID_List_SizeW(&mut len, filter_ptr, flags) } != CR_SUCCESS {
        return Vec::new();
    }
    let mut buffer = vec![0u16; len as usize];
    // SAFETY: tampon de `len` caractères.
    if unsafe { CM_Get_Device_ID_ListW(filter_ptr, buffer.as_mut_ptr(), len, flags) } != CR_SUCCESS
    {
        return Vec::new();
    }
    split_multi_sz(&buffer)
}

fn locate_devnode(instance_id: &str) -> Option<u32> {
    let id = to_wide(instance_id);
    let mut devinst = 0u32;
    // SAFETY: identifiant terminé par un zéro.
    let r = unsafe { CM_Locate_DevNodeW(&mut devinst, id.as_ptr(), CM_LOCATE_DEVNODE_NORMAL) };
    (r == CR_SUCCESS).then_some(devinst)
}

/// Fonctions réseau des appareils USB (RNDIS, NCM, ECM) et l'état de leur
/// pilote : un partage de connexion sans pilote ne crée aucune carte réseau.
pub(crate) fn usb_network_functions() -> Vec<crate::network::UsbNetFunction> {
    device_id_list(Some("USB"))
        .iter()
        .filter_map(|id| {
            let devinst = locate_devnode(id)?;
            let bytes = devnode_property(devinst, &DEVPKEY_Device_CompatibleIds)?;
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect();
            let kind = crate::network::network_function_kind(&split_multi_sz(&units))?;
            Some(crate::network::UsbNetFunction {
                description: devnode_string(devinst, &DEVPKEY_Device_BusReportedDeviceDesc)
                    .or_else(|| devnode_string(devinst, &DEVPKEY_Device_DeviceDesc))
                    .unwrap_or_else(|| id.clone()),
                kind,
                problem: devnode_problem(devinst),
                driver: devnode_string(devinst, &DEVPKEY_Device_Service),
            })
        })
        .collect()
}

/// Contrôleurs USB par adresse PCI (bus, appareil, fonction), avec le même
/// numéro de bus que dans le rapport : sert à situer les événements ETW.
pub(crate) fn controller_buses() -> HashMap<(u32, u32, u32), String> {
    let u32_property = |devinst: u32, key: &DEVPROPKEY| {
        let b = devnode_property(devinst, key)?;
        Some(u32::from_le_bytes(b.get(..4)?.try_into().ok()?))
    };
    interface_list(&GUID_DEVINTERFACE_USB_HOST_CONTROLLER)
        .iter()
        .enumerate()
        .filter_map(|(i, path)| {
            let devinst = interface_devnode(path)?;
            let bus = u32_property(devinst, &DEVPKEY_Device_BusNumber)?;
            let address = u32_property(devinst, &DEVPKEY_Device_Address)?;
            Some(((bus, address >> 16, address & 0xffff), (i + 1).to_string()))
        })
        .collect()
}

/// Description PnP du périphérique qui expose une interface (nom d'une carte
/// graphique à partir de son chemin d'adaptateur, par exemple).
pub(crate) fn interface_description(path: &str) -> Option<String> {
    let devnode = interface_devnode(path)?;
    devnode_string(devnode, &DEVPKEY_Device_FriendlyName)
        .or_else(|| devnode_string(devnode, &DEVPKEY_Device_DeviceDesc))
}

fn interface_devnode(path: &str) -> Option<u32> {
    let wide = to_wide(path);
    let mut kind = 0u32;
    let mut size = 0u32;
    // SAFETY: premier appel pour connaître la taille.
    unsafe {
        CM_Get_Device_Interface_PropertyW(
            wide.as_ptr(),
            &DEVPKEY_Device_InstanceId,
            &mut kind,
            null_mut(),
            &mut size,
            0,
        )
    };
    let mut buffer = vec![0u8; size as usize];
    // SAFETY: tampon de `size` octets.
    let r = unsafe {
        CM_Get_Device_Interface_PropertyW(
            wide.as_ptr(),
            &DEVPKEY_Device_InstanceId,
            &mut kind,
            buffer.as_mut_ptr(),
            &mut size,
            0,
        )
    };
    if r != CR_SUCCESS {
        return None;
    }
    locate_devnode(&from_wide_bytes(&buffer))
}

fn devnode_property(devinst: u32, key: &DEVPROPKEY) -> Option<Vec<u8>> {
    let mut kind = 0u32;
    let mut size = 0u32;
    // SAFETY: premier appel pour connaître la taille.
    unsafe { CM_Get_DevNode_PropertyW(devinst, key, &mut kind, null_mut(), &mut size, 0) };
    if size == 0 {
        return None;
    }
    let mut buffer = vec![0u8; size as usize];
    // SAFETY: tampon de `size` octets.
    let r = unsafe {
        CM_Get_DevNode_PropertyW(devinst, key, &mut kind, buffer.as_mut_ptr(), &mut size, 0)
    };
    (r == CR_SUCCESS).then_some(buffer)
}

fn devnode_string(devinst: u32, key: &DEVPROPKEY) -> Option<String> {
    let text = from_wide_bytes(&devnode_property(devinst, key)?);
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn devnode_problem(devinst: u32) -> Option<u32> {
    let mut status = 0u32;
    let mut problem = 0u32;
    // SAFETY: pointeurs valides.
    let r = unsafe { CM_Get_DevNode_Status(&mut status, &mut problem, devinst, 0) };
    (r == CR_SUCCESS && status & DN_HAS_PROBLEM != 0).then_some(problem)
}

// ---------------------------------------------------------------------------
// Divers

fn os_version() -> Option<String> {
    let read = |name: &str| -> Option<String> {
        let key = to_wide(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
        let value = to_wide(name);
        let mut buffer = vec![0u16; 256];
        let mut size = (buffer.len() * 2) as u32;
        // SAFETY: tampon de `size` octets.
        let r = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                null_mut(),
                buffer.as_mut_ptr() as *mut c_void,
                &mut size,
            )
        };
        (r == 0).then(|| from_wide(&buffer))
    };
    let build: u32 = read("CurrentBuild")?.parse().ok()?;
    // ProductName indique toujours "Windows 10", même sous Windows 11.
    let name = if build >= 22000 {
        "Windows 11"
    } else {
        "Windows 10"
    };
    Some(match read("DisplayVersion") {
        Some(v) => format!("{name} {v} (build {build})"),
        None => format!("{name} (build {build})"),
    })
}

/// Chemin ouvrable par CreateFileW à partir d'un nom de hub ou d'un lien
/// symbolique (`\??\USB#...`, `\\?\USB#...` ou nom nu).
fn device_path(name: &str) -> String {
    let bare = name
        .strip_prefix(r"\??\")
        .or_else(|| name.strip_prefix(r"\\?\"))
        .or_else(|| name.strip_prefix(r"\\.\"))
        .unwrap_or(name);
    format!(r"\\.\{bare}")
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

fn from_wide_bytes(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    from_wide(&units)
}

fn split_multi_sz(buffer: &[u16]) -> Vec<String> {
    buffer
        .split(|&c| c == 0)
        .filter(|s| !s.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}
