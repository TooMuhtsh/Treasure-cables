use serde::{Deserialize, Serialize};

/// Vitesse de signalisation d'un lien USB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Speed {
    /// USB 1.x Low Speed, 1,5 Mb/s.
    Low,
    /// USB 1.x Full Speed, 12 Mb/s.
    Full,
    /// USB 2.0 High Speed, 480 Mb/s.
    High,
    /// USB 3.2 Gen 1x1 (ex-USB 3.0), 5 Gb/s.
    Gen1x1,
    /// USB 3.2 Gen 1x2, 10 Gb/s sur deux paires (rare).
    Gen1x2,
    /// USB 3.2 Gen 2x1 (ex-USB 3.1), 10 Gb/s.
    Gen2x1,
    /// USB 3.2 Gen 2x2, 20 Gb/s (Type-C uniquement).
    Gen2x2,
    /// USB4 Gen 2x2, 20 Gb/s.
    Usb4Gen2,
    /// USB4 Gen 3x2, 40 Gb/s (Thunderbolt 3/4).
    Usb4Gen3,
    /// USB4 Gen 4, 80 Gb/s (USB4 v2, Thunderbolt 5).
    Usb4Gen4,
}

impl Speed {
    pub fn mbps(self) -> u32 {
        match self {
            Speed::Low => 1, // 1,5 Mb/s, arrondi
            Speed::Full => 12,
            Speed::High => 480,
            Speed::Gen1x1 => 5_000,
            Speed::Gen1x2 | Speed::Gen2x1 => 10_000,
            Speed::Gen2x2 | Speed::Usb4Gen2 => 20_000,
            Speed::Usb4Gen3 => 40_000,
            Speed::Usb4Gen4 => 80_000,
        }
    }

    /// Nom commercial et débit, pour l'affichage.
    pub fn label(self) -> &'static str {
        match self {
            Speed::Low => "USB 1.x Low Speed (1,5 Mb/s)",
            Speed::Full => "USB 1.x Full Speed (12 Mb/s)",
            Speed::High => "USB 2.0 High Speed (480 Mb/s)",
            Speed::Gen1x1 => "USB 3.2 Gen 1 (5 Gb/s)",
            Speed::Gen1x2 => "USB 3.2 Gen 1x2 (10 Gb/s)",
            Speed::Gen2x1 => "USB 3.2 Gen 2 (10 Gb/s)",
            Speed::Gen2x2 => "USB 3.2 Gen 2x2 (20 Gb/s)",
            Speed::Usb4Gen2 => "USB4 20 Gb/s",
            Speed::Usb4Gen3 => "USB4 40 Gb/s",
            Speed::Usb4Gen4 => "USB4 80 Gb/s",
        }
    }

    /// Débit seul, pour les affichages compacts.
    pub fn short(self) -> &'static str {
        match self {
            Speed::Low => "1,5 Mb/s",
            Speed::Full => "12 Mb/s",
            Speed::High => "480 Mb/s",
            Speed::Gen1x1 => "5 Gb/s",
            Speed::Gen1x2 | Speed::Gen2x1 => "10 Gb/s",
            Speed::Gen2x2 | Speed::Usb4Gen2 => "20 Gb/s",
            Speed::Usb4Gen3 => "40 Gb/s",
            Speed::Usb4Gen4 => "80 Gb/s",
        }
    }

    /// Vrai pour toutes les vitesses SuperSpeed et au-delà (paires USB 3 utilisées).
    pub fn is_superspeed(self) -> bool {
        self.mbps() >= 5_000
    }

    /// Interprète la vitesse exprimée en Mb/s et le nombre de voies,
    /// tels que Linux les expose dans sysfs (`speed`, `rx_lanes`).
    pub fn from_mbps(mbps: f64, lanes: Option<u8>) -> Option<Speed> {
        let two_lanes = lanes == Some(2);
        // En dixièmes de Mb/s, pour comparer exactement (1,5 Mb/s).
        let tenths = (mbps * 10.0).round() as u64;
        Some(match tenths {
            15 => Speed::Low,
            120 => Speed::Full,
            4_800 => Speed::High,
            50_000 => Speed::Gen1x1,
            100_000 if two_lanes => Speed::Gen1x2,
            100_000 => Speed::Gen2x1,
            200_000 => Speed::Gen2x2,
            _ => return None,
        })
    }
}

impl PartialOrd for Speed {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Ordre par débit ; à débit égal, l'ordre de déclaration départage.
impl Ord for Speed {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.mbps()
            .cmp(&other.mbps())
            .then((*self as u8).cmp(&(*other as u8)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_linux_sysfs_speeds() {
        assert_eq!(Speed::from_mbps(1.5, None), Some(Speed::Low));
        assert_eq!(Speed::from_mbps(480.0, Some(1)), Some(Speed::High));
        assert_eq!(Speed::from_mbps(10_000.0, Some(1)), Some(Speed::Gen2x1));
        assert_eq!(Speed::from_mbps(10_000.0, Some(2)), Some(Speed::Gen1x2));
        assert_eq!(Speed::from_mbps(20_000.0, Some(2)), Some(Speed::Gen2x2));
        assert_eq!(Speed::from_mbps(42.0, None), None);
    }

    #[test]
    fn ordering_follows_bandwidth() {
        assert!(Speed::High < Speed::Gen1x1);
        assert!(Speed::Gen1x1 < Speed::Gen2x1);
        assert!(Speed::Gen2x2 < Speed::Usb4Gen3);
        assert!(!Speed::High.is_superspeed());
        assert!(Speed::Gen1x1.is_superspeed());
    }
}
