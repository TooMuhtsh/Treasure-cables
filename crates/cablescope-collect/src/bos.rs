//! Décodage du descripteur BOS (Binary Object Store), commun à toutes les
//! plateformes : il indique les vitesses que l'appareil *sait* atteindre,
//! même quand il fonctionne plus lentement.

use cablescope_model::{AltMode, AltModeState, Speed};

const BOS_DESCRIPTOR: u8 = 0x0f;
const DEVICE_CAPABILITY: u8 = 0x10;
const CAP_SUPERSPEED: u8 = 0x03;
const CAP_SUPERSPEED_PLUS: u8 = 0x0a;
const CAP_BILLBOARD: u8 = 0x0d;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BosInfo {
    /// Vitesse maximale annoncée. Le nombre de voies (x2) n'est pas
    /// décrit par le BOS : Gen 2x2 apparaît ici comme Gen 2.
    pub max_speed: Option<Speed>,
    /// L'appareil expose une capacité Billboard (mode alternatif DisplayPort,
    /// Thunderbolt... non négocié ou en échec).
    pub billboard: bool,
    pub alt_modes: Vec<AltMode>,
}

pub fn parse(data: &[u8]) -> Option<BosInfo> {
    if data.len() < 5 || data[1] != BOS_DESCRIPTOR {
        return None;
    }
    let total = usize::from(u16::from_le_bytes([data[2], data[3]])).min(data.len());
    let mut info = BosInfo::default();
    let mut offset = usize::from(data[0]);
    while offset + 3 <= total {
        let len = usize::from(data[offset]);
        if len < 3 || offset + len > total {
            break;
        }
        let cap = &data[offset..offset + len];
        if cap[1] == DEVICE_CAPABILITY {
            match cap[2] {
                CAP_SUPERSPEED => {
                    // wSpeedsSupported, bit 3 : 5 Gb/s.
                    if len >= 6 && u16::from_le_bytes([cap[4], cap[5]]) & 0b1000 != 0 {
                        info.max_speed = info.max_speed.max(Some(Speed::Gen1x1));
                    }
                }
                CAP_SUPERSPEED_PLUS => {
                    info.max_speed = info.max_speed.max(superspeed_plus_max(cap));
                }
                CAP_BILLBOARD => {
                    info.billboard = true;
                    info.alt_modes = billboard_alt_modes(cap);
                }
                _ => {}
            }
        }
        offset += len;
    }
    Some(info)
}

/// Lit les attributs de sous-lien (bmSublinkSpeedAttr) de la capacité SuperSpeedPlus.
fn superspeed_plus_max(cap: &[u8]) -> Option<Speed> {
    if cap.len() < 12 {
        return None;
    }
    let attributes = u32::from_le_bytes([cap[4], cap[5], cap[6], cap[7]]);
    let count = (attributes & 0x1f) as usize + 1;
    let mut best = None;
    for i in 0..count {
        let start = 12 + i * 4;
        let Some(raw) = cap.get(start..start + 4) else {
            break;
        };
        let attr = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
        let exponent = (attr >> 4) & 0b11;
        let mantissa = u64::from(attr >> 16);
        let bits_per_s = mantissa * 1000u64.pow(exponent);
        let speed = match bits_per_s {
            b if b >= 10_000_000_000 => Speed::Gen2x1,
            b if b >= 5_000_000_000 => Speed::Gen1x1,
            _ => continue,
        };
        best = best.max(Some(speed));
    }
    best
}

/// Capacité Billboard (Billboard Device Class 1.2, tableau 3-6) :
/// bmConfigured (32 octets, 2 bits par mode) à l'offset 8, puis les modes
/// (wSVID, bAlternateMode, iAlternateModeString) à partir de l'offset 44.
fn billboard_alt_modes(cap: &[u8]) -> Vec<AltMode> {
    if cap.len() < 44 {
        return Vec::new();
    }
    let count = usize::from(cap[4]).min(34);
    (0..count)
        .filter_map(|i| {
            let entry = cap.get(44 + i * 4..44 + i * 4 + 4)?;
            let bits = (cap[8 + i / 4] >> ((i % 4) * 2)) & 0b11;
            Some(AltMode {
                svid: u16::from_le_bytes([entry[0], entry[1]]),
                state: match bits {
                    0 => AltModeState::Error,
                    1 => AltModeState::NotAttempted,
                    2 => AltModeState::Unsuccessful,
                    _ => AltModeState::Configured,
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bos(caps: &[&[u8]]) -> Vec<u8> {
        let total: usize = 5 + caps.iter().map(|c| c.len()).sum::<usize>();
        let mut out = vec![5, BOS_DESCRIPTOR, total as u8, 0, caps.len() as u8];
        caps.iter().for_each(|c| out.extend_from_slice(c));
        out
    }

    const USB2_EXT: [u8; 7] = [7, 0x10, 0x02, 0x02, 0, 0, 0];
    const SS: [u8; 10] = [10, 0x10, 0x03, 0, 0x0e, 0, 1, 10, 0xff, 0x07];

    #[test]
    fn usb2_only_device_has_no_superspeed() {
        let info = parse(&bos(&[&USB2_EXT])).unwrap();
        assert_eq!(info.max_speed, None);
    }

    #[test]
    fn superspeed_capability_means_gen1() {
        let info = parse(&bos(&[&USB2_EXT, &SS])).unwrap();
        assert_eq!(info.max_speed, Some(Speed::Gen1x1));
    }

    #[test]
    fn superspeed_plus_10gbps_means_gen2() {
        // Deux attributs (Rx et Tx) : 10 Gb/s, exposant 3 (Gb/s), protocole SSP.
        let rx: u32 = (10 << 16) | (1 << 14) | (3 << 4) | 1;
        let tx: u32 = rx | (1 << 7);
        let mut ssp = vec![20, 0x10, 0x0a, 0];
        ssp.extend_from_slice(&1u32.to_le_bytes()); // SSAC = 1 -> 2 attributs
        ssp.extend_from_slice(&[0, 0, 0, 0]); // wFunctionalitySupport + réservé
        ssp.extend_from_slice(&rx.to_le_bytes());
        ssp.extend_from_slice(&tx.to_le_bytes());
        let info = parse(&bos(&[&USB2_EXT, &SS, &ssp])).unwrap();
        assert_eq!(info.max_speed, Some(Speed::Gen2x1));
    }

    #[test]
    fn billboard_is_detected() {
        let billboard = [4u8, 0x10, 0x0d, 0];
        assert!(parse(&bos(&[&billboard])).unwrap().billboard);
    }

    #[test]
    fn billboard_alt_modes_are_decoded() {
        // Deux modes : DisplayPort en échec (2), Thunderbolt réussi (3).
        let mut cap = vec![52u8, 0x10, 0x0d, 0, 2, 0, 0, 0];
        let mut configured = [0u8; 32];
        configured[0] = 0b11_10;
        cap.extend_from_slice(&configured);
        cap.extend_from_slice(&[0x21, 0x01, 0, 0]); // bcdVersion, failure info, réservé
        cap.extend_from_slice(&[0x01, 0xff, 0, 0]);
        cap.extend_from_slice(&[0x87, 0x80, 0, 0]);
        let info = parse(&bos(&[&cap])).unwrap();
        assert_eq!(
            info.alt_modes,
            vec![
                AltMode {
                    svid: 0xff01,
                    state: AltModeState::Unsuccessful
                },
                AltMode {
                    svid: 0x8087,
                    state: AltModeState::Configured
                },
            ]
        );
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse(&[1, 2, 3]), None);
        assert_eq!(parse(&[5, 0x01, 5, 0, 0]), None);
    }
}
