//! Test d'un câble avec une sonde au bout (l'application Treasure Probe sur
//! un téléphone) : client du protocole décrit dans docs/protocole-sonde.md.
//!
//! Le test tourne sur son propre fil (connexions et mesures bloquantes) ; la
//! fenêtre suit son avancement et récupère le verdict à la fin.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};
use treasure_audit::cable_test::CableVerdict;
use treasure_audit::probe_test::{self, ProbeMeasurements};
use treasure_collect::Options;
use treasure_model::Report;

pub const CONTROL_PORT: u16 = 47820;
pub const DATA_PORT: u16 = 47821;
const PROTOCOL: u64 = 1;
/// Durée de chaque mesure de débit.
const MEASURE: Duration = Duration::from_secs(4);
/// Le téléphone doit être trouvé dans ce délai.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(120);
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(60);
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);
/// Variable d'environnement qui désigne directement une sonde
/// ("192.168.42.129" ou "hôte:port"), pour le développement.
pub const PROBE_ENV: &str = "TREASURE_PROBE";

#[derive(Debug, Clone, Serialize)]
pub struct ProbeProgress {
    /// `searching`, `approval` ou `measuring`.
    pub phase: &'static str,
    pub text: String,
    /// Nom de la sonde, une fois connue ("samsung SM-S911B").
    pub probe: Option<String>,
    /// Ce que voit la recherche (carte réseau du partage, réponse de l'application).
    pub detail: Option<String>,
}

pub struct ProbeTest {
    progress: Arc<Mutex<ProbeProgress>>,
    cancel: Arc<AtomicBool>,
    handle: JoinHandle<Result<(CableVerdict, Report), String>>,
}

impl ProbeTest {
    pub fn start(options: &Options) -> Self {
        let progress = Arc::new(Mutex::new(ProbeProgress {
            phase: "searching",
            text: "Recherche du téléphone…".to_owned(),
            probe: None,
            detail: None,
        }));
        let cancel = Arc::new(AtomicBool::new(false));
        let session = Session {
            options: options.clone(),
            progress: progress.clone(),
            cancel: cancel.clone(),
        };
        let handle = std::thread::Builder::new()
            .name("test-sonde".into())
            .spawn(move || session.run())
            .expect("création du fil de test");
        ProbeTest {
            progress,
            cancel,
            handle,
        }
    }

