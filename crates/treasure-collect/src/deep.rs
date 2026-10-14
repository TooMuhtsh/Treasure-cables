//! Diagnostic approfondi : analyse des événements de la pile USB.
//!
//! La capture elle-même dépend du système (ETW sous Windows, voir
//! `windows_etw`) ; ce module ne fait que rattacher chaque événement à un
//! port et compter, ce qui le rend testable partout.
//!
//! Rattachement (pilote USBHUB3) : les événements de « rundown » publiés au
//! démarrage de l'écoute donnent, pour chaque appareil, son hub, son chemin
//! de ports et son contrôleur ; les événements de reconnaissance donnent
//! (hub, appareil, port) pour les appareils branchés ensuite. Les erreurs ne
//! citent souvent que l'appareil ou le hub et un numéro de port.

use std::collections::{BTreeMap, HashMap};

use treasure_model::{Device, LinkDiagnostics, PortActivity, Report};

/// Fournisseur ETW d'où vient un événement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    /// Microsoft-Windows-USB-USBHUB3.
    Hub3,
    /// Microsoft-Windows-USB-USBXHCI.
    Xhci,
}

/// Champs utiles d'un événement, extraits par nom (`fid_*` du manifeste).
#[derive(Debug, Clone, Default)]
pub struct UsbEvent {
    pub id: u16,
    pub usb_device: Option<u64>,
    pub hub_device: Option<u64>,
    pub port: Option<u32>,
    pub nt_status: Option<u32>,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    /// Ports traversés depuis le hub racine (rundown).
    pub port_path: Vec<u32>,
    /// Contrôleur (bus, appareil, fonction PCI).
    pub pci: Option<(u32, u32, u32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Rundown,
    EnumerationStop,
    EnumerationError,
    DescriptorError,
    PortError,
    HubReset,
    TransferError,
    Removal,
    ControllerError,
}

/// Vrai si l'événement intéresse l'analyse (évite de décoder les autres).
pub fn is_relevant(provider: Provider, id: u16) -> bool {
    classify(provider, id).is_some()
}

/// Identifiants d'événements des manifestes USBHUB3 et USBXHCI.
fn classify(provider: Provider, id: u16) -> Option<Kind> {
    Some(match (provider, id) {
        (Provider::Hub3, 6) => Kind::Rundown,
        (Provider::Hub3, 61) => Kind::EnumerationStop,
        (Provider::Hub3, 62..=65 | 160..=178 | 181..=184 | 186 | 187 | 189 | 190 | 197) => {
            Kind::EnumerationError
        }
        (Provider::Hub3, 70..=85 | 191 | 192) => Kind::DescriptorError,
        (Provider::Hub3, 102 | 103) => Kind::PortError,
        (Provider::Hub3, 120) => Kind::HubReset,
        (Provider::Hub3, 130..=133) => Kind::TransferError,
        (Provider::Hub3, 194..=196) => Kind::Removal,
        (Provider::Xhci, 28 | 30 | 34 | 49) => Kind::ControllerError,
        _ => return None,
    })
}

pub struct Analyzer {
    /// Contrôleur PCI -> préfixe d'emplacement ("2").
    buses: HashMap<(u32, u32, u32), String>,
    /// Appareil -> emplacement ("2-17").
    devices: HashMap<u64, String>,
    /// Hub -> préfixe des emplacements de ses ports ("2", "2-3").
    hubs: HashMap<u64, String>,
    activity: BTreeMap<String, PortActivity>,
    controller_errors: u32,
    events: u32,
}

impl Analyzer {
    pub fn new(buses: HashMap<(u32, u32, u32), String>) -> Self {
        Self {
            buses,
            devices: HashMap::new(),
            hubs: HashMap::new(),
            activity: BTreeMap::new(),
            controller_errors: 0,
            events: 0,
        }
    }

    /// Événements pertinents reçus (pour l'affichage en direct).
    pub fn events(&self) -> u32 {
        self.events
    }

