//! Export du rapport JSON horodaté.

use std::io;
use std::path::PathBuf;

use treasure_model::Report;

use crate::to_json;

/// Enregistre le rapport dans le dossier courant (celui de l'exécutable en
/// cas de double-clic), ou à défaut à côté de l'exécutable.
pub fn export(report: &Report) -> io::Result<PathBuf> {
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
