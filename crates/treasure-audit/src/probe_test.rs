//! Test d'un câble avec une sonde au bout (l'application Treasure Probe sur
//! un téléphone) : en plus de la vitesse négociée, le logiciel fait passer
//! des données par le câble dans les deux sens et la sonde rapporte la charge
//! qu'elle reçoit. Voir docs/protocole-sonde.md.

use treasure_model::{Device, Report};

use crate::Severity;
use crate::cable_test::{CableVerdict, Outcome, speed_verdict};

/// Ce qu'a donné une séance de mesures avec la sonde.
#[derive(Debug, Clone, Default)]
pub struct ProbeMeasurements<'a> {
    /// La sonde telle que vue dans l'arbre USB, si elle a été retrouvée.
    pub device: Option<&'a Device>,
    /// Nom annoncé par la sonde ("samsung SM-S911B").
    pub probe_name: String,
    pub to_computer_mbps: Option<f64>,
    pub to_probe_mbps: Option<f64>,
    /// Mesure pendant laquelle la liaison a lâché ("vers l'ordinateur").
    pub dropped_during: Option<String>,
    /// Erreurs de lien relevées pendant les mesures (administrateur).
    pub link_errors: u32,
    /// Courant entrant dans la batterie de la sonde, en milliampères.
    pub charge_current_ma: Option<i64>,
}

/// Retrouve la sonde dans l'arbre USB : Android annonce en USB le même
/// fabricant et le même modèle que ceux que l'application transmet.
pub fn find_probe_device<'a>(
    report: &'a Report,
    manufacturer: &str,
    model: &str,
) -> Option<&'a Device> {
    let same = |a: Option<&str>, b: &str| {
        let b = b.trim();
        !b.is_empty() && a.is_some_and(|a| a.trim().eq_ignore_ascii_case(b))
    };
    let devices: Vec<&Device> = report.devices().filter(|d| !d.is_hub).collect();
    devices
        .iter()
        .find(|d| same(d.product.as_deref(), model))
        .or_else(|| {
            // Le modèle est parfois tronqué ou décoré : un seul appareil du
            // fabricant suffit alors à conclure.
            let mut candidates = devices
                .iter()
                .filter(|d| same(d.manufacturer.as_deref(), manufacturer));
            match (candidates.next(), candidates.next()) {
                (Some(only), None) => Some(only),
                _ => None,
            }
        })
        .copied()
}

pub fn evaluate(m: &ProbeMeasurements) -> CableVerdict {
    let measured = throughput_sentence(m);
    let charge = charge_sentence(m);

    if let Some(during) = &m.dropped_during {
        return CableVerdict {
            outcome: Outcome::LinkDropped,
            severity: Severity::Problem,
            title: "La liaison s'est coupée pendant le test".to_owned(),
            detail: format!(
                "La connexion avec {} a lâché pendant la mesure {during}. Un faux contact ou des fils abîmés dans le câble ou un connecteur en sont la cause la plus probable : refaites le test en bougeant légèrement le câble pour le confirmer.{measured}",
                m.probe_name
            ),
            device: Some(m.probe_name.clone()),
            speed: m.device.and_then(|d| d.speed).map(|s| s.value),
        };
    }

    let mut verdict = match m.device {
        Some(device) => speed_verdict(device, m.link_errors),
        None => CableVerdict {
            outcome: Outcome::Ok,
            severity: if m.link_errors > 0 {
                Severity::Warning
            } else {
                Severity::Info
            },
            title: "Ce câble transporte les données".to_owned(),
            detail: format!(
                "{} n'a pas été retrouvé dans l'arbre USB : la vitesse négociée est inconnue.{}",
                m.probe_name,
                if m.link_errors > 0 {
                    format!(
                        " Attention : {} erreur(s) de lien pendant le test, la connexion est fragile.",
                        m.link_errors
                    )
                } else {
                    String::new()
                }
            ),
            device: Some(m.probe_name.clone()),
            speed: None,
        },
    };
    verdict.detail.push_str(&measured);
    verdict.detail.push_str(&charge);
    verdict
}

fn throughput_sentence(m: &ProbeMeasurements) -> String {
    let parts: Vec<String> = [
        (m.to_computer_mbps, "vers l'ordinateur"),
        (m.to_probe_mbps, "vers le téléphone"),
    ]
    .into_iter()
    .filter_map(|(mbps, way)| mbps.map(|v| format!("{} {way}", mbps_label(v))))
    .collect();
    if parts.is_empty() {
        return String::new();
    }
    format!(
        " Débit mesuré à travers le câble : {} (le téléphone et le partage de connexion le plafonnent ; il montre surtout que la liaison tient sous charge).",
        parts.join(", ")
    )
}

