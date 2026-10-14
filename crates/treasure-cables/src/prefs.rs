//! Préférences de l'utilisateur, conservées d'un lancement à l'autre.
//!
//! La fenêtre change d'adresse (port aléatoire) à chaque lancement : la
//! mémoire du navigateur, liée à l'adresse, ne survivrait pas. Le fichier vit
//! donc dans le dossier de configuration de l'utilisateur, pas à côté de
//! l'exécutable portable.

use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// Suit le thème du système.
    #[default]
    Auto,
    Light,
    Dark,
}

impl Theme {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Theme::Auto),
            "light" => Some(Theme::Light),
            "dark" => Some(Theme::Dark),
            _ => None,
        }
    }

    /// Valeur de l'attribut `data-theme` de la page, sauf en automatique.
    pub fn attribute(self) -> Option<&'static str> {
        match self {
            Theme::Auto => None,
            Theme::Light => Some("light"),
            Theme::Dark => Some("dark"),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Prefs {
    #[serde(default)]
    pub theme: Theme,
}

fn path() -> Option<PathBuf> {
    let dir = if cfg!(windows) {
        PathBuf::from(std::env::var_os("APPDATA")?).join("Treasure Cables")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?
            .join("treasure-cables")
    };
    Some(dir.join("preferences.json"))
}

/// Préférences enregistrées, ou valeurs par défaut (fichier absent ou illisible).
pub fn load() -> Prefs {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(prefs: &Prefs) -> io::Result<()> {
    let path = path().ok_or_else(|| io::Error::other("dossier de configuration introuvable"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(prefs).map_err(io::Error::other)?;
    std::fs::write(path, json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_or_missing_fields_fall_back_to_defaults() {
        let prefs: Prefs = serde_json::from_str("{}").unwrap();
        assert_eq!(prefs.theme, Theme::Auto);
        let prefs: Prefs = serde_json::from_str(r#"{"theme":"dark","autre":1}"#).unwrap();
        assert_eq!(prefs.theme, Theme::Dark);
        assert_eq!(Theme::parse("light"), Some(Theme::Light));
        assert_eq!(Theme::parse("rose"), None);
    }
}
