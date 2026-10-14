//! Menu interactif : ce que voit la personne qui double-clique sur l'outil.

use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use crossterm::cursor::MoveTo;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, Clear, ClearType};
use treasure_collect::Options;
use treasure_model::Report;

use crate::{print_report, to_json};

enum Action {
    Refresh,
    Details,
    Deep,
    Export,
    Quit,
}

fn menu(detailed: bool) -> String {
    let view = if detailed {
        "Vue simple"
    } else {
        "Vue détaillée"
    };
    format!(
        "[R] Rafraîchir   [V] {view}   [D] Diagnostic approfondi   [E] Exporter (JSON)   [Q] Quitter"
    )
}

pub fn run(options: &Options, start_deep: bool) -> io::Result<()> {
    let mut report = analyse(options)?;
    let mut detailed = false;
    if start_deep && let Some(r) = deep_session(options, report.as_ref().ok())? {
        report = Ok(r);
    }
    loop {
        clear()?;
        match &report {
            Ok(r) => print_report(r, detailed),
            Err(e) => println!("Erreur pendant l'analyse : {e}"),
        }
        loop {
            println!("\n{}", menu(detailed));
            io::stdout().flush()?;
            match read_key()? {
                Action::Refresh => {
                    report = analyse(options)?;
                    break;
                }
                Action::Details => {
                    detailed = !detailed;
                    break;
                }
                Action::Deep => {
                    if let Some(r) = deep_session(options, report.as_ref().ok())? {
                        report = Ok(r);
                        break;
                    }
                }
                Action::Quit => return Ok(()),
                Action::Export => match &report {
                    Ok(r) => match export(r) {
                        Ok(path) => {
                            println!("\n✔ Rapport enregistré : {}", path.display());
                            if !options.include_serials {
                                println!("  Sans numéros de série : il peut être partagé.");
                            }
                        }
                        Err(e) => println!("\n✖ Export impossible : {e}"),
                    },
                    Err(_) => println!("\nRien à exporter : l'analyse a échoué."),
                },
            }
        }
    }
}

fn analyse(options: &Options) -> io::Result<Result<Report, String>> {
    clear()?;
    println!("Analyse des ports et appareils USB…");
    Ok(treasure_collect::collect(options).map_err(|e| e.to_string()))
}

/// Écoute de la pile USB pendant que l'utilisateur branche et débranche.
/// Renvoie un nouveau rapport qui inclut le bilan, ou rien si annulé.
/// `before` : l'analyse affichée avant l'écoute, qui permet de nommer un
/// appareil débranché pendant l'écoute.
fn deep_session(options: &Options, before: Option<&Report>) -> io::Result<Option<Report>> {
    if cfg!(windows) && !treasure_collect::is_elevated() {
        println!(
            "\nLe diagnostic approfondi écoute la pile USB de Windows pendant que vous branchez \
             et débranchez : il faut les droits administrateur."
        );
        println!("Relancer Treasure Cables en administrateur ? [O] Oui   [N] Non");
        io::stdout().flush()?;
        if read_yes_no()? {
            match treasure_collect::relaunch_elevated("--deep") {
                Ok(()) => println!(
                    "\n✔ Une fenêtre administrateur s'ouvre (validez l'invite de Windows)."
                ),
                Err(e) => println!("\n✖ {e}"),
            }
        }
        return Ok(None);
    }
    let capture = match treasure_collect::start_deep_capture() {
        Ok(c) => c,
        Err(e) => {
            println!("\n✖ {e}");
            return Ok(None);
        }
    };
    clear()?;
    println!("Diagnostic approfondi : écoute de la pile USB\n");
    println!(
        "  1. Débranchez l'appareil ou le câble à tester (un téléphone : branchez-le simplement)."
    );
    println!(
        "  2. Rebranchez-le, plusieurs fois si besoin, en laissant quelques secondes entre deux essais."
    );
    println!("  3. Appuyez sur une touche pour terminer (arrêt automatique après 2 minutes).\n");
    terminal::enable_raw_mode()?;
    let waited = loop {
        print!(
            "\r  ⏱ {:>3} s · {} événement(s) USB   ",
            capture.elapsed_s(),
            capture.events()
        );
        io::stdout().flush()?;
        if capture.elapsed_s() >= 120 {
            break Ok(());
        }
        match event::poll(Duration::from_millis(500)) {
            Ok(true) => match event::read() {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => break Ok(()),
                Ok(_) => {}
                Err(e) => break Err(e),
            },
            Ok(false) => {}
            Err(e) => break Err(e),
        }
    };
    terminal::disable_raw_mode()?;
    println!("\n\nAnalyse…");
    // Arrête toujours l'écoute, même si la lecture du clavier a échoué.
    let diagnostics = capture.stop();
    waited?;
    let mut report = match treasure_collect::collect(options) {
        Ok(r) => r,
        Err(e) => {
            println!("✖ {e}");
            return Ok(None);
        }
    };
    let mut diagnostics = diagnostics;
    for port in &mut diagnostics.ports {
        let Some(location) = port.location.as_deref() else {
            continue;
        };
        if port.vendor_id.is_some() || report.devices().any(|d| d.location == location) {
            continue;
        }
        if let Some(device) = before.and_then(|b| b.devices().find(|d| d.location == location)) {
            port.vendor_id = device.vendor_id;
            port.product_id = device.product_id;
            port.device_name = Some(device.display_name());
        }
    }
    report.link_diagnostics = Some(diagnostics);
    Ok(Some(report))
}

