//! Version portable de Treasure Cables (ligne de commande).

use std::process::ExitCode;

use treasure_audit::{Confidence, Finding, Severity};
use treasure_model::{Device, Report, Source, Sourced, Speed};

const USAGE: &str = "\
Treasure Cables : audit des ports, câbles et appareils USB

Usage : treasure-cables [options]

Options :
  --json             Écrit le rapport JSON (à partager ou à ouvrir sur le site)
  --input <fichier>  Audite un rapport JSON existant au lieu de la machine
  --include-serials  Inclut les numéros de série (exclus par défaut)
  -h, --help         Affiche cette aide";

struct Args {
    json: bool,
    input: Option<String>,
    include_serials: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        json: false,
        input: None,
        include_serials: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--json" => args.json = true,
            "--include-serials" => args.include_serials = true,
            "--input" => args.input = Some(it.next().ok_or("--input attend un fichier")?),
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

    let report = match &args.input {
        Some(path) => std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<Report>(&s).map_err(|e| e.to_string())),
        None => treasure_collect::collect(&treasure_collect::Options {
            include_serials: args.include_serials,
        })
        .map_err(|e| e.to_string()),
    };
    let report = match report {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Erreur : {e}");
            return ExitCode::FAILURE;
        }
    };

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("rapport sérialisable")
        );
        return ExitCode::SUCCESS;
    }

    print_tree(&report);
    print_findings(&treasure_audit::audit(&report));
    for note in &report.notes {
        println!("ℹ {note}");
    }
    pause_if_double_clicked();
    ExitCode::SUCCESS
}

/// Lancé par double-clic, Windows ouvre une console qui se refermerait
/// aussitôt : on attend que l'utilisateur ait lu.
fn pause_if_double_clicked() {
    #[cfg(windows)]
    {
        let mut processes = [0u32; 2];
        // SAFETY: tampon de deux entrées, taille passée.
        let count = unsafe {
            windows_sys::Win32::System::Console::GetConsoleProcessList(processes.as_mut_ptr(), 2)
        };
        if count == 1 {
            println!("\nAppuyez sur Entrée pour fermer.");
            let _ = std::io::stdin().read_line(&mut String::new());
        }
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
            speed_text(bus.root_hub.speed, None)
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

fn print_device(device: &Device, prefix: &str, last: bool) {
    let branch = if last { "└─ " } else { "├─ " };
    let ids = match (device.vendor_id, device.product_id) {
        (Some(v), Some(p)) => format!(" ({v:04x}:{p:04x})"),
        _ => String::new(),
    };
    println!(
        "{prefix}{branch}{} {}{ids} : {}",
        device.location,
        device.display_name(),
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
