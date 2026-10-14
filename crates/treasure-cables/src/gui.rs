//! Interface graphique : l'exécutable sert l'interface web (embarquée) sur
//! 127.0.0.1 et l'ouvre dans une fenêtre Edge en mode application.
//!
//! Sécurité : écoute uniquement en local, sur un port aléatoire ; chaque
//! appel d'API exige le jeton aléatoire transmis à la fenêtre, et l'en-tête
//! Host doit désigner ce serveur (protection contre le « DNS rebinding »).
//!
//! Tout tourne sur un seul fil : entre deux requêtes, le serveur fait avancer
//! le test de câble et le diagnostic approfondi. Il s'arrête quand la fenêtre
//! est fermée (signal de départ, ou plus de nouvelles depuis 3 minutes).

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::time::{Duration, Instant};

use serde::Serialize;
use tiny_http::{Header, Method, Request, Response, Server};
use treasure_audit::cable_test::{self, CableVerdict};
use treasure_collect::{ChangeWatcher, DeepCapture, Options};
use treasure_model::Report;
use treasure_present::Presentation;

use crate::export;
use crate::prefs::{self, Prefs, Theme};
use crate::session::{self, CableWatch, DEEP_TIMEOUT};

mod assets {
    include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));
}

/// Sans nouvelles de la fenêtre pendant ce délai, l'outil s'arrête. Les
/// navigateurs ralentissent les minuteries des fenêtres réduites (une par
/// minute au plus) : le délai doit rester large.
const IDLE_TIMEOUT: Duration = Duration::from_secs(180);
/// Après un branchement, délai sans nouvel événement avant de relancer
/// l'analyse : l'appareil finit de se présenter (pilotes, vitesse).
const CHANGE_SETTLE: Duration = Duration::from_millis(1500);
/// Durée maximale d'une attente de la fenêtre (`/api/wait`).
const WAIT_TIMEOUT: Duration = Duration::from_secs(25);

pub struct Config {
    pub options: Options,
    pub start_deep: bool,
    pub open_window: bool,
    /// Rapport chargé depuis un fichier : fonctions en direct désactivées.
    pub fixed_report: Option<Report>,
}

pub fn run(config: Config) -> io::Result<()> {
    let server = Server::http("127.0.0.1:0").map_err(io::Error::other)?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .ok_or_else(|| io::Error::other("adresse locale indisponible"))?;
    let token = random_token();
    let url = format!("http://127.0.0.1:{port}/?t={token}");

    let live = config.fixed_report.is_none();
    let mut app = App {
        options: config.options,
        report: Err("Analyse en cours…".to_owned()),
        live,
        cable: None,
        cable_results: Vec::new(),
        deep: None,
        last_contact: Instant::now(),
        closing_since: None,
        exit: false,
        message: None,
        prefs: prefs::load(),
        watcher: None,
        seen_changes: 0,
        pending_change: None,
        version: 0,
        waiters: Vec::new(),
    };
    if live {
        app.watcher = ChangeWatcher::start();
        app.seen_changes = app.watcher.as_ref().map_or(0, ChangeWatcher::count);
    }
    app.report = match config.fixed_report {
        Some(report) => Ok(report),
        None => treasure_collect::collect(&app.options).map_err(|e| e.to_string()),
    };
    if config.start_deep && live {
        app.start_deep();
    }

    let opened = config.open_window && open_window(&url);
    if !opened {
        crate::platform::attach_console();
        println!("Treasure Cables est disponible à l'adresse :\n  {url}");
        println!("(Ctrl+C pour quitter)");
        if config.open_window {
            // Aucun navigateur n'a pu s'ouvrir : on donne l'adresse, sans
            // bloquer le serveur.
            let text = format!("Ouvrez cette adresse dans votre navigateur :\n{url}");
            std::thread::spawn(move || crate::platform::message(&text));
        }
    }

    let host_ok = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    while !app.exit {
        match server.recv_timeout(Duration::from_millis(200)) {
            Ok(Some(request)) => app.handle(request, &token, &host_ok),
            Ok(None) => {}
            Err(e) => return Err(e),
        }
        app.tick();
        if app
            .closing_since
            .is_some_and(|t| t.elapsed() > Duration::from_secs(3))
        {
            break; // l'écoute éventuelle est arrêtée proprement à la sortie
        }
        if opened
            && app.last_contact.elapsed() > IDLE_TIMEOUT
            && app.cable.is_none()
            && app.deep.is_none()
        {
            break;
        }
    }
    Ok(())
}

