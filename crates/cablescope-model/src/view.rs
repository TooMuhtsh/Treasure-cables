//! Vue « par prise » : ce que l'utilisateur voit sur sa machine.
//!
//! Le système décrit des ports logiques, rattachés à des contrôleurs ; une
//! prise physique USB 3 en compte deux (sa moitié USB 2 et sa moitié USB 3),
//! parfois sur deux contrôleurs différents. Cette vue les réunit, place sous
//! chaque prise les appareils qui y sont branchés (un hub ou un dock garde
//! ses propres appareils en dessous) et met à part les appareils internes.
//! Partagée par l'application et le site web.

use std::collections::HashMap;

use crate::{Bus, Connector, Device, PortInfo, Report, Sourced, Speed};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlugKind {
    TypeC,
    /// Accessible à l'utilisateur, type non précisé (USB-A en pratique).
    Other,
    Internal,
}

impl PlugKind {
    pub fn label(self) -> &'static str {
        match self {
            PlugKind::TypeC => "USB-C",
            PlugKind::Other => "USB-A",
            PlugKind::Internal => "interne",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Plug<'a> {
    pub kind: PlugKind,
    /// Ports logiques qui forment la prise ("1-2", "2-6").
    pub ports: Vec<PortInfo>,
    /// Ce que la prise sait faire (le meilleur de ses ports).
    pub max_speed: Option<Sourced<Speed>>,
    /// Appareils branchés directement sur la prise.
    pub devices: Vec<&'a Device>,
}

impl Plug<'_> {
    pub fn locations(&self) -> Vec<&str> {
        self.ports
            .iter()
            .filter_map(|p| p.location.as_deref())
            .collect()
    }
}

#[derive(Debug, Clone, Default)]
pub struct MachineView<'a> {
    /// Prises accessibles, USB-C d'abord, libres comprises.
    pub plugs: Vec<Plug<'a>>,
    /// Appareils intégrés à la machine (webcam, Bluetooth, lecteur...).
    pub internal: Vec<&'a Device>,
    /// Ports déclarés accessibles mais qui ne mènent à aucune prise (voir
    /// [`is_unwired`]) ; écartés des prises.
    pub unwired: Vec<String>,
}

/// Port fantôme : déclaré accessible par le micrologiciel, mais seul port
/// USB 2.0 d'un contrôleur par ailleurs USB 3, sans moitié USB 3 et libre.
/// C'est la configuration des contrôleurs USB-C intégrés d'Intel : leur
/// unique port USB 2.0 n'est relié à aucune prise (les fils USB 2.0 des
/// prises USB-C passent par l'autre contrôleur). Sur un contrôleur USB 2.0
/// classique, les ports USB 2.0 sont nombreux et la règle ne s'applique pas.
pub fn is_unwired(report: &Report, bus: &Bus, port: &PortInfo) -> bool {
    let usb2_only =
        |p: &PortInfo| p.companion.is_none() && p.max_speed.is_some_and(|s| s.value <= Speed::High);
    let Some(location) = port.location.as_deref() else {
        return false;
    };
    let accessible = !matches!(
        port.connector,
        Some(Connector::Internal | Connector::TypeC) | None
    );
    let ports = &bus.root_hub.ports;
    accessible
        && usb2_only(port)
        && ports.iter().filter(|p| usb2_only(p)).count() == 1
        && ports
            .iter()
            .any(|p| p.max_speed.is_some_and(|s| s.value >= Speed::Gen1x1))
        && !report.buses.iter().any(|b| {
            b.root_hub.children.iter().any(|d| {
                d.location == location
                    || d.port.as_ref().and_then(|p| p.location.as_deref()) == Some(location)
            }) || b
                .root_hub
                .ports
                .iter()
                .any(|p| p.companion.as_deref() == Some(location))
        })
}

