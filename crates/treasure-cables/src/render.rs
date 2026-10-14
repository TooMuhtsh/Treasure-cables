//! Affichage des appareils USB : vue par prise (par défaut) et vue détaillée.

use treasure_model::view::{PlugKind, capability_label, machine_view};
use treasure_model::{Connector, Device, DisplayTransport, PortInfo, Report, Source};

pub fn print_usb(report: &Report, detailed: bool) {
    if detailed {
        print_detailed(report);
    } else {
        print_plugs(report);
    }
    print_typec_ports(report);
}

// ---------------------------------------------------------------------------
// Vue par prise

fn print_plugs(report: &Report) {
    let view = machine_view(report);
    println!("Prises USB");
    if view.plugs.is_empty() {
        println!("  Aucune prise détectée.");
    }
    // Un écran annoncé « sur USB-C » par le pilote graphique ne peut être
    // rattaché à sa prise que s'il n'y en a qu'une.
    let type_c_plugs = view
        .plugs
        .iter()
        .filter(|p| p.kind == PlugKind::TypeC)
        .count();
    let usb_c_display = report
        .displays
        .iter()
        .find(|d| d.transport == Some(DisplayTransport::UsbC))
        .filter(|_| type_c_plugs == 1);

    for plug in &view.plugs {
        let mut line = format!("  ● {}", plug.kind.label());
        if let Some(speed) = plug.max_speed {
            line.push_str(&format!(" · {}", capability_label(speed)));
        }
        let display_here = plug.kind == PlugKind::TypeC && usb_c_display.is_some();
        if let Some(display) = usb_c_display.filter(|_| display_here) {
            line.push_str(&format!(
                " · écran {}",
                display.name.as_deref().unwrap_or("externe")
            ));
        }
        if plug.devices.is_empty() && !display_here {
            line.push_str(" · libre");
        }
        println!("{line}");
        for (i, device) in plug.devices.iter().enumerate() {
            print_plug_device(device, "    ", i + 1 == plug.devices.len());
        }
    }

    if !view.internal.is_empty() {
        println!("\nAppareils internes");
        for device in &view.internal {
            println!("  · {} · {}", device.display_name(), compact_speed(device));
        }
    }
}

/// Un appareil et, s'il s'agit d'un hub ou d'un dock, ce qui y est branché
/// (tout ce qui partage le même adaptateur).
fn print_plug_device(device: &Device, prefix: &str, last: bool) {
    let branch = if last { "└─ " } else { "├─ " };
    let kind = match (device.is_hub, device.children.is_empty()) {
        (true, true) => " (hub)",
        (true, false) => " (hub ou dock)",
        _ => "",
    };
    println!(
        "{prefix}{branch}{}{kind} · {}",
        device.display_name(),
        compact_speed(device)
    );
    let child_prefix = format!("{prefix}{}", if last { "   " } else { "│  " });
    for (i, child) in device.children.iter().enumerate() {
        print_plug_device(child, &child_prefix, i + 1 == device.children.len());
    }
}

