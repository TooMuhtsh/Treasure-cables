//! Interface plein écran (ratatui) : ce que voit la personne qui
//! double-clique sur l'outil. Rien ne défile : chaque touche met à jour la
//! même vue.

use std::io;
use std::time::{Duration, Instant};

use cablescope_audit::Severity;
use cablescope_audit::cable_test::{self, CableVerdict};
use cablescope_collect::{DeepCapture, Options};
use cablescope_model::Report;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, Paragraph, Tabs, Wrap};

use crate::session::{self, CableWatch, DEEP_TIMEOUT};
use crate::{export, text};

const TABS: [&str; 4] = ["Prises", "Détails", "Écrans", "Diagnostic"];

pub fn run(options: &Options, start_deep: bool) -> io::Result<()> {
    let mut terminal = ratatui::init();
    let result = App::new(options.clone()).run(&mut terminal, start_deep);
    ratatui::restore();
    result
}

enum Screen {
    Main,
    /// Proposition de relancer l'outil en administrateur.
    Elevation,
    Cable(CableStep),
    Deep(DeepRun),
}

enum CableStep {
    Unplug,
    Watching(Box<CableWatch>),
    Verdict,
}

struct DeepRun {
    capture: DeepCapture,
    started: Instant,
}

struct App {
    options: Options,
    report: Result<Report, String>,
    tab: usize,
    scroll: u16,
    status: Option<(String, Color)>,
    screen: Screen,
    cable_results: Vec<CableVerdict>,
    quit: bool,
}

impl App {
    fn new(options: Options) -> Self {
        Self {
            options,
            report: Err("Analyse en cours…".to_owned()),
            tab: 0,
            scroll: 0,
            status: None,
            screen: Screen::Main,
            cable_results: Vec::new(),
            quit: false,
        }
    }