pub fn machine_view(report: &Report) -> MachineView<'_> {
    let mut groups: Vec<(Vec<PortInfo>, Vec<&Device>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();

    let mut add_port =
        |groups: &mut Vec<(Vec<PortInfo>, Vec<&Device>)>, port: &PortInfo| -> usize {
            let keys: Vec<&String> = port.location.iter().chain(port.companion.iter()).collect();
            let existing = keys.iter().find_map(|k| index.get(*k).copied());
            let group = match existing {
                Some(g) => g,
                None => {
                    groups.push((Vec::new(), Vec::new()));
                    groups.len() - 1
                }
            };
            if !groups[group]
                .0
                .iter()
                .any(|p| p.location.is_some() && p.location == port.location)
            {
                groups[group].0.push(port.clone());
            }
            for key in keys {
                index.insert(key.clone(), group);
            }
            group
        };

    // Ports des hubs racine : ce sont les prises de la machine.
    let mut unwired = Vec::new();
    for bus in &report.buses {
        for port in &bus.root_hub.ports {
            if is_unwired(report, bus, port) {
                unwired.extend(port.location.clone());
            } else {
                add_port(&mut groups, port);
            }
        }
    }
    // Appareils branchés directement sur la machine.
    for bus in &report.buses {
        for device in &bus.root_hub.children {
            let port = device.port.clone().unwrap_or_else(|| PortInfo {
                location: Some(device.location.clone()),
                ..Default::default()
            });
            let port = PortInfo {
                location: port
                    .location
                    .clone()
                    .or_else(|| Some(device.location.clone())),
                ..port
            };
            let group = add_port(&mut groups, &port);
            groups[group].1.push(device);
        }
    }

    let mut view = MachineView {
        unwired,
        ..Default::default()
    };
    for (ports, devices) in groups {
        let connectors: Vec<Option<Connector>> = ports.iter().map(|p| p.connector).collect();
        let kind = if connectors.contains(&Some(Connector::TypeC)) {
            PlugKind::TypeC
        } else if !connectors.is_empty()
            && connectors.iter().all(|c| *c == Some(Connector::Internal))
        {
            PlugKind::Internal
        } else {
            PlugKind::Other
        };
        if kind == PlugKind::Internal {
            view.internal.extend(devices);
            continue;
        }
        let max_speed = ports
            .iter()
            .filter_map(|p| p.max_speed)
            .max_by(|a, b| a.value.cmp(&b.value));
        view.plugs.push(Plug {
            kind,
            ports,
            max_speed,
            devices,
        });
    }
    view.plugs.sort_by(|a, b| {
        a.kind.cmp(&b.kind).then_with(|| {
            natural(a.locations().first().copied()).cmp(&natural(b.locations().first().copied()))
        })
    });
    view
}

fn natural(location: Option<&str>) -> Vec<u32> {
    location
        .unwrap_or_default()
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|n| n.parse().ok())
        .collect()
}

