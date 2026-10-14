//! Test d'un câble : on compare la machine avant et après le branchement.
//!
//! Le PC sait que l'utilisateur est censé brancher un appareil : l'absence
//! de réaction devient une preuve (câble de charge seule), et la vitesse
//! obtenue se compare à ce que savent faire l'appareil et la prise.

use cablescope_model::{Device, LinkDiagnostics, Report, Speed};
use serde::{Deserialize, Serialize};

use crate::Severity;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Rien n'est apparu : pas de fils de données.
    NoData,
    /// Un appareil a été vu mais pas reconnu.
    NotRecognized,
    /// L'appareil sait faire de l'USB 3, la prise aussi, le lien est en USB 2.
    Usb2Limited,
    /// Lien SuperSpeed, mais sous la capacité de l'appareil.
    BelowCapability,
    /// La prise utilisée est elle-même limitée : le test ne peut pas conclure.
    PortLimited,
    /// La liaison s'est coupée pendant une mesure (test avec un téléphone).
    LinkDropped,
    Ok,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CableVerdict {
    pub outcome: Outcome,
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    /// Appareil qui a servi au test.
    pub device: Option<String>,
    pub speed: Option<Speed>,
    /// Notes en bref (transfert, charge, général), pour lire et comparer
    /// d'un coup d'œil ; `detail` garde l'explication complète.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scores: Vec<Score>,
    /// Le détail, rangé : une ligne par constat (« Vitesse obtenue · 5 Gb/s »).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facts: Vec<VerdictFact>,
    /// Ce qu'il faut faire, en une phrase, s'il y a quelque chose à faire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advice: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerdictFact {
    pub label: String,
    pub value: String,
}

pub(crate) fn fact(label: &str, value: impl Into<String>) -> VerdictFact {
    VerdictFact {
        label: label.to_owned(),
        value: value.into(),
    }
}

/// Critère noté.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Criterion {
    Transfer,
    Charge,
    Overall,
}

/// Teinte de la pastille.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    Good,
    Fair,
    Bad,
    Unknown,
}

/// Une note : « Transfert · Excellent · 5 Gb/s ».
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub criterion: Criterion,
    pub rating: String,
    pub tone: Tone,
    /// Chiffre qui justifie la note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

impl Score {
    pub fn new(criterion: Criterion, rating: &str, tone: Tone, value: Option<String>) -> Self {
        Score {
            criterion,
            rating: rating.to_owned(),
            tone,
            value,
        }
    }
}

/// Appareils apparus entre les deux relevés (les plus hauts dans l'arbre :
/// un dock et ses appareils comptent pour un).
pub fn new_devices<'a>(before: &Report, after: &'a Report) -> Vec<&'a Device> {
    let known: Vec<(&str, Option<u16>, Option<u16>)> = before
        .devices()
        .map(|d| (d.location.as_str(), d.vendor_id, d.product_id))
        .collect();
    let is_new = |d: &Device| !known.contains(&(d.location.as_str(), d.vendor_id, d.product_id));
    let mut found = Vec::new();
    for bus in &after.buses {
        collect_new(&bus.root_hub.children, &is_new, &mut found);
    }
    found
}

fn collect_new<'a>(
    devices: &'a [Device],
    is_new: &dyn Fn(&Device) -> bool,
    out: &mut Vec<&'a Device>,
) {
    for device in devices {
        if is_new(device) {
            out.push(device);
        } else {
            collect_new(&device.children, is_new, out);
        }
    }
}