    fn run(&mut self, terminal: &mut DefaultTerminal, start_deep: bool) -> io::Result<()> {
        self.refresh(terminal)?;
        if start_deep {
            self.start_deep();
        }
        while !self.quit {
            terminal.draw(|f| self.draw(f))?;
            if event::poll(Duration::from_millis(200))?
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                self.on_key(key, terminal)?;
            }
            self.tick(terminal)?;
        }
        Ok(())
    }

    /// Affiche un message pendant une opération bloquante.
    fn busy(&mut self, terminal: &mut DefaultTerminal, message: &str) -> io::Result<()> {
        self.status = Some((message.to_owned(), Color::Cyan));
        terminal.draw(|f| self.draw(f))?;
        Ok(())
    }

    fn refresh(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        self.busy(terminal, "Analyse des ports et appareils USB…")?;
        self.report = cablescope_collect::collect(&self.options).map_err(|e| e.to_string());
        self.status = None;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Clavier

    fn on_key(&mut self, key: KeyEvent, terminal: &mut DefaultTerminal) -> io::Result<()> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return Ok(());
        }
        let screen = std::mem::replace(&mut self.screen, Screen::Main);
        self.screen = match screen {
            Screen::Main => {
                self.on_main_key(key, terminal)?;
                return Ok(());
            }
            Screen::Elevation => {
                match key.code {
                    KeyCode::Char('o' | 'O' | 'y' | 'Y') | KeyCode::Enter => {
                        self.status = Some(match cablescope_collect::relaunch_elevated("--deep") {
                            Ok(()) => (
                                "Une fenêtre administrateur s'ouvre : validez l'invite de Windows."
                                    .to_owned(),
                                Color::Green,
                            ),
                            Err(e) => (e.to_string(), Color::Red),
                        });
                    }
                    _ => {}
                }
                Screen::Main
            }
            Screen::Cable(CableStep::Unplug) => match key.code {
                KeyCode::Enter | KeyCode::Char(' ') => {
                    self.busy(terminal, "Relevé de la situation actuelle…")?;
                    self.status = None;
                    match CableWatch::start(&self.options) {
                        Ok(watch) => Screen::Cable(CableStep::Watching(Box::new(watch))),
                        Err(e) => {
                            self.status = Some((format!("Analyse impossible : {e}"), Color::Red));
                            Screen::Main
                        }
                    }
                }
                KeyCode::Esc | KeyCode::Char('q') => Screen::Main,
                _ => Screen::Cable(CableStep::Unplug),
            },
            Screen::Cable(CableStep::Watching(watch)) => match key.code {
                KeyCode::Enter | KeyCode::Char(' ') => self.finish_cable_test(watch, terminal)?,
                KeyCode::Esc => Screen::Main, // abandon : la capture est fermée en sortant
                _ => Screen::Cable(CableStep::Watching(watch)),
            },
            Screen::Cable(CableStep::Verdict) => match key.code {
                KeyCode::Char('a' | 'A') => Screen::Cable(CableStep::Unplug),
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') => {
                    self.tab = 0;
                    self.scroll = 0;
                    Screen::Main
                }
                _ => Screen::Cable(CableStep::Verdict),
            },
            Screen::Deep(run) => match key.code {
                KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Esc | KeyCode::Char('q') => {
                    self.finish_deep(run, terminal)?;
                    Screen::Main
                }
                _ => Screen::Deep(run),
            },
        };
        Ok(())
    }

    fn on_main_key(&mut self, key: KeyEvent, terminal: &mut DefaultTerminal) -> io::Result<()> {
        match key.code {
            KeyCode::Char('q' | 'Q') | KeyCode::Esc => self.quit = true,
            KeyCode::Tab | KeyCode::Right => self.select_tab((self.tab + 1) % TABS.len()),
            KeyCode::BackTab | KeyCode::Left => {
                self.select_tab((self.tab + TABS.len() - 1) % TABS.len())
            }
            KeyCode::Char(c @ '1'..='4') => self.select_tab(c as usize - '1' as usize),
            KeyCode::Char('v' | 'V') => self.select_tab(if self.tab == 1 { 0 } else { 1 }),
            KeyCode::Up => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Down => self.scroll = self.scroll.saturating_add(1),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
            KeyCode::Home => self.scroll = 0,
            KeyCode::Char('r' | 'R') | KeyCode::F(5) => self.refresh(terminal)?,
            KeyCode::Char('c' | 'C') => {
                self.status = None;
                self.screen = Screen::Cable(CableStep::Unplug);
            }
            KeyCode::Char('d' | 'D') => self.start_deep(),
            KeyCode::Char('e' | 'E') => {
                self.status = Some(match &self.report {
                    Ok(report) => match export::export(report) {
                        Ok(path) => (
                            format!("Rapport enregistré : {}", path.display()),
                            Color::Green,
                        ),
                        Err(e) => (format!("Export impossible : {e}"), Color::Red),
                    },
                    Err(_) => (
                        "Rien à exporter : l'analyse a échoué.".to_owned(),
                        Color::Red,
                    ),
                });
            }
            _ => {}
        }
        Ok(())
    }

    fn select_tab(&mut self, tab: usize) {
        self.tab = tab;
        self.scroll = 0;
    }

    // -----------------------------------------------------------------------
    // Opérations longues

    fn tick(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        if let Screen::Cable(CableStep::Watching(watch)) = &mut self.screen {
            watch.poll(&self.options);
            if watch.is_done()
                && let Screen::Cable(CableStep::Watching(watch)) =
                    std::mem::replace(&mut self.screen, Screen::Main)
            {
                self.screen = self.finish_cable_test(watch, terminal)?;
            }
        }
        if let Screen::Deep(run) = &self.screen
            && run.started.elapsed() >= DEEP_TIMEOUT
            && let Screen::Deep(run) = std::mem::replace(&mut self.screen, Screen::Main)
        {
            self.finish_deep(run, terminal)?;
        }
        Ok(())
    }

    fn finish_cable_test(
        &mut self,
        watch: Box<CableWatch>,
        terminal: &mut DefaultTerminal,
    ) -> io::Result<Screen> {
        self.busy(terminal, "Analyse du branchement…")?;
        match watch.finish(&self.options) {
            Ok((verdict, after)) => {
                self.report = Ok(after);
                self.cable_results.push(verdict);
                self.status = None;
                Ok(Screen::Cable(CableStep::Verdict))
            }
            Err(e) => {
                self.status = Some((format!("Analyse impossible : {e}"), Color::Red));
                Ok(Screen::Main)
            }
        }
    }

    fn start_deep(&mut self) {
        if cfg!(windows) && !cablescope_collect::is_elevated() {
            self.screen = Screen::Elevation;
            return;
        }
        match cablescope_collect::start_deep_capture() {
            Ok(capture) => {
                self.status = None;
                self.screen = Screen::Deep(DeepRun {
                    capture,
                    started: Instant::now(),
                });
            }
            Err(e) => self.status = Some((e.to_string(), Color::Red)),
        }
    }

    fn finish_deep(&mut self, run: DeepRun, terminal: &mut DefaultTerminal) -> io::Result<()> {
        self.busy(terminal, "Analyse de l'écoute…")?;
        let before = self.report.as_ref().ok().cloned();
        match session::finish_deep(run.capture, before.as_ref(), &self.options) {
            Ok(report) => {
                self.report = Ok(report);
                self.status = None;
                self.select_tab(3);
            }
            Err(e) => self.status = Some((format!("Analyse impossible : {e}"), Color::Red)),
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Affichage

    fn draw(&self, f: &mut Frame) {
        let [header, tabs, body, summary, keys] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(f.area());

        let title = match &self.report {
            Ok(r) => text::title(r),
            Err(_) => format!("CableScope {}", env!("CARGO_PKG_VERSION")),
        };
        f.render_widget(
            Paragraph::new(format!(" {title}")).style(
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            header,
        );

        match &self.screen {
            Screen::Main | Screen::Elevation => {
                self.draw_tabs(f, tabs);
                self.draw_body(f, body);
            }
            Screen::Cable(step) => self.draw_cable(f, tabs.union(body), step),
            Screen::Deep(run) => draw_deep(f, tabs.union(body), run),
        }
        if matches!(self.screen, Screen::Elevation) {
            draw_elevation(f, body);
        }
        self.draw_summary(f, summary);
        f.render_widget(Paragraph::new(self.keys()), keys);
    }

    fn draw_tabs(&self, f: &mut Frame, area: Rect) {
        let findings = self
            .report
            .as_ref()
            .map(cablescope_audit::audit)
            .unwrap_or_default();
        let alerts = findings
            .iter()
            .filter(|x| x.severity >= Severity::Warning)
            .count();
        let diagnostic = if alerts == 0 {
            "Diagnostic ✔".to_owned()
        } else {
            format!("Diagnostic ⚠ {alerts}")
        };
        let titles: Vec<String> = TABS[..3]
            .iter()
            .map(|t| (*t).to_owned())
            .chain([diagnostic])
            .collect();
        f.render_widget(
            Tabs::new(titles)
                .select(self.tab)
                .divider("│")
                .highlight_style(
                    Style::new()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD | Modifier::REVERSED),
                ),
            area,
        );
    }

    fn draw_body(&self, f: &mut Frame, area: Rect) {
        let content = match &self.report {
            Ok(report) => match self.tab {
                0 => text::plugs(report),
                1 => text::details(report),
                2 => text::displays(report),
                _ => text::diagnosis(report),
            },
            Err(message) => message.clone(),
        };
        let lines = styled(&content);
        let max_scroll = (lines.len() as u16).saturating_sub(area.height.saturating_sub(2));
        f.render_widget(
            Paragraph::new(Text::from(lines))
                .block(Block::bordered().title(format!(" {} ", TABS[self.tab])))
                .scroll((self.scroll.min(max_scroll), 0)),
            area,
        );
    }

    fn draw_summary(&self, f: &mut Frame, area: Rect) {
        let (message, color) = match &self.status {
            Some((m, c)) => (m.clone(), *c),
            None => match &self.report {
                Ok(report) => {
                    let findings = cablescope_audit::audit(report);
                    let problems = findings
                        .iter()
                        .filter(|x| x.severity == Severity::Problem)
                        .count();
                    let warnings = findings
                        .iter()
                        .filter(|x| x.severity == Severity::Warning)
                        .count();
                    match (problems, warnings) {
                        (0, 0) => ("✔ Aucun problème détecté".to_owned(), Color::Green),
                        (0, w) => (
                            format!("⚠ {w} point(s) à vérifier : onglet Diagnostic"),
                            Color::Yellow,
                        ),
                        (p, _) => (format!("✖ {p} problème(s) : onglet Diagnostic"), Color::Red),
                    }
                }
                Err(e) => (e.clone(), Color::Red),
            },
        };
        f.render_widget(
            Paragraph::new(format!(" {message}"))
                .style(Style::new().fg(color).add_modifier(Modifier::BOLD)),
            area,
        );
    }

    fn keys(&self) -> Line<'static> {
        let keys: &[(&str, &str)] = match &self.screen {
            Screen::Main => &[
                ("C", "Tester un câble"),
                ("R", "Rafraîchir"),
                ("D", "Diagnostic approfondi"),
                ("E", "Exporter"),
                ("←→", "Onglets"),
                ("↑↓", "Défiler"),
                ("Q", "Quitter"),
            ],
            Screen::Elevation => &[("O", "Oui"), ("N", "Non")],
            Screen::Cable(CableStep::Unplug) => {
                &[("Entrée", "C'est débranché"), ("Échap", "Annuler")]
            }
            Screen::Cable(CableStep::Watching(_)) => {
                &[("Entrée", "Terminer maintenant"), ("Échap", "Annuler")]
            }
            Screen::Cable(CableStep::Verdict) => {
                &[("A", "Tester un autre câble"), ("Entrée", "Retour")]
            }
            Screen::Deep(_) => &[("Entrée", "Terminer l'écoute")],
        };
        let mut spans = Vec::new();
        for (key, label) in keys {
            spans.push(Span::styled(
                format!(" {key} "),
                Style::new().fg(Color::Black).bg(Color::Gray),
            ));
            spans.push(Span::raw(format!(" {label}  ")));
        }
        Line::from(spans)
    }

    fn draw_cable(&self, f: &mut Frame, area: Rect, step: &CableStep) {
        let mut lines: Vec<Line> = Vec::new();
        let bold = Style::new().add_modifier(Modifier::BOLD);
        match step {
            CableStep::Unplug => {
                lines.push(Line::styled("Étape 1 sur 2", bold));
                lines.push(Line::raw(""));
                lines.push(Line::raw(
                    "Débranchez l'appareil qui servira au test (téléphone, clé USB, SSD...).",
                ));
                lines.push(Line::raw(
                    "Gardez le câble à tester sous la main, puis appuyez sur Entrée.",
                ));
                lines.push(Line::raw(""));
                lines.push(Line::styled(
                    "Conseil : un appareil USB 3 (SSD, clé USB 3) permet aussi de vérifier si le câble est limité à l'USB 2.",
                    Style::new().fg(Color::DarkGray),
                ));
            }
            CableStep::Watching(watch) => {
                lines.push(Line::styled("Étape 2 sur 2", bold));
                lines.push(Line::raw(""));
                lines.push(Line::raw(
                    "Branchez maintenant l'appareil au PC avec le câble à tester.",
                ));
                lines.push(Line::raw(""));
                let remaining = watch.remaining_s();
                lines.push(Line::raw(format!(
                    "⏱ surveillance en cours… ({remaining} s restantes)"
                )));
                if watch.detected().is_empty() {
                    lines.push(Line::styled(
                        "Aucun appareil détecté pour l'instant.",
                        Style::new().fg(Color::DarkGray),
                    ));
                } else {
                    lines.push(Line::styled(
                        format!("✔ Détecté : {}", watch.detected().join(", ")),
                        Style::new().fg(Color::Green),
                    ));
                }
                lines.push(Line::raw(""));
                lines.push(Line::styled(
                    if watch.listens_link_errors() {
                        "Mode administrateur : les erreurs de lien sont aussi surveillées."
                    } else {
                        "Lancé en administrateur, le test surveillerait aussi les erreurs de lien."
                    },
                    Style::new().fg(Color::DarkGray),
                ));
            }
            CableStep::Verdict => {
                if let Some(verdict) = self.cable_results.last() {
                    lines.push(Line::styled(
                        format!(
                            "{} {}",
                            icon(verdict.severity, verdict.outcome),
                            verdict.title
                        ),
                        severity_style(verdict.severity, verdict.outcome)
                            .add_modifier(Modifier::BOLD),
                    ));
                    lines.push(Line::raw(""));
                    lines.push(Line::raw(verdict.detail.clone()));
                    if verdict.outcome == cable_test::Outcome::NoData {
                        lines.push(Line::raw(""));
                        lines.push(Line::styled(
                            "Si l'appareil était déjà branché à l'étape 1, recommencez le test (touche A).",
                            Style::new().fg(Color::DarkGray),
                        ));
                    }
                }
            }
        }
        if !self.cable_results.is_empty() {
            lines.push(Line::raw(""));
            lines.push(Line::styled("Câbles testés", bold));
            for (i, verdict) in self.cable_results.iter().enumerate() {
                let device = verdict
                    .device
                    .as_deref()
                    .map(|d| format!(" · {d}"))
                    .unwrap_or_default();
                lines.push(Line::styled(
                    format!(
                        "  Câble {} : {} {}{device}",
                        i + 1,
                        icon(verdict.severity, verdict.outcome),
                        verdict.title
                    ),
                    severity_style(verdict.severity, verdict.outcome),
                ));
            }
        }
        f.render_widget(
            Paragraph::new(Text::from(lines))
                .wrap(Wrap { trim: false })
                .block(Block::bordered().title(" Tester un câble ")),
            area,
        );
    }
}