/// Ce que sait faire une prise, en clair. Une capacité seulement déduite des
/// protocoles du port est une borne basse ("USB 3, 5 Gb/s ou plus").
pub fn capability_label(speed: Sourced<Speed>) -> String {
    match (speed.value, speed.source) {
        (Speed::Gen1x1, crate::Source::Inferred) => "USB 3 (5 Gb/s ou plus)".to_owned(),
        (value, _) => value.label().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bus, HostInfo, SCHEMA_VERSION, ToolInfo};

    fn port(
        location: &str,
        connector: Connector,
        companion: Option<&str>,
        speed: Speed,
    ) -> PortInfo {
        PortInfo {
            location: Some(location.into()),
            number: None,
            connector: Some(connector),
            max_speed: Some(Sourced::inferred(speed)),
            companion: companion.map(Into::into),
        }
    }

    fn device(location: &str, name: &str, port: PortInfo) -> Device {
        Device {
            location: location.into(),
            product: Some(name.into()),
            port: Some(port),
            ..Default::default()
        }
    }

    /// Configuration relevée sur un portable Intel : la prise USB-C a sa
    /// moitié USB 3 sur le contrôleur 1 et sa moitié USB 2 sur le contrôleur 2.
    fn laptop() -> Report {
        let type_c_usb2 = port("2-6", Connector::TypeC, Some("1-2"), Speed::Gen1x1);
        let usb_a = port("2-1", Connector::Unknown, Some("2-17"), Speed::Gen1x1);
        let webcam_port = port("2-5", Connector::Internal, None, Speed::High);
        let bus = |id: &str, ports: Vec<PortInfo>, children: Vec<Device>| Bus {
            id: id.into(),
            controller: None,
            root_hub: Device {
                location: id.into(),
                is_hub: true,
                ports,
                children,
                ..Default::default()
            },
        };
        Report {
            schema_version: SCHEMA_VERSION,
            tool: ToolInfo {
                name: "t".into(),
                version: "0".into(),
            },
            host: HostInfo {
                os: "windows".into(),
                os_version: None,
            },
            generated_at: None,
            buses: vec![
                bus(
                    "usb1",
                    vec![
                        port("1-1", Connector::Unknown, None, Speed::High),
                        port("1-2", Connector::TypeC, Some("2-6"), Speed::Gen1x1),
                    ],
                    vec![],
                ),
                bus(
                    "usb2",
                    vec![
                        usb_a.clone(),
                        webcam_port.clone(),
                        port("2-3", Connector::Unknown, Some("2-19"), Speed::Gen1x1),
                    ],
                    vec![
                        device("2-5", "Webcam", webcam_port),
                        device("2-6", "TouchScreen", type_c_usb2),
                        device(
                            "2-17",
                            "Sandisk",
                            port("2-17", Connector::Unknown, Some("2-1"), Speed::Gen1x1),
                        ),
                    ],
                ),
            ],
            typec_ports: vec![],
            displays: vec![],
            usb4_routers: vec![],
            link_diagnostics: None,
            notes: vec![],
        }
    }

    #[test]
    fn groups_ports_into_physical_plugs() {
        let report = laptop();
        let view = machine_view(&report);
        let summary: Vec<(PlugKind, Vec<&str>, Vec<String>)> = view
            .plugs
            .iter()
            .map(|p| {
                (
                    p.kind,
                    p.locations(),
                    p.devices.iter().map(|d| d.display_name()).collect(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (
                    PlugKind::TypeC,
                    vec!["1-2", "2-6"],
                    vec!["TouchScreen".to_owned()]
                ),
                (
                    PlugKind::Other,
                    vec!["2-1", "2-17"],
                    vec!["Sandisk".to_owned()]
                ),
                (PlugKind::Other, vec!["2-3"], vec![]),
            ]
        );
        assert_eq!(view.internal.len(), 1);
        assert_eq!(view.internal[0].display_name(), "Webcam");
        assert_eq!(view.unwired, vec!["1-1"]);
    }

    /// Une vraie prise USB-A 2.0 sur un contrôleur qui a d'autres ports
    /// USB 2.0 n'est pas un fantôme.
    #[test]
    fn usb2_socket_on_regular_controller_is_kept() {
        let mut report = laptop();
        report.buses[1]
            .root_hub
            .ports
            .push(port("2-9", Connector::Unknown, None, Speed::High));
        let view = machine_view(&report);
        assert!(view.plugs.iter().any(|p| p.locations() == vec!["2-9"]));
        assert_eq!(view.unwired, vec!["1-1"]);
    }

    /// Un appareil branché prouve que le port est câblé.
    #[test]
    fn occupied_port_is_never_unwired() {
        let mut report = laptop();
        let p = port("1-1", Connector::Unknown, None, Speed::High);
        report.buses[0]
            .root_hub
            .children
            .push(device("1-1", "Clé", p));
        let view = machine_view(&report);
        assert!(view.unwired.is_empty());
        assert!(view.plugs.iter().any(|p| p.locations() == vec!["1-1"]));
    }

    #[test]
    fn real_windows_report_plugs() {
        let json = include_str!("../../../examples/rapport-windows-reel.json");
        let report: Report = serde_json::from_str(json).unwrap();
        let view = machine_view(&report);
        let type_c: Vec<_> = view
            .plugs
            .iter()
            .filter(|p| p.kind == PlugKind::TypeC)
            .collect();
        assert_eq!(type_c.len(), 1);
        assert_eq!(type_c[0].locations(), vec!["1-2", "2-6"]);
        assert_eq!(type_c[0].devices[0].display_name(), "TouchScreen");
        let sandisk = view
            .plugs
            .iter()
            .find(|p| {
                p.devices
                    .iter()
                    .any(|d| d.display_name() == "Sandisk 3.2Gen1")
            })
            .unwrap();
        assert_eq!(sandisk.locations(), vec!["2-3", "2-19"]);
        assert_eq!(view.plugs.len(), 4);
        assert_eq!(view.unwired, vec!["1-1"]);
        assert_eq!(view.internal.len(), 2);
    }

    #[test]
    fn capability_of_usb3_port_is_a_lower_bound() {
        assert_eq!(
            capability_label(Sourced::inferred(Speed::Gen1x1)),
            "USB 3 (5 Gb/s ou plus)"
        );
        assert_eq!(
            capability_label(Sourced::declared(Speed::High)),
            "USB 2.0 High Speed (480 Mb/s)"
        );
    }
}
