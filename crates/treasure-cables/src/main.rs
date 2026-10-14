//! Version portable de Treasure Cables (ligne de commande).

mod menu;

use std::io::IsTerminal;
use std::process::ExitCode;

use treasure_audit::{Confidence, Finding, Severity};
use treasure_model::{Connector, Device, Report, Source, Sourced, Speed};

const USAGE: &str = "\
Treasure Cables : audit des ports, câbles et appareils USB

Usage : treasure-cables [options]

Sans option, dans une console : diagnostic et menu interactif
(rafraîchir, diagnostic approfondi, exporter le rapport JSON, quitter).

Options :
  --json             Écrit le rapport JSON (à partager ou à ouvrir sur le site)
  --input <fichier>  Audite un rapport JSON existant au lieu de la machine
  --deep             Ouvre directement le diagnostic approfondi (administrateur)
  --include-serials  Inclut les numéros de série (exclus par défaut)
  -V, --version      Affiche la version
  -h, --help         Affiche cette aide";

struct Args {
    json: bool,
    deep: bool,
    input: Option<String>,
    include_serials: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        json: false,
        deep: false,
        input: None,
        include_serials: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--json" => args.json = true,
            "--deep" => args.deep = true,
            "--include-serials" => args.include_serials = true,
            "--input" => args.input = Some(it.next().ok_or("--input attend un fichier")?),
            "-V" | "--version" => {
                println!(
                    "treasure-cables {} ({})",
                    env!("CARGO_PKG_VERSION"),
                    env!("TREASURE_BUILD")
                );
                std::process::exit(0);
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("option inconnue : {other}\n\n{USAGE}")),
        }
    }
    Ok(args)
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let options = treasure_collect::Options {
        include_serials: args.include_serials,
    };

    // Sans option, dans une console : menu interactif (cas du double-clic).
    let interactive = !args.json
        && args.input.is_none()
        && std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal();
    if interactive {
        return match menu::run(&options, args.deep) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("Erreur : {e}");
                ExitCode::FAILURE
            }
        };
    }

    let report = match &args.input {
        Some(path) => std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<Report>(&s).map_err(|e| e.to_string())),
        None => treasure_collect::collect(&options).map_err(|e| e.to_string()),
    };
    let report = match report {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Erreur : {e}");
            return ExitCode::FAILURE;
        }
    };

    if args.json {
        println!("{}", to_json(&report));
        return ExitCode::SUCCESS;
    }
    print_report(&report);
    ExitCode::SUCCESS
}

fn to_json(report: &Report) -> String {
    serde_json::to_string_pretty(report).expect("rapport sérialisable")
}

fn print_report(report: &Report) {
    println!(
        "Treasure Cables {} ({})\n",
        env!("CARGO_PKG_VERSION"),
        env!("TREASURE_BUILD")
    );
    print_tree(report);
    print_displays(report);
    print_link_diagnostics(report);
    print_findings(&treasure_audit::audit(report));
    for note in &report.notes {
        println!("ℹ {note}");
    }
}