fn draw_deep(f: &mut Frame, area: Rect, run: &DeepRun) {
    let lines = vec![
        Line::styled(
            "Écoute de la pile USB",
            Style::new().add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::raw("1. Débranchez l'appareil ou le câble à tester."),
        Line::raw(
            "2. Rebranchez-le, plusieurs fois si besoin, en laissant quelques secondes entre deux essais.",
        ),
        Line::raw("3. Appuyez sur Entrée pour terminer (arrêt automatique après 2 minutes)."),
        Line::raw(""),
        Line::styled(
            format!(
                "⏱ {} s · {} événement(s) USB",
                run.capture.elapsed_s(),
                run.capture.events()
            ),
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
    ];
    f.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(" Diagnostic approfondi ")),
        area,
    );
}

fn draw_elevation(f: &mut Frame, area: Rect) {
    let [popup] = Layout::horizontal([Constraint::Max(70)])
        .flex(Flex::Center)
        .areas(area);
    let [popup] = Layout::vertical([Constraint::Length(8)])
        .flex(Flex::Center)
        .areas(popup);
    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(vec![
            Line::raw("Le diagnostic approfondi écoute la pile USB de Windows pendant que vous"),
            Line::raw("branchez et débranchez : il faut les droits administrateur."),
            Line::raw(""),
            Line::styled(
                "Relancer CableScope en administrateur ?",
                Style::new().add_modifier(Modifier::BOLD),
            ),
            Line::raw("[O] Oui    [N] Non"),
        ])
        .wrap(Wrap { trim: false })
        .block(
            Block::bordered()
                .title(" Droits administrateur ")
                .border_style(Style::new().fg(Color::Yellow)),
        ),
        popup,
    );
}