pub fn evaluate(before: &Report, after: &Report, link: Option<&LinkDiagnostics>) -> CableVerdict {
    let errors: u32 = link
        .map(|l| l.ports.iter().map(|p| p.link_errors()).sum())
        .unwrap_or(0);

    let new = new_devices(before, after);
    let Some(device) = new
        .iter()
        .find(|d| d.problem.is_none())
        .or(new.first())
        .copied()
    else {
        let v = if errors > 0 {
            CableVerdict {
                outcome: Outcome::NotRecognized,
                severity: Severity::Problem,
                title: "L'appareil n'a pas pu être reconnu".to_owned(),
                detail: format!(
                    "Le PC a vu un branchement mais {errors} tentative(s) de reconnaissance ont échoué : câble défaillant, trop long ou mal serti, ou connecteur encrassé."
                ),
                device: None,
                speed: None,
                scores: Vec::new(),
                facts: Vec::new(),
                advice: None,
            }
        } else {
            CableVerdict {
                outcome: Outcome::NoData,
                severity: Severity::Problem,
                title: "Rien n'a été détecté : ce câble ne transporte pas les données".to_owned(),
                detail: "C'est probablement un câble de charge seule (très courant), ou ses fils de données sont coupés. Un appareil USB se signale au PC dès son branchement, même un téléphone verrouillé. Pour en être sûr, vérifiez que l'appareil est bien détecté avec un autre câble sur la même prise.".to_owned(),
                device: None,
                speed: None,
                scores: Vec::new(),
                facts: Vec::new(),
                advice: None,
            }
        };
        return scored(v, None, None, errors);
    };

    speed_verdict(device, errors)
}

/// Note de transfert d'après le verdict : vitesse obtenue, comparée à ce
/// que sait faire l'appareil.
pub fn transfer_score(v: &CableVerdict, max: Option<Speed>, errors: u32) -> Score {
    let speed = v.speed;
    let short = |s: Option<Speed>| s.map(|s| s.short().to_owned());
    let instead = || match (speed, max) {
        (Some(s), Some(m)) => Some(format!("{} au lieu de {}", s.short(), m.short())),
        _ => short(speed),
    };
    let (rating, tone, value) = match v.outcome {
        Outcome::NoData => ("Aucun", Tone::Bad, None),
        Outcome::NotRecognized => ("Défaillant", Tone::Bad, None),
        Outcome::LinkDropped => ("Instable", Tone::Bad, short(speed)),
        Outcome::Usb2Limited => ("Limité", Tone::Fair, instead()),
        Outcome::BelowCapability => ("Moyen", Tone::Fair, instead()),
        Outcome::PortLimited => ("À confirmer", Tone::Unknown, short(speed)),
        Outcome::Ok if errors > 0 => ("Instable", Tone::Fair, short(speed)),
        Outcome::Ok => match speed {
            Some(s) if s.is_superspeed() && max.is_none_or(|m| s >= m) => {
                ("Excellent", Tone::Good, short(speed))
            }
            Some(s) if !s.is_superspeed() => (
                "Bon",
                Tone::Good,
                Some(format!("{} (appareil USB 2)", s.short())),
            ),
            _ => ("Bon", Tone::Good, short(speed)),
        },
    };
    Score::new(Criterion::Transfer, rating, tone, value)
}

/// Note générale, déduite des autres.
pub fn overall_score(v: &CableVerdict, scores: &[Score]) -> Score {
    let transfer = scores
        .iter()
        .find(|s| s.criterion == Criterion::Transfer)
        .map_or(Tone::Unknown, |s| s.tone);
    let (rating, tone, value) = match (v.outcome, transfer) {
        (Outcome::NoData, _) => ("Charge seulement", Tone::Fair, "pas de données"),
        (_, Tone::Bad) => ("À éviter", Tone::Bad, "liaison défaillante"),
        (Outcome::Usb2Limited, _) => ("Usage limité", Tone::Fair, "charge et petits appareils"),
        (_, Tone::Fair) => ("Usage limité", Tone::Fair, "liaison à surveiller"),
        (_, Tone::Unknown) => ("À confirmer", Tone::Unknown, "refaire sur une prise USB 3"),
        (_, Tone::Good) => ("Bon câble", Tone::Good, "données et charge"),
    };
    Score::new(Criterion::Overall, rating, tone, Some(value.to_owned()))
}

