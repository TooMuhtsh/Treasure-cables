//! Collecte des informations USB auprès du système d'exploitation.
//!
//! Chaque plateforme a son module ; tous produisent le même [`Report`],
//! que l'audit et la future version web savent lire.

pub mod bos;
pub mod deep;
pub mod watch;
pub use watch::ChangeWatcher;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(windows)]
pub mod windows;
#[cfg(windows)]
mod windows_display;
#[cfg(windows)]
mod windows_etw;
#[cfg(windows)]
mod windows_gpu;

#[cfg(windows)]
pub use windows_etw::{DeepCapture, is_elevated, relaunch_elevated};

/// Démarre le diagnostic approfondi : écoute de la pile USB pendant que
/// l'utilisateur branche et débranche (droits administrateur).
#[cfg(windows)]
pub fn start_deep_capture() -> Result<DeepCapture, Error> {
    DeepCapture::start(windows::controller_buses())
}

#[cfg(not(windows))]
pub use deep_unsupported::{DeepCapture, is_elevated, relaunch_elevated, start_deep_capture};

#[cfg(not(windows))]
mod deep_unsupported {
    use treasure_model::LinkDiagnostics;

    use crate::Error;

    const MESSAGE: &str =
        "le diagnostic approfondi n'est disponible que sous Windows pour l'instant";

    pub struct DeepCapture;

    impl DeepCapture {
        pub fn events(&self) -> u32 {
            0
        }
        pub fn elapsed_s(&self) -> u32 {
            0
        }
        pub fn stop(self) -> LinkDiagnostics {
            LinkDiagnostics::default()
        }
    }

    pub fn is_elevated() -> bool {
        false
    }

    pub fn relaunch_elevated(_arguments: &str) -> Result<(), Error> {
        Err(Error::Unsupported(MESSAGE))
    }

    pub fn start_deep_capture() -> Result<DeepCapture, Error> {
        Err(Error::Unsupported(MESSAGE))
    }
}

use treasure_model::{HostInfo, Report, SCHEMA_VERSION, ToolInfo};

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Inclure les numéros de série (désactivé par défaut : un rapport
    /// est fait pour être partagé).
    pub include_serials: bool,
    /// Collecte légère pour la surveillance répétée (test de câble) : pas de
    /// lecture de descripteurs auprès des appareils, seulement l'état des ports.
    pub light: bool,
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
            Error::Unsupported(message) => f.write_str(message),
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
    let mut report = platform_collect(options)?;
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
fn platform_collect(_options: &Options) -> Result<Report, Error> {
    linux::collect_from(std::path::Path::new("/sys"))
}

#[cfg(windows)]
fn platform_collect(options: &Options) -> Result<Report, Error> {
    windows::collect(options.light)
}

#[cfg(not(any(target_os = "linux", windows)))]
fn platform_collect(_options: &Options) -> Result<Report, Error> {
    Err(Error::Unsupported("plateforme non prise en charge"))
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
        link_diagnostics: None,
        notes: Vec::new(),
    }
}
