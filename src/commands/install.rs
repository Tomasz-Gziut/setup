use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::constants::PRESET_DIR;
use crate::services::winget::WingetService;
use crate::types::{AppConfig, Config};

struct PresetInfo {
    name: String,
    path: String,
    app_count: usize,
    all_installed: bool,
}

struct SelectorApp {
    presets: Vec<PresetInfo>,
    cursor: usize,
    list_state: ListState,
    should_quit: bool,
    selected_path: Option<String>,
}

impl SelectorApp {
    fn new(installed_ids: &HashSet<String>) -> Result<Self> {
        let preset_dir = Path::new(PRESET_DIR);
        if !preset_dir.exists() {
            bail!("Preset directory not found: {}", PRESET_DIR);
        }

        let mut presets: Vec<PresetInfo> = fs::read_dir(preset_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
            .filter_map(|e| {
                let path = e.path();
                let name = path.file_stem()?.to_str()?.to_string();
                let content = fs::read_to_string(&path).ok()?;
                let config: Config = serde_json::from_str(&content).ok()?;
                let app_count = config.apps.len();
                let all_installed = config.apps.iter().all(|a| {
                    a.available_in_winget == false || installed_ids.contains(&a.id.to_lowercase())
                });
                Some(PresetInfo {
                    name,
                    path: path.to_string_lossy().into_owned(),
                    app_count,
                    all_installed,
                })
            })
            .collect();

        if presets.is_empty() {
            bail!("No presets found in {}", PRESET_DIR);
        }

        presets.sort_by(|a, b| a.name.cmp(&b.name));

        let mut list_state = ListState::default();
        list_state.select(Some(0));

        Ok(Self {
            presets,
            cursor: 0,
            list_state,
            should_quit: false,
            selected_path: None,
        })
    }

    fn move_cursor(&mut self, delta: i32) {
        let len = self.presets.len() as i32;
        if len == 0 {
            return;
        }
        self.cursor = ((self.cursor as i32 + delta).rem_euclid(len)) as usize;
        self.list_state.select(Some(self.cursor));
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();

        let layout = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(area);

        let title = Paragraph::new("Select Preset to Install")
            .style(Style::default().fg(Color::Cyan))
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(title, layout[0]);

        let items: Vec<ListItem> = self
            .presets
            .iter()
            .map(|p| {
                let status_sym = if p.all_installed { "✓" } else { "○" };
                let status_color = if p.all_installed { Color::Green } else { Color::Yellow };
                let status_text = if p.all_installed { "Installed" } else { "Pending" };

                let line = Line::from(vec![
                    Span::styled(format!("{} ", status_sym), Style::default().fg(status_color)),
                    Span::raw(fit_cell(&p.name, 35)),
                    Span::styled(
                        format!(" {:>3} apps  ", p.app_count),
                        Style::default().fg(Color::Gray),
                    ),
                    Span::styled(status_text, Style::default().fg(status_color)),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Presets"))
            .highlight_style(Style::default().bg(Color::Cyan).fg(Color::Black))
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, layout[1], &mut self.list_state);

        let controls = Paragraph::new(
            "  [↑↓/j/k] Navigate   [PgUp/PgDn] Jump   [Enter] Install   [q/Esc] Quit",
        )
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::ALL));
        frame.render_widget(controls, layout[2]);
    }
}

fn run_preset_selector() -> Result<Option<String>> {
    let winget = WingetService::new();
    let installed: HashSet<String> = winget
        .get_installed_apps()
        .into_iter()
        .map(|a| a.id.to_lowercase())
        .collect();

    let mut app = SelectorApp::new(&installed)?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    loop {
        terminal.draw(|f| app.draw(f))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => app.move_cursor(-1),
                    KeyCode::Down | KeyCode::Char('j') => app.move_cursor(1),
                    KeyCode::PageUp => app.move_cursor(-10),
                    KeyCode::PageDown => app.move_cursor(10),
                    KeyCode::Enter => {
                        app.selected_path =
                            Some(app.presets[app.cursor].path.clone());
                        app.should_quit = true;
                    }
                    KeyCode::Char('q') | KeyCode::Esc => {
                        app.should_quit = true;
                    }
                    _ => {}
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(app.selected_path)
}

// ── Installation TUI ─────────────────────────────────────────────────────────

enum InstallMsg {
    AppStart { idx: usize },
    AppProgress { idx: usize, progress: f64, last_line: String },
    AppDone { idx: usize, status: String, message: String },
    Done,
}

#[derive(Clone, PartialEq)]
enum AppRunStatus {
    Waiting,
    Running,
    Ok,
    Skip,
    Fail,
}

struct AppState {
    name: String,
    progress: f64,
    status: AppRunStatus,
    last_line: String,
    message: String,
}

const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

struct InstallerApp {
    apps: Vec<AppState>,
    done: bool,
    scroll_offset: usize,
    start_time: Instant,
    rx: mpsc::Receiver<InstallMsg>,
}

impl InstallerApp {
    fn new(app_configs: &[AppConfig], rx: mpsc::Receiver<InstallMsg>) -> Self {
        Self {
            apps: app_configs
                .iter()
                .map(|a| AppState {
                    name: a.name.clone(),
                    progress: 0.0,
                    status: AppRunStatus::Waiting,
                    last_line: String::new(),
                    message: String::new(),
                })
                .collect(),
            done: false,
            scroll_offset: 0,
            start_time: Instant::now(),
            rx,
        }
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                InstallMsg::AppStart { idx } => {
                    if let Some(a) = self.apps.get_mut(idx) {
                        a.status = AppRunStatus::Running;
                    }
                }
                InstallMsg::AppProgress { idx, progress, last_line } => {
                    if let Some(a) = self.apps.get_mut(idx) {
                        a.progress = progress;
                        let clean = strip_ansi_cr(&last_line);
                        if !clean.trim().is_empty() {
                            a.last_line = clean;
                        }
                    }
                }
                InstallMsg::AppDone { idx, status, message } => {
                    if let Some(a) = self.apps.get_mut(idx) {
                        a.progress = 1.0;
                        a.status = match status.as_str() {
                            "OK" => AppRunStatus::Ok,
                            "SKIP" => AppRunStatus::Skip,
                            _ => AppRunStatus::Fail,
                        };
                        a.message = message;
                    }
                }
                InstallMsg::Done => {
                    self.done = true;
                }
            }
        }
    }

    fn overall_progress(&self) -> f64 {
        if self.apps.is_empty() {
            return 1.0;
        }
        self.apps.iter().map(|a| a.progress).sum::<f64>() / self.apps.len() as f64
    }

    fn draw(&self, frame: &mut Frame) {
        if self.done {
            self.draw_summary(frame);
        } else {
            self.draw_progress(frame);
        }
    }

    fn draw_progress(&self, frame: &mut Frame) {
        let area = frame.area();

        let chunks = Layout::vertical([
            Constraint::Length(3), // title
            Constraint::Length(3), // overall gauge
            Constraint::Min(3),    // per-app list
            Constraint::Length(3), // status bar
        ])
        .split(area);

        frame.render_widget(
            Paragraph::new("Installing Applications")
                .style(Style::default().fg(Color::Cyan))
                .block(Block::default().borders(Borders::ALL)),
            chunks[0],
        );

        let done_count = self
            .apps
            .iter()
            .filter(|a| matches!(a.status, AppRunStatus::Ok | AppRunStatus::Skip | AppRunStatus::Fail))
            .count();
        frame.render_widget(
            Gauge::default()
                .block(Block::default().borders(Borders::ALL).title("Overall"))
                .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
                .ratio(self.overall_progress())
                .label(format!("{}/{}", done_count, self.apps.len())),
            chunks[1],
        );

        let running = self.apps.iter().filter(|a| a.status == AppRunStatus::Running).count();
        let waiting = self.apps.iter().filter(|a| a.status == AppRunStatus::Waiting).count();
        self.render_app_list(frame, chunks[2], true);
        frame.render_widget(
            Paragraph::new(format!(
                "  Running: {}  Waiting: {}    [↑↓/j/k] scroll",
                running, waiting
            ))
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL)),
            chunks[3],
        );
    }

    fn draw_summary(&self, frame: &mut Frame) {
        let area = frame.area();

        let ok = self.apps.iter().filter(|a| a.status == AppRunStatus::Ok).count();
        let fail = self.apps.iter().filter(|a| a.status == AppRunStatus::Fail).count();
        let skip = self.apps.iter().filter(|a| a.status == AppRunStatus::Skip).count();

        let (title_text, title_color) = if fail == 0 {
            ("Installation Complete", Color::Green)
        } else {
            ("Installation Complete (with errors)", Color::Red)
        };

        let chunks = Layout::vertical([
            Constraint::Length(3), // title
            Constraint::Min(3),    // results list
            Constraint::Length(3), // summary + enter
        ])
        .split(area);

        frame.render_widget(
            Paragraph::new(title_text)
                .style(Style::default().fg(title_color))
                .block(Block::default().borders(Borders::ALL)),
            chunks[0],
        );

        self.render_app_list(frame, chunks[1], false);

        let summary = format!(
            "  ✓ OK: {}   ✗ Failed: {}   ─ Skipped: {}    [Enter] return to menu",
            ok, fail, skip
        );
        let summary_color = if fail > 0 { Color::Red } else { Color::Green };
        frame.render_widget(
            Paragraph::new(summary)
                .style(Style::default().fg(summary_color))
                .block(Block::default().borders(Borders::ALL)),
            chunks[2],
        );
    }

    fn render_app_list(&self, frame: &mut Frame, area: ratatui::layout::Rect, in_progress: bool) {
        let list_inner_h = area.height.saturating_sub(2) as usize;
        let list_inner_w = area.width.saturating_sub(2) as usize;
        let name_w = (list_inner_w / 3).clamp(12, 30);

        let spin_frame = (self.start_time.elapsed().as_millis() / 80) as usize;

        // progress bars only shown during install, not in summary
        let (bar_w, pct_w) = if in_progress { (16usize, 4usize) } else { (0, 0) };
        let bar_section = if in_progress { bar_w + 2 + 1 + pct_w + 1 } else { 0 };
        let last_w = list_inner_w.saturating_sub(2 + name_w + 1 + bar_section);

        let items: Vec<ListItem> = self
            .apps
            .iter()
            .enumerate()
            .skip(self.scroll_offset)
            .take(list_inner_h)
            .map(|(idx, app)| {
                let (sym, col) = match &app.status {
                    AppRunStatus::Waiting => ("·".to_string(), Color::DarkGray),
                    AppRunStatus::Running => (
                        SPINNER[(spin_frame + idx) % SPINNER.len()].to_string(),
                        Color::Yellow,
                    ),
                    AppRunStatus::Ok => ("✓".to_string(), Color::Green),
                    AppRunStatus::Skip => ("─".to_string(), Color::Yellow),
                    AppRunStatus::Fail => ("✗".to_string(), Color::Red),
                };
                let bar_col = match &app.status {
                    AppRunStatus::Running => Color::Cyan,
                    AppRunStatus::Ok => Color::Green,
                    AppRunStatus::Fail => Color::Red,
                    AppRunStatus::Skip | AppRunStatus::Waiting => Color::DarkGray,
                };
                let last = match &app.status {
                    AppRunStatus::Waiting => "waiting…".to_string(),
                    AppRunStatus::Ok | AppRunStatus::Skip | AppRunStatus::Fail => {
                        truncate(&app.message, last_w.max(1))
                    }
                    AppRunStatus::Running => truncate(&app.last_line, last_w.max(1)),
                };

                let mut spans = vec![
                    Span::styled(format!("{} ", sym), Style::default().fg(col)),
                    Span::raw(fit_cell(&app.name, name_w)),
                ];
                if in_progress {
                    let filled = (app.progress * bar_w as f64) as usize;
                    let bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(bar_w - filled));
                    let pct = format!("{:>3}%", (app.progress * 100.0) as u8);
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(bar, Style::default().fg(bar_col)));
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(pct, Style::default().fg(bar_col)));
                }
                spans.push(Span::raw(" "));
                spans.push(Span::styled(last, Style::default().fg(Color::DarkGray)));

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list_title = if in_progress {
            let running = self.apps.iter().filter(|a| a.status == AppRunStatus::Running).count();
            format!("Applications ({} running)", running)
        } else {
            format!("Results ({} apps)", self.apps.len())
        };
        frame.render_widget(
            List::new(items)
                .block(Block::default().borders(Borders::ALL).title(list_title)),
            area,
        );
    }
}

