use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

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
use crate::types::Config;

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
    AppStart { idx: usize, name: String, id: String },
    AppLine(String),
    AppDone { name: String, status: String, message: String },
    Done,
}

struct InstallerApp {
    total: usize,
    current_idx: usize,
    current_name: String,
    current_id: String,
    log_lines: Vec<String>,
    results: Vec<(String, String, String)>,
    done: bool,
    rx: mpsc::Receiver<InstallMsg>,
}

impl InstallerApp {
    fn new(total: usize, rx: mpsc::Receiver<InstallMsg>) -> Self {
        Self {
            total,
            current_idx: 0,
            current_name: String::new(),
            current_id: String::new(),
            log_lines: Vec::new(),
            results: Vec::new(),
            done: false,
            rx,
        }
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                InstallMsg::AppStart { idx, name, id } => {
                    self.current_idx = idx;
                    self.current_name = name;
                    self.current_id = id;
                    self.log_lines.clear();
                }
                InstallMsg::AppLine(line) => {
                    let clean = strip_ansi_cr(&line);
                    if !clean.trim().is_empty() {
                        self.log_lines.push(clean);
                    }
                }
                InstallMsg::AppDone { name, status, message } => {
                    self.results.push((name, status, message));
                }
                InstallMsg::Done => {
                    self.done = true;
                }
            }
        }
    }

    fn draw(&self, frame: &mut Frame) {
        let area = frame.area();

        let chunks = Layout::vertical([
            Constraint::Length(3), // title
            Constraint::Length(3), // progress gauge
            Constraint::Length(3), // current app
            Constraint::Length(7), // winget output log
            Constraint::Min(3),    // results list
            Constraint::Length(3), // status / controls
        ])
        .split(area);

        // Title
        let title_text = if self.done {
            "Installation Complete"
        } else {
            "Installing Applications"
        };
        let title = Paragraph::new(title_text)
            .style(Style::default().fg(Color::Cyan))
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(title, chunks[0]);

        // Progress gauge
        let done_count = self.results.len();
        let ratio = if self.total == 0 {
            1.0_f64
        } else {
            (done_count as f64 / self.total as f64).min(1.0)
        };
        let gauge_color = if self.done { Color::Green } else { Color::Cyan };
        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title("Progress"))
            .gauge_style(Style::default().fg(gauge_color).bg(Color::DarkGray))
            .ratio(ratio)
            .label(format!("{}/{}", done_count, self.total));
        frame.render_widget(gauge, chunks[1]);

        // Current app
        let current_text = if self.done {
            "All done".to_string()
        } else if self.current_name.is_empty() {
            "Starting...".to_string()
        } else {
            format!("{} ({})", self.current_name, self.current_id)
        };
        let current_color = if self.done { Color::Green } else { Color::Yellow };
        let current = Paragraph::new(current_text)
            .style(Style::default().fg(current_color))
            .block(Block::default().borders(Borders::ALL).title("Current"));
        frame.render_widget(current, chunks[2]);

        // Winget output log — last N lines that fit
        let log_inner_h = chunks[3].height.saturating_sub(2) as usize;
        let log_inner_w = chunks[3].width.saturating_sub(2) as usize;
        let start = self.log_lines.len().saturating_sub(log_inner_h);
        let log_lines: Vec<Line> = self.log_lines[start..]
            .iter()
            .map(|l| Line::from(truncate(l, log_inner_w)))
            .collect();
        let log = Paragraph::new(log_lines)
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title("Output"));
        frame.render_widget(log, chunks[3]);

        // Results list — newest first
        let res_w = chunks[4].width.saturating_sub(2) as usize;
        let name_w = (res_w / 2).min(35);
        let msg_w = res_w.saturating_sub(name_w + 12);
        let result_items: Vec<ListItem> = self
            .results
            .iter()
            .rev()
            .map(|(name, status, msg)| {
                let (sym, color) = match status.as_str() {
                    "OK" => ("✓", Color::Green),
                    "SKIP" => ("─", Color::Yellow),
                    _ => ("✗", Color::Red),
                };
                let line = Line::from(vec![
                    Span::styled(format!("{} ", sym), Style::default().fg(color)),
                    Span::raw(fit_cell(name, name_w)),
                    Span::styled(
                        format!(" {:<8}", status),
                        Style::default().fg(color),
                    ),
                    Span::styled(
                        truncate(msg, msg_w),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]);
                ListItem::new(line)
            })
            .collect();
        let results_title = format!("Results ({}/{})", done_count, self.total);
        let results_widget = List::new(result_items)
            .block(Block::default().borders(Borders::ALL).title(results_title));
        frame.render_widget(results_widget, chunks[4]);

        // Bottom status / controls
        let bottom_text = if self.done {
            let ok = self.results.iter().filter(|(_, s, _)| s == "OK").count();
            let skip = self.results.iter().filter(|(_, s, _)| s == "SKIP").count();
            let fail = self.results.iter().filter(|(_, s, _)| s == "FAIL").count();
            format!(
                "  OK: {}  Skipped: {}  Failed: {}    [Press any key to exit]",
                ok, skip, fail
            )
        } else {
            format!(
                "  Installing {} of {}…  please wait",
                self.current_idx + 1,
                self.total
            )
        };
        let controls = Paragraph::new(bottom_text)
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(controls, chunks[5]);
    }
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
    let total = config.apps.len();

    let (tx, rx) = mpsc::channel::<InstallMsg>();

    thread::spawn(move || {
        let winget = WingetService::new();
        for (idx, app) in config.apps.iter().enumerate() {
            let _ = tx.send(InstallMsg::AppStart {
                idx,
                name: app.name.clone(),
                id: app.id.clone(),
            });

            if !app.available_in_winget {
                let _ = tx.send(InstallMsg::AppDone {
                    name: app.name.clone(),
                    status: "SKIP".to_string(),
                    message: app.note.clone().unwrap_or_default(),
                });
                continue;
            }

            let tx_line = tx.clone();
            let result = winget.install_app(&app.id, move |line| {
                let _ = tx_line.send(InstallMsg::AppLine(line.to_string()));
            });

            let _ = tx.send(InstallMsg::AppDone {
                name: app.name.clone(),
                status: if result.success { "OK".to_string() } else { "FAIL".to_string() },
                message: result.message,
            });
        }
        let _ = tx.send(InstallMsg::Done);
    });

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = InstallerApp::new(total, rx);

    loop {
        app.poll();
        terminal.draw(|f| app.draw(f))?;

        if app.done {
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        break;
                    }
                }
            }
        } else if event::poll(Duration::from_millis(50))? {
            let _ = event::read();
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