    pub fn feed(&mut self, provider: Provider, event: &UsbEvent) {
        let Some(kind) = classify(provider, event.id) else {
            return;
        };
        if kind != Kind::Rundown {
            self.events += 1;
        }
        // Tout événement qui cite (hub, appareil, port) situe l'appareil.
        if let (Some(hub), Some(device), Some(port)) =
            (event.hub_device, event.usb_device, event.port)
            && let Some(prefix) = self.hubs.get(&hub)
        {
            let location = child_location(prefix, port);
            self.devices.insert(device, location);
        }
        match kind {
            Kind::Rundown => self.rundown(event),
            Kind::ControllerError => self.controller_errors += 1,
            _ => {
                let entry = self.entry(event);
                match kind {
                    Kind::EnumerationStop if event.nt_status.unwrap_or(0) == 0 => {
                        entry.connections += 1
                    }
                    Kind::EnumerationStop | Kind::EnumerationError => entry.enumeration_errors += 1,
                    Kind::DescriptorError => entry.descriptor_errors += 1,
                    Kind::PortError => entry.port_errors += 1,
                    Kind::HubReset => entry.hub_resets += 1,
                    Kind::TransferError => entry.transfer_errors += 1,
                    Kind::Removal => entry.removals += 1,
                    Kind::Rundown | Kind::ControllerError => {}
                }
            }
        }
    }

    /// Rundown : l'appareil, son chemin de ports et son contrôleur.
    fn rundown(&mut self, event: &UsbEvent) {
        let Some(bus) = event.pci.and_then(|pci| self.buses.get(&pci)).cloned() else {
            return;
        };
        let Some((&last, parents)) = event.port_path.split_last() else {
            return;
        };
        let parent = parents.iter().fold(bus, |loc, &p| child_location(&loc, p));
        if let Some(hub) = event.hub_device {
            self.hubs.insert(hub, parent.clone());
        }
        if let Some(device) = event.usb_device {
            let location = child_location(&parent, last);
            // Un appareil qui est lui-même un hub préfixe ses ports.
            self.hubs.insert(device, location.clone());
            self.devices.insert(device, location);
        }
    }

    fn entry(&mut self, event: &UsbEvent) -> &mut PortActivity {
        let device_location = event.usb_device.and_then(|d| self.devices.get(&d)).cloned();
        let hub_location = match (event.usb_device.or(event.hub_device), event.port) {
            (Some(hub), Some(port)) => self
                .hubs
                .get(&hub)
                .map(|prefix| child_location(prefix, port)),
            _ => None,
        };
        let location = device_location.or(hub_location);
        let key = match (&location, event.port, event.vendor_id, event.product_id) {
            (Some(loc), ..) => loc.clone(),
            (None, Some(port), Some(v), Some(p)) => format!("~port {port} {v:04x}:{p:04x}"),
            (None, Some(port), ..) => format!("~port {port}"),
            (None, None, Some(v), Some(p)) => format!("~{v:04x}:{p:04x}"),
            _ => "~".to_owned(),
        };
        let entry = self.activity.entry(key).or_insert_with(|| PortActivity {
            location,
            port: event.port,
            ..Default::default()
        });
        if event.vendor_id.is_some() {
            entry.vendor_id = event.vendor_id;
            entry.product_id = event.product_id;
        }
        entry
    }