fn read_yes_no() -> io::Result<bool> {
    terminal::enable_raw_mode()?;
    let answer = loop {
        match event::read() {
            Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char(c) if matches!(c.to_ascii_lowercase(), 'o' | 'y') => break Ok(true),
                KeyCode::Char(c) if c.eq_ignore_ascii_case(&'n') => break Ok(false),
                KeyCode::Esc => break Ok(false),
                _ => {}
            },
            Ok(_) => {}
            Err(e) => break Err(e),
        }
    };
    terminal::disable_raw_mode()?;
    answer
}

fn clear() -> io::Result<()> {
    execute!(io::stdout(), Clear(ClearType::All), MoveTo(0, 0))
}

/// Attend une touche du menu (sans Entrée). Échap et Ctrl+C quittent,
/// F5 rafraîchit.
fn read_key() -> io::Result<Action> {
    terminal::enable_raw_mode()?;
    let result = loop {
        match event::read() {
            Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                let action = match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        Some(Action::Quit)
                    }
                    KeyCode::Char(c) => match c.to_ascii_lowercase() {
                        'r' => Some(Action::Refresh),
                        'd' => Some(Action::Deep),
                        'v' => Some(Action::Details),
                        'e' => Some(Action::Export),
                        'q' => Some(Action::Quit),
                        _ => None,
                    },
                    KeyCode::F(5) => Some(Action::Refresh),
                    KeyCode::Esc => Some(Action::Quit),
                    _ => None,
                };
                if let Some(action) = action {
                    break Ok(action);
                }
            }
            Ok(_) => {}
            Err(e) => break Err(e),
        }
    };
    terminal::disable_raw_mode()?;
    result
}

/// Enregistre le rapport dans le dossier courant (celui de l'exécutable en
/// cas de double-clic), ou à défaut à côté de l'exécutable.
fn export(report: &Report) -> io::Result<PathBuf> {
    let name = format!(
        "rapport-treasure-cables-{}.json",
        report.generated_at.map(timestamp).unwrap_or_default()
    );
    let json = to_json(report);
    let primary = std::env::current_dir().map(|d| d.join(&name));
    let fallback = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(&name)));
    let mut last_error = None;
    for path in primary.into_iter().chain(fallback) {
        match std::fs::write(&path, &json) {
            Ok(()) => return Ok(std::path::absolute(&path).unwrap_or(path)),
            Err(e) => last_error = Some(e),
        }
    }
    Err(last_error.unwrap_or_else(|| io::Error::other("aucun dossier accessible")))
}

/// Horodatage UTC "AAAAMMJJ-HHMMSS" (sans dépendance de date).
fn timestamp(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    let secs = unix % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

/// Jours depuis 1970-01-01 -> (année, mois, jour), algorithme de H. Hinnant.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_utc_dates() {
        assert_eq!(timestamp(0), "19700101-000000");
        assert_eq!(timestamp(951_782_400), "20000229-000000");
        assert_eq!(timestamp(1_760_452_212), "20251014-143012");
    }
}
