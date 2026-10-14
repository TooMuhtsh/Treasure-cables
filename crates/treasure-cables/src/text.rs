//! Texte du rapport, partagé par l'interface plein écran et la sortie en
//! ligne de commande (`--input`, redirection).

use std::fmt::Write;

use treasure_audit::{Confidence, Finding, Severity};
use treasure_model::view::{PlugKind, capability_label, is_unwired, machine_view};
use treasure_model::{Connector, Device, DisplayTransport, PortInfo, Report, Source};

macro_rules! w {
    ($out:expr) => {
        $out.push('\n')
    };
    ($out:expr, $($arg:tt)*) => {{
        let _ = writeln!($out, $($arg)*);
    }};
}

/// "Treasure Cables 0.1.0 (abc1234) · Windows 11 24H2 (build 26100)".
pub fn title(report: &Report) -> String {
    format!(
        "Treasure Cables {} ({}) · {}",
        env!("CARGO_PKG_VERSION"),
        env!("TREASURE_BUILD"),
        report.host.os_version.as_deref().unwrap_or(&report.host.os)
    )
}

/// Prises physiques, appareils internes, ports USB-C (Linux).
pub fn plugs(report: &Report) -> String {
    let mut out = String::new();
    print_usb(&mut out, report, false);
    out
}

/// Arbre détaillé par contrôleur.
pub fn details(report: &Report) -> String {
    let mut out = String::new();
    print_usb(&mut out, report, true);
    out
}

/// Écrans et routeurs USB4.
pub fn displays(report: &Report) -> String {
    let mut out = String::new();
    print_displays(&mut out, report);
    if out.trim().is_empty() {
        out.push_str("Aucun écran externe détecté.\n");
    }
    out.trim_start().to_owned()
}

/// Constats de l'audit, bilan du diagnostic approfondi, notes de collecte.
pub fn diagnosis(report: &Report) -> String {
    let mut out = String::new();
    print_findings(&mut out, &treasure_audit::audit(report));
    print_link_diagnostics(&mut out, report);
    for note in &report.notes {
        w!(out, "ℹ {note}");
    }
    out.trim_start().to_owned()
}

/// Rapport complet en texte (sortie non interactive).
pub fn plain(report: &Report, detailed: bool) -> String {
    let mut out = format!("{}\n\n", title(report));
    print_usb(&mut out, report, detailed);
    print_displays(&mut out, report);
    print_link_diagnostics(&mut out, report);
    print_findings(&mut out, &treasure_audit::audit(report));
    for note in &report.notes {
        w!(out, "ℹ {note}");
    }
    out
}

fn print_usb(out: &mut String, report: &Report, detailed: bool) {
    if detailed {
        print_detailed(out, report);
    } else {
        print_plugs(out, report);
    }
    print_typec_ports(out, report);
}

// ---------------------------------------------------------------------------
// Vue par prise

fn print_plugs(out: &mut String, report: &Report) {
    let view = machine_view(report);
    w!(out, "Prises USB");
    if view.plugs.is_empty() {
        w!(out, "  Aucune prise détectée.");
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
        w!(out, "{line}");
        for (i, device) in plug.devices.iter().enumerate() {
            print_plug_device(out, device, "    ", i + 1 == plug.devices.len());
        }
    }

    if let Some(note) = treasure_present::unwired_note(&view.unwired) {
        w!(out, "  ({note})");
    }

    if !view.internal.is_empty() {
        w!(out, "\nAppareils internes");
        for device in &view.internal {
            w!(
                out,
                "  · {} · {}",
                device.display_name(),
                compact_speed(device)
            );
        }
    }
}

