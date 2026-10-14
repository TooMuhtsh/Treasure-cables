//! Cartes réseau de la machine. Sert à trouver une sonde branchée en partage
//! de connexion USB (voir docs/protocole-sonde.md) : le téléphone est la
//! passerelle de la carte réseau que ce partage fait apparaître.

use std::net::Ipv4Addr;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Adapter {
    /// Nom donné par le système ("Ethernet 3", "usb0").
    pub name: String,
    /// Description du pilote ("Remote NDIS based Internet Sharing Device").
    pub description: String,
    pub up: bool,
    pub gateways: Vec<Ipv4Addr>,
    /// Carte d'un partage de connexion USB (RNDIS ou NCM).
    pub usb_tether: bool,
}

/// Passerelles des partages de connexion USB actifs : les adresses où
/// chercher une sonde.
pub fn usb_tether_gateways() -> Vec<Ipv4Addr> {
    adapters()
        .into_iter()
        .filter(|a| a.up && a.usb_tether)
        .flat_map(|a| a.gateways)
        .collect()
}

/// Reconnaît le pilote d'un partage de connexion USB d'après sa description
/// (les libellés de Windows sont traduits : on s'en tient aux sigles).
pub fn is_usb_tether_description(description: &str) -> bool {
    let d = description.to_ascii_lowercase();
    ["ndis", "ncm", "android", "tether"]
        .iter()
        .any(|k| d.contains(k))
}

#[cfg(windows)]
pub fn adapters() -> Vec<Adapter> {
    use windows_sys::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_UNSPEC, SOCKADDR_IN};

    let flags = GAA_FLAG_INCLUDE_GATEWAYS
        | GAA_FLAG_SKIP_ANYCAST
        | GAA_FLAG_SKIP_MULTICAST
        | GAA_FLAG_SKIP_DNS_SERVER;
    // Tampon aligné pour la liste chaînée renvoyée par le système.
    let mut size = 16 * 1024u32;
    let mut buffer: Vec<u64>;
    loop {
        buffer = vec![0u64; size.div_ceil(8) as usize];
        // SAFETY: tampon de `size` octets au moins.
        let status = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_UNSPEC),
                flags,
                std::ptr::null(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        match status {
            NO_ERROR => break,
            ERROR_BUFFER_OVERFLOW => continue,
            _ => return Vec::new(),
        }
    }

    let wide = |p: *const u16| -> String {
        if p.is_null() {
            return String::new();
        }
        // SAFETY: chaîne terminée par un zéro fournie par le système.
        let len = (0..).take_while(|&i| unsafe { *p.add(i) } != 0).count();
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, len) })
    };

    let mut adapters = Vec::new();
    let mut current = buffer.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    // SAFETY: liste chaînée valide tant que `buffer` vit.
    while let Some(a) = unsafe { current.as_ref() } {
        let mut gateways = Vec::new();
        let mut gateway = a.FirstGatewayAddress;
        while let Some(g) = unsafe { gateway.as_ref() } {
            let sockaddr = g.Address.lpSockaddr;
            if !sockaddr.is_null() && unsafe { (*sockaddr).sa_family } == AF_INET {
                let sin = unsafe { &*sockaddr.cast::<SOCKADDR_IN>() };
                let raw = unsafe { sin.sin_addr.S_un.S_addr };
                gateways.push(Ipv4Addr::from(raw.to_ne_bytes()));
            }
            gateway = g.Next;
        }
        let description = wide(a.Description);
        adapters.push(Adapter {
            name: wide(a.FriendlyName),
            usb_tether: is_usb_tether_description(&description),
            description,
            up: a.OperStatus == IfOperStatusUp,
            gateways,
        });
        current = a.Next;
    }
    adapters
}

#[cfg(target_os = "linux")]
pub fn adapters() -> Vec<Adapter> {
    let routes = std::fs::read_to_string("/proc/net/route").unwrap_or_default();
    let mut adapters = Vec::new();
    for entry in std::fs::read_dir("/sys/class/net")
        .into_iter()
        .flatten()
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let driver = std::fs::read_link(path.join("device/driver"))
            .ok()
            .and_then(|l| l.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let up = std::fs::read_to_string(path.join("operstate")).is_ok_and(|s| s.trim() == "up");
        adapters.push(Adapter {
            gateways: linux_gateways(&routes, &name),
            usb_tether: matches!(driver.as_str(), "rndis_host" | "cdc_ncm" | "cdc_ether"),
            description: driver,
            name,
            up,
        });
    }
    adapters
}

/// Passerelles d'une interface d'après /proc/net/route (adresses écrites en
/// hexadécimal, octets dans l'ordre de la mémoire).
#[cfg(any(target_os = "linux", test))]
fn linux_gateways(routes: &str, interface: &str) -> Vec<Ipv4Addr> {
    let mut gateways: Vec<Ipv4Addr> = routes
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            (fields.next()? == interface).then_some(())?;
            let raw = u32::from_str_radix(fields.nth(1)?, 16).ok()?;
            (raw != 0).then(|| Ipv4Addr::from(raw.to_le_bytes()))
        })
        .collect();
    gateways.dedup();
    gateways
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn adapters() -> Vec<Adapter> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tether_drivers_are_recognised_in_any_language() {
        assert!(is_usb_tether_description(
            "Remote NDIS based Internet Sharing Device"
        ));
        assert!(is_usb_tether_description(
            "Périphérique de partage Internet basé sur NDIS distant"
        ));
        assert!(is_usb_tether_description("UsbNcm Host Device"));
        assert!(!is_usb_tether_description("Intel(R) Wi-Fi 6E AX211 160MHz"));
        assert!(!is_usb_tether_description(
            "Realtek PCIe GbE Family Controller"
        ));
    }

    #[test]
    fn gateways_from_proc_net_route() {
        let routes = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
            usb0\t00000000\t812AA8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\n\
            usb0\t002AA8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n\
            wlan0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0\n";
        assert_eq!(
            linux_gateways(routes, "usb0"),
            vec![Ipv4Addr::new(192, 168, 42, 129)]
        );
        assert_eq!(
            linux_gateways(routes, "wlan0"),
            vec![Ipv4Addr::new(192, 168, 1, 1)]
        );
    }
}
