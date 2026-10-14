//! Collecte sous Linux, uniquement par lecture de sysfs (aucun droit requis).
//!
//! - `/sys/bus/usb/devices` : arbre USB, vitesses négociées, voies, ports.
//! - `bos_descriptors` (Linux 6.6+) : vitesses que l'appareil sait atteindre.
//! - `/sys/class/typec` : ports USB-C, partenaires et câbles à e-marker.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use treasure_model::typec::{self, Cable, CableVdo, Partner, TypeCPort};
use treasure_model::{
    Bcd, Bus, Connector, Device, Lanes, PortInfo, Report, Source, Sourced, Speed,
};

use crate::{Error, bos, empty_report};

pub fn collect_from(sys: &Path) -> Result<Report, Error> {
    let os_version = read(&sys.join("../proc/sys/kernel/osrelease"))
        .or_else(|| read(Path::new("/proc/sys/kernel/osrelease")));
    let mut report = empty_report(os_version.map(|v| format!("Linux {v}")));

    let usb_dir = sys.join("bus/usb/devices");
    let mut names: Vec<String> = match fs::read_dir(&usb_dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            // Les interfaces ("1-2:1.0") et les ports ("usb1-port2") ne sont pas des appareils.
            .filter(|n| !n.contains(':') && !n.contains("-port"))
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            report
                .notes
                .push("Aucun bus USB visible (/sys/bus/usb absent).".to_owned());
            Vec::new()
        }
        Err(e) => return Err(e.into()),
    };
    names.sort_by_key(|a| natural_key(a));

    let mut devices: BTreeMap<String, Device> = BTreeMap::new();
    let mut bos_missing = false;
    for name in &names {
        let dir = usb_dir.join(name);
        let device = read_device(&usb_dir, name, &dir, &mut bos_missing);
        devices.insert(name.clone(), device);
    }
    if bos_missing && !devices.is_empty() {
        report.notes.push(
            "Descripteurs BOS indisponibles (noyau antérieur à 6.6) : les capacités maximales des appareils sont déduites du bcdUSB, avec moins de certitude.".to_owned(),
        );
    }

    // Reconstruit l'arbre : on rattache les appareils les plus profonds d'abord.
    let mut order: Vec<String> = devices.keys().cloned().collect();
    order.sort_by_key(|n| std::cmp::Reverse(depth(n)));
    for name in order {
        let Some(parent) = parent_of(&name) else {
            continue;
        };
        if let Some(device) = devices.remove(&name) {
            match devices.get_mut(&parent) {
                Some(p) => p.children.push(device),
                None => {
                    devices.insert(name, device);
                }
            }
        }
    }
    for device in devices.values_mut() {
        sort_children(device);
    }

    for (name, root_hub) in devices {
        let controller = fs::canonicalize(usb_dir.join(&name)).ok().and_then(|p| {
            p.parent().map(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
        });
        report.buses.push(Bus {
            id: name,
            controller,
            root_hub,
        });
    }
    report.buses.sort_by_key(|a| natural_key(&a.id));

    report.typec_ports = read_typec(&sys.join("class/typec"));
    Ok(report)
}