fn estimate_winget_progress(line: &str) -> Option<f64> {
    if let Some(pct) = extract_pct(line) {
        return Some((pct / 100.0).clamp(0.0, 0.99));
    }
    let lower = line.to_lowercase();
    if lower.contains("found ") {
        return Some(0.05);
    }
    if lower.contains("downloading") {
        return Some(0.15);
    }
    if lower.contains("verif") {
        return Some(0.65);
    }
    if lower.contains("starting package install") || lower.contains("starting install") {
        return Some(0.80);
    }
    if lower.contains("successfully installed") || lower.contains("successfully uninstalled") {
        return Some(0.95);
    }
    if lower.contains("already installed") {
        return Some(0.99);
    }
    None
}

fn extract_pct(s: &str) -> Option<f64> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'%' {
                if let Ok(n) = s[start..i].parse::<f64>() {
                    if (0.0..=100.0).contains(&n) {
                        return Some(n);
                    }
                }
            }
        } else {
            i += 1;
        }
    }
    None
}

fn strip_ansi_cr(s: &str) -> String {
    // After carriage returns, only keep the last segment (winget redraws the same line)
    let s = s.rsplit('\r').next().unwrap_or(s);
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\x1b' && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            i += 2;
            while i < bytes.len() && !(0x40..=0x7E).contains(&bytes[i]) {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
        } else {
            let ch = s[i..].chars().next().unwrap_or('\0');
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn perform_installation(config_path: &str) -> Result<()> {
    let abs = Path::new(config_path).canonicalize().unwrap_or_else(|_| config_path.into());

    if !abs.exists() {
        bail!("Config file not found: {}", abs.display());
    }

    let content = fs::read_to_string(&abs)?;
    let config: Config = serde_json::from_str(&content)?;

    let (tx, rx) = mpsc::channel::<InstallMsg>();
    let apps_spawn = config.apps.clone();
    let tx_coord = tx.clone();
    drop(tx);

    thread::spawn(move || {
        let handles: Vec<_> = apps_spawn
            .into_iter()
            .enumerate()
            .map(|(idx, app)| {
                let tx = tx_coord.clone();
                thread::spawn(move || {
                    if !app.available_in_winget {
                        let _ = tx.send(InstallMsg::AppDone {
                            idx,
                            status: "SKIP".to_string(),
                            message: app.note.unwrap_or_default(),
                        });
                        return;
                    }
                    let _ = tx.send(InstallMsg::AppStart { idx });
                    let winget = WingetService::new();
                    let mut cur_prog = 0.0f64;
                    let result = winget.install_app(&app.id, |line| {
                        if let Some(p) = estimate_winget_progress(line) {
                            cur_prog = cur_prog.max(p);
                        }
                        let _ = tx.send(InstallMsg::AppProgress {
                            idx,
                            progress: cur_prog,
                            last_line: line.to_string(),
                        });
                    });
                    let _ = tx.send(InstallMsg::AppDone {
                        idx,
                        status: if result.success { "OK".to_string() } else { "FAIL".to_string() },
                        message: result.message,
                    });
                })
            })
            .collect();

        for h in handles {
            h.join().ok();
        }
        let _ = tx_coord.send(InstallMsg::Done);
    });

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut installer = InstallerApp::new(&config.apps, rx);
    let mut was_done = false;

    loop {
        installer.poll();

        // Child installers (MSI/NSIS spawned by winget) may reset console mode;
        // re-enable raw mode once when done so crossterm can read events again.
        if installer.done && !was_done {
            was_done = true;
            let _ = enable_raw_mode();
        }

        terminal.draw(|f| installer.draw(f))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Enter if installer.done => break,
                        KeyCode::Up | KeyCode::Char('k') => {
                            installer.scroll_offset =
                                installer.scroll_offset.saturating_sub(1);
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            let max = installer.apps.len().saturating_sub(1);
                            if installer.scroll_offset < max {
                                installer.scroll_offset += 1;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

pub fn run(config_path: Option<String>) -> Result<()> {
    if let Some(path) = config_path {
        return perform_installation(&path);
    }

    match run_preset_selector()? {
        Some(path) => perform_installation(&path),
        None => Ok(()),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if UnicodeWidthStr::width(s) <= max {
        s.to_string()
    } else if max == 0 {
        String::new()
    } else {
        let ellipsis_width = UnicodeWidthChar::width('…').unwrap_or(1);
        let text_width = max.saturating_sub(ellipsis_width);
        let mut width = 0;
        let mut out = String::new();
        for ch in s.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if width + ch_width > text_width {
                break;
            }
            out.push(ch);
            width += ch_width;
        }
        out.push('…');
        out
    }
}

fn fit_cell(s: &str, width: usize) -> String {
    let mut out = truncate(s, width);
    let used = UnicodeWidthStr::width(out.as_str());
    if used < width {
        out.push_str(&" ".repeat(width - used));
    }
    out
}