struct App {
    options: Options,
    report: Result<Report, String>,
    live: bool,
    cable: Option<CableWatch>,
    cable_results: Vec<CableVerdict>,
    deep: Option<(DeepCapture, Instant)>,
    last_contact: Instant,
    /// Fermeture annoncée par la fenêtre ; annulée si elle se manifeste
    /// de nouveau (simple rechargement de la page).
    closing_since: Option<Instant>,
    exit: bool,
    /// Message ponctuel pour l'interface (affiché une fois).
    message: Option<Message>,
    prefs: Prefs,
    /// Branchements et débranchements : l'analyse est relancée seule.
    watcher: Option<ChangeWatcher>,
    seen_changes: u64,
    /// Dernier changement signalé, pas encore analysé.
    pending_change: Option<Instant>,
    /// Version du rapport, augmentée à chaque analyse automatique.
    version: u64,
    /// Requêtes de la fenêtre laissées ouvertes jusqu'à la prochaine version.
    waiters: Vec<(Request, Instant)>,
}

#[derive(Serialize, Clone)]
struct Message {
    level: &'static str,
    text: String,
}

#[derive(Serialize)]
struct State<'a> {
    presentation: Option<Presentation>,
    error: Option<&'a str>,
    /// Faux pour un rapport chargé depuis un fichier.
    live: bool,
    platform: &'static str,
    elevated: bool,
    build: &'static str,
    cable: Option<CableState>,
    cable_results: &'a [CableVerdict],
    deep: Option<DeepState>,
    message: Option<Message>,
    theme: Theme,
    version: u64,
}

#[derive(Serialize)]
struct CableState {
    remaining_s: u64,
    detected: Vec<String>,
    listens_link_errors: bool,
}

#[derive(Serialize)]
struct DeepState {
    elapsed_s: u32,
    events: u32,
}

impl App {
    fn handle(&mut self, request: Request, token: &str, host_ok: &[String]) {
        let url = request.url().to_owned();
        let path = url.split('?').next().unwrap_or("/").to_owned();
        let host = header(&request, "Host").unwrap_or_default();
        if !host_ok.iter().any(|h| h.eq_ignore_ascii_case(&host)) {
            return respond(request, 403, "text/plain", b"Forbidden".to_vec());
        }

        if !path.starts_with("/api/") {
            let name = if path == "/" {
                "index.html"
            } else {
                path.trim_start_matches('/')
            };
            return match assets::ASSETS.iter().find(|(n, _)| *n == name) {
                // Le thème choisi est posé dès la page servie : pas
                // d'éclair clair avant l'application du thème sombre.
                Some((n, data)) if *n == "index.html" => {
                    let html = String::from_utf8_lossy(data);
                    let html = match self.prefs.theme.attribute() {
                        Some(theme) => html.replacen(
                            "<html lang=\"fr\">",
                            &format!("<html lang=\"fr\" data-theme=\"{theme}\">"),
                            1,
                        ),
                        None => html.into_owned(),
                    };
                    respond(request, 200, mime(n), html.into_bytes())
                }
                Some((n, data)) => respond(request, 200, mime(n), data.to_vec()),
                None => respond(request, 404, "text/plain", b"Not found".to_vec()),
            };
        }

        if header(&request, "X-Token").as_deref() != Some(token) {
            return respond(request, 401, "text/plain", b"Unauthorized".to_vec());
        }
        self.last_contact = Instant::now();
        self.closing_since = None;
        let method = request.method().clone();
        match (method, path.as_str()) {
            (Method::Get, "/api/state") | (Method::Post, "/api/heartbeat") => {}
            // Attente d'une nouvelle analyse : la réponse part au prochain
            // branchement ou débranchement (ou au bout de WAIT_TIMEOUT).
            (Method::Get, "/api/wait") => {
                let since = url
                    .split_once("v=")
                    .and_then(|(_, v)| v.split('&').next())
                    .and_then(|v| v.parse::<u64>().ok());
                if self.watcher.is_some() && since == Some(self.version) {
                    self.waiters.push((request, Instant::now()));
                    return;
                }
            }
            (Method::Post, "/api/bye") => {
                // La fenêtre se ferme... ou se recharge : on attend un peu.
                self.closing_since = Some(Instant::now());
                return respond(request, 204, "text/plain", Vec::new());
            }
            (Method::Post, "/api/refresh") if self.live => {
                self.report = self.collect_keeping_diagnostics();
            }
            (Method::Post, "/api/export") => self.export(),
            (Method::Post, "/api/cable/start") if self.live => {
                match CableWatch::start(&self.options) {
                    Ok(watch) => self.cable = Some(watch),
                    Err(e) => self.error(format!("Analyse impossible : {e}")),
                }
            }
            (Method::Post, "/api/cable/finish") => self.finish_cable(),
            (Method::Post, "/api/cable/cancel") => self.cable = None,
            (Method::Post, "/api/cable/clear") => self.cable_results.clear(),
            (Method::Post, "/api/deep/start") if self.live => self.start_deep(),
            (Method::Post, "/api/deep/finish") => self.finish_deep(),
            (Method::Post, p) if p.starts_with("/api/theme/") => {
                match Theme::parse(&p["/api/theme/".len()..]) {
                    Some(theme) => {
                        self.prefs.theme = theme;
                        // Un échec d'écriture ne gêne que le prochain lancement.
                        let _ = prefs::save(&self.prefs);
                    }
                    None => return respond(request, 400, "text/plain", b"Bad request".to_vec()),
                }
            }
            (Method::Post, "/api/elevate") => match treasure_collect::relaunch_elevated("--deep") {
                Ok(()) => {
                    self.message = Some(Message {
                        level: "ok",
                        text: "Treasure Cables continue dans la fenêtre administrateur.".to_owned(),
                    });
                    self.exit = true;
                }
                Err(e) => self.error(e.to_string()),
            },
            _ => return respond(request, 404, "text/plain", b"Not found".to_vec()),
        }
        let body = self.state_json();
        respond(request, 200, "application/json", body.into_bytes());
    }

