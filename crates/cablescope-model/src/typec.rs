//! Ports USB Type-C, partenaires et câbles (e-marker).
//!
//! Le décodage des VDO (Vendor Defined Objects) suit la spécification
//! USB Power Delivery 3.1. Ces informations sont *déclarées* par la puce
//! e-marker du câble : elles décrivent ce que le câble prétend supporter,
//! pas un débit mesuré.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TypeCPort {
    /// "port0"...
    pub id: String,
    pub data_role: Option<String>,
    pub power_role: Option<String>,
    pub power_operation_mode: Option<String>,
    pub partner: Option<Partner>,
    pub cable: Option<Cable>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Partner {
    pub kind: Option<String>,
    pub supports_usb_pd: Option<bool>,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cable {
    /// Actif ou passif.
    pub kind: Option<CableKind>,
    pub plug_type: Option<String>,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    /// Identifiant de certification USB-IF (XID), 0 si absent.
    pub certification_id: Option<u32>,
    /// Capacités annoncées par l'e-marker.
    pub emarker: Option<CableVdo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CableKind {
    Passive,
    Active,
}

/// Vitesse maximale annoncée par un câble (champ "USB Highest Speed").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CableSpeed {
    Usb2,
    Usb32Gen1,
    /// USB 3.2 Gen 2 ou USB4 Gen 2 (10 Gb/s par voie).
    Usb32Gen2,
    /// USB4 Gen 3 (20 Gb/s par voie, 40 Gb/s au total).
    Usb4Gen3,
    /// USB4 Gen 4 (40 Gb/s par voie, 80 Gb/s au total).
    Usb4Gen4,
}

impl CableSpeed {
    pub fn label(self) -> &'static str {
        match self {
            CableSpeed::Usb2 => "USB 2.0 uniquement (480 Mb/s)",
            CableSpeed::Usb32Gen1 => "USB 3.2 Gen 1 (5 Gb/s)",
            CableSpeed::Usb32Gen2 => "USB 3.2 Gen 2 / USB4 20 Gb/s",
            CableSpeed::Usb4Gen3 => "USB4 40 Gb/s",
            CableSpeed::Usb4Gen4 => "USB4 80 Gb/s",
        }
    }
}

/// Contenu décodé du "Passive/Active Cable VDO" (VDO1 de l'identité SOP').
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CableVdo {
    pub raw: u32,
    pub highest_speed: Option<CableSpeed>,
    /// Courant VBUS maximal, en mA (3000 ou 5000).
    pub max_current_ma: Option<u32>,
    /// Tension VBUS maximale, en V.
    pub max_voltage_v: Option<u32>,
    /// Le câble annonce le mode EPR (Extended Power Range, jusqu'à 240 W).
    pub epr_capable: bool,
}

impl CableVdo {
    /// Décode le VDO câble (PD 3.1, tableaux 6-41 et 6-42).
    pub fn decode(raw: u32) -> Self {
        let highest_speed = match raw & 0b111 {
            0b000 => Some(CableSpeed::Usb2),
            0b001 => Some(CableSpeed::Usb32Gen1),
            0b010 => Some(CableSpeed::Usb32Gen2),
            0b011 => Some(CableSpeed::Usb4Gen3),
            0b100 => Some(CableSpeed::Usb4Gen4),
            _ => None,
        };
        let max_current_ma = match (raw >> 5) & 0b11 {
            0b01 => Some(3000),
            0b10 => Some(5000),
            _ => None,
        };
        let max_voltage_v = match (raw >> 9) & 0b11 {
            0b00 => Some(20),
            0b01 => Some(30),
            0b10 => Some(40),
            _ => Some(50),
        };
        Self {
            raw,
            highest_speed,
            max_current_ma,
            max_voltage_v,
            epr_capable: (raw >> 17) & 1 == 1,
        }
    }

    /// Puissance maximale que le câble peut transporter, en W.
    ///
    /// Un câble 5 A n'atteint 240 W qu'en EPR (48 V) ; sinon la limite PD
    /// standard est de 20 V.
    pub fn max_power_w(&self) -> Option<u32> {
        let amps = self.max_current_ma? / 1000;
        let volts = if self.epr_capable { 48 } else { 20 };
        Some(amps * volts)
    }
}

/// Type de produit déclaré dans l'en-tête d'identité (ID Header VDO) d'un câble.
pub fn cable_kind_from_id_header(id_header: u32) -> Option<CableKind> {
    match (id_header >> 27) & 0b111 {
        0b011 => Some(CableKind::Passive),
        0b100 => Some(CableKind::Active),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_passive_5a_usb4_epr_cable() {
        // USB4 Gen3, 5 A, 50 V, EPR.
        let raw = 0b011 | (0b10 << 5) | (0b11 << 9) | (1 << 17);
        let vdo = CableVdo::decode(raw);
        assert_eq!(vdo.highest_speed, Some(CableSpeed::Usb4Gen3));
        assert_eq!(vdo.max_current_ma, Some(5000));
        assert_eq!(vdo.max_voltage_v, Some(50));
        assert!(vdo.epr_capable);
        assert_eq!(vdo.max_power_w(), Some(240));
    }

    #[test]
    fn decodes_usb2_3a_cable() {
        let vdo = CableVdo::decode(0b01 << 5);
        assert_eq!(vdo.highest_speed, Some(CableSpeed::Usb2));
        assert_eq!(vdo.max_power_w(), Some(60));
    }

    #[test]
    fn id_header_product_type() {
        assert_eq!(
            cable_kind_from_id_header(0b011 << 27),
            Some(CableKind::Passive)
        );
        assert_eq!(
            cable_kind_from_id_header(0b100 << 27),
            Some(CableKind::Active)
        );
        assert_eq!(cable_kind_from_id_header(0), None);
    }
}