    pub fn finish(self, duration_s: u32) -> LinkDiagnostics {
        LinkDiagnostics {
            duration_s,
            ports: self.activity.into_values().collect(),
            controller_errors: self.controller_errors,
        }
    }
}

/// Complète le bilan d'écoute à l'aide des relevés faits avant et après :
///
/// - un port que l'écoute n'a pas pu situer (hub absent du rundown, par
///   exemple un contrôleur sans aucun appareil au départ) est rattaché à
///   l'appareil apparu ou disparu sur un port de même numéro, s'il est le
///   seul candidat ;
/// - chaque port situé reçoit le nom de son appareil, même débranché depuis.
pub fn complete(diagnostics: &mut LinkDiagnostics, before: Option<&Report>, after: &Report) {
    let identity = |d: &Device| (d.location.clone(), d.vendor_id, d.product_id);
    let before_devices: Vec<&Device> = before.map(|b| b.devices().collect()).unwrap_or_default();
    let after_devices: Vec<&Device> = after.devices().collect();
    let changed: Vec<&Device> = after_devices
        .iter()
        .filter(|d| !before_devices.iter().any(|b| identity(b) == identity(d)))
        .chain(
            before_devices
                .iter()
                .filter(|b| !after_devices.iter().any(|d| identity(d) == identity(b))),
        )
        .copied()
        .collect();
    let all: Vec<&Device> = after_devices
        .iter()
        .chain(&before_devices)
        .copied()
        .collect();
    let on_port = |devices: &[&Device], n: u32| -> Vec<String> {
        let mut locations: Vec<String> = devices
            .iter()
            .filter(|d| last_port(&d.location) == Some(n))
            .map(|d| d.location.clone())
            .collect();
        locations.sort();
        locations.dedup();
        locations
    };

    for port in &mut diagnostics.ports {
        if port.location.is_none()
            && let Some(n) = port.port
        {
            let changed_here = on_port(&changed, n);
            let candidates = if changed_here.is_empty() {
                on_port(&all, n)
            } else {
                changed_here
            };
            if let [only] = candidates.as_slice() {
                port.location = Some(only.clone());
            }
        }
        if let Some(location) = port.location.as_deref()
            && port.device_name.is_none()
            && let Some(device) = all.iter().find(|d| d.location == location)
        {
            port.device_name = Some(device.display_name());
            port.vendor_id = port.vendor_id.or(device.vendor_id);
            port.product_id = port.product_id.or(device.product_id);
        }
    }

    // Deux entrées peuvent désormais désigner le même port : on les fusionne.
    let mut merged: Vec<PortActivity> = Vec::new();
    for port in diagnostics.ports.drain(..) {
        match merged
            .iter_mut()
            .find(|m| m.location.is_some() && m.location == port.location)
        {
            Some(m) => {
                m.connections += port.connections;
                m.enumeration_errors += port.enumeration_errors;
                m.descriptor_errors += port.descriptor_errors;
                m.port_errors += port.port_errors;
                m.transfer_errors += port.transfer_errors;
                m.hub_resets += port.hub_resets;
                m.removals += port.removals;
                m.device_name = m.device_name.take().or(port.device_name);
            }
            None => merged.push(port),
        }
    }
    diagnostics.ports = merged;
}

/// "1-2" -> 2 ; "2-3.4" -> 4.
fn last_port(location: &str) -> Option<u32> {
    location.rsplit(['-', '.']).next()?.parse().ok()
}

/// "2" + port 6 -> "2-6" ; "2-3" + port 1 -> "2-3.1".
pub(crate) fn child_location(parent: &str, port: u32) -> String {
    if parent.contains('-') {
        format!("{parent}.{port}")
    } else {
        format!("{parent}-{port}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HUB_ROOT: u64 = 0x1000;
    const SSD: u64 = 0x2000;
    const NEW_DEVICE: u64 = 0x3000;
    const PCI: (u32, u32, u32) = (0, 20, 0);

    fn analyzer() -> Analyzer {
        let mut a = Analyzer::new(HashMap::from([(PCI, "2".to_owned())]));
        // Rundown : le SSD sur le port 17 du hub racine du bus 2.
        a.feed(
            Provider::Hub3,
            &UsbEvent {
                id: 6,
                hub_device: Some(HUB_ROOT),
                usb_device: Some(SSD),
                port: Some(17),
                port_path: vec![17],
                pci: Some(PCI),
                ..Default::default()
            },
        );
        a
    }

    #[test]
    fn errors_are_attributed_to_rundown_devices() {
        let mut a = analyzer();
        let error = UsbEvent {
            id: 131,
            usb_device: Some(SSD),
            port: Some(17),
            ..Default::default()
        };
        a.feed(Provider::Hub3, &error);
        a.feed(Provider::Hub3, &error);
        let d = a.finish(30);
        assert_eq!(d.ports.len(), 1);
        assert_eq!(d.ports[0].location.as_deref(), Some("2-17"));
        assert_eq!(d.ports[0].transfer_errors, 2);
    }

    #[test]
    fn replugged_device_is_located_through_its_hub() {
        let mut a = analyzer();
        // Nouvel appareil sur le port 6 du hub racine : deux échecs puis succès.
        let enumeration = |id, status| UsbEvent {
            id,
            hub_device: Some(HUB_ROOT),
            usb_device: Some(NEW_DEVICE),
            port: Some(6),
            nt_status: Some(status),
            ..Default::default()
        };
        a.feed(Provider::Hub3, &enumeration(61, 0xc000_0001));
        a.feed(
            Provider::Hub3,
            &UsbEvent {
                id: 197,
                usb_device: Some(NEW_DEVICE),
                port: Some(6),
                vendor_id: Some(0x0781),
                product_id: Some(0x55b8),
                ..Default::default()
            },
        );
        a.feed(Provider::Hub3, &enumeration(61, 0));
        let d = a.finish(30);
        let port = d
            .ports
            .iter()
            .find(|p| p.location.as_deref() == Some("2-6"))
            .unwrap();
        assert_eq!(port.enumeration_errors, 2);
        assert_eq!(port.connections, 1);
        assert_eq!(port.vendor_id, Some(0x0781));
        assert_eq!(port.link_errors(), 2);
    }

    #[test]
    fn unknown_device_falls_back_to_port_number() {
        let mut a = Analyzer::new(HashMap::new());
        a.feed(
            Provider::Hub3,
            &UsbEvent {
                id: 102,
                usb_device: Some(0x9),
                port: Some(3),
                ..Default::default()
            },
        );
        a.feed(
            Provider::Xhci,
            &UsbEvent {
                id: 30,
                ..Default::default()
            },
        );
        a.feed(
            Provider::Hub3,
            &UsbEvent {
                id: 999,
                ..Default::default()
            },
        );
        assert_eq!(a.events(), 2);
        let d = a.finish(10);
        assert_eq!(d.ports[0].location, None);
        assert_eq!(d.ports[0].port, Some(3));
        assert_eq!(d.ports[0].port_errors, 1);
        assert_eq!(d.controller_errors, 1);
    }

    fn report_with(devices: Vec<Device>) -> Report {
        Report {
            schema_version: treasure_model::SCHEMA_VERSION,
            tool: treasure_model::ToolInfo {
                name: "t".into(),
                version: "0".into(),
            },
            host: treasure_model::HostInfo {
                os: "windows".into(),
                os_version: None,
            },
            generated_at: None,
            buses: vec![treasure_model::Bus {
                id: "usb1".into(),
                controller: None,
                root_hub: Device {
                    location: "usb1".into(),
                    is_hub: true,
                    children: devices,
                    ..Default::default()
                },
            }],
            typec_ports: vec![],
            displays: vec![],
            usb4_routers: vec![],
            link_diagnostics: None,
            notes: vec![],
        }
    }

    fn phone(location: &str) -> Device {
        Device {
            location: location.into(),
            product: Some("SAMSUNG_Android".into()),
            vendor_id: Some(0x04e8),
            product_id: Some(0x6860),
            ..Default::default()
        }
    }

    #[test]
    fn unplaced_port_is_matched_to_the_device_that_appeared() {
        // Cas réel : contrôleur vide au début de l'écoute, téléphone branché
        // sur son port 2 ; une clé reste branchée sur un autre port 2.
        let stays = Device {
            location: "2-2".into(),
            ..phone("2-2")
        };
        let before = report_with(vec![stays.clone()]);
        let after = report_with(vec![stays, phone("1-2")]);
        let mut d = LinkDiagnostics {
            duration_s: 8,
            ports: vec![PortActivity {
                port: Some(2),
                connections: 1,
                ..Default::default()
            }],
            controller_errors: 0,
        };
        complete(&mut d, Some(&before), &after);
        assert_eq!(d.ports[0].location.as_deref(), Some("1-2"));
        assert_eq!(d.ports[0].device_name.as_deref(), Some("SAMSUNG_Android"));
    }

    #[test]
    fn ambiguous_port_is_left_alone_and_duplicates_merge() {
        let before = report_with(vec![]);
        let after = report_with(vec![phone("1-2"), phone("2-2")]);
        let mut d = LinkDiagnostics {
            duration_s: 8,
            ports: vec![
                PortActivity {
                    port: Some(2),
                    connections: 1,
                    ..Default::default()
                },
                PortActivity {
                    location: Some("1-2".into()),
                    connections: 1,
                    ..Default::default()
                },
                PortActivity {
                    location: Some("1-2".into()),
                    port_errors: 2,
                    ..Default::default()
                },
            ],
            controller_errors: 0,
        };
        complete(&mut d, Some(&before), &after);
        assert_eq!(d.ports.len(), 2);
        assert_eq!(d.ports[0].location, None);
        assert_eq!(d.ports[1].connections, 1);
        assert_eq!(d.ports[1].port_errors, 2);
        assert_eq!(last_port("2-3.4"), Some(4));
    }

    #[test]
    fn nested_hub_ports_get_dotted_locations() {
        let mut a = Analyzer::new(HashMap::from([(PCI, "1".to_owned())]));
        a.feed(
            Provider::Hub3,
            &UsbEvent {
                id: 6,
                hub_device: Some(0xa),
                usb_device: Some(0xb),
                port_path: vec![2, 4],
                pci: Some(PCI),
                ..Default::default()
            },
        );
        a.feed(
            Provider::Hub3,
            &UsbEvent {
                id: 102,
                usb_device: Some(0xb),
                ..Default::default()
            },
        );
        let d = a.finish(5);
        assert_eq!(d.ports[0].location.as_deref(), Some("1-2.4"));
    }
}
