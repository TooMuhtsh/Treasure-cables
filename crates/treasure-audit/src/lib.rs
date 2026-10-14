//! Règles de diagnostic : transforment un [`Report`] en constats lisibles.
//!
//! Ce crate est volontairement pur (pas d'accès au système) : le même code
//! tourne dans la version portable et, compilé en WebAssembly, sur le site web.

use serde::{Deserialize, Serialize};
use treasure_model::typec::{CableKind, CableSpeed, TypeCPort};
use treasure_model::{
    AltModeState, ConnectionProblem, Device, Display, DisplayTransport, DpLink, LinkDiagnostics,
    Report, Source,
};

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
    for display in &report.displays {
        check_display(display, &mut findings);
    }
    if let Some(diagnostics) = &report.link_diagnostics {
        check_link_diagnostics(report, diagnostics, &mut findings);
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

/// Écran externe en dessous de son mode natif : réglage volontaire, ou
/// liaison (câble, adaptateur, dock) qui ne transporte pas assez de débit.
fn check_display(display: &Display, out: &mut Vec<Finding>) {
    let Some(connection) = display.connection else {
        return;
    };
    if !connection.is_external_cable() {
        return;
    }
    let (Some(w), Some(h), Some(nw), Some(nh)) = (
        display.width,
        display.height,
        display.native_width,
        display.native_height,
    ) else {
        return;
    };
    let lower_resolution = u64::from(w) * u64::from(h) < u64::from(nw) * u64::from(nh);
    let lower_refresh = match (display.refresh_hz, display.native_refresh_hz) {
        (Some(hz), Some(native)) => hz + 1.0 < native,
        _ => false,
    };
    if !lower_resolution && !lower_refresh {
        return;
    }
    let mode = |w: u32, h: u32, hz: Option<f64>| match hz {
        Some(hz) => format!("{w}×{h} à {hz:.0} Hz"),
        None => format!("{w}×{h}"),
    };
    let name = display.name.as_deref().unwrap_or("L'écran externe");
    let required = display.native_bandwidth_gbps().unwrap_or(0.0);
    let gbps = format!("{required:.1}").replace('.', ",");
    // Repères (débit utile) : DisplayPort 2 voies HBR = 4,32 Gb/s ;
    // 2 voies HBR2 (dock USB-C qui garde 2 voies pour l'USB 3) = 8,64 Gb/s ;
    // HDMI 1.4 = 8,16 Gb/s.
    let (severity, confidence, detail) = if display.mirrored {
        (
            Severity::Info,
            Confidence::Likely,
            "L'affichage est dupliqué : Windows applique la même résolution à tous les écrans. Choisissez « Étendre » ou « Deuxième écran uniquement » (touches Windows + P) pour retrouver le mode natif. Le câble n'est pas en cause.".to_owned(),
        )
    } else if let Some(link) = &display.link {
        // Le lien réel est connu : il tranche.
        if link.capacity_gbps() >= required {
            return; // le lien transporte le mode natif : c'est un réglage
        }
        let mut causes = Vec::new();
        if let Some(max) = link.max_lanes
            && link.lanes < max
        {
            causes.push(if display.transport == Some(DisplayTransport::UsbC) {
                format!(
                    "seulement {} voies sur {max} : en USB-C, les autres sont réservées à l'USB 3 (dock ou écran avec hub USB 3) ou absentes du câble",
                    link.lanes
                )
            } else {
                format!("seulement {} voies sur {max}", link.lanes)
            });
        }
        if let Some(max) = link.max_lane_rate_mbps
            && link.lane_rate_mbps < max
        {
            causes.push(format!(
                "débit par voie {} au lieu de {} : l'entraînement du lien a échoué au débit supérieur, signe d'un câble ou d'un adaptateur de qualité insuffisante",
                DpLink::rate_name(link.lane_rate_mbps),
                DpLink::rate_name(max)
            ));
        }
        let causes = if causes.is_empty() {
            String::new()
        } else {
            format!(" En cause : {}.", causes.join(" ; "))
        };
        (
            Severity::Warning,
            Confidence::Likely,
            format!(
                "Le lien DisplayPort négocié ({}) ne transporte pas les {gbps} Gb/s du mode natif.{causes}",
                link.label()
            ),
        )
    } else if required <= 4.32 {
        // N'importe quelle liaison transporte ce mode : c'est un réglage
        // choisi dans Windows, pas un sujet de câble. On ne dit rien.
        return;
    } else if required <= 8.64 {
        (
            Severity::Info,
            Confidence::Possible,
            format!(
                "Le mode natif demande environ {gbps} Gb/s, ce qu'une liaison même basique transporte. Vérifiez d'abord la résolution choisie dans Windows ; sinon, suspectez un adaptateur ou un câble ancien. Liaison : {}.",
                connection.label()
            ),
        )
    } else {
        (
            Severity::Warning,
            Confidence::Possible,
            format!(
                "Le mode natif demande environ {gbps} Gb/s : plus qu'un DisplayPort à 2 voies HBR2 (8,6 Gb/s, cas d'un dock USB-C qui garde 2 voies pour l'USB 3) ou qu'un HDMI 1.4 (8,2 Gb/s). Si ce n'est pas un réglage de Windows, le câble, l'adaptateur ou le dock limite le débit. Liaison : {}.",
                connection.label()
            ),
        )
    };
    out.push(Finding {
        code: "display_below_native_mode",
        severity,
        confidence,
        subject: name.to_owned(),
        title: format!(
            "{name} fonctionne en {} au lieu de son mode natif {}",
            mode(w, h, display.refresh_hz),
            mode(nw, nh, display.native_refresh_hz)
        ),
        detail,
    });
}

/// Bilan d'une écoute de la pile USB : les erreurs de lien sont la preuve
/// la plus directe d'un câble ou d'un connecteur défaillant.
fn check_link_diagnostics(report: &Report, diagnostics: &LinkDiagnostics, out: &mut Vec<Finding>) {
    let mut total_errors = 0;
    for port in &diagnostics.ports {
        let subject = port
            .location
            .clone()
            .or_else(|| port.port.map(|p| format!("port {p}")))
            .unwrap_or_else(|| "port inconnu".to_owned());
        let name = port
            .location
            .as_deref()
            .and_then(|loc| report.devices().find(|d| d.location == loc))
            .map(|d| d.display_name())
            .or_else(|| match (port.vendor_id, port.product_id) {
                (Some(v), Some(p)) => Some(format!("{v:04x}:{p:04x}")),
                _ => None,
            });
        let errors = port.link_errors();
        total_errors += errors;
        if errors > 0 {
            let mut details = Vec::new();
            for (count, label) in [
                (port.enumeration_errors, "erreur(s) de reconnaissance"),
                (port.port_errors, "erreur(s) de port"),
                (port.transfer_errors, "erreur(s) de transfert"),
                (port.hub_resets, "réinitialisation(s) de hub"),
            ] {
                if count > 0 {
                    details.push(format!("{count} {label}"));
                }
            }
            let never_recognized = port.connections == 0 && port.enumeration_errors > 0;
            let outcome = if never_recognized {
                "L'appareil n'a jamais été reconnu."
            } else if port.connections > 0 {
                "L'appareil a fini par être reconnu, mais la connexion est instable."
            } else {
                ""
            };
            out.push(Finding {
                code: "link_errors",
                severity: if never_recognized { Severity::Problem } else { Severity::Warning },
                confidence: if errors >= 3 { Confidence::Likely } else { Confidence::Possible },
                subject: subject.clone(),
                title: match &name {
                    Some(n) => format!("Lien USB instable sur {subject} ({n})"),
                    None => format!("Lien USB instable sur {subject}"),
                },
                detail: format!(
                    "Pendant l'écoute : {}. {outcome} Causes habituelles : câble abîmé ou trop long, connecteur encrassé ou desserré. Essayez un autre câble : si les erreurs suivent le câble, c'est lui.",
                    details.join(", ")
                ),
            });
        }
        if port.descriptor_errors > 0 {
            out.push(Finding {
                code: "descriptor_errors",
                severity: Severity::Info,
                confidence: Confidence::Certain,
                subject: subject.clone(),
                title: format!(
                    "{} annonce des descripteurs non conformes",
                    name.as_deref().unwrap_or("L'appareil")
                ),
                detail: "Défaut du micrologiciel de l'appareil, pas du câble. Il fonctionne souvent malgré tout.".to_owned(),
            });
        }
    }
    if diagnostics.controller_errors > 0 {
        out.push(Finding {
            code: "controller_errors",
            severity: Severity::Warning,
            confidence: Confidence::Possible,
            subject: "contrôleur USB".to_owned(),
            title: format!(
                "Le contrôleur USB a signalé {} erreur(s) ou réinitialisation(s)",
                diagnostics.controller_errors
            ),
            detail: "Un appareil ou un câble défaillant peut perturber tout le contrôleur. Si le problème persiste, mettez à jour le BIOS et les pilotes du chipset.".to_owned(),
        });
    }
    if total_errors == 0 && diagnostics.controller_errors == 0 {
        let connections: u32 = diagnostics.ports.iter().map(|p| p.connections).sum();
        out.push(Finding {
            code: "link_clean",
            severity: Severity::Info,
            confidence: Confidence::Certain,
            subject: "diagnostic approfondi".to_owned(),
            title: format!(
                "Aucune erreur de lien USB pendant {} s d'écoute",
                diagnostics.duration_s
            ),
            detail: format!(
                "{connections} reconnaissance(s) d'appareil réussie(s). Les câbles et connecteurs utilisés pendant l'écoute n'ont montré aucun signe de faiblesse."
            ),
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
        AltMode, Bus, DisplayConnection, DisplayTransport, DpLink, HostInfo, PortActivity,
        PortInfo, SCHEMA_VERSION, Sourced, Speed, ToolInfo,
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
            displays: vec![],
            usb4_routers: vec![],
            link_diagnostics: None,
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
    fn external_display_below_native_refresh_is_flagged() {
        let mut report = report_with(Device::default());
        let display = Display {
            name: Some("DELL U2723QE".into()),
            connection: Some(DisplayConnection::DisplayPort),
            width: Some(3840),
            height: Some(2160),
            refresh_hz: Some(30.0),
            native_width: Some(3840),
            native_height: Some(2160),
            native_refresh_hz: Some(60.0),
            mirrored: false,
            gpu: None,
            transport: None,
            link: None,
        };
        report.displays.push(display.clone());
        report.displays.push(Display {
            refresh_hz: Some(59.997),
            ..display
        });
        let findings = audit(&report);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].code, "display_below_native_mode");
        assert!(findings[0].title.contains("30 Hz"));
        // 4K à 60 Hz demande ~12,9 Gb/s : la liaison peut être en cause.
        assert_eq!(findings[0].severity, Severity::Warning);
    }

    /// Rapport réel (Windows 11, portable Intel + RTX 3060, écran USB-C
    /// ANMITE réglé volontairement sous son mode natif, diagnostic approfondi
    /// sans erreur) : seule la confirmation du lien sain doit ressortir.
    #[test]
    fn real_windows_report_has_no_false_positive() {
        let json = include_str!("../../../examples/rapport-windows-reel.json");
        let report: Report = serde_json::from_str(json).unwrap();
        let codes: Vec<_> = audit(&report).iter().map(|f| f.code).collect();
        assert_eq!(codes, vec!["link_clean"]);
    }

    #[test]
    fn link_diagnostics_findings() {
        let mut report = report_with(ssd(Speed::Gen1x1, None));
        report.link_diagnostics = Some(LinkDiagnostics {
            duration_s: 30,
            ports: vec![PortActivity {
                location: Some("1-2".into()),
                connections: 1,
                enumeration_errors: 3,
                ..Default::default()
            }],
            controller_errors: 0,
        });
        let findings = audit(&report);
        let f = findings.iter().find(|f| f.code == "link_errors").unwrap();
        assert_eq!(f.confidence, Confidence::Likely);
        assert_eq!(f.severity, Severity::Warning);
        assert!(f.title.contains("SSD"));
        assert!(f.detail.contains("instable"));

        report.link_diagnostics = Some(LinkDiagnostics {
            duration_s: 30,
            ports: vec![PortActivity {
                connections: 2,
                ..Default::default()
            }],
            controller_errors: 0,
        });
        let codes: Vec<_> = audit(&report).iter().map(|f| f.code).collect();
        assert!(codes.contains(&"link_clean"));
        assert!(!codes.contains(&"link_errors"));
    }

    #[test]
    fn known_link_decides_display_findings() {
        let display = Display {
            name: Some("Écran 4K".into()),
            connection: Some(DisplayConnection::DisplayPort),
            width: Some(3840),
            height: Some(2160),
            refresh_hz: Some(30.0),
            native_width: Some(3840),
            native_height: Some(2160),
            native_refresh_hz: Some(60.0),
            mirrored: false,
            gpu: None,
            transport: Some(DisplayTransport::UsbC),
            link: Some(DpLink {
                lane_rate_mbps: 5400,
                lanes: 2,
                max_lane_rate_mbps: Some(5400),
                max_lanes: Some(4),
                source: "test".into(),
            }),
        };
        let mut report = report_with(Device::default());
        report.displays.push(display);
        // 2 voies HBR2 = 8,64 Gb/s < ~12,9 Gb/s : la liaison limite.
        let findings = audit(&report);
        assert_eq!(findings[0].confidence, Confidence::Likely);
        assert!(findings[0].detail.contains("2 voies sur 4"));
        assert!(findings[0].detail.contains("USB 3"));

        // 4 voies HBR2 = 17,28 Gb/s : le mode natif passe, c'est un réglage.
        report.displays[0].link.as_mut().unwrap().lanes = 4;
        assert!(audit(&report).is_empty());
    }

    #[test]
    fn low_bandwidth_display_mode_is_silent_unless_mirrored() {
        // Cas réel : écran 1920×1200 volontairement réglé en 1920×1080.
        let display = Display {
            name: Some("ANMITE".into()),
            connection: Some(DisplayConnection::DisplayPort),
            width: Some(1920),
            height: Some(1080),
            refresh_hz: Some(60.0),
            native_width: Some(1920),
            native_height: Some(1200),
            native_refresh_hz: Some(60.0),
            mirrored: false,
            gpu: None,
            transport: None,
            link: None,
        };
        let mut report = report_with(Device::default());
        report.displays.push(display);
        assert!(audit(&report).is_empty());

        report.displays[0].mirrored = true;
        let findings = audit(&report);
        assert!(findings[0].detail.contains("dupliqué"));
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
