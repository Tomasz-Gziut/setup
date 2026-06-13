use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;
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
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};

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

        // Title
        let title = Paragraph::new("Select Preset to Install")
            .style(Style::default().fg(Color::Cyan))
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(title, layout[0]);

        // Preset list
        let items: Vec<ListItem> = self
            .presets
            .iter()
            .map(|p| {
                let status_sym = if p.all_installed { "✓" } else { "○" };
                let status_color = if p.all_installed { Color::Green } else { Color::Yellow };
                let status_text = if p.all_installed { "Installed" } else { "Pending" };

                let line = Line::from(vec![
                    Span::styled(format!("{} ", status_sym), Style::default().fg(status_color)),
                    Span::raw(format!("{:<35}", truncate(&p.name, 35))),
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

        // Controls
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

fn perform_installation(config_path: &str) -> Result<()> {
    let winget = WingetService::new();
    let abs = Path::new(config_path).canonicalize().unwrap_or_else(|_| config_path.into());

    println!("\nLoading config: {}", abs.display());

    if !abs.exists() {
        bail!("Config file not found: {}", abs.display());
    }

    let content = fs::read_to_string(&abs)?;
    let config: Config = serde_json::from_str(&content)?;

    println!("Found {} applications to install\n", config.apps.len());

    let mut results: Vec<(String, &str, String)> = vec![];

    for app in &config.apps {
        println!("Installing: {} ({})", app.name, app.id);

        if !app.available_in_winget {
            println!("  SKIP – not available via winget");
            if let Some(note) = &app.note {
                println!("  Note: {}", note);
            }
            results.push((app.name.clone(), "SKIP", app.note.clone().unwrap_or_default()));
            continue;
        }

        let result = winget.install_app(&app.id, |line| println!("  {}", line));

        if result.success {
            println!("  OK – {}", result.message);
            results.push((app.name.clone(), "OK", result.message));
        } else {
            println!("  FAIL – {}", result.message);
            results.push((app.name.clone(), "FAIL", result.message));
        }
    }

    // Summary table
    println!("\n\nInstallation Summary\n");
    let w = 30usize;
    let sep = format!("+-{}-+--------+-{}-+", "-".repeat(w), "-".repeat(50));
    println!("{sep}");
    println!("| {:<w$} | Status | {:<50} |", "Application", "Message");
    println!("{sep}");
    for (name, status, msg) in &results {
        let n = truncate(name, w);
        let m = truncate(msg, 50);
        println!("| {n:<w$} | {status:<6} | {m:<50} |");
    }
    println!("{sep}");

    let ok = results.iter().filter(|(_, s, _)| *s == "OK").count();
    let skip = results.iter().filter(|(_, s, _)| *s == "SKIP").count();
    let fail = results.iter().filter(|(_, s, _)| *s == "FAIL").count();
    println!("\nTotal: {}  OK: {}  Skipped: {}  Failed: {}\n", results.len(), ok, skip, fail);

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
    if s.len() <= max { s.to_string() } else { format!("{}…", &s[..max.saturating_sub(1)]) }
}
