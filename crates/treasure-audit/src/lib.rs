//! Règles de diagnostic : transforment un [`Report`] en constats lisibles.
//!
//! Ce crate est volontairement pur (pas d'accès au système) : le même code
//! tourne dans la version portable et, compilé en WebAssembly, sur le site web.

use serde::{Deserialize, Serialize};
use treasure_model::typec::{CableKind, CableSpeed, TypeCPort};
use treasure_model::{AltModeState, ConnectionProblem, Device, Report, Source};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Problem,
}

/// Degré de certitude d'un constat. Treasure Cables distingue toujours ce
/// qu'il sait de ce qu'il suppose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Possible,
    Likely,
    Certain,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    /// Identifiant stable de la règle (sert aussi de clé de traduction).
    pub code: &'static str,
    pub severity: Severity,
    pub confidence: Confidence,
    /// Emplacement de l'appareil ou identifiant du port concerné.
    pub subject: String,
    pub title: String,
    pub detail: String,
}

pub fn audit(report: &Report) -> Vec<Finding> {
    let mut findings = Vec::new();
    for device in report.devices() {
        check_problem(device, &mut findings);
        check_speed(device, &mut findings);
        check_alt_modes(device, &mut findings);
    }
    for port in &report.typec_ports {
        check_cable(port, &mut findings);
    }
    findings.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then(b.confidence.cmp(&a.confidence))
    });
    findings
}

fn check_problem(device: &Device, out: &mut Vec<Finding>) {
    let Some(problem) = device.problem else {
        return;
    };
    let (title, detail) = match problem {
        ConnectionProblem::EnumerationFailed => (
            "L'appareil n'a pas pu être reconnu",
            "Le système n'a pas réussi à lire l'appareil. Un câble défectueux, trop long ou de mauvaise qualité est une cause fréquente : essayez un autre câble, puis un autre port.",
        ),
        ConnectionProblem::NotEnoughPower => (
            "Alimentation insuffisante",
            "L'appareil demande plus de courant que le port ne peut en fournir. Branchez-le sur un port de l'ordinateur ou sur un hub alimenté.",
        ),
        ConnectionProblem::Overcurrent => (
            "Surintensité détectée sur le port",
            "Le port a coupé l'alimentation. Débranchez l'appareil : un câble ou un appareil endommagé peut provoquer un court-circuit.",
        ),
        ConnectionProblem::NotEnoughBandwidth => (
            "Bande passante insuffisante",
            "Trop d'appareils partagent le même contrôleur. Déplacez certains appareils vers un autre port.",
        ),
        ConnectionProblem::HubNestedTooDeeply => (
            "Trop de hubs en cascade",
            "L'USB limite le nombre de hubs chaînés. Branchez l'appareil plus près de l'ordinateur.",
        ),
        ConnectionProblem::GeneralFailure | ConnectionProblem::Other => (
            "Erreur de connexion",
            "Le système signale une erreur sur cette connexion.",
        ),
    };
    out.push(Finding {
        code: "connection_problem",
        severity: Severity::Problem,
        confidence: Confidence::Certain,
        subject: device.location.clone(),
        title: title.to_owned(),
        detail: detail.to_owned(),
    });
}

