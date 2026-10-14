//! Modèle de données de Treasure Cables.
//!
//! Ce crate ne dépend d'aucune API système : il est partagé entre les
//! collecteurs natifs (Windows, Linux) et la future version web (WebAssembly),
//! qui lira les rapports JSON produits par la version portable.

mod speed;
pub mod typec;
pub mod view;

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub displays: Vec<Display>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub usb4_routers: Vec<Usb4Router>,
    /// Résultat d'une écoute de la pile USB (diagnostic approfondi).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_diagnostics: Option<LinkDiagnostics>,
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
    /// Emplacement du port, au même format que celui des appareils ("2-17").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    pub number: Option<u8>,
    pub connector: Option<Connector>,
    /// Vitesse maximale que le port sait faire.
    pub max_speed: Option<Sourced<Speed>>,
    /// Emplacement du port compagnon (le même connecteur vu sur l'autre bus :
    /// USB 2 <-> USB 3).
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
    /// Ports en aval d'un hub (libres ou occupés) : ce que la machine sait faire.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<PortInfo>,
    /// Modes alternatifs annoncés par un appareil Billboard (BOS 0x0D).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alt_modes: Vec<AltMode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Device>,
}

/// Écran actif.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Display {
    pub name: Option<String>,
    pub connection: Option<DisplayConnection>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub refresh_hz: Option<f64>,
    /// Mode natif (préféré) annoncé par l'écran (EDID).
    pub native_width: Option<u32>,
    pub native_height: Option<u32>,
    pub native_refresh_hz: Option<f64>,
    /// Affichage dupliqué : la même image est envoyée à plusieurs écrans,
    /// Windows impose alors une résolution commune.
    #[serde(default)]
    pub mirrored: bool,
    /// Carte graphique qui pilote l'écran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<String>,
    /// Chemin physique de la sortie (USB-C, Thunderbolt...), si le pilote le dit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<DisplayTransport>,
    /// Lien DisplayPort négocié, lu auprès du pilote graphique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<DpLink>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayTransport {
    /// Port vidéo classique (DP, HDMI).
    Native,
    /// Port USB-C en mode alternatif DisplayPort.
    UsbC,
    Thunderbolt,
    Usb4,
}

impl DisplayTransport {
    pub fn label(self) -> &'static str {
        match self {
            DisplayTransport::Native => "port vidéo natif",
            DisplayTransport::UsbC => "port USB-C (mode alternatif DisplayPort)",
            DisplayTransport::Thunderbolt => "Thunderbolt",
            DisplayTransport::Usb4 => "USB4",
        }
    }
}

/// Lien DisplayPort : débit par voie et nombre de voies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DpLink {
    /// Débit par voie actuel, en Mb/s (1620 RBR, 2700 HBR, 5400 HBR2, 8100 HBR3).
    pub lane_rate_mbps: u32,
    pub lanes: u8,
    /// Maximum annoncé (écran, ou écran et carte graphique selon la source).
    pub max_lane_rate_mbps: Option<u32>,
    pub max_lanes: Option<u8>,
    /// D'où vient l'information ("Intel IGCL", "NVIDIA NVAPI").
    pub source: String,
}

impl DpLink {
    /// Débit utile en Gb/s (codage 8b/10b des débits RBR à HBR3).
    pub fn capacity_gbps(&self) -> f64 {
        f64::from(self.lane_rate_mbps) * f64::from(self.lanes) * 0.8 / 1000.0
    }

    pub fn rate_name(mbps: u32) -> &'static str {
        match mbps {
            1620 => "RBR",
            2700 => "HBR",
            5400 => "HBR2",
            8100 => "HBR3",
            _ => "débit eDP",
        }
    }

    /// "4 voies HBR2 (17,3 Gb/s)".
    pub fn label(&self) -> String {
        format!(
            "{} voie{} {} ({:.1} Gb/s)",
            self.lanes,
            if self.lanes > 1 { "s" } else { "" },
            Self::rate_name(self.lane_rate_mbps),
            self.capacity_gbps()
        )
        .replace('.', ",")
    }
}

/// Débit d'un mode vidéo en Gb/s : 8 bits par couleur, temps de suppression
/// réduits (CVT-RB, environ +8 % de pixels).
pub fn mode_bandwidth_gbps(width: u32, height: u32, refresh_hz: f64) -> f64 {
    f64::from(width) * f64::from(height) * refresh_hz * 1.08 * 24.0 / 1e9
}