    fn state_json(&mut self) -> String {
        let state = State {
            presentation: self.report.as_ref().ok().map(treasure_present::present),
            error: self.report.as_ref().err().map(String::as_str),
            live: self.live,
            platform: std::env::consts::OS,
            elevated: treasure_collect::is_elevated(),
            build: env!("TREASURE_BUILD"),
            cable: self.cable.as_ref().map(|w| CableState {
                remaining_s: w.remaining_s(),
                detected: w.detected().to_vec(),
                listens_link_errors: w.listens_link_errors(),
            }),
            cable_results: &self.cable_results,
            deep: self.deep.as_ref().map(|(capture, _)| DeepState {
                elapsed_s: capture.elapsed_s(),
                events: capture.events(),
            }),
            message: self.message.take(),
            theme: self.prefs.theme,
            version: self.version,
        };
        serde_json::to_string(&state).unwrap_or_else(|_| "{}".to_owned())
    }

    fn tick(&mut self) {
        if let Some(watcher) = &self.watcher {
            let count = watcher.count();
            if count != self.seen_changes {
                self.seen_changes = count;
                self.pending_change = Some(Instant::now());
            }
        }
        if self
            .pending_change
            .is_some_and(|t| t.elapsed() >= CHANGE_SETTLE)
        {
            self.pending_change = None;
            // Pendant un test de câble ou une écoute, c'est leur propre
            // analyse finale qui compte (et qui sert de point de comparaison).
            if self.cable.is_none() && self.deep.is_none() {
                self.auto_refresh();
            }
        }
        if self
            .waiters
            .iter()
            .any(|(_, since)| since.elapsed() >= WAIT_TIMEOUT)
        {
            let (expired, waiting) = std::mem::take(&mut self.waiters)
                .into_iter()
                .partition(|(_, since)| since.elapsed() >= WAIT_TIMEOUT);
            self.waiters = waiting;
            self.answer(expired);
        }

        if let Some(watch) = &mut self.cable {
            watch.poll(&self.options);
            if watch.is_done() {
                self.finish_cable();
            }
        }
        if self
            .deep
            .as_ref()
            .is_some_and(|(_, started)| started.elapsed() >= DEEP_TIMEOUT)
        {
            self.finish_deep();
        }
    }

