//! Modèle de données de Treasure Cables.
//!
//! Ce crate ne dépend d'aucune API système : il est partagé entre les
//! collecteurs natifs (Windows, Linux) et la future version web (WebAssembly),
//! qui lira les rapports JSON produits par la version portable.

mod speed;
pub mod typec;

pub use speed::Speed;

use serde::{Deserialize, Serialize};

/// Version du format de rapport. À incrémenter à chaque changement
/// incompatible du JSON, pour que le site web sache le lire.
pub const SCHEMA_VERSION: u32 = 1;

/// Rapport complet produit par un collecteur.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub tool: ToolInfo,
    pub host: HostInfo,
    /// Secondes depuis l'epoch Unix.
    pub generated_at: Option<u64>,
    pub buses: Vec<Bus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub typec_ports: Vec<typec::TypeCPort>,
    /// Limites rencontrées pendant la collecte (droits, API absentes...).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostInfo {
    /// "windows", "linux"...
    pub os: String,
    pub os_version: Option<String>,
}

/// Contrôleur hôte et son hub racine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bus {
    /// Identifiant stable dans le rapport ("usb1", "PCI\\VEN_8086&DEV_..."...).
    pub id: String,
    pub controller: Option<String>,
    pub root_hub: Device,
}

/// Origine d'une information : c'est ce qui permet de dire à l'utilisateur
/// ce qui est mesuré, ce qui est seulement annoncé, et ce qui est déduit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// État réel du lien, rapporté par le contrôleur ou le hub.
    Negotiated,
    /// Annoncé par l'appareil, le port ou le câble (descripteurs, e-marker).
    Declared,
    /// Déduit par Treasure Cables à partir d'autres informations.
    Inferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sourced<T> {
    pub value: T,
    pub source: Source,
}

impl<T> Sourced<T> {
    pub fn negotiated(value: T) -> Self {
        Self {
            value,
            source: Source::Negotiated,
        }
    }
    pub fn declared(value: T) -> Self {
        Self {
            value,
            source: Source::Declared,
        }
    }
    pub fn inferred(value: T) -> Self {
        Self {
            value,
            source: Source::Inferred,
        }
    }
}

/// Version BCD (bcdUSB, bcdDevice), ex. 0x0320 pour USB 3.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Bcd(pub u16);

impl std::fmt::Display for Bcd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let major = (self.0 >> 8) as u8;
        let minor = (self.0 >> 4) & 0xf;
        let sub = self.0 & 0xf;
        // Les chiffres BCD sont affichés tels quels ("3.20" -> "3.2").
        let major = format!("{:x}", major);
        if sub == 0 {
            write!(f, "{major}.{minor:x}")
        } else {
            write!(f, "{major}.{minor:x}{sub:x}")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Connector {
    TypeA,
    TypeC,
    /// Port interne (non accessible à l'utilisateur).
    Internal,
    Unknown,
}

/// Informations sur le port amont d'un appareil (le port du hub où il est branché).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PortInfo {
    pub number: Option<u8>,
    pub connector: Option<Connector>,
    /// Vitesse maximale que le port sait faire.
    pub max_speed: Option<Sourced<Speed>>,
    /// Port compagnon (le même connecteur vu sur l'autre bus : USB 2 <-> USB 3).
    pub companion: Option<String>,
}

/// Problème signalé par le système pour une connexion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionProblem {
    NotEnoughPower,
    NotEnoughBandwidth,
    EnumerationFailed,
    Overcurrent,
    HubNestedTooDeeply,
    GeneralFailure,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lanes {
    pub rx: u8,
    pub tx: u8,
}

/// Appareil USB (les hubs sont des appareils avec des enfants).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Device {
    /// Emplacement lisible et stable dans le rapport ("1-2.3", "Port_#0002.Hub_#0003").
    pub location: String,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    /// Absent par défaut : un rapport peut être partagé publiquement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    pub device_class: Option<u8>,
    /// bcdUSB annoncé par l'appareil.
    pub usb_version: Option<Bcd>,
    /// Vitesse à laquelle le lien fonctionne réellement.
    pub speed: Option<Sourced<Speed>>,
    /// Vitesse maximale que l'appareil sait faire.
    pub max_speed: Option<Sourced<Speed>>,
    pub lanes: Option<Lanes>,
    /// Consommation maximale annoncée (bMaxPower), en mA.
    pub max_power_ma: Option<u32>,
    pub is_hub: bool,
    pub port: Option<PortInfo>,
    pub problem: Option<ConnectionProblem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Device>,
}

impl Device {
    /// Nom à afficher : produit, sinon fabricant, sinon VID:PID.
    pub fn display_name(&self) -> String {
        match (&self.product, &self.manufacturer) {
            (Some(p), _) => p.clone(),
            (None, Some(m)) => m.clone(),
            (None, None) => match (self.vendor_id, self.product_id) {
                (Some(v), Some(p)) => format!("{v:04x}:{p:04x}"),
                _ => "Appareil inconnu".to_owned(),
            },
        }
    }

    /// Parcours en profondeur (l'appareil lui-même puis ses descendants).
    pub fn walk(&self) -> Vec<&Device> {
        let mut out = vec![self];
        for child in &self.children {
            out.extend(child.walk());
        }
        out
    }
}

impl Report {
    pub fn devices(&self) -> impl Iterator<Item = &Device> {
        self.buses.iter().flat_map(|b| b.root_hub.walk())
    }

    /// Supprime les informations identifiantes avant un partage.
    pub fn redact(&mut self) {
        fn redact_device(d: &mut Device) {
            d.serial = None;
            d.children.iter_mut().for_each(redact_device);
        }
        self.buses
            .iter_mut()
            .for_each(|b| redact_device(&mut b.root_hub));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bcd_display() {
        assert_eq!(Bcd(0x0200).to_string(), "2.0");
        assert_eq!(Bcd(0x0210).to_string(), "2.1");
        assert_eq!(Bcd(0x0320).to_string(), "3.2");
        assert_eq!(Bcd(0x0110).to_string(), "1.1");
        assert_eq!(Bcd(0x0201).to_string(), "2.01");
    }

    #[test]
    fn redact_removes_serials_recursively() {
        let mut report = Report {
            schema_version: SCHEMA_VERSION,
            tool: ToolInfo {
                name: "t".into(),
                version: "0".into(),
            },
            host: HostInfo {
                os: "linux".into(),
                os_version: None,
            },
            generated_at: None,
            buses: vec![Bus {
                id: "usb1".into(),
                controller: None,
                root_hub: Device {
                    serial: Some("a".into()),
                    children: vec![Device {
                        serial: Some("b".into()),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            }],
            typec_ports: vec![],
            notes: vec![],
        };
        report.redact();
        assert!(report.devices().all(|d| d.serial.is_none()));
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("serial"));
    }

    #[test]
    fn example_report_matches_schema() {
        // Le rapport d'exemple sert de référence au futur site web.
        let json = include_str!("../../../examples/rapport-exemple.json");
        let report: Report = serde_json::from_str(json).unwrap();
        assert_eq!(report.schema_version, SCHEMA_VERSION);
        assert_eq!(report.devices().count(), 2);
    }
}