fn read_device(usb_dir: &Path, name: &str, dir: &Path, bos_missing: &mut bool) -> Device {
    let lanes = match (
        read_num::<u8>(&dir.join("rx_lanes")),
        read_num::<u8>(&dir.join("tx_lanes")),
    ) {
        (Some(rx), Some(tx)) => Some(Lanes { rx, tx }),
        _ => None,
    };
    let speed = read(&dir.join("speed"))
        .and_then(|s| s.parse::<f64>().ok())
        .and_then(|mbps| Speed::from_mbps(mbps, lanes.map(|l| l.rx)))
        .map(Sourced::negotiated);
    let usb_version = read(&dir.join("version")).and_then(|v| parse_bcd_version(&v));
    let device_class = read_hex::<u8>(&dir.join("bDeviceClass"));
    let is_hub = device_class == Some(9);

    let bos = fs::read(dir.join("bos_descriptors"))
        .ok()
        .map(|d| bos::parse(&d));
    let alt_modes = bos
        .iter()
        .flatten()
        .flat_map(|b| b.alt_modes.clone())
        .collect();
    let max_speed = match bos {
        Some(info) => info.and_then(|b| b.max_speed).map(Sourced::declared),
        None => {
            *bos_missing = true;
            // Sans BOS, un bcdUSB >= 3.0 indique au moins un appareil SuperSpeed.
            // Un appareil USB 3 branché en USB 2 annonce souvent 2.10 : on ne
            // peut alors rien conclure.
            usb_version
                .filter(|v| v.0 >= 0x0300)
                .map(|_| Sourced::inferred(Speed::Gen1x1))
        }
    };
    // La vitesse actuelle est une borne basse de la capacité réelle.
    let max_speed = match (max_speed, speed) {
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

    Device {
        location: name.to_owned(),
        vendor_id: read_hex(&dir.join("idVendor")),
        product_id: read_hex(&dir.join("idProduct")),
        manufacturer: read(&dir.join("manufacturer")),
        product: read(&dir.join("product")),
        serial: read(&dir.join("serial")),
        device_class,
        usb_version,
        speed,
        max_speed,
        lanes,
        max_power_ma: read(&dir.join("bMaxPower"))
            .and_then(|p| p.trim_end_matches("mA").trim().parse().ok()),
        is_hub,
        port: read_port(usb_dir, dir),
        problem: None,
        alt_modes,
        children: Vec::new(),
    }
}

/// Port amont : lien `port` vers `usbX-portN`, avec son éventuel compagnon (`peer`)
/// sur l'autre bus du même contrôleur (USB 2 <-> USB 3).
fn read_port(usb_dir: &Path, dir: &Path) -> Option<PortInfo> {
    let port_dir = fs::canonicalize(dir.join("port")).ok()?;
    let port_name = port_dir.file_name()?.to_string_lossy().into_owned();
    let number = port_name.rsplit("port").next().and_then(|n| n.parse().ok());

    let connector = read(&port_dir.join("connect_type")).map(|t| match t.as_str() {
        "hardwired" => Connector::Internal,
        _ => Connector::Unknown,
    });
    let connector = if port_dir.join("connector").exists() {
        // Le port est relié à un connecteur USB-C (lien vers /sys/class/typec).
        Some(Connector::TypeC)
    } else {
        connector
    };

    let peer = fs::canonicalize(port_dir.join("peer")).ok();
    let companion = peer
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned());

    // Capacité du port : la vitesse du hub qui le porte, et celle du hub
    // qui porte son compagnon.
    let hub_speed = |port: &Path| -> Option<Speed> {
        let hub = port.parent()?.parent()?;
        let name = hub.file_name()?.to_string_lossy().into_owned();
        let hub_dir = usb_dir.join(&name);
        let hub_dir = if hub_dir.exists() {
            hub_dir
        } else {
            hub.to_path_buf()
        };
        let lanes = read_num::<u8>(&hub_dir.join("rx_lanes"));
        read(&hub_dir.join("speed"))?
            .parse::<f64>()
            .ok()
            .and_then(|mbps| Speed::from_mbps(mbps, lanes))
    };
    let max_speed = [Some(port_dir.as_path()), peer.as_deref()]
        .into_iter()
        .flatten()
        .filter_map(hub_speed)
        .max()
        .map(Sourced::declared);

    Some(PortInfo {
        number,
        connector,
        max_speed,
        companion,
    })
}

fn read_typec(dir: &Path) -> Vec<TypeCPort> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut ports: Vec<TypeCPort> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let is_port = name.starts_with("port") && !name.contains('-');
            is_port.then(|| read_typec_port(dir, &name))
        })
        .collect();
    ports.sort_by_key(|a| natural_key(&a.id));
    ports
}