    pub fn progress(&self) -> ProbeProgress {
        self.progress
            .lock()
            .map(|p| p.clone())
            .unwrap_or(ProbeProgress {
                phase: "measuring",
                text: String::new(),
                probe: None,
                detail: None,
            })
    }

    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }

    /// Verdict et nouveau rapport ; à n'appeler qu'une fois le test fini.
    pub fn finish(self) -> Result<(CableVerdict, Report), String> {
        self.handle
            .join()
            .unwrap_or_else(|_| Err("Le test s'est interrompu.".to_owned()))
    }

    /// Le fil s'arrête de lui-même à la prochaine vérification.
    pub fn cancel(self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

struct Session {
    options: Options,
    progress: Arc<Mutex<ProbeProgress>>,
    cancel: Arc<AtomicBool>,
}

/// Erreur qui met fin au test (message pour l'utilisateur).
type Fail = String;

impl Session {
    fn set(&self, phase: &'static str, text: &str) {
        if let Ok(mut p) = self.progress.lock() {
            p.phase = phase;
            p.text = text.to_owned();
        }
    }

    fn cancelled(&self) -> Result<(), Fail> {
        if self.cancel.load(Ordering::Relaxed) {
            Err("Test annulé.".to_owned())
        } else {
            Ok(())
        }
    }

    fn run(self) -> Result<(CableVerdict, Report), Fail> {
        let (stream, address) = self.search()?;
        let mut control = Control::new(stream)?;

        control.send(&json!({
            "type": "hello",
            "protocol": PROTOCOL,
            "software": "Treasure Cables",
            "version": env!("CARGO_PKG_VERSION"),
            "computer": computer_name(),
        }))?;
        let hello = control.expect("hello", REPLY_TIMEOUT)?;
        if hello["protocol"].as_u64() != Some(PROTOCOL) {
            return Err("L'application du téléphone n'est pas à la même version : mettez à jour Treasure Cables et Treasure Probe.".to_owned());
        }
        let manufacturer = hello["device"]["manufacturer"].as_str().unwrap_or_default();
        let model = hello["device"]["model"].as_str().unwrap_or_default();
        let probe_name = format!("{manufacturer} {model}").trim().to_owned();
        let probe_name = if probe_name.is_empty() {
            "Le téléphone".to_owned()
        } else {
            probe_name
        };
        if let Ok(mut p) = self.progress.lock() {
            p.probe = Some(probe_name.clone());
        }

        self.set("approval", "Acceptez le test sur l'écran du téléphone.");
        let session = loop {
            self.cancelled()?;
            match control.next(Duration::from_secs(1)) {
                Ok(Some(m)) if m["type"] == "accepted" => {
                    break m["session"].as_str().unwrap_or_default().to_owned();
                }
                Ok(Some(m)) if m["type"] == "refused" => {
                    return Err("Le test a été refusé sur le téléphone.".to_owned());
                }
                Ok(_) if control.since_last() < APPROVAL_TIMEOUT => {}
                Ok(_) => {
                    return Err("Le test n'a pas été accepté sur le téléphone à temps.".to_owned());
                }
                Err(e) => return Err(e),
            }
        };

        // Écoute des erreurs de lien pendant les mesures (administrateur).
        let capture = treasure_collect::is_elevated()
            .then(treasure_collect::start_deep_capture)
            .and_then(Result::ok);

        let mut measures = ProbeMeasurements {
            probe_name: probe_name.clone(),
            ..Default::default()
        };
        let mut charge = Vec::new();
        charge.extend(self.battery(&mut control));

        for (direction, label, text) in [
            (
                "to_computer",
                "vers l'ordinateur",
                "Mesure du débit du téléphone vers l'ordinateur…",
            ),
            (
                "to_probe",
                "vers le téléphone",
                "Mesure du débit de l'ordinateur vers le téléphone…",
            ),
        ] {
            self.cancelled()?;
            self.set("measuring", text);
            let _ = control.send(&json!({"type": "status", "text": text}));
            match measure(address.ip(), &session, direction) {
                Ok(mbps) if direction == "to_computer" => measures.to_computer_mbps = Some(mbps),
                Ok(mbps) => measures.to_probe_mbps = Some(mbps),
                Err(_) => {
                    measures.dropped_during = Some(label.to_owned());
                    break;
                }
            }
            charge.extend(self.battery(&mut control));
        }

        self.set("measuring", "Analyse du résultat…");
        let link = capture.map(treasure_collect::DeepCapture::stop);
        let mut report = treasure_collect::collect(&self.options)
            .map_err(|e| format!("Analyse impossible : {e}"))?;
        if let Some(mut link) = link {
            treasure_collect::deep::complete(&mut link, None, &report);
            measures.link_errors = link.ports.iter().map(|p| p.link_errors()).sum();
            report.link_diagnostics = Some(link);
        }
        // La dernière mesure de charge reflète le courant pendant l'échange.
        measures.charge_current_ma = charge.last().copied();
        measures.device = probe_test::find_probe_device(&report, manufacturer, model);
        let verdict = probe_test::evaluate(&measures);

        let severity = serde_json::to_value(verdict.severity).unwrap_or(Value::Null);
        let _ = control.send(&json!({
            "type": "verdict",
            "severity": severity,
            "title": verdict.title,
            "detail": verdict.detail,
        }));
        let _ = control.send(&json!({"type": "bye"}));
        Ok((verdict, report))
    }

    /// Cherche une sonde sur les partages de connexion USB, jusqu'à la
    /// trouver ou jusqu'au délai.
    fn search(&self) -> Result<(TcpStream, SocketAddr), Fail> {
        let started = Instant::now();
        loop {
            self.cancelled()?;
            let candidates = candidates();
            let mut seen = Vec::new();
            for candidate in &candidates {
                if candidate.address.ip().is_unspecified() {
                    seen.push((candidate, std::io::ErrorKind::AddrNotAvailable));
                    continue;
                }
                match TcpStream::connect_timeout(&candidate.address, Duration::from_millis(800)) {
                    Ok(stream) => {
                        self.set_detail(None);
                        return Ok((stream, candidate.address));
                    }
                    Err(e) if candidate.usb => seen.push((candidate, e.kind())),
                    Err(_) => {}
                }
            }
            let detail = search_detail(&seen);
            self.set_detail(Some(detail.clone()));
            if started.elapsed() > SEARCH_TIMEOUT {
                return Err(format!(
                    "Le téléphone n'a pas été trouvé. {detail} Vérifiez que l'application Treasure Probe est ouverte et que le partage de connexion USB est activé. Si le téléphone ne propose pas ce partage, ou indique « pas de transfert de données », le câble ne transporte probablement pas les données."
                ));
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    fn set_detail(&self, detail: Option<String>) {
        if let Ok(mut p) = self.progress.lock() {
            p.detail = detail;
        }
    }

    /// Courant de charge en mA, si la sonde le connaît.
    fn battery(&self, control: &mut Control) -> Option<i64> {
        control.send(&json!({"type": "battery"})).ok()?;
        let reply = control.expect("battery", REPLY_TIMEOUT).ok()?;
        reply["current_ua"].as_i64().map(|ua| ua / 1000)
    }
}

/// Une adresse où chercher une sonde.
struct Candidate {
    address: SocketAddr,
    /// Carte réseau d'un partage de connexion USB (sinon : essai de secours).
    usb: bool,
    /// Description de la carte réseau.
    label: String,
}

/// Adresses où chercher une sonde : les passerelles des partages USB
/// d'abord, puis celles des autres cartes actives, au cas où Windows nomme
/// la carte du partage autrement que prévu (la poignée de main confirme
/// qu'il s'agit bien d'une sonde). Une carte de partage sans passerelle est
/// signalée avec une adresse non spécifiée.
fn candidates() -> Vec<Candidate> {
    if let Ok(target) = std::env::var(PROBE_ENV) {
        let target = if target.contains(':') {
            target
        } else {
            format!("{target}:{CONTROL_PORT}")
        };
        return target
            .to_socket_addrs()
            .map(|a| a.collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .map(|address| Candidate {
                address,
                usb: true,
                label: PROBE_ENV.to_owned(),
            })
            .collect();
    }
    let mut adapters: Vec<_> = treasure_collect::network::adapters()
        .into_iter()
        .filter(|a| a.up || a.usb_tether)
        .collect();
    adapters.sort_by_key(|a| !a.usb_tether);
    let mut out: Vec<Candidate> = Vec::new();
    for adapter in adapters {
        let label = if adapter.description.is_empty() {
            adapter.name.clone()
        } else {
            adapter.description.clone()
        };
        if adapter.usb_tether && adapter.gateways.is_empty() {
            out.push(Candidate {
                address: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), CONTROL_PORT),
                usb: true,
                label,
            });
            continue;
        }
        for ip in adapter.gateways {
            if !out.iter().any(|c| c.address.ip() == IpAddr::V4(ip)) {
                out.push(Candidate {
                    address: SocketAddr::new(IpAddr::V4(ip), CONTROL_PORT),
                    usb: adapter.usb_tether,
                    label: label.clone(),
                });
            }
        }
    }
    out
}

/// Explique l'état de la recherche, d'après les cartes du partage USB vues.
fn search_detail(seen: &[(&Candidate, std::io::ErrorKind)]) -> String {
    let Some((candidate, error)) = seen.first() else {
        return "L'ordinateur ne voit aucune carte réseau de partage de connexion USB.".to_owned();
    };
    let label = &candidate.label;
    let ip = candidate.address.ip();
    if ip.is_unspecified() {
        return format!(
            "Partage USB vu (« {label} »), mais le téléphone ne lui a pas encore donné d'adresse."
        );
    }
    if *error == std::io::ErrorKind::ConnectionRefused {
        format!(
            "Partage USB vu (« {label} », téléphone en {ip}), mais l'application ne répond pas : affiche-t-elle « Prêt à tester le câble » ?"
        )
    } else {
        format!("Partage USB vu (« {label} », téléphone en {ip}), mais le téléphone ne répond pas.")
    }
}

/// Une mesure de débit sur le port de données ; renvoie des Mb/s. Une erreur
/// signifie que la liaison a lâché pendant la mesure.
fn measure(ip: IpAddr, session: &str, direction: &str) -> std::io::Result<f64> {
    let data_port = std::env::var(PROBE_ENV)
        .ok()
        .and_then(|t| t.rsplit_once(':').and_then(|(_, p)| p.parse::<u16>().ok()))
        .map_or(DATA_PORT, |control| control + 1);
    let mut stream =
        TcpStream::connect_timeout(&SocketAddr::new(ip, data_port), Duration::from_secs(3))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let header = json!({
        "session": session,
        "direction": direction,
        "duration_ms": MEASURE.as_millis() as u64,
    });
    stream.write_all(format!("{header}\n").as_bytes())?;

    let interrupted = || std::io::Error::other("liaison interrompue");
    if direction == "to_computer" {
        let started = Instant::now();
        let mut buffer = vec![0u8; 256 * 1024];
        let mut total = 0u64;
        loop {
            match stream.read(&mut buffer)? {
                0 => break,
                n => total += n as u64,
            }
        }
        let elapsed = started.elapsed();
        // Une fin bien avant la durée prévue est une coupure.
        if elapsed < MEASURE / 2 || total == 0 {
            return Err(interrupted());
        }
        Ok(total as f64 * 8.0 / elapsed.as_secs_f64() / 1e6)
    } else {
        let chunk = vec![0u8; 256 * 1024];
        let started = Instant::now();
        while started.elapsed() < MEASURE {
            stream.write_all(&chunk)?;
        }
        stream.shutdown(Shutdown::Write)?;
        // La sonde compte ce qu'elle a reçu et sur quelle durée.
        stream.set_read_timeout(Some(REPLY_TIMEOUT))?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line)?;
        let reply: Value = serde_json::from_str(line.trim()).map_err(|_| interrupted())?;
        let received = reply["received"].as_u64().ok_or_else(interrupted)?;
        let elapsed_ms = reply["elapsed_ms"]
            .as_u64()
            .filter(|&ms| ms > 0)
            .ok_or_else(interrupted)?;
        Ok(received as f64 * 8.0 / (elapsed_ms as f64 / 1000.0) / 1e6)
    }
}

/// Connexion de contrôle : une ligne de JSON par message.
struct Control {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
    last: Instant,
}

impl Control {
    fn new(stream: TcpStream) -> Result<Self, Fail> {
        let lost = |_| "La connexion avec le téléphone a été perdue.".to_owned();
        let writer = stream.try_clone().map_err(lost)?;
        Ok(Control {
            writer,
            reader: BufReader::new(stream),
            last: Instant::now(),
        })
    }

    fn send(&mut self, message: &Value) -> Result<(), Fail> {
        self.writer
            .write_all(format!("{message}\n").as_bytes())
            .map_err(|_| "La connexion avec le téléphone a été perdue.".to_owned())
    }

    fn since_last(&self) -> Duration {
        self.last.elapsed()
    }

    /// Message suivant, ou `None` si rien n'est arrivé dans le délai.
    fn next(&mut self, timeout: Duration) -> Result<Option<Value>, Fail> {
        let lost = || "La connexion avec le téléphone a été perdue.".to_owned();
        self.reader
            .get_ref()
            .set_read_timeout(Some(timeout.max(Duration::from_millis(10))))
            .map_err(|_| lost())?;
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => Err(lost()),
            Ok(_) => {
                let message: Value = serde_json::from_str(line.trim())
                    .map_err(|_| "Le téléphone a envoyé un message illisible.".to_owned())?;
                if message["type"] == "error" {
                    return Err(format!(
                        "Le téléphone a signalé une erreur : {}",
                        message["message"].as_str().unwrap_or("inconnue")
                    ));
                }
                Ok(Some(message))
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(_) => Err(lost()),
        }
    }

    /// Attend un message d'un type donné (les autres sont ignorés).
    fn expect(&mut self, kind: &str, timeout: Duration) -> Result<Value, Fail> {
        let started = Instant::now();
        while started.elapsed() < timeout {
            if let Some(message) = self.next(timeout.saturating_sub(started.elapsed()))?
                && message["type"] == kind
            {
                self.last = Instant::now();
                return Ok(message);
            }
        }
        Err("Le téléphone ne répond pas.".to_owned())
    }
}

fn computer_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_owned())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "cet ordinateur".to_owned())
}