    /// Nouvelle analyse après un branchement ou un débranchement, annoncée
    /// à la fenêtre avec les appareils apparus ou disparus.
    fn auto_refresh(&mut self) {
        let Ok(after) = self.collect_keeping_diagnostics() else {
            return; // l'analyse manuelle reste possible
        };
        if let Ok(before) = &self.report {
            let names = |devices: Vec<&treasure_model::Device>| {
                devices
                    .iter()
                    .map(|d| d.display_name())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let added = names(cable_test::new_devices(before, &after));
            let removed = names(cable_test::new_devices(&after, before));
            let mut parts = Vec::new();
            if !added.is_empty() {
                parts.push(format!("branché : {added}"));
            }
            if !removed.is_empty() {
                parts.push(format!("débranché : {removed}"));
            }
            if !parts.is_empty() {
                self.message = Some(Message {
                    level: "ok",
                    text: parts.join(" · "),
                });
            }
        }
        self.report = Ok(after);
        self.version += 1;
        let waiters = std::mem::take(&mut self.waiters);
        self.answer(waiters);
    }

    /// Nouvelle analyse ; le bilan d'un diagnostic approfondi passé est
    /// conservé (il porte sur la période d'écoute, toujours valable).
    fn collect_keeping_diagnostics(&self) -> Result<Report, String> {
        let mut report = treasure_collect::collect(&self.options).map_err(|e| e.to_string())?;
        if let Ok(previous) = &self.report {
            report.link_diagnostics = previous.link_diagnostics.clone();
        }
        Ok(report)
    }

    fn answer(&mut self, waiters: Vec<(Request, Instant)>) {
        if waiters.is_empty() {
            return;
        }
        let body = self.state_json();
        for (request, _) in waiters {
            respond(request, 200, "application/json", body.clone().into_bytes());
        }
    }

    fn finish_cable(&mut self) {
        let Some(watch) = self.cable.take() else {
            return;
        };
        match watch.finish(&self.options) {
            Ok((verdict, report)) => {
                self.cable_results.push(verdict);
                self.report = Ok(report);
            }
            Err(e) => self.error(format!("Analyse impossible : {e}")),
        }
    }

    fn start_deep(&mut self) {
        if cfg!(windows) && !treasure_collect::is_elevated() {
            self.message = Some(Message {
                level: "elevation",
                text: "Le diagnostic approfondi demande les droits administrateur.".to_owned(),
            });
            return;
        }
        match treasure_collect::start_deep_capture() {
            Ok(capture) => self.deep = Some((capture, Instant::now())),
            Err(e) => self.error(e.to_string()),
        }
    }

    fn finish_deep(&mut self) {
        let Some((capture, _)) = self.deep.take() else {
            return;
        };
        let before = self.report.as_ref().ok().cloned();
        match session::finish_deep(capture, before.as_ref(), &self.options) {
            Ok(report) => self.report = Ok(report),
            Err(e) => self.error(format!("Analyse impossible : {e}")),
        }
    }

    fn export(&mut self) {
        self.message = Some(match &self.report {
            Ok(report) => match export::export(report) {
                Ok(path) => Message {
                    level: "ok",
                    text: format!("Rapport enregistré : {}", path.display()),
                },
                Err(e) => Message {
                    level: "error",
                    text: format!("Export impossible : {e}"),
                },
            },
            Err(_) => Message {
                level: "error",
                text: "Rien à exporter : l'analyse a échoué.".to_owned(),
            },
        });
    }

    fn error(&mut self, text: String) {
        self.message = Some(Message {
            level: "error",
            text,
        });
    }
}

fn header(request: &Request, name: &str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str().to_owned())
}

fn respond(request: Request, status: u16, content_type: &str, body: Vec<u8>) {
    let headers = [
        ("Content-Type", content_type),
        ("Cache-Control", "no-store"),
        ("X-Content-Type-Options", "nosniff"),
        ("Referrer-Policy", "no-referrer"),
        (
            "Content-Security-Policy",
            "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'",
        ),
    ];
    let mut response = Response::from_data(body).with_status_code(status);
    for (name, value) in headers {
        if let Ok(h) = Header::from_bytes(name.as_bytes(), value.as_bytes()) {
            response = response.with_header(h);
        }
    }
    let _ = request.respond(response);
}

fn mime(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

/// 128 bits imprévisibles : les clés de `RandomState` sont tirées de
/// l'aléa du système d'exploitation.
fn random_token() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or_default();
    (0..2)
        .map(|i| {
            let mut h = RandomState::new().build_hasher();
            h.write_u64(nanos ^ i);
            format!("{:016x}", h.finish())
        })
        .collect()
}

/// Ouvre l'interface dans une fenêtre d'application (Edge, sinon Chrome ou
/// Chromium), à défaut dans le navigateur par défaut.
fn open_window(url: &str) -> bool {
    use std::process::Command;
    let app_flag = format!("--app={url}");
    let profile = std::env::temp_dir().join("treasure-cables-fenetre");
    let profile_flag = format!("--user-data-dir={}", profile.display());
    let args = [
        app_flag.as_str(),
        profile_flag.as_str(),
        "--window-size=1180,860",
        "--no-first-run",
        "--no-default-browser-check",
    ];

    for browser in app_browsers() {
        if Command::new(&browser).args(args).spawn().is_ok() {
            return true;
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .is_ok()
    }
    #[cfg(not(windows))]
    {
        Command::new("xdg-open").arg(url).spawn().is_ok()
    }
}

fn app_browsers() -> Vec<std::path::PathBuf> {
    #[cfg(windows)]
    {
        let mut candidates = Vec::new();
        for var in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(var) {
                let base = std::path::PathBuf::from(base);
                candidates.push(base.join(r"Microsoft\Edge\Application\msedge.exe"));
                candidates.push(base.join(r"Google\Chrome\Application\chrome.exe"));
            }
        }
        candidates.into_iter().filter(|p| p.exists()).collect()
    }
    #[cfg(not(windows))]
    {
        [
            "microsoft-edge",
            "google-chrome",
            "chromium",
            "chromium-browser",
        ]
        .iter()
        .filter_map(|name| {
            std::env::var_os("PATH").and_then(|paths| {
                std::env::split_paths(&paths)
                    .map(|dir| dir.join(name))
                    .find(|p| p.exists())
            })
        })
        .collect()
    }
}