/// Un appareil et, s'il s'agit d'un hub ou d'un dock, ce qui y est branché
/// (tout ce qui partage le même adaptateur).
fn print_plug_device(out: &mut String, device: &Device, prefix: &str, last: bool) {
    let branch = if last { "└─ " } else { "├─ " };
    let kind = match (device.is_hub, device.children.is_empty()) {
        (true, true) => " (hub)",
        (true, false) => " (hub ou dock)",
        _ => "",
    };
    w!(
        out,
        "{prefix}{branch}{}{kind} · {}",
        device.display_name(),
        compact_speed(device)
    );
    let child_prefix = format!("{prefix}{}", if last { "   " } else { "│  " });
    for (i, child) in device.children.iter().enumerate() {
        print_plug_device(out, child, &child_prefix, i + 1 == device.children.len());
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

fn print_detailed(out: &mut String, report: &Report) {
    w!(out, "Contrôleurs et appareils USB (vue détaillée)");
    for bus in &report.buses {
        let controller = bus
            .controller
            .as_deref()
            .map(|c| format!(" [{c}]"))
            .unwrap_or_default();
        w!(out, "\n{}{controller}", bus.id);
        // Ports accessibles un par un ; les ports internes sont seulement comptés.
        let (internal, accessible): (Vec<&PortInfo>, Vec<&PortInfo>) = bus
            .root_hub
            .ports
            .iter()
            .partition(|p| p.connector == Some(Connector::Internal));
        for port in &accessible {
            let unwired = if is_unwired(report, bus, port) {
                " · non câblé"
            } else {
                ""
            };
            w!(out, "  port {}{unwired}", port_summary(port));
        }
        if !internal.is_empty() {
            w!(out, "  + {} port(s) interne(s)", internal.len());
        }
        for (i, child) in bus.root_hub.children.iter().enumerate() {
            print_detailed_device(out, child, "  ", i + 1 == bus.root_hub.children.len());
        }
    }
}

fn print_detailed_device(out: &mut String, device: &Device, prefix: &str, last: bool) {
    let branch = if last { "└─ " } else { "├─ " };
    let ids = match (device.vendor_id, device.product_id) {
        (Some(v), Some(p)) => format!(" ({v:04x}:{p:04x})"),
        _ => String::new(),
    };
    w!(
        out,
        "{prefix}{branch}{} {}{ids}",
        device.location,
        device.display_name()
    );
    let pad = format!("{prefix}{}   ", if last { "   " } else { "│  " });

    if let Some(speed) = device.speed {
        w!(
            out,
            "{pad}vitesse : {} [{}]",
            speed.value.label(),
            source(speed.source)
        );
    }
    if let Some(max) = device.max_speed {
        w!(
            out,
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
        w!(out, "{pad}{}", facts.join(" · "));
    }
    if let Some(port) = &device.port {
        w!(out, "{pad}port : {}", port_summary(port));
    }
    if let Some(problem) = device.problem {
        w!(out, "{pad}✖ problème : {problem:?}");
    }
    for mode in &device.alt_modes {
        w!(
            out,
            "{pad}mode alternatif {} : {:?}",
            mode.name(),
            mode.state
        );
    }
    let child_prefix = format!("{prefix}{}", if last { "   " } else { "│  " });
    for (i, child) in device.children.iter().enumerate() {
        print_detailed_device(out, child, &child_prefix, i + 1 == device.children.len());
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

fn print_typec_ports(out: &mut String, report: &Report) {
    if report.typec_ports.is_empty() {
        return;
    }
    w!(out, "\nPorts USB-C");
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
        w!(out, "  {} : {state}{roles}", port.id);
        if let Some(vdo) = port.cable.as_ref().and_then(|c| c.emarker) {
            let speed = vdo
                .highest_speed
                .map(|s| s.label())
                .unwrap_or("vitesse inconnue");
            let power = vdo
                .max_power_w()
                .map(|w| format!(", {w} W max"))
                .unwrap_or_default();
            w!(out, "    câble (e-marker) : {speed}{power}");
        }
    }
}

fn print_link_diagnostics(out: &mut String, report: &Report) {
    let Some(diagnostics) = &report.link_diagnostics else {
        return;
    };
    w!(
        out,
        "\nDiagnostic approfondi ({} s d'écoute)",
        diagnostics.duration_s
    );
    if diagnostics.ports.is_empty() {
        w!(out, "  Aucun branchement USB détecté pendant l'écoute.");
    }
    for port in &diagnostics.ports {
        let place = port
            .location
            .clone()
            .or_else(|| port.port.map(|p| format!("port {p}")))
            .unwrap_or_else(|| "port inconnu".to_owned());
        let name = port
            .location
            .as_deref()
            .and_then(|loc| report.devices().find(|d| d.location == loc))
            .map(|d| d.display_name())
            .or_else(|| port.device_name.clone())
            .or_else(|| match (port.vendor_id, port.product_id) {
                (Some(v), Some(p)) => Some(format!("{v:04x}:{p:04x}")),
                _ => None,
            })
            .map(|n| format!(" {n}"))
            .unwrap_or_default();
        let errors = port.link_errors();
        let verdict = if errors == 0 {
            "aucune erreur".to_owned()
        } else {
            format!("{errors} erreur(s) de lien")
        };
        w!(
            out,
            "  {place}{name} : {} branchement(s) reconnu(s), {verdict}",
            port.connections
        );
        let detail = port.breakdown();
        if !detail.is_empty() {
            w!(out, "    {}", detail.join(" · "));
        }
    }
    if diagnostics.controller_errors > 0 {
        w!(
            out,
            "  {} erreur(s) ou réinitialisation(s) d'un contrôleur USB",
            diagnostics.controller_errors
        );
    }
}

fn print_displays(out: &mut String, report: &Report) {
    if !report.displays.is_empty() {
        w!(out, "\nÉcrans");
        for d in &report.displays {
            let name = d.name.as_deref().unwrap_or("Écran");
            let connection = d
                .connection
                .map(|c| c.label())
                .unwrap_or("liaison inconnue");
            let mode = match (d.width, d.height, d.refresh_hz) {
                (Some(w), Some(h), Some(hz)) => format!("{w}×{h} à {hz:.0} Hz"),
                (Some(w), Some(h), None) => format!("{w}×{h}"),
                _ => "mode inconnu".to_owned(),
            };
            let native = match (d.native_width, d.native_height, d.native_refresh_hz) {
                (Some(w), Some(h), Some(hz)) => format!(" (natif : {w}×{h} à {hz:.0} Hz)"),
                _ => String::new(),
            };
            let mirrored = if d.mirrored {
                ", affichage dupliqué"
            } else {
                ""
            };
            w!(out, "  {name} : {connection}, {mode}{native}{mirrored}");
            if let Some(t) = d.transport {
                w!(out, "    sortie : {}", t.label());
            }
            if let Some(link) = &d.link {
                let max = match (link.max_lanes, link.max_lane_rate_mbps) {
                    (Some(l), Some(r)) => format!(
                        " ; maximum : {l} voie{} {}",
                        if l > 1 { "s" } else { "" },
                        treasure_model::DpLink::rate_name(r)
                    ),
                    _ => String::new(),
                };
                w!(out, "    lien DisplayPort : {}{max}", link.label());
                if let Some(needed) = d.current_bandwidth_gbps() {
                    let share = needed / link.capacity_gbps() * 100.0;
                    w!(
                        out,
                        "    mode actuel : environ {} Gb/s, soit {share:.0} % du lien",
                        format!("{needed:.1}").replace('.', ",")
                    );
                }
            }
            if let Some(gpu) = &d.gpu {
                w!(out, "    carte graphique : {gpu}");
            }
        }
    }
    if !report.usb4_routers.is_empty() {
        w!(out, "\nUSB4 / Thunderbolt");
        for r in &report.usb4_routers {
            let role = if r.host {
                "routeur de l'ordinateur"
            } else {
                "appareil USB4 branché"
            };
            w!(out, "  {} : {role}", r.name);
        }
    }
}

fn print_findings(out: &mut String, findings: &[Finding]) {
    w!(out);
    if findings.is_empty() {
        w!(out, "✔ Aucun problème détecté.");
        return;
    }
    w!(out, "Diagnostic");
    for f in findings {
        let icon = match f.severity {
            Severity::Problem => "✖",
            Severity::Warning => "⚠",
            Severity::Info => "ℹ",
        };
        let confidence = match f.confidence {
            Confidence::Certain => "certain",
            Confidence::Likely => "probable",
            Confidence::Possible => "possible",
        };
        w!(out, "{icon} [{}] {} ({confidence})", f.subject, f.title);
        w!(out, "    {}", f.detail);
    }
}
