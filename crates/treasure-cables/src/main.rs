//! Version portable de Treasure Cables (ligne de commande).

mod export;
mod session;
mod text;
mod tui;

use std::io::IsTerminal;
use std::process::ExitCode;

use treasure_model::Report;

const USAGE: &str = "\
Treasure Cables : audit des ports, câbles et appareils USB

Usage : treasure-cables [options]

Sans option, dans une console : diagnostic et menu interactif
(rafraîchir, vue détaillée, diagnostic approfondi, exporter en JSON, quitter).

Options :
  --json             Écrit le rapport JSON (à partager ou à ouvrir sur le site)
  --input <fichier>  Audite un rapport JSON existant au lieu de la machine
  --details          Vue détaillée (arbre par contrôleur) au lieu de la vue par prise
  --deep             Ouvre directement le diagnostic approfondi (administrateur)
  --include-serials  Inclut les numéros de série (exclus par défaut)
  -V, --version      Affiche la version
  -h, --help         Affiche cette aide";

struct Args {
    json: bool,
    details: bool,
    deep: bool,
    input: Option<String>,
    include_serials: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        json: false,
        details: false,
        deep: false,
        input: None,
        include_serials: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--json" => args.json = true,
            "--deep" => args.deep = true,
            "--details" => args.details = true,
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
        ..Default::default()
    };

    // Sans option, dans une console : menu interactif (cas du double-clic).
    let interactive = !args.json
        && args.input.is_none()
        && std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal();
    if interactive {
        return match tui::run(&options, args.deep) {
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
    print!("{}", text::plain(&report, args.details));
    ExitCode::SUCCESS
}

fn to_json(report: &Report) -> String {
    serde_json::to_string_pretty(report).expect("rapport sérialisable")
}
