//! Menu interactif : ce que voit la personne qui double-clique sur l'outil.

use std::io::{self, Write};
use std::path::PathBuf;

use crossterm::cursor::MoveTo;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, Clear, ClearType};
use treasure_collect::Options;
use treasure_model::Report;

use crate::{print_report, to_json};

enum Action {
    Refresh,
    Export,
    Quit,
}

pub fn run(options: &Options) -> io::Result<()> {
    loop {
        clear()?;
        println!("Analyse des ports et appareils USB…");
        let report = treasure_collect::collect(options);
        clear()?;
        match &report {
            Ok(r) => print_report(r),
            Err(e) => println!("Erreur pendant l'analyse : {e}"),
        }
        loop {
            println!("\n[R] Rafraîchir   [E] Exporter le rapport (JSON)   [Q] Quitter");
            io::stdout().flush()?;
            match read_key()? {
                Action::Refresh => break,
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