fn check_speed(device: &Device, out: &mut Vec<Finding>) {
    let (Some(speed), Some(max)) = (device.speed, device.max_speed) else {
        return;
    };
    // Un hub USB 3 apparaît en deux moitiés (USB 2 et USB 3) : sa moitié USB 2
    // fonctionne normalement en High Speed, ce n'est pas un défaut.
    if speed.value >= max.value || device.is_hub {
        return;
    }
    let port_max = device.port.as_ref().and_then(|p| p.max_speed);
    let name = device.display_name();

    if max.value.is_superspeed() && !speed.value.is_superspeed() {
        // L'appareil sait faire de l'USB 3 mais fonctionne en USB 2 : les paires
        // SuperSpeed ne sont pas utilisées.
        let port_is_superspeed = port_max.is_some_and(|p| p.value.is_superspeed());
        let (confidence, cause) = if port_is_superspeed {
            (
                Confidence::Likely,
                "Le port et l'appareil savent faire mieux : le câble est très probablement un câble USB 2.0 (ou ses fils USB 3 sont abîmés).",
            )
        } else if port_max.is_some() {
            (
                Confidence::Certain,
                "Le port lui-même est limité à l'USB 2.0. Branchez l'appareil sur un port USB 3 (souvent bleu, ou marqué SS).",
            )
        } else {
            (
                Confidence::Possible,
                "En cause : le câble (USB 2.0 uniquement), le port, ou un hub intermédiaire.",
            )
        };
        let confidence = if max.source == Source::Inferred {
            confidence.min(Confidence::Possible)
        } else {
            confidence
        };
        out.push(Finding {
            code: "superspeed_device_on_usb2_link",
            severity: Severity::Warning,
            confidence,
            subject: device.location.clone(),
            title: format!(
                "{name} fonctionne en USB 2.0 alors qu'il supporte {}",
                max.value.label()
            ),
            detail: format!("Vitesse actuelle : {}. {cause}", speed.value.label()),
        });
        return;
    }

    // Lien SuperSpeed, mais pas au maximum de l'appareil. Une capacité de port
    // seulement déduite n'est qu'une borne basse : on ne l'accuse pas.
    let (limited_by_port, confidence, cause) = match port_max {
        Some(p) if p.value > speed.value => (
            false,
            Confidence::Likely,
            "Le port supporte davantage : le câble est probablement la limite.",
        ),
        Some(p) if p.source != Source::Inferred => (
            true,
            Confidence::Certain,
            "Le port est la limite, pas le câble.",
        ),
        _ => (
            false,
            Confidence::Possible,
            "En cause : le câble ou le port.",
        ),
    };
    out.push(Finding {
        code: "link_below_device_capability",
        severity: if limited_by_port {
            Severity::Info
        } else {
            Severity::Warning
        },
        confidence,
        subject: device.location.clone(),
        title: format!(
            "{name} fonctionne en {} au lieu de {}",
            speed.value.label(),
            max.value.label()
        ),
        detail: cause.to_owned(),
    });
}

fn check_alt_modes(device: &Device, out: &mut Vec<Finding>) {
    let on_usb2_link = device.speed.is_some_and(|s| !s.value.is_superspeed());
    for mode in &device.alt_modes {
        let state = match mode.state {
            AltModeState::Configured => continue,
            AltModeState::NotAttempted => "n'a pas été tenté",
            AltModeState::Unsuccessful => "a échoué",
            AltModeState::Error => "est en erreur",
        };
        // Un mode alternatif a besoin des paires rapides du câble : s'il échoue
        // alors que l'appareil n'a qu'un lien USB 2, le câble est le suspect n°1.
        let (confidence, cause) = if on_usb2_link {
            (
                Confidence::Likely,
                "L'appareil n'obtient qu'un lien USB 2.0 : le câble ne transporte probablement pas les signaux rapides (câble USB 2.0 ou de charge).",
            )
        } else {
            (
                Confidence::Possible,
                "En cause : un câble sans e-marker ou non prévu pour ce mode, ou un port de l'ordinateur qui ne le gère pas.",
            )
        };
        out.push(Finding {
            code: "alt_mode_not_entered",
            severity: Severity::Warning,
            confidence,
            subject: device.location.clone(),
            title: format!(
                "Le mode {} de {} {state}",
                mode.name(),
                device.display_name()
            ),
            detail: cause.to_owned(),
        });
    }
}

