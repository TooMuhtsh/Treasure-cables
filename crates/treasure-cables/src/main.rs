//! Version portable de Treasure Cables.

// Version publiée sous Windows : application graphique, sans console au
// double-clic (les modes texte se rattachent au terminal appelant).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod export;
mod gui;
mod platform;
mod prefs;
mod session;
mod text;
mod tui;

use std::io::IsTerminal;
use std::process::ExitCode;

use treasure_model::Report;

const USAGE: &str = "\
Treasure Cables : audit des ports, câbles et appareils USB

Usage : treasure-cables [options]

Sans option (double-clic) : interface graphique dans une fenêtre
(analyse, test de câble, diagnostic approfondi, export).

Options :
  --tui              Interface plein écran dans la console
  --gui              Interface graphique (avec --input : affiche ce rapport)
  --no-window        Interface graphique sans ouvrir de fenêtre (affiche l'adresse)
  --json             Écrit le rapport JSON (à partager ou à ouvrir sur le site)
  --input <fichier>  Audite un rapport JSON existant au lieu de la machine
  --details          Vue détaillée (arbre par contrôleur) au lieu de la vue par prise
  --deep             Ouvre directement le diagnostic approfondi (administrateur)
  --include-serials  Inclut les numéros de série (exclus par défaut)
  -V, --version      Affiche la version
  -h, --help         Affiche cette aide";

struct Args {
    json: bool,
    tui: bool,
    gui: bool,
    no_window: bool,
    details: bool,
    deep: bool,
    input: Option<String>,
    include_serials: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        json: false,
        tui: false,
        gui: false,
        no_window: false,
        details: false,
        deep: false,
        input: None,
        include_serials: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--json" => args.json = true,
            "--tui" => args.tui = true,
            "--gui" => args.gui = true,
            "--no-window" => {
                args.gui = true;
                args.no_window = true;
            }
            "--deep" => args.deep = true,
            "--details" => args.details = true,
            "--include-serials" => args.include_serials = true,
            "--input" => args.input = Some(it.next().ok_or("--input attend un fichier")?),
            "-V" | "--version" => {
                platform::attach_console();
                println!(
                    "treasure-cables {} ({})",
                    env!("CARGO_PKG_VERSION"),
                    env!("TREASURE_BUILD")
                );
                std::process::exit(0);
            }
            "-h" | "--help" => {
                platform::attach_console();
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
            platform::attach_console();
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let options = treasure_collect::Options {
        include_serials: args.include_serials,
        ..Default::default()
    };

    if args.tui {
        platform::own_console();
        return match tui::run(&options, args.deep) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("Erreur : {e}");
                ExitCode::FAILURE
            }
        };
    }

    // Par défaut : interface graphique. Ailleurs que sous Windows, seulement
    // depuis un terminal interactif (pas quand la sortie est redirigée).
    let text_request = args.json || args.details || (args.input.is_some() && !args.gui);
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if !text_request && (cfg!(windows) || args.gui || interactive) {
        let fixed_report = match args.input.as_deref().map(read_report).transpose() {
            Ok(r) => r,
            Err(e) => {
                platform::message(&format!("Rapport illisible : {e}"));
                return ExitCode::FAILURE;
            }
        };
        return match gui::run(gui::Config {
            options,
            start_deep: args.deep,
            open_window: !args.no_window,
            fixed_report,
        }) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                platform::message(&format!("Treasure Cables n'a pas pu démarrer : {e}"));
                ExitCode::FAILURE
            }
        };
    }

    platform::attach_console();
    let report = match &args.input {
        Some(path) => read_report(path),
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

fn read_report(path: &str) -> Result<Report, String> {
    std::fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|s| serde_json::from_str::<Report>(&s).map_err(|e| e.to_string()))
}

fn to_json(report: &Report) -> String {
    serde_json::to_string_pretty(report).expect("rapport sérialisable")
}
