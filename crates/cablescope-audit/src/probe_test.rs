//! Test d'un câble avec une sonde au bout (l'application CableScope Probe sur
//! un téléphone) : en plus de la vitesse négociée, le logiciel fait passer
//! des données par le câble dans les deux sens et la sonde rapporte la charge
//! qu'elle reçoit. Voir docs/protocole-sonde.md.

use cablescope_model::{Device, Report};

use crate::Severity;
use crate::cable_test::{
    CableVerdict, Criterion, Outcome, Score, Tone, advice, device_facts, fact, overall_score,
    speed_verdict, transfer_score,
};

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
        let v = CableVerdict {
            outcome: Outcome::LinkDropped,
            severity: Severity::Problem,
            title: "La liaison s'est coupée pendant le test".to_owned(),
            detail: format!(
                "La connexion avec {} a lâché pendant la mesure {during}. Un faux contact ou des fils abîmés dans le câble ou un connecteur en sont la cause la plus probable : refaites le test en bougeant légèrement le câble pour le confirmer.{measured}",
                m.probe_name
            ),
            device: Some(m.probe_name.clone()),
            speed: m.device.and_then(|d| d.speed).map(|s| s.value),
            scores: Vec::new(),
            facts: Vec::new(),
            advice: None,
        };
        return with_scores(v, m);
    }

    let mut verdict = match m.device {
        Some(device) => {
            let mut v = speed_verdict(device, m.link_errors);
            // Le nom USB est souvent générique ("SAMSUNG_Android") : le
            // modèle annoncé par la sonde parle davantage.
            v.detail = v.detail.replace(&device.display_name(), &m.probe_name);
            v.device = Some(m.probe_name.clone());
            v
        }
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
            scores: Vec::new(),
            facts: Vec::new(),
            advice: None,
        },
    };
    verdict.detail.push_str(&measured);
    verdict.detail.push_str(&superspeed_sentence(m));
    verdict.detail.push_str(&charge);
    with_scores(verdict, m)
}

/// Notes (transfert avec le débit mesuré, charge reçue, général), constats
/// rangés et conseil du test avec la sonde.
fn with_scores(mut v: CableVerdict, m: &ProbeMeasurements) -> CableVerdict {
    let max = m.device.and_then(|d| d.max_speed).map(|s| s.value);
    let mut transfer = transfer_score(&v, max, m.link_errors);
    let best = m
        .to_computer_mbps
        .into_iter()
        .chain(m.to_probe_mbps)
        .fold(0.0, f64::max);
    if best > 0.0 {
        let measured = format!("{} mesurés", mbps_label(best));
        transfer.value = Some(match transfer.value {
            Some(speed) => format!("{speed} · {measured}"),
            None => measured,
        });
    }
    let mut scores = vec![transfer];
    if let Some(ma) = m.charge_current_ma {
        let (rating, tone) = match ma {
            1500.. => ("Rapide", Tone::Good),
            500.. => ("Normale", Tone::Good),
            1.. => ("Lente", Tone::Fair),
            _ => ("Nulle", Tone::Unknown),
        };
        let value = if ma > 0 {
            amps_label(ma)
        } else {
            "batterie pleine ?".to_owned()
        };
        scores.push(Score::new(Criterion::Charge, rating, tone, Some(value)));
    }
    let overall = overall_score(&v, &scores);
    scores.push(overall);
    v.scores = scores;

    // Constats : l'appareil sous le nom du modèle, puis les mesures.
    let mut facts = device_facts(m.device, m.link_errors);
    match facts.first_mut() {
        Some(first) if first.label == "Appareil" => first.value = m.probe_name.clone(),
        _ => facts.insert(0, fact("Appareil", m.probe_name.clone())),
    }
    if m.device.is_none() {
        facts.insert(
            1,
            fact("Vitesse obtenue", "inconnue (appareil non retrouvé en USB)"),
        );
    }
    if let Some(v) = m.to_computer_mbps {
        facts.push(fact("Débit vers le PC", mbps_label(v)));
    }
    if let Some(v) = m.to_probe_mbps {
        facts.push(fact("Débit vers le téléphone", mbps_label(v)));
    }
    if let Some(during) = &m.dropped_during {
        facts.push(fact("Coupure", format!("pendant la mesure {during}")));
    }
    if let Some(ma) = m.charge_current_ma {
        let value = if ma > 0 {
            amps_label(ma)
        } else {
            "nulle".to_owned()
        };
        facts.push(fact("Charge reçue", value));
    }
    v.facts = facts;
    v.advice = advice(&v, max, m.link_errors);
    v
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

/// Ce que le débit mesuré prouve sur les fils USB 3 du câble.
fn superspeed_sentence(m: &ProbeMeasurements) -> String {
    let negotiated_superspeed = m
        .device
        .and_then(|d| d.speed)
        .is_some_and(|s| s.value.is_superspeed());
    let best = m
        .to_computer_mbps
        .into_iter()
        .chain(m.to_probe_mbps)
        .fold(0.0, f64::max);
    if !negotiated_superspeed || best == 0.0 {
        String::new()
    } else if best > 480.0 {
        " Plus de 480 Mb/s ont circulé, au-delà de ce que permet l'USB 2.0 : les fils USB 3 du câble fonctionnent réellement.".to_owned()
    } else {
        " Le débit est resté sous celui de l'USB 2.0 malgré le lien USB 3 : c'est souvent la limite du téléphone, mais si un autre câble fait nettement mieux, celui-ci est en cause.".to_owned()
    }
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
    use cablescope_model::{PortInfo, Sourced, Speed};

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
        assert!(
            v.detail.contains("fils USB 3 du câble fonctionnent"),
            "{}",
            v.detail
        );
        assert!(
            v.detail.starts_with("samsung SM-S911B fonctionne"),
            "{}",
            v.detail
        );
        assert_eq!(v.device.as_deref(), Some("samsung SM-S911B"));
        assert_eq!(v.facts[0].value, "samsung SM-S911B");
        assert!(
            v.facts
                .iter()
                .any(|f| f.label == "Débit vers le PC" && f.value == "412 Mb/s")
        );
        assert_eq!(v.advice, None);
        let ratings: Vec<(&str, &str)> = v
            .scores
            .iter()
            .map(|s| (s.rating.as_str(), s.value.as_deref().unwrap_or("")))
            .collect();
        assert_eq!(
            ratings,
            vec![
                ("Excellent", "5 Gb/s · 1,5 Gb/s mesurés"),
                ("Normale", "1,25 A"),
                ("Bon câble", "données et charge"),
            ]
        );
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