fn read_typec_port(dir: &Path, name: &str) -> TypeCPort {
    let port = dir.join(name);
    let partner_dir = dir.join(format!("{name}-partner"));
    let cable_dir = dir.join(format!("{name}-cable"));

    let partner = partner_dir.exists().then(|| {
        let id_header =
            read_hex::<u32>(&partner_dir.join("identity/id_header")).filter(|v| *v != 0);
        let product = read_hex::<u32>(&partner_dir.join("identity/product")).filter(|v| *v != 0);
        Partner {
            kind: read(&partner_dir.join("type"))
                .or_else(|| read(&partner_dir.join("accessory_mode"))),
            supports_usb_pd: read(&partner_dir.join("supports_usb_power_delivery"))
                .map(|v| v == "yes"),
            vendor_id: id_header.map(|h| (h & 0xffff) as u16),
            product_id: product.map(|p| (p >> 16) as u16),
        }
    });

    let cable = cable_dir.exists().then(|| {
        let id_header = read_hex::<u32>(&cable_dir.join("identity/id_header")).filter(|v| *v != 0);
        let product = read_hex::<u32>(&cable_dir.join("identity/product")).filter(|v| *v != 0);
        let vdo =
            read_hex::<u32>(&cable_dir.join("identity/product_type_vdo1")).filter(|v| *v != 0);
        let kind = match read(&cable_dir.join("type")).as_deref() {
            Some("active") => Some(typec::CableKind::Active),
            Some("passive") => Some(typec::CableKind::Passive),
            _ => id_header.and_then(typec::cable_kind_from_id_header),
        };
        Cable {
            kind,
            plug_type: read(&cable_dir.join("plug_type")),
            vendor_id: id_header.map(|h| (h & 0xffff) as u16),
            product_id: product.map(|p| (p >> 16) as u16),
            certification_id: read_hex(&cable_dir.join("identity/cert_stat")),
            emarker: vdo.map(CableVdo::decode),
        }
    });

    TypeCPort {
        id: name.to_owned(),
        data_role: read(&port.join("data_role")).map(|r| selected(&r)),
        power_role: read(&port.join("power_role")).map(|r| selected(&r)),
        power_operation_mode: read(&port.join("power_operation_mode")),
        partner,
        cable,
    }
}

/// "host [device]" -> "device" : sysfs met le choix actif entre crochets.
fn selected(value: &str) -> String {
    value
        .split_whitespace()
        .find_map(|w| w.strip_prefix('[')?.strip_suffix(']'))
        .unwrap_or(value)
        .to_owned()
}

