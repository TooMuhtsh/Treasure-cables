//! Collecte des informations USB auprès du système d'exploitation.
//!
//! Chaque plateforme a son module ; tous produisent le même [`Report`],
//! que l'audit et la future version web savent lire.

pub mod bos;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(windows)]
pub mod windows;
#[cfg(windows)]
mod windows_display;
#[cfg(windows)]
mod windows_gpu;

use treasure_model::{HostInfo, Report, SCHEMA_VERSION, ToolInfo};

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Inclure les numéros de série (désactivé par défaut : un rapport
    /// est fait pour être partagé).
    pub include_serials: bool,
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Unsupported(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "erreur d'accès au système : {e}"),
            Error::Unsupported(os) => write!(f, "plateforme non prise en charge : {os}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub fn collect(options: &Options) -> Result<Report, Error> {
    let mut report = platform_collect()?;
    if !options.include_serials {
        report.redact();
    }
    report.generated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs());
    Ok(report)
}

#[cfg(target_os = "linux")]
fn platform_collect() -> Result<Report, Error> {
    linux::collect_from(std::path::Path::new("/sys"))
}

#[cfg(windows)]
fn platform_collect() -> Result<Report, Error> {
    windows::collect()
}

#[cfg(not(any(target_os = "linux", windows)))]
fn platform_collect() -> Result<Report, Error> {
    Err(Error::Unsupported(std::env::consts::OS))
}

pub(crate) fn empty_report(os_version: Option<String>) -> Report {
    Report {
        schema_version: SCHEMA_VERSION,
        tool: ToolInfo {
            name: "treasure-cables".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        },
        host: HostInfo {
            os: std::env::consts::OS.to_owned(),
            os_version,
        },
        generated_at: None,
        buses: Vec::new(),
        typec_ports: Vec::new(),
        displays: Vec::new(),
        usb4_routers: Vec::new(),
        notes: Vec::new(),
    }
}