fn charge_sentence(m: &ProbeMeasurements) -> String {
    match m.charge_current_ma {
        Some(ma) if ma > 0 => format!(
            " Charge : {} entrent dans la batterie du téléphone.",
            amps_label(ma)
        ),
        Some(_) => " Le téléphone est alimenté mais sa batterie ne se charge pas (déjà pleine, ou courant trop faible).".to_owned(),
        None => String::new(),
    }
}

fn mbps_label(mbps: f64) -> String {
    if mbps >= 1000.0 {
        format!("{:.1} Gb/s", mbps / 1000.0).replace('.', ",")
    } else {
        format!("{} Mb/s", mbps.round() as u64)
    }
}

fn amps_label(ma: i64) -> String {
    if ma >= 1000 {
        format!("{:.2} A", ma as f64 / 1000.0).replace('.', ",")
    } else {
        format!("{ma} mA")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use treasure_model::{PortInfo, Sourced, Speed};

    fn phone(speed: Speed) -> Device {
        Device {
            location: "2-6".into(),
            manufacturer: Some("samsung".into()),
            product: Some("SM-S911B".into()),
            speed: Some(Sourced::negotiated(speed)),
            max_speed: Some(Sourced::declared(Speed::Gen1x1)),
            port: Some(PortInfo {
                max_speed: Some(Sourced::inferred(Speed::Gen1x1)),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn measures(device: Option<&Device>) -> ProbeMeasurements<'_> {
        ProbeMeasurements {
            device,
            probe_name: "samsung SM-S911B".into(),
            to_computer_mbps: Some(412.4),
            to_probe_mbps: Some(1530.0),
            charge_current_ma: Some(1250),
            ..Default::default()
        }
    }

    #[test]
    fn probe_is_found_by_model_then_by_manufacturer() {
        let json = include_str!("../../../examples/rapport-windows-reel.json");
        let mut report: Report = serde_json::from_str(json).unwrap();
        report.buses[1].root_hub.children.push(phone(Speed::Gen1x1));
        let found = find_probe_device(&report, "samsung", "SM-S911B").unwrap();
        assert_eq!(found.location, "2-6");
        let found = find_probe_device(&report, "Samsung", "Galaxy S23").unwrap();
        assert_eq!(found.location, "2-6");
        assert!(find_probe_device(&report, "Google", "Pixel 8").is_none());
    }

    #[test]
    fn healthy_usb3_cable() {
        let device = phone(Speed::Gen1x1);
        let v = evaluate(&measures(Some(&device)));
        assert_eq!(v.outcome, Outcome::Ok);
        assert!(
            v.detail.contains("412 Mb/s vers l'ordinateur"),
            "{}",
            v.detail
        );
        assert!(
            v.detail.contains("1,5 Gb/s vers le téléphone"),
            "{}",
            v.detail
        );
        assert!(v.detail.contains("1,25 A entrent"), "{}", v.detail);
    }

    #[test]
    fn usb2_cable_with_usb3_phone() {
        let device = phone(Speed::High);
        let v = evaluate(&measures(Some(&device)));
        assert_eq!(v.outcome, Outcome::Usb2Limited);
        assert_eq!(v.severity, Severity::Warning);
    }

    #[test]
    fn dropped_link_is_a_problem() {
        let device = phone(Speed::Gen1x1);
        let m = ProbeMeasurements {
            dropped_during: Some("vers le téléphone".into()),
            to_probe_mbps: None,
            ..measures(Some(&device))
        };
        let v = evaluate(&m);
        assert_eq!(v.outcome, Outcome::LinkDropped);
        assert_eq!(v.severity, Severity::Problem);
        assert!(v.detail.contains("pendant la mesure vers le téléphone"));
    }

    #[test]
    fn phone_not_found_in_usb_tree() {
        let m = ProbeMeasurements {
            link_errors: 2,
            charge_current_ma: Some(-40),
            ..measures(None)
        };
        let v = evaluate(&m);
        assert_eq!(v.severity, Severity::Warning);
        assert!(v.detail.contains("vitesse négociée est inconnue"));
        assert!(v.detail.contains("ne se charge pas"));
    }
}
