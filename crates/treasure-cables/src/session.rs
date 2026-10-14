//! Opérations longues partagées par les interfaces (console et fenêtre) :
//! test d'un câble et diagnostic approfondi.

use std::time::{Duration, Instant};

use treasure_audit::cable_test::{self, CableVerdict};
use treasure_collect::{DeepCapture, Options};
use treasure_model::Report;

/// Durée maximale de surveillance pendant un test de câble.
pub const CABLE_TIMEOUT: Duration = Duration::from_secs(20);
/// Délai laissé à l'appareil pour finir de se présenter une fois apparu.
const CABLE_SETTLE: Duration = Duration::from_secs(3);
/// Arrêt automatique du diagnostic approfondi.
pub const DEEP_TIMEOUT: Duration = Duration::from_secs(120);

fn light(options: &Options) -> Options {
    Options {
        light: true,
        ..options.clone()
    }
}

/// Surveillance d'un branchement : relevé avant, puis apparition d'appareils.
pub struct CableWatch {
    baseline: Report,
    started: Instant,
    last_poll: Instant,
    seen: Option<Instant>,
    detected: Vec<String>,
    capture: Option<DeepCapture>,
}

impl CableWatch {
    /// Relève la situation actuelle (l'appareil doit être débranché). En
    /// administrateur, écoute aussi les erreurs de lien.
    pub fn start(options: &Options) -> Result<Self, String> {
        let baseline = treasure_collect::collect(&light(options)).map_err(|e| e.to_string())?;
        let capture = treasure_collect::is_elevated()
            .then(treasure_collect::start_deep_capture)
            .and_then(Result::ok);
        Ok(Self {
            baseline,
            started: Instant::now(),
            last_poll: Instant::now(),
            seen: None,
            detected: Vec::new(),
            capture,
        })
    }

    /// Relève l'état des ports, au plus une fois par seconde.
    pub fn poll(&mut self, options: &Options) {
        if self.last_poll.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_poll = Instant::now();
        if let Ok(now) = treasure_collect::collect(&light(options)) {
            self.detected = cable_test::new_devices(&self.baseline, &now)
                .iter()
                .map(|d| d.display_name())
                .collect();
            if !self.detected.is_empty() && self.seen.is_none() {
                self.seen = Some(Instant::now());
            }
        }
    }

    /// Vrai quand l'appareil a fini de se présenter, ou au bout du délai.
    pub fn is_done(&self) -> bool {
        self.seen.is_some_and(|t| t.elapsed() >= CABLE_SETTLE)
            || self.started.elapsed() >= CABLE_TIMEOUT
    }

    pub fn remaining_s(&self) -> u64 {
        CABLE_TIMEOUT
            .saturating_sub(self.started.elapsed())
            .as_secs()
    }

    pub fn detected(&self) -> &[String] {
        &self.detected
    }

    pub fn listens_link_errors(&self) -> bool {
        self.capture.is_some()
    }

    /// Verdict, et nouveau rapport complet (avec le bilan d'écoute éventuel).
    pub fn finish(self, options: &Options) -> Result<(CableVerdict, Report), String> {
        let mut link = self.capture.map(DeepCapture::stop);
        let mut after = treasure_collect::collect(options).map_err(|e| e.to_string())?;
        if let Some(link) = &mut link {
            treasure_collect::deep::complete(link, Some(&self.baseline), &after);
        }
        let verdict = cable_test::evaluate(&self.baseline, &after, link.as_ref());
        after.link_diagnostics = link;
        Ok((verdict, after))
    }
}

/// Termine le diagnostic approfondi et renvoie le rapport qui en contient le
/// bilan. `before` permet de nommer un appareil débranché pendant l'écoute.
pub fn finish_deep(
    capture: DeepCapture,
    before: Option<&Report>,
    options: &Options,
) -> Result<Report, String> {
    let mut diagnostics = capture.stop();
    let mut report = treasure_collect::collect(options).map_err(|e| e.to_string())?;
    treasure_collect::deep::complete(&mut diagnostics, before, &report);
    report.link_diagnostics = Some(diagnostics);
    Ok(report)
}