fn print_tree(report: &Report) {
    println!(
        "Appareils USB ({})",
        report.host.os_version.as_deref().unwrap_or(&report.host.os)
    );
    for bus in &report.buses {
        let controller = bus
            .controller
            .as_deref()
            .map(|c| format!(" [{c}]"))
            .unwrap_or_default();
        println!(
            "\n{}{controller} : {}",
            bus.id,
            ports_summary(&bus.root_hub)
        );
        for (i, child) in bus.root_hub.children.iter().enumerate() {
            print_device(child, "", i + 1 == bus.root_hub.children.len());
        }
    }
    if !report.typec_ports.is_empty() {
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
            println!(
                "  {} : {state}{}",
                port.id,
                if roles.is_empty() {
                    String::new()
                } else {
                    format!(" ({roles})")
                }
            );
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
}

/// "4 ports : 2 USB 3.x, 2 USB 2.0 ; dont 2 USB-C".
fn ports_summary(hub: &Device) -> String {
    let ports: Vec<_> = hub
        .ports
        .iter()
        .filter(|p| p.connector != Some(Connector::Internal))
        .collect();
    if ports.is_empty() {
        return speed_text(hub.speed, None);
    }
    let usb3 = ports
        .iter()
        .filter(|p| p.max_speed.is_some_and(|s| s.value.is_superspeed()))
        .count();
    let type_c = ports
        .iter()
        .filter(|p| p.connector == Some(Connector::TypeC))
        .count();
    let mut text = format!("{} ports accessibles", ports.len());
    let mut kinds = Vec::new();
    if usb3 > 0 {
        kinds.push(format!("{usb3} USB 3.x"));
    }
    if ports.len() > usb3 {
        kinds.push(format!("{} USB 2.0", ports.len() - usb3));
    }
    text.push_str(&format!(" ({})", kinds.join(", ")));
    if type_c > 0 {
        text.push_str(&format!(", dont {type_c} USB-C"));
    }
    text
}

fn print_link_diagnostics(report: &Report) {
    let Some(diagnostics) = &report.link_diagnostics else {
        return;
    };
    println!(
        "\nDiagnostic approfondi ({} s d'écoute)",
        diagnostics.duration_s
    );
    if diagnostics.ports.is_empty() {
        println!("  Aucune activité USB observée : rien n'a été branché ou débranché.");
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
        println!(
            "  {place}{name} : {} reconnaissance(s), {} retrait(s), {verdict}",
            port.connections, port.removals
        );
    }
}

fn print_displays(report: &Report) {
    if !report.displays.is_empty() {
        println!("\nÉcrans");
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
            println!("  {name} : {connection}, {mode}{native}{mirrored}");
            if let Some(t) = d.transport {
                println!("    sortie : {}", t.label());
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
                println!("    lien DisplayPort : {}{max}", link.label());
            }
            if let Some(gpu) = &d.gpu {
                println!("    carte graphique : {gpu}");
            }
        }
    }
    if !report.usb4_routers.is_empty() {
        println!("\nUSB4 / Thunderbolt");
        for r in &report.usb4_routers {
            let role = if r.host {
                "routeur de l'ordinateur"
            } else {
                "appareil USB4 branché"
            };
            println!("  {} : {role}", r.name);
        }
    }
}

fn print_device(device: &Device, prefix: &str, last: bool) {
    let branch = if last { "└─ " } else { "├─ " };
    let ids = match (device.vendor_id, device.product_id) {
        (Some(v), Some(p)) => format!(" ({v:04x}:{p:04x})"),
        _ => String::new(),
    };
    let type_c = device
        .port
        .as_ref()
        .is_some_and(|p| p.connector == Some(Connector::TypeC));
    println!(
        "{prefix}{branch}{} {}{ids}{} : {}",
        device.location,
        device.display_name(),
        if type_c { " [USB-C]" } else { "" },
        speed_text(device.speed, device.max_speed)
    );
    let child_prefix = format!("{prefix}{}", if last { "   " } else { "│  " });
    for (i, child) in device.children.iter().enumerate() {
        print_device(child, &child_prefix, i + 1 == device.children.len());
    }
}

fn speed_text(speed: Option<Sourced<Speed>>, max: Option<Sourced<Speed>>) -> String {
    let current = speed.map(|s| s.value.label()).unwrap_or("vitesse inconnue");
    match max {
        Some(m) if speed.is_some_and(|s| s.value < m.value) => {
            let how = if m.source == Source::Inferred {
                "probablement capable de"
            } else {
                "capable de"
            };
            format!("{current}, {how} {}", m.value.label())
        }
        _ => current.to_owned(),
    }
}

fn print_findings(findings: &[Finding]) {
    println!();
    if findings.is_empty() {
        println!("✔ Aucun problème détecté.");
        return;
    }
    println!("Diagnostic");
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
        println!("{icon} [{}] {} ({confidence})", f.subject, f.title);
        println!("    {}", f.detail);
    }
}