fn icon(severity: Severity, outcome: cable_test::Outcome) -> &'static str {
    match (severity, outcome) {
        (Severity::Problem, _) => "✖",
        (Severity::Warning, _) => "⚠",
        (_, cable_test::Outcome::Ok) => "✔",
        _ => "ℹ",
    }
}

fn severity_style(severity: Severity, outcome: cable_test::Outcome) -> Style {
    match (severity, outcome) {
        (Severity::Problem, _) => Style::new().fg(Color::Red),
        (Severity::Warning, _) => Style::new().fg(Color::Yellow),
        (_, cable_test::Outcome::Ok) => Style::new().fg(Color::Green),
        _ => Style::new().fg(Color::Cyan),
    }
}

/// Couleurs selon les marqueurs du texte (✖ ⚠ ✔ ℹ) et titres de section.
fn styled(content: &str) -> Vec<Line<'static>> {
    content
        .lines()
        .map(|line| {
            let style = if line.contains('✖') {
                Style::new().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else if line.contains('⚠') {
                Style::new().fg(Color::Yellow)
            } else if line.contains('✔') {
                Style::new().fg(Color::Green)
            } else if line.starts_with('ℹ') {
                Style::new().fg(Color::Cyan)
            } else if !line.is_empty() && !line.starts_with(' ') {
                Style::new().add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            Line::styled(line.to_owned(), style)
        })
        .collect()
}