fn read(path: &Path) -> Option<String> {
    let value = fs::read_to_string(path).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn read_num<T: std::str::FromStr>(path: &Path) -> Option<T> {
    read(path)?.parse().ok()
}

fn read_hex<T: TryFrom<u64>>(path: &Path) -> Option<T> {
    let value = read(path)?;
    let value = value.trim_start_matches("0x");
    T::try_from(u64::from_str_radix(value, 16).ok()?).ok()
}

/// " 3.20" -> Bcd(0x0320).
fn parse_bcd_version(value: &str) -> Option<Bcd> {
    let (major, minor) = value.trim().split_once('.')?;
    let major = u16::from_str_radix(major, 16).ok()?;
    let minor = u16::from_str_radix(&format!("{minor:0<2}")[..2], 16).ok()?;
    Some(Bcd(major << 8 | minor))
}

/// "usb1" -> profondeur 0, "1-2" -> 1, "1-2.3" -> 2.
fn depth(name: &str) -> usize {
    match name.split_once('-') {
        Some((_, ports)) => 1 + ports.matches('.').count(),
        None => 0,
    }
}

/// "1-2.3" -> "1-2", "1-2" -> "usb1", "usb1" -> rien.
fn parent_of(name: &str) -> Option<String> {
    let (bus, ports) = name.split_once('-')?;
    Some(match ports.rsplit_once('.') {
        Some((parent, _)) => format!("{bus}-{parent}"),
        None => format!("usb{bus}"),
    })
}

fn natural_key(name: &str) -> Vec<u32> {
    name.split(|c: char| !c.is_ascii_digit())
        .filter_map(|n| n.parse().ok())
        .collect()
}

fn sort_children(device: &mut Device) {
    device.children.sort_by_key(|a| natural_key(&a.location));
    device.children.iter_mut().for_each(sort_children);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    struct FakeSys {
        root: tempfile::TempDir,
    }

    impl FakeSys {
        fn new() -> Self {
            Self {
                root: tempfile::tempdir().unwrap(),
            }
        }
        fn sys(&self) -> PathBuf {
            self.root.path().join("sys")
        }
        fn write(&self, rel: &str, content: &str) {
            let path = self.sys().join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        fn write_bytes(&self, rel: &str, content: &[u8]) {
            let path = self.sys().join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        fn link(&self, rel: &str, target: &str) {
            let path = self.sys().join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            symlink(self.sys().join(target), path).unwrap();
        }
        fn device(&self, devpath: &str, name: &str, attrs: &[(&str, &str)]) {
            for (k, v) in attrs {
                self.write(&format!("{devpath}/{k}"), v);
            }
            self.link(&format!("bus/usb/devices/{name}"), devpath);
        }
    }

    /// Un contrôleur xHCI : bus 1 (USB 2) et bus 2 (USB 3, 10 Gb/s), port 2
    /// partagé, un SSD 10 Gb/s branché avec un câble USB 2.
    fn xhci_with_ssd_on_usb2_cable() -> FakeSys {
        let fake = FakeSys::new();
        let pci = "devices/pci0000:00/0000:00:14.0";
        fake.device(
            &format!("{pci}/usb1"),
            "usb1",
            &[
                ("speed", "480"),
                ("bDeviceClass", "09"),
                ("version", " 2.00"),
                ("product", "xHCI Host Controller"),
            ],
        );
        fake.device(
            &format!("{pci}/usb2"),
            "usb2",
            &[
                ("speed", "10000"),
                ("rx_lanes", "1"),
                ("bDeviceClass", "09"),
                ("version", " 3.10"),
            ],
        );
        fake.write(
            &format!("{pci}/usb1/1-0:1.0/usb1-port2/connect_type"),
            "hotplug",
        );
        fake.write(
            &format!("{pci}/usb2/2-0:1.0/usb2-port2/connect_type"),
            "hotplug",
        );
        fake.link(
            &format!("{pci}/usb1/1-0:1.0/usb1-port2/peer"),
            &format!("{pci}/usb2/2-0:1.0/usb2-port2"),
        );
        fake.link(
            &format!("{pci}/usb2/2-0:1.0/usb2-port2/peer"),
            &format!("{pci}/usb1/1-0:1.0/usb1-port2"),
        );

        let ssd = format!("{pci}/usb1/1-2");
        fake.device(
            &ssd,
            "1-2",
            &[
                ("speed", "480"),
                ("rx_lanes", "1"),
                ("tx_lanes", "1"),
                ("version", " 2.10"),
                ("idVendor", "0bda"),
                ("idProduct", "9210"),
                ("product", "SSD NVMe"),
                ("serial", "SECRET"),
                ("bDeviceClass", "00"),
                ("bMaxPower", "896mA"),
            ],
        );
        fake.link(
            &format!("{ssd}/port"),
            &format!("{pci}/usb1/1-0:1.0/usb1-port2"),
        );
        // BOS : USB 2.0 Extension + SuperSpeed + SuperSpeedPlus 10 Gb/s.
        let rx: u32 = (10 << 16) | (1 << 14) | (3 << 4) | 1;
        let mut bos = vec![
            5, 0x0f, 0, 0, 3, 7, 0x10, 0x02, 0x02, 0, 0, 0, 10, 0x10, 0x03, 0, 0x0e, 0, 1, 10,
            0xff, 0x07,
        ];
        bos.extend_from_slice(&[16, 0x10, 0x0a, 0]);
        bos.extend_from_slice(&0u32.to_le_bytes());
        bos.extend_from_slice(&[0, 0, 0, 0]);
        bos.extend_from_slice(&rx.to_le_bytes());
        let total = bos.len() as u16;
        bos[2..4].copy_from_slice(&total.to_le_bytes());
        fake.write_bytes(&format!("{ssd}/bos_descriptors"), &bos);
        fake
    }

    #[test]
    fn builds_tree_with_negotiated_and_max_speeds() {
        let fake = xhci_with_ssd_on_usb2_cable();
        let report = collect_from(&fake.sys()).unwrap();
        assert_eq!(report.buses.len(), 2);
        assert_eq!(report.buses[0].id, "usb1");
        assert_eq!(report.buses[0].controller.as_deref(), Some("0000:00:14.0"));

        let ssd = &report.buses[0].root_hub.children[0];
        assert_eq!(ssd.location, "1-2");
        assert_eq!(ssd.vendor_id, Some(0x0bda));
        assert_eq!(ssd.usb_version, Some(Bcd(0x0210)));
        assert_eq!(ssd.speed, Some(Sourced::negotiated(Speed::High)));
        assert_eq!(ssd.max_speed, Some(Sourced::declared(Speed::Gen2x1)));
        assert_eq!(ssd.max_power_ma, Some(896));

        let port = ssd.port.as_ref().unwrap();
        assert_eq!(port.number, Some(2));
        assert_eq!(port.companion.as_deref(), Some("usb2-port2"));
        assert_eq!(port.max_speed, Some(Sourced::declared(Speed::Gen2x1)));
    }

    #[test]
    fn audit_blames_the_cable() {
        let fake = xhci_with_ssd_on_usb2_cable();
        let report = collect_from(&fake.sys()).unwrap();
        // Vérifié ici plutôt que dans treasure-audit pour couvrir la chaîne complète.
        let ssd = &report.buses[0].root_hub.children[0];
        assert!(ssd.max_speed.unwrap().value.is_superspeed());
        assert!(!ssd.speed.unwrap().value.is_superspeed());
        assert!(
            ssd.port
                .as_ref()
                .unwrap()
                .max_speed
                .unwrap()
                .value
                .is_superspeed()
        );
    }

    #[test]
    fn reads_typec_cable_emarker() {
        let fake = FakeSys::new();
        fake.write("class/typec/port0/data_role", "[host] device");
        fake.write("class/typec/port0/power_role", "[source] sink");
        fake.write(
            "class/typec/port0-partner/supports_usb_power_delivery",
            "yes",
        );
        fake.write("class/typec/port0-cable/type", "passive");
        fake.write("class/typec/port0-cable/plug_type", "type-c");
        fake.write("class/typec/port0-cable/identity/id_header", "0x18002e99");
        fake.write("class/typec/port0-cable/identity/product", "0x00010100");
        fake.write("class/typec/port0-cable/identity/cert_stat", "0x00000000");
        // USB 3.2 Gen 2, 5 A, 20 V.
        fake.write(
            "class/typec/port0-cable/identity/product_type_vdo1",
            "0x00000042",
        );
        let report = collect_from(&fake.sys()).unwrap();

        let port = &report.typec_ports[0];
        assert_eq!(port.data_role.as_deref(), Some("host"));
        assert_eq!(port.power_role.as_deref(), Some("source"));
        let cable = port.cable.as_ref().unwrap();
        assert_eq!(cable.kind, Some(typec::CableKind::Passive));
        assert_eq!(cable.vendor_id, Some(0x2e99));
        let vdo = cable.emarker.unwrap();
        assert_eq!(vdo.highest_speed, Some(typec::CableSpeed::Usb32Gen2));
        assert_eq!(vdo.max_current_ma, Some(5000));
        assert_eq!(vdo.max_power_w(), Some(100));
    }

    #[test]
    fn helpers() {
        assert_eq!(parse_bcd_version(" 3.20"), Some(Bcd(0x0320)));
        assert_eq!(parse_bcd_version("2.00"), Some(Bcd(0x0200)));
        assert_eq!(parse_bcd_version("1.1"), Some(Bcd(0x0110)));
        assert_eq!(parent_of("1-2.3.4").as_deref(), Some("1-2.3"));
        assert_eq!(parent_of("3-1").as_deref(), Some("usb3"));
        assert_eq!(parent_of("usb3"), None);
        assert_eq!(depth("1-2.3"), 2);
        assert_eq!(selected("host [device]"), "device");
    }
}