fn check_cable(port: &TypeCPort, out: &mut Vec<Finding>) {
    let Some(cable) = &port.cable else {
        if port.partner.is_some() {
            out.push(Finding {
                code: "cable_without_emarker",
                severity: Severity::Info,
                confidence: Confidence::Possible,
                subject: port.id.clone(),
                title: "Câble USB-C sans e-marker détecté (ou non visible)".to_owned(),
                detail: "Un câble sans puce e-marker est limité à 3 A (60 W). Il peut aussi s'agir d'un câble à e-marker que ce contrôleur ne rapporte pas.".to_owned(),
            });
        }
        return;
    };
    let Some(vdo) = cable.emarker else { return };

    if vdo.highest_speed == Some(CableSpeed::Usb2) {
        out.push(Finding {
            code: "cable_usb2_only",
            severity: Severity::Info,
            confidence: Confidence::Certain,
            subject: port.id.clone(),
            title: "Le câble USB-C est limité à l'USB 2.0 (480 Mb/s)".to_owned(),
            detail: "D'après sa puce e-marker, ce câble ne transporte pas de données rapides : il convient à la charge, pas à un SSD, un dock ou un écran.".to_owned(),
        });
    } else if let Some(speed) = vdo.highest_speed {
        out.push(Finding {
            code: "cable_declared_speed",
            severity: Severity::Info,
            confidence: Confidence::Certain,
            subject: port.id.clone(),
            title: format!("Câble USB-C déclaré {}", speed.label()),
            detail: "Valeur annoncée par la puce e-marker du câble, pas un débit mesuré."
                .to_owned(),
        });
    }

    if let Some(watts) = vdo.max_power_w() {
        let kind = match cable.kind {
            Some(CableKind::Active) => "actif",
            Some(CableKind::Passive) => "passif",
            None => "de type inconnu",
        };
        out.push(Finding {
            code: "cable_declared_power",
            severity: Severity::Info,
            confidence: Confidence::Certain,
            subject: port.id.clone(),
            title: format!("Câble {kind} prévu pour {watts} W au maximum"),
            detail: format!(
                "Courant maximal annoncé : {} A.",
                vdo.max_current_ma.unwrap_or_default() / 1000
            ),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use treasure_model::typec::{Cable, CableVdo, Partner};
    use treasure_model::{
        AltMode, Bus, HostInfo, PortInfo, SCHEMA_VERSION, Sourced, Speed, ToolInfo,
    };

    fn report_with(device: Device) -> Report {
        Report {
            schema_version: SCHEMA_VERSION,
            tool: ToolInfo {
                name: "t".into(),
                version: "0".into(),
            },
            host: HostInfo {
                os: "test".into(),
                os_version: None,
            },
            generated_at: None,
            buses: vec![Bus {
                id: "usb1".into(),
                controller: None,
                root_hub: Device {
                    location: "usb1".into(),
                    is_hub: true,
                    children: vec![device],
                    ..Default::default()
                },
            }],
            typec_ports: vec![],
            notes: vec![],
        }
    }

    fn ssd(speed: Speed, port_max: Option<Speed>) -> Device {
        Device {
            location: "1-2".into(),
            product: Some("SSD".into()),
            speed: Some(Sourced::negotiated(speed)),
            max_speed: Some(Sourced::declared(Speed::Gen2x1)),
            port: Some(PortInfo {
                max_speed: port_max.map(Sourced::declared),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn usb3_device_at_high_speed_on_usb3_port_blames_cable() {
        let findings = audit(&report_with(ssd(Speed::High, Some(Speed::Gen2x1))));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].code, "superspeed_device_on_usb2_link");
        assert_eq!(findings[0].confidence, Confidence::Likely);
        assert!(findings[0].detail.contains("câble"));
    }

    #[test]
    fn usb3_device_on_usb2_port_blames_port() {
        let findings = audit(&report_with(ssd(Speed::High, Some(Speed::High))));
        assert_eq!(findings[0].confidence, Confidence::Certain);
        assert!(findings[0].detail.contains("port lui-même"));
    }

    #[test]
    fn gen1_link_on_gen1_port_is_only_info() {
        let findings = audit(&report_with(ssd(Speed::Gen1x1, Some(Speed::Gen1x1))));
        assert_eq!(findings[0].code, "link_below_device_capability");
        assert_eq!(findings[0].severity, Severity::Info);
    }

    #[test]
    fn gen1_link_on_port_with_inferred_capability_is_only_possible() {
        let mut device = ssd(Speed::Gen1x1, None);
        device.port = Some(PortInfo {
            max_speed: Some(Sourced::inferred(Speed::Gen1x1)),
            ..Default::default()
        });
        let findings = audit(&report_with(device));
        assert_eq!(findings[0].confidence, Confidence::Possible);
    }

    #[test]
    fn failed_displayport_on_usb2_link_blames_cable() {
        let mut device = ssd(Speed::High, None);
        device.max_speed = None;
        device.alt_modes = vec![AltMode {
            svid: 0xff01,
            state: AltModeState::Unsuccessful,
        }];
        let findings = audit(&report_with(device));
        assert_eq!(findings[0].code, "alt_mode_not_entered");
        assert_eq!(findings[0].confidence, Confidence::Likely);
        assert!(findings[0].title.contains("DisplayPort"));
    }

    #[test]
    fn full_speed_link_is_silent() {
        let findings = audit(&report_with(ssd(Speed::Gen2x1, Some(Speed::Gen2x1))));
        assert!(findings.is_empty());
    }

    #[test]
    fn usb2_emarker_cable_is_reported() {
        let mut report = report_with(Device::default());
        report.typec_ports.push(TypeCPort {
            id: "port0".into(),
            partner: Some(Partner::default()),
            cable: Some(Cable {
                kind: Some(CableKind::Passive),
                emarker: Some(CableVdo::decode(0b10 << 5)),
                ..Default::default()
            }),
            ..Default::default()
        });
        let codes: Vec<_> = audit(&report).iter().map(|f| f.code).collect();
        assert!(codes.contains(&"cable_usb2_only"));
        assert!(codes.contains(&"cable_declared_power"));
    }
}
