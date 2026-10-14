//! Présentation d'un rapport : tout ce qu'une interface affiche, déjà mis en
//! forme et en français.
//!
//! L'application (fenêtre locale) et le futur site web (WebAssembly)
//! affichent la même chose : seule la source du rapport change. Les
//! interfaces n'ont donc aucune logique métier à recoder.

use serde::Serialize;
use treasure_audit::{Confidence, Severity};
use treasure_model::view::{PlugKind, capability_label, is_unwired, machine_view};
use treasure_model::{
    AltModeState, Connector, Device, Display, DpLink, PortInfo, Report, Source, Sourced, Speed,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Ok,
    Info,
    Warning,
    Problem,
}

impl From<Severity> for Level {
    fn from(s: Severity) -> Self {
        match s {
            Severity::Info => Level::Info,
            Severity::Warning => Level::Warning,
            Severity::Problem => Level::Problem,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Presentation {
    pub os: String,
    pub tool_version: String,
    pub generated_at: Option<u64>,
    pub summary: Summary,
    pub plugs: Vec<PlugView>,
    pub internal: Vec<DeviceView>,
    pub controllers: Vec<ControllerView>,
    pub displays: Vec<DisplayView>,
    pub usb4: Vec<Usb4View>,
    pub typec_ports: Vec<TypeCView>,
    pub findings: Vec<FindingView>,
    pub link_diagnostics: Option<LinkView>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub level: Level,
    pub problems: usize,
    pub warnings: usize,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlugView {
    /// "usb_c" ou "usb_a".
    pub kind: &'static str,
    pub label: &'static str,
    pub capability: Option<String>,
    pub capability_mbps: Option<u32>,
    pub locations: Vec<String>,
    /// Écran rattaché à cette prise (USB-C, quand le pilote l'indique).
    pub display: Option<String>,
    pub devices: Vec<DeviceView>,
}

/// Verdict compact sur la vitesse d'un appareil.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeedVerdict {
    /// Au maximum de ce que l'appareil sait faire.
    AtMax,
    /// Plus lent que ce que l'appareil sait faire.
    Below,
    /// Capacité de l'appareil inconnue.
    Unknown,
    Problem,
}

#[derive(Debug, Clone, Serialize)]
pub struct Fact {
    pub label: &'static str,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceView {
    pub name: String,
    pub location: String,
    pub ids: Option<String>,
    pub is_hub: bool,
    pub speed: Option<String>,
    pub speed_mbps: Option<u32>,
    pub max: Option<String>,
    pub max_mbps: Option<u32>,
    pub verdict: SpeedVerdict,
    pub verdict_label: Option<String>,
    /// Détails pour la vue experte.
    pub facts: Vec<Fact>,
    pub children: Vec<DeviceView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ControllerView {
    pub id: String,
    pub name: Option<String>,
    pub ports: Vec<String>,
    pub internal_ports: usize,
    pub devices: Vec<DeviceView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LinkUsage {
    pub label: String,
    pub max: Option<String>,
    /// Part du lien utilisée par le mode actuel.
    pub usage_percent: Option<u32>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DisplayView {
    pub name: String,
    pub internal: bool,
    pub connection: Option<&'static str>,
    pub mode: Option<String>,
    pub native: Option<String>,
    pub below_native: bool,
    pub mirrored: bool,
    pub transport: Option<&'static str>,
    pub gpu: Option<String>,
    pub link: Option<LinkUsage>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Usb4View {
    pub name: String,
    pub role: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct TypeCView {
    pub id: String,
    pub connected: bool,
    pub roles: Option<String>,
    pub cable: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindingView {
    pub code: &'static str,
    pub level: Level,
    pub confidence: &'static str,
    pub subject: String,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LinkView {
    pub duration_s: u32,
    pub ports: Vec<LinkPortView>,
    /// Réinitialisations ou erreurs internes d'un contrôleur USB.
    pub controller_errors: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct LinkPortView {
    pub place: String,
    pub name: Option<String>,
    pub ids: Option<String>,
    pub connections: u32,
    pub errors: u32,
    /// Détail des erreurs et débranchements observés.
    pub detail: Vec<String>,
}

pub fn present(report: &Report) -> Presentation {
    let findings = treasure_audit::audit(report);
    let problems = findings
        .iter()
        .filter(|f| f.severity == Severity::Problem)
        .count();
    let warnings = findings
        .iter()
        .filter(|f| f.severity == Severity::Warning)
        .count();
    let summary = match (problems, warnings) {
        (0, 0) => Summary {
            level: Level::Ok,
            problems,
            warnings,
            label: "Aucun problème détecté".to_owned(),
        },
        (0, w) => Summary {
            level: Level::Warning,
            problems,
            warnings,
            label: format!("{w} point(s) à vérifier"),
        },
        (p, _) => Summary {
            level: Level::Problem,
            problems,
            warnings,
            label: format!("{p} problème(s) détecté(s)"),
        },
    };

    let view = machine_view(report);
    let type_c_plugs = view
        .plugs
        .iter()
        .filter(|p| p.kind == PlugKind::TypeC)
        .count();
    let usb_c_display = report
        .displays
        .iter()
        .find(|d| d.transport == Some(treasure_model::DisplayTransport::UsbC))
        .filter(|_| type_c_plugs == 1)
        .map(|d| d.name.clone().unwrap_or_else(|| "écran externe".to_owned()));

    let plugs = view
        .plugs
        .iter()
        .map(|plug| PlugView {
            kind: if plug.kind == PlugKind::TypeC {
                "usb_c"
            } else {
                "usb_a"
            },
            label: plug.kind.label(),
            capability: plug.max_speed.map(capability_label),
            capability_mbps: plug.max_speed.map(|s| s.value.mbps()),
            locations: plug.locations().iter().map(|l| (*l).to_owned()).collect(),
            display: usb_c_display
                .clone()
                .filter(|_| plug.kind == PlugKind::TypeC),
            devices: plug.devices.iter().map(|d| device_view(d)).collect(),
        })
        .collect();

    let controllers = report
        .buses
        .iter()
        .map(|bus| {
            let (internal, accessible): (Vec<&PortInfo>, Vec<&PortInfo>) = bus
                .root_hub
                .ports
                .iter()
                .partition(|p| p.connector == Some(Connector::Internal));
            ControllerView {
                id: bus.id.clone(),
                name: bus.controller.clone(),
                ports: accessible
                    .into_iter()
                    .map(|p| {
                        let summary = port_summary(p);
                        if is_unwired(report, bus, p) {
                            format!("{summary} · non câblé")
                        } else {
                            summary
                        }
                    })
                    .collect(),
                internal_ports: internal.len(),
                devices: bus.root_hub.children.iter().map(device_view).collect(),
            }
        })
        .collect();

    Presentation {
        os: report
            .host
            .os_version
            .clone()
            .unwrap_or_else(|| report.host.os.clone()),
        tool_version: report.tool.version.clone(),
        generated_at: report.generated_at,
        summary,
        plugs,
        internal: view.internal.iter().map(|d| device_view(d)).collect(),
        controllers,
        displays: report.displays.iter().map(display_view).collect(),
        usb4: report
            .usb4_routers
            .iter()
            .map(|r| Usb4View {
                name: r.name.clone(),
                role: if r.host {
                    "routeur de l'ordinateur"
                } else {
                    "appareil USB4 branché"
                },
            })
            .collect(),
        typec_ports: report
            .typec_ports
            .iter()
            .map(|p| TypeCView {
                id: p.id.clone(),
                connected: p.partner.is_some(),
                roles: Some(
                    [p.data_role.as_deref(), p.power_role.as_deref()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(", "),
                )
                .filter(|r| !r.is_empty()),
                cable: p.cable.as_ref().and_then(|c| c.emarker).map(|vdo| {
                    let speed = vdo
                        .highest_speed
                        .map(|s| s.label())
                        .unwrap_or("vitesse inconnue");
                    match vdo.max_power_w() {
                        Some(w) => format!("{speed}, {w} W max"),
                        None => speed.to_owned(),
                    }
                }),
            })
            .collect(),
        findings: findings
            .iter()
            .map(|f| FindingView {
                code: f.code,
                level: f.severity.into(),
                confidence: match f.confidence {
                    Confidence::Certain => "certain",
                    Confidence::Likely => "probable",
                    Confidence::Possible => "possible",
                },
                subject: f.subject.clone(),
                title: f.title.clone(),
                detail: f.detail.clone(),
            })
            .collect(),
        link_diagnostics: report.link_diagnostics.as_ref().map(|d| LinkView {
            duration_s: d.duration_s,
            ports: d
                .ports
                .iter()
                .map(|p| LinkPortView {
                    place: p
                        .location
                        .clone()
                        .or_else(|| p.port.map(|n| format!("port {n}")))
                        .unwrap_or_else(|| "port inconnu".to_owned()),
                    name: p
                        .location
                        .as_deref()
                        .and_then(|loc| report.devices().find(|d| d.location == loc))
                        .map(|d| d.display_name())
                        .or_else(|| p.device_name.clone())
                        .or_else(|| match (p.vendor_id, p.product_id) {
                            (Some(v), Some(pid)) => Some(format!("{v:04x}:{pid:04x}")),
                            _ => None,
                        }),
                    ids: match (p.vendor_id, p.product_id) {
                        (Some(v), Some(pid)) => Some(format!("{v:04x}:{pid:04x}")),
                        _ => None,
                    },
                    connections: p.connections,
                    errors: p.link_errors(),
                    detail: p.breakdown(),
                })
                .collect(),
            controller_errors: d.controller_errors,
        }),
        notes: unwired_note(&view.unwired)
            .into_iter()
            .chain(report.notes.iter().cloned())
            .collect(),
    }
}

/// Explique pourquoi des ports annoncés par le système ne sont pas montrés.
pub fn unwired_note(unwired: &[String]) -> Option<String> {
    match unwired {
        [] => None,
        [one] => Some(format!(
            "Le port {one} n'est pas montré parmi les prises : le système le déclare \
             accessible, mais c'est le seul port USB 2.0 d'un contrôleur USB 3, \
             relié à aucune prise sur ce type de machine."
        )),
        many => Some(format!(
            "Les ports {} ne sont pas montrés parmi les prises : le système les déclare \
             accessibles, mais ils ne sont reliés à aucune prise sur ce type de machine.",
            many.join(", ")
        )),
    }
}

fn device_view(device: &Device) -> DeviceView {
    let speed = device.speed.map(|s| s.value);
    let max = device.max_speed.map(|s| s.value);
    let (verdict, verdict_label) = if device.problem.is_some() {
        (
            SpeedVerdict::Problem,
            Some("problème de connexion".to_owned()),
        )
    } else {
        match (speed, max) {
            (Some(s), Some(m)) if !device.is_hub && s < m => (
                SpeedVerdict::Below,
                Some(format!("peut atteindre {}", m.short())),
            ),
            (Some(_), Some(_)) if !device.is_hub => {
                (SpeedVerdict::AtMax, Some("au maximum".to_owned()))
            }
            _ => (SpeedVerdict::Unknown, None),
        }
    };
    DeviceView {
        name: device.display_name(),
        location: device.location.clone(),
        ids: match (device.vendor_id, device.product_id) {
            (Some(v), Some(p)) => Some(format!("{v:04x}:{p:04x}")),
            _ => None,
        },
        is_hub: device.is_hub,
        speed: speed.map(|s| s.short().to_owned()),
        speed_mbps: speed.map(Speed::mbps),
        max: max.map(|m| m.short().to_owned()),
        max_mbps: max.map(Speed::mbps),
        verdict,
        verdict_label,
        facts: facts(device),
        children: device.children.iter().map(device_view).collect(),
    }
}

fn facts(device: &Device) -> Vec<Fact> {
    let mut facts = Vec::new();
    let sourced = |s: Sourced<Speed>| format!("{} ({})", s.value.label(), source(s.source));
    if let Some(s) = device.speed {
        facts.push(Fact {
            label: "Vitesse",
            value: sourced(s),
        });
    }
    if let Some(m) = device.max_speed {
        facts.push(Fact {
            label: "Capacité",
            value: sourced(m),
        });
    }
    if let Some(m) = &device.manufacturer {
        facts.push(Fact {
            label: "Fabricant",
            value: m.clone(),
        });
    }
    if let Some(v) = device.usb_version {
        facts.push(Fact {
            label: "Version USB annoncée",
            value: v.to_string(),
        });
    }
    if let Some(c) = device.device_class {
        facts.push(Fact {
            label: "Classe",
            value: format!("{c:02x}"),
        });
    }
    if let Some(ma) = device.max_power_ma {
        facts.push(Fact {
            label: "Consommation annoncée",
            value: format!("{ma} mA"),
        });
    }
    if let Some(l) = device.lanes {
        facts.push(Fact {
            label: "Voies",
            value: format!("{}×{}", l.rx, l.tx),
        });
    }
    if let Some(port) = &device.port {
        facts.push(Fact {
            label: "Port",
            value: port_summary(port),
        });
    }
    if let Some(problem) = device.problem {
        facts.push(Fact {
            label: "Problème",
            value: problem.label().to_owned(),
        });
    }
    for mode in &device.alt_modes {
        let state = match mode.state {
            AltModeState::Configured => "actif",
            AltModeState::NotAttempted => "non tenté",
            AltModeState::Unsuccessful => "échec",
            AltModeState::Error => "erreur",
        };
        facts.push(Fact {
            label: "Mode alternatif",
            value: format!("{} : {state}", mode.name()),
        });
    }
    facts
}

/// "2-17 · USB-A · USB 3 (5 Gb/s ou plus) · jumeau 2-1".
fn port_summary(port: &PortInfo) -> String {
    let mut parts = vec![
        port.location
            .clone()
            .or_else(|| port.number.map(|n| format!("n°{n}")))
            .unwrap_or_else(|| "?".to_owned()),
    ];
    match port.connector {
        Some(Connector::TypeC) => parts.push("USB-C".to_owned()),
        Some(Connector::TypeA) => parts.push("USB-A".to_owned()),
        Some(Connector::Internal) => parts.push("interne".to_owned()),
        _ => {}
    }
    if let Some(speed) = port.max_speed {
        parts.push(capability_label(speed));
    }
    if let Some(c) = &port.companion {
        parts.push(format!("jumeau {c}"));
    }
    parts.join(" · ")
}

fn source(source: Source) -> &'static str {
    match source {
        Source::Negotiated => "mesurée",
        Source::Declared => "déclarée",
        Source::Inferred => "déduite",
    }
}

fn display_view(d: &Display) -> DisplayView {
    let mode = |w: Option<u32>, h: Option<u32>, hz: Option<f64>| match (w, h, hz) {
        (Some(w), Some(h), Some(hz)) => Some(format!("{w}×{h} à {hz:.0} Hz")),
        (Some(w), Some(h), None) => Some(format!("{w}×{h}")),
        _ => None,
    };
    let below_native = match (d.width, d.height, d.native_width, d.native_height) {
        (Some(w), Some(h), Some(nw), Some(nh)) => {
            u64::from(w) * u64::from(h) < u64::from(nw) * u64::from(nh)
                || matches!((d.refresh_hz, d.native_refresh_hz), (Some(a), Some(b)) if a + 1.0 < b)
        }
        _ => false,
    };
    DisplayView {
        name: d.name.clone().unwrap_or_else(|| {
            if d.connection == Some(treasure_model::DisplayConnection::Internal) {
                "Écran intégré".to_owned()
            } else {
                "Écran".to_owned()
            }
        }),
        internal: d.connection == Some(treasure_model::DisplayConnection::Internal),
        connection: d.connection.map(|c| c.label()),
        mode: mode(d.width, d.height, d.refresh_hz),
        native: mode(d.native_width, d.native_height, d.native_refresh_hz),
        below_native,
        mirrored: d.mirrored,
        transport: d.transport.map(|t| t.label()),
        gpu: d.gpu.clone(),
        link: d.link.as_ref().map(|link| LinkUsage {
            label: link.label(),
            max: match (link.max_lanes, link.max_lane_rate_mbps) {
                (Some(l), Some(r)) => Some(format!(
                    "{l} voie{} {}",
                    if l > 1 { "s" } else { "" },
                    DpLink::rate_name(r)
                )),
                _ => None,
            },
            usage_percent: d
                .current_bandwidth_gbps()
                .map(|needed| (needed / link.capacity_gbps() * 100.0).round() as u32),
            source: link.source.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_windows_report() {
        let json = include_str!("../../../examples/rapport-windows-reel.json");
        let report: Report = serde_json::from_str(json).unwrap();
        let p = present(&report);
        assert_eq!(p.summary.level, Level::Ok);
        assert_eq!(p.plugs.len(), 4);
        assert!(p.notes[0].starts_with("Le port 1-1 n'est pas montré"));
        assert!(
            p.controllers[0]
                .ports
                .iter()
                .any(|s| s.starts_with("1-1") && s.ends_with("non câblé"))
        );
        assert_eq!(p.plugs[0].kind, "usb_c");
        assert_eq!(p.plugs[0].devices[0].name, "TouchScreen");
        assert_eq!(p.internal.len(), 2);
        let anmite = p.displays.iter().find(|d| d.name == "ANMITE").unwrap();
        assert!(anmite.below_native);
        assert_eq!(anmite.link.as_ref().unwrap().usage_percent, Some(75));
        assert_eq!(p.findings.len(), 1);
        assert_eq!(p.link_diagnostics.as_ref().unwrap().ports[0].connections, 2);
        // Sérialisable pour l'interface.
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"kind\":\"usb_c\""));
    }

    #[test]
    fn speed_verdicts() {
        let device = Device {
            location: "1-2".into(),
            speed: Some(Sourced::negotiated(Speed::High)),
            max_speed: Some(Sourced::declared(Speed::Gen2x1)),
            ..Default::default()
        };
        let v = device_view(&device);
        assert_eq!(v.verdict, SpeedVerdict::Below);
        assert_eq!(v.verdict_label.as_deref(), Some("peut atteindre 10 Gb/s"));
    }
}