/// Ajoute au verdict ses notes, ses constats et son conseil.
fn scored(
    mut v: CableVerdict,
    device: Option<&Device>,
    max: Option<Speed>,
    errors: u32,
) -> CableVerdict {
    let transfer = transfer_score(&v, max, errors);
    let overall = overall_score(&v, std::slice::from_ref(&transfer));
    v.scores = vec![transfer, overall];
    v.facts = device_facts(device, errors);
    v.advice = advice(&v, max, errors);
    v
}

/// Constats sur l'appareil qui a servi au test.
pub(crate) fn device_facts(device: Option<&Device>, errors: u32) -> Vec<VerdictFact> {
    let mut facts = Vec::new();
    if let Some(d) = device {
        facts.push(fact("Appareil", d.display_name()));
        if let Some(s) = d.speed {
            facts.push(fact("Vitesse obtenue", s.value.label()));
        }
        if let Some(m) = d.max_speed {
            facts.push(fact("Maximum de l'appareil", m.value.label()));
        }
        if let Some(p) = d.port.as_ref().and_then(|p| p.max_speed) {
            facts.push(fact("Prise", cablescope_model::view::capability_label(p)));
        }
    }
    if errors > 0 {
        facts.push(fact("Erreurs de lien", errors.to_string()));
    }
    facts
}

/// Le conseil, en une phrase, d'après l'issue du test.
pub fn advice(v: &CableVerdict, max: Option<Speed>, errors: u32) -> Option<String> {
    let text = match v.outcome {
        Outcome::NoData => {
            "Câble de charge seule probable : vérifiez que l'appareil est détecté avec un autre câble sur la même prise."
        }
        Outcome::NotRecognized => {
            "Essayez une autre prise, puis un autre câble : si le problème suit le câble, c'est lui."
        }
        Outcome::Usb2Limited => {
            "Bien pour charger et pour les petits appareils, pas pour un SSD ou un dock."
        }
        Outcome::BelowCapability => {
            "Le câble ou la prise limite le débit : essayez une autre prise pour les départager."
        }
        Outcome::PortLimited => {
            "Refaites le test sur une prise USB 3 pour savoir si le câble gère l'USB 3."
        }
        Outcome::LinkDropped => {
            "Refaites le test en bougeant légèrement le câble : si la liaison lâche encore, c'est un faux contact."
        }
        Outcome::Ok if errors > 0 => {
            "Connexion fragile : essayez un autre câble ou nettoyez les connecteurs."
        }
        Outcome::Ok
            if v.speed.is_some_and(|s| !s.is_superspeed())
                && max.is_none_or(|m| !m.is_superspeed()) =>
        {
            "Pour savoir si le câble gère l'USB 3, refaites le test avec un appareil USB 3 (SSD, clé USB 3)."
        }
        Outcome::Ok => return None,
    };
    Some(text.to_owned())
}

/// Verdict sur la vitesse obtenue par un appareil reconnu ; `errors` compte
/// les erreurs de lien relevées pendant le test.
pub fn speed_verdict(device: &Device, errors: u32) -> CableVerdict {
    scored(
        speed_verdict_text(device, errors),
        Some(device),
        device.max_speed.map(|m| m.value),
        errors,
    )
}