/// "5 Gb/s ✔ au maximum", "480 Mb/s ⚠ peut atteindre 10 Gb/s".
fn compact_speed(device: &Device) -> String {
    if device.problem.is_some() {
        return "✖ problème de connexion".to_owned();
    }
    match (device.speed, device.max_speed) {
        (Some(s), Some(m)) if !device.is_hub && s.value < m.value => {
            format!("{} ⚠ peut atteindre {}", s.value.short(), m.value.short())
        }
        (Some(s), Some(_)) if !device.is_hub => format!("{} ✔ au maximum", s.value.short()),
        (Some(s), _) => s.value.short().to_owned(),
        (None, _) => "vitesse inconnue".to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Vue détaillée

fn print_detailed(report: &Report) {
    println!("Contrôleurs et appareils USB (vue détaillée)");
    for bus in &report.buses {
        let controller = bus
            .controller
            .as_deref()
            .map(|c| format!(" [{c}]"))
            .unwrap_or_default();
        println!("\n{}{controller}", bus.id);
        // Ports accessibles un par un ; les ports internes sont seulement comptés.
        let (internal, accessible): (Vec<&PortInfo>, Vec<&PortInfo>) = bus
            .root_hub
            .ports
            .iter()
            .partition(|p| p.connector == Some(Connector::Internal));
        for port in &accessible {
            println!("  port {}", port_summary(port));
        }
        if !internal.is_empty() {
            println!("  + {} port(s) interne(s)", internal.len());
        }
        for (i, child) in bus.root_hub.children.iter().enumerate() {
            print_detailed_device(child, "  ", i + 1 == bus.root_hub.children.len());
        }
    }
}

fn print_detailed_device(device: &Device, prefix: &str, last: bool) {
    let branch = if last { "└─ " } else { "├─ " };
    let ids = match (device.vendor_id, device.product_id) {
        (Some(v), Some(p)) => format!(" ({v:04x}:{p:04x})"),
        _ => String::new(),
    };
    println!(
        "{prefix}{branch}{} {}{ids}",
        device.location,
        device.display_name()
    );
    let pad = format!("{prefix}{}   ", if last { "   " } else { "│  " });

    if let Some(speed) = device.speed {
        println!(
            "{pad}vitesse : {} [{}]",
            speed.value.label(),
            source(speed.source)
        );
    }
    if let Some(max) = device.max_speed {
        println!(
            "{pad}capacité : {} [{}]",
            max.value.label(),
            source(max.source)
        );
    }
    let mut facts = Vec::new();
    if let Some(manufacturer) = &device.manufacturer {
        facts.push(format!("fabricant {manufacturer}"));
    }
    if let Some(v) = device.usb_version {
        facts.push(format!("USB {v} annoncé"));
    }
    if let Some(class) = device.device_class {
        facts.push(format!("classe {class:02x}"));
    }
    if let Some(ma) = device.max_power_ma {
        facts.push(format!("{ma} mA"));
    }
    if let Some(l) = device.lanes {
        facts.push(format!("voies {}×{}", l.rx, l.tx));
    }
    if !facts.is_empty() {
        println!("{pad}{}", facts.join(" · "));
    }
    if let Some(port) = &device.port {
        println!("{pad}port : {}", port_summary(port));
    }
    if let Some(problem) = device.problem {
        println!("{pad}✖ problème : {problem:?}");
    }
    for mode in &device.alt_modes {
        println!("{pad}mode alternatif {} : {:?}", mode.name(), mode.state);
    }
    let child_prefix = format!("{prefix}{}", if last { "   " } else { "│  " });
    for (i, child) in device.children.iter().enumerate() {
        print_detailed_device(child, &child_prefix, i + 1 == device.children.len());
    }
}

/// "2-17 USB-A, USB 3 (5 Gb/s ou plus), jumeau 2-1".
fn port_summary(port: &PortInfo) -> String {
    let mut parts = vec![
        port.location
            .clone()
            .or_else(|| port.number.map(|n| format!("n°{n}")))
            .unwrap_or_else(|| "?".to_owned()),
    ];
    match port.connector {
        Some(Connector::TypeC) => parts.push("USB-C".to_owned()),
        Some(Connector::TypeA) => parts.push("USB-A".to_owned()),
        Some(Connector::Internal) => parts.push("interne".to_owned()),
        _ => {}
    }
    if let Some(speed) = port.max_speed {
        parts.push(capability_label(speed));
    }
    if let Some(companion) = &port.companion {
        parts.push(format!("jumeau {companion}"));
    }
    parts.join(", ")
}

fn source(source: Source) -> &'static str {
    match source {
        Source::Negotiated => "mesurée",
        Source::Declared => "déclarée",
        Source::Inferred => "déduite",
    }
}

// ---------------------------------------------------------------------------
// Ports USB-C décrits par le système (Linux)

fn print_typec_ports(report: &Report) {
    if report.typec_ports.is_empty() {
        return;
    }
    println!("\nPorts USB-C");
    for port in &report.typec_ports {
        let roles = [port.data_role.as_deref(), port.power_role.as_deref()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", ");
        let state = if port.partner.is_some() {
            "connecté"
        } else {
            "libre"
        };
        let roles = if roles.is_empty() {
            String::new()
        } else {
            format!(" ({roles})")
        };
        println!("  {} : {state}{roles}", port.id);
        if let Some(vdo) = port.cable.as_ref().and_then(|c| c.emarker) {
            let speed = vdo
                .highest_speed
                .map(|s| s.label())
                .unwrap_or("vitesse inconnue");
            let power = vdo
                .max_power_w()
                .map(|w| format!(", {w} W max"))
                .unwrap_or_default();
            println!("    câble (e-marker) : {speed}{power}");
        }
    }
}