impl Display {
    /// Débit approximatif nécessaire au mode natif, en Gb/s (8 bits par
    /// couleur, temps de suppression réduits CVT-RB : environ +8 %).
    pub fn native_bandwidth_gbps(&self) -> Option<f64> {
        Some(mode_bandwidth_gbps(
            self.native_width?,
            self.native_height?,
            self.native_refresh_hz.unwrap_or(60.0),
        ))
    }

    /// Débit approximatif nécessaire au mode actuel, en Gb/s.
    pub fn current_bandwidth_gbps(&self) -> Option<f64> {
        Some(mode_bandwidth_gbps(
            self.width?,
            self.height?,
            self.refresh_hz.unwrap_or(60.0),
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayConnection {
    Internal,
    /// Port DisplayPort, ou USB-C en mode alternatif DisplayPort.
    DisplayPort,
    /// DisplayPort transporté dans un tunnel USB4 / Thunderbolt.
    DisplayPortUsbTunnel,
    Hdmi,
    Dvi,
    Vga,
    Wireless,
    Other,
}

impl DisplayConnection {
    pub fn label(self) -> &'static str {
        match self {
            DisplayConnection::Internal => "écran intégré",
            DisplayConnection::DisplayPort => "DisplayPort (port DP ou USB-C en mode alternatif)",
            DisplayConnection::DisplayPortUsbTunnel => {
                "DisplayPort dans un tunnel USB4 / Thunderbolt"
            }
            DisplayConnection::Hdmi => "HDMI",
            DisplayConnection::Dvi => "DVI",
            DisplayConnection::Vga => "VGA",
            DisplayConnection::Wireless => "sans fil (Miracast)",
            DisplayConnection::Other => "autre",
        }
    }

    pub fn is_external_cable(self) -> bool {
        !matches!(
            self,
            DisplayConnection::Internal | DisplayConnection::Wireless | DisplayConnection::Other
        )
    }
}

/// Écoute de la pile USB pendant que l'utilisateur branche et débranche :
/// les erreurs de lien sont la preuve la plus directe d'un câble défaillant.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LinkDiagnostics {
    pub duration_s: u32,
    pub ports: Vec<PortActivity>,
    /// Réinitialisations ou erreurs internes d'un contrôleur USB.
    pub controller_errors: u32,
}

/// Activité observée sur un port pendant l'écoute.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PortActivity {
    /// Emplacement du port ("2-17"), quand il a pu être déterminé.
    pub location: Option<String>,
    pub port: Option<u32>,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    /// Nom de l'appareil vu sur ce port (utile s'il a été débranché depuis).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    /// Appareils reconnus avec succès.
    pub connections: u32,
    /// Erreurs pendant la reconnaissance (lecture des descripteurs, adresse...).
    pub enumeration_errors: u32,
    /// Descripteurs non conformes : défaut de l'appareil, pas du câble.
    pub descriptor_errors: u32,
    pub port_errors: u32,
    pub transfer_errors: u32,
    pub hub_resets: u32,
    pub removals: u32,
}

impl PortActivity {
    /// Erreurs qui peuvent venir du câble ou du connecteur.
    pub fn link_errors(&self) -> u32 {
        self.enumeration_errors + self.port_errors + self.transfer_errors + self.hub_resets
    }
}

/// Routeur USB4 / Thunderbolt (l'hôte, ou un appareil USB4 branché).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Usb4Router {
    pub name: String,
    /// Vrai pour le routeur de l'ordinateur, faux pour un appareil branché.
    pub host: bool,
}

/// Mode alternatif USB-C (DisplayPort, Thunderbolt, USB4...) tel que décrit
/// par un descripteur Billboard : l'appareil le publie justement quand la
/// négociation n'a pas abouti.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AltMode {
    /// Identifiant du standard (SVID) : 0xFF01 DisplayPort, 0x8087 Thunderbolt, 0xFF00 USB4.
    pub svid: u16,
    pub state: AltModeState,
}

impl AltMode {
    pub fn name(&self) -> &'static str {
        match self.svid {
            0xff00 => "USB4",
            0xff01 => "DisplayPort",
            0xff02 => "MHL",
            0x8087 => "Thunderbolt",
            _ => "mode alternatif propriétaire",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AltModeState {
    /// Erreur non précisée.
    Error,
    /// Pas tenté, ou quitté.
    NotAttempted,
    /// Tenté sans succès.
    Unsuccessful,
    Configured,
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
            displays: vec![],
            usb4_routers: vec![],
            link_diagnostics: None,
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