fn speed_verdict_text(device: &Device, errors: u32) -> CableVerdict {
    let fragile = if errors > 0 {
        format!(
            " Attention : {errors} erreur(s) de lien pendant le test, la connexion est fragile."
        )
    } else {
        String::new()
    };
    let name = device.display_name();
    let base = |outcome, severity, title: String, detail: String| CableVerdict {
        outcome,
        severity,
        title,
        detail,
        device: Some(name.clone()),
        speed: device.speed.map(|s| s.value),
        scores: Vec::new(),
        facts: Vec::new(),
        advice: None,
    };

    if device.problem.is_some() {
        return base(
            Outcome::NotRecognized,
            Severity::Problem,
            format!("{name} est apparu mais n'a pas été reconnu"),
            "Le PC voit l'appareil sans parvenir à l'utiliser : câble défaillant, trop long ou mal serti, ou connecteur encrassé. Essayez une autre prise pour écarter la prise.".to_owned(),
        );
    }
    let Some(speed) = device.speed.map(|s| s.value) else {
        return base(
            Outcome::Ok,
            Severity::Info,
            "Ce câble transporte les données".to_owned(),
            format!("{name} a été détecté.{fragile}"),
        );
    };
    let max = device.max_speed.map(|m| m.value);
    let port_superspeed = device
        .port
        .as_ref()
        .and_then(|p| p.max_speed)
        .is_some_and(|p| p.value.is_superspeed());

    if let Some(max) = max
        && max.is_superspeed()
        && !speed.is_superspeed()
    {
        return if port_superspeed {
            base(
                Outcome::Usb2Limited,
                Severity::Warning,
                "Ce câble est limité à l'USB 2.0 (480 Mb/s)".to_owned(),
                format!(
                    "{name} sait faire {} et la prise est USB 3, mais le lien s'est établi en {} : le câble n'a pas les fils USB 3 (ou ils sont abîmés). Il convient à la charge et aux petits appareils, pas à un SSD ou un dock.{fragile}",
                    max.label(),
                    speed.label()
                ),
            )
        } else {
            base(
                Outcome::PortLimited,
                Severity::Info,
                "La prise utilisée est limitée à l'USB 2.0".to_owned(),
                format!(
                    "Le câble transporte les données ({}), mais la prise ne permet pas de vérifier s'il supporte l'USB 3. Refaites le test sur une prise USB 3.{fragile}",
                    speed.label()
                ),
            )
        };
    }
    if let Some(max) = max
        && speed < max
    {
        return base(
            Outcome::BelowCapability,
            if errors > 0 {
                Severity::Warning
            } else {
                Severity::Info
            },
            format!("Lien établi à {} au lieu de {}", speed.short(), max.short()),
            format!(
                "{name} sait faire {} mais fonctionne en {} : le câble (ou la prise) limite le débit.{fragile}",
                max.label(),
                speed.label()
            ),
        );
    }

    let detail = if max.is_some_and(|m| m.is_superspeed()) {
        format!(
            "{name} fonctionne en {}, son maximum.{fragile}",
            speed.label()
        )
    } else {
        format!(
            "{name} fonctionne en {}. L'appareil ne va pas plus vite : ce test ne dit pas si le câble supporte l'USB 3. Pour le savoir, refaites-le avec un appareil USB 3 (SSD, clé USB 3).{fragile}",
            speed.label()
        )
    };
    base(
        Outcome::Ok,
        if errors > 0 {
            Severity::Warning
        } else {
            Severity::Info
        },
        format!("Ce câble transporte les données ({})", speed.short()),
        detail,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cablescope_model::{
        Bus, ConnectionProblem, HostInfo, PortActivity, PortInfo, SCHEMA_VERSION, Sourced, ToolInfo,
    };

    fn report(children: Vec<Device>) -> Report {
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
                id: "usb2".into(),
                controller: None,
                root_hub: Device {
                    location: "usb2".into(),
                    is_hub: true,
                    children,
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

    fn receiver() -> Device {
        Device {
            location: "2-4".into(),
            vendor_id: Some(0x046d),
            product_id: Some(0xc547),
            product: Some("USB Receiver".into()),
            speed: Some(Sourced::negotiated(Speed::Full)),
            ..Default::default()
        }
    }

    fn device(speed: Speed, max: Option<Speed>, port: Speed) -> Device {
        Device {
            location: "2-3".into(),
            vendor_id: Some(0x0781),
            product_id: Some(0x55b8),
            product: Some("Sandisk".into()),
            speed: Some(Sourced::negotiated(speed)),
            max_speed: max.map(Sourced::declared),
            port: Some(PortInfo {
                max_speed: Some(Sourced::inferred(port)),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn nothing_appeared_means_charge_only_cable() {
        let before = report(vec![receiver()]);
        let verdict = evaluate(&before, &before.clone(), None);
        assert_eq!(verdict.outcome, Outcome::NoData);
        assert_eq!(verdict.severity, Severity::Problem);
        assert_eq!(ratings(&verdict), vec!["Aucun", "Charge seulement"]);
        assert!(verdict.advice.is_some());
    }

    fn ratings(v: &CableVerdict) -> Vec<&str> {
        v.scores.iter().map(|s| s.rating.as_str()).collect()
    }

    #[test]
    fn failed_attempts_without_device_mean_bad_cable() {
        let before = report(vec![]);
        let link = LinkDiagnostics {
            duration_s: 10,
            ports: vec![PortActivity {
                enumeration_errors: 3,
                ..Default::default()
            }],
            controller_errors: 0,
        };
        assert_eq!(
            evaluate(&before, &before.clone(), Some(&link)).outcome,
            Outcome::NotRecognized
        );
    }

    #[test]
    fn usb3_device_at_usb2_on_usb3_port_means_usb2_cable() {
        let before = report(vec![receiver()]);
        let after = report(vec![
            receiver(),
            device(Speed::High, Some(Speed::Gen1x1), Speed::Gen1x1),
        ]);
        let verdict = evaluate(&before, &after, None);
        assert_eq!(verdict.outcome, Outcome::Usb2Limited);
        assert_eq!(verdict.device.as_deref(), Some("Sandisk"));
        assert_eq!(ratings(&verdict), vec!["Limité", "Usage limité"]);
        assert_eq!(
            verdict.scores[0].value.as_deref(),
            Some("480 Mb/s au lieu de 5 Gb/s")
        );
        let labels: Vec<&str> = verdict.facts.iter().map(|f| f.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Appareil",
                "Vitesse obtenue",
                "Maximum de l'appareil",
                "Prise"
            ]
        );
    }

    #[test]
    fn usb2_port_cannot_judge_usb3() {
        let after = report(vec![device(Speed::High, Some(Speed::Gen1x1), Speed::High)]);
        assert_eq!(
            evaluate(&report(vec![]), &after, None).outcome,
            Outcome::PortLimited
        );
    }

    #[test]
    fn full_speed_is_ok_and_usb2_device_gets_caveat() {
        let after = report(vec![device(
            Speed::Gen1x1,
            Some(Speed::Gen1x1),
            Speed::Gen1x1,
        )]);
        let verdict = evaluate(&report(vec![]), &after, None);
        assert_eq!(verdict.outcome, Outcome::Ok);
        assert!(verdict.detail.contains("son maximum"));
        assert_eq!(ratings(&verdict), vec!["Excellent", "Bon câble"]);
        assert_eq!(verdict.advice, None);

        let phone = Device {
            max_speed: None,
            ..device(Speed::High, None, Speed::Gen1x1)
        };
        let verdict = evaluate(&report(vec![]), &report(vec![phone]), None);
        assert_eq!(verdict.outcome, Outcome::Ok);
        assert!(verdict.detail.contains("ne dit pas"));
        assert!(
            verdict
                .advice
                .as_deref()
                .is_some_and(|a| a.contains("USB 3"))
        );
    }

    #[test]
    fn unrecognized_device_is_reported() {
        let broken = Device {
            problem: Some(ConnectionProblem::EnumerationFailed),
            ..device(Speed::Full, None, Speed::Gen1x1)
        };
        let verdict = evaluate(&report(vec![]), &report(vec![broken]), None);
        assert_eq!(verdict.outcome, Outcome::NotRecognized);
    }

    #[test]
    fn dock_counts_once() {
        let dock = Device {
            location: "2-1".into(),
            is_hub: true,
            children: vec![Device {
                location: "2-1.1".into(),
                ..receiver()
            }],
            ..Default::default()
        };
        let after = report(vec![dock]);
        let new = new_devices(&report(vec![]), &after);
        assert_eq!(new.len(), 1);
        assert!(new[0].is_hub);
    }
}
