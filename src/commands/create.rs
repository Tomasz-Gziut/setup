use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};

use crate::constants::PRESET_DIR;
use crate::services::winget::WingetService;
use crate::types::{AppConfig, Config, InstalledApp};

const PAGE_SIZE: usize = 15;

// ─── installed match ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct InstalledMatch {
    version: String,
    source: String,
}

// ─── panel ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Panel {
    Available,
    Selected,
}

// ─── UI mode ─────────────────────────────────────────────────────────────────

enum Mode {
    Normal,
    FilenameInput { input: String },
    Confirm { message: String, action: PendingAction },
    Progress { state: ProgressState },
}

#[derive(Clone)]
enum PendingAction {
    Install(Vec<AppConfig>),
    Uninstall(Vec<AppConfig>),
}

struct ProgressState {
    title: String,
    lines: Vec<String>,
    done: bool,
    result_msg: String,
    rx: mpsc::Receiver<ProgressMsg>,
}

enum ProgressMsg {
    Line(String),
    Done(String),
}

// ─── main app state ──────────────────────────────────────────────────────────

struct CreateApp {
    mode: Mode,
    panel: Panel,

    cursor_avail: usize,
    cursor_sel: usize,
    list_avail: ListState,
    list_sel: ListState,

    filter_text: String,
    show_installed_only: bool,

    all_apps: Vec<InstalledApp>,
    filtered: Vec<InstalledApp>,
    selected: Vec<AppConfig>,

    installed_map: HashMap<String, InstalledMatch>,
    installed_by_name: HashMap<String, InstalledMatch>,
    installed_entries: Vec<(String, InstalledMatch)>,

    search_cache: HashMap<String, Vec<InstalledApp>>,
    search_rx: Option<mpsc::Receiver<(String, Vec<InstalledApp>)>>,
    is_searching: bool,
    last_filter_change: Option<Instant>,

    status_msg: Option<(String, Color, Instant)>,
    should_quit: bool,
}

// Free function to look up installed match without borrowing all of self
fn find_installed_fields<'a>(
    installed_map: &'a HashMap<String, InstalledMatch>,
    installed_by_name: &'a HashMap<String, InstalledMatch>,
    installed_entries: &'a [(String, InstalledMatch)],
    app: &InstalledApp,
) -> Option<&'a InstalledMatch> {
    if let Some(m) = installed_map.get(&app.id.to_lowercase()) {
        return Some(m);
    }
    let norm = normalize_name(&app.name);
    if norm.is_empty() {
        return None;
    }
    if let Some(m) = installed_by_name.get(&norm) {
        return Some(m);
    }
    if norm.len() >= 3 {
        installed_entries.iter().find_map(|(n, m)| {
            if n.len() >= 3 && (n.contains(norm.as_str()) || norm.contains(n.as_str())) {
                Some(m)
            } else {
                None
            }
        })
    } else {
        None
    }
}

impl CreateApp {
    fn new(winget: &WingetService) -> Self {
        let installed_apps: Vec<InstalledApp> = winget
            .get_installed_apps()
            .into_iter()
            .filter(|a| !winget.is_system_app(a))
            .collect();

        let all_apps = winget.search_app("");

        let mut app = Self {
            mode: Mode::Normal,
            panel: Panel::Available,
            cursor_avail: 0,
            cursor_sel: 0,
            list_avail: ListState::default(),
            list_sel: ListState::default(),
            filter_text: String::new(),
            show_installed_only: false,
            all_apps: all_apps.clone(),
            filtered: all_apps,
            selected: vec![],
            installed_map: HashMap::new(),
            installed_by_name: HashMap::new(),
            installed_entries: vec![],
            search_cache: HashMap::new(),
            search_rx: None,
            is_searching: false,
            last_filter_change: None,
            status_msg: None,
            should_quit: false,
        };

        app.rebuild_installed_index(installed_apps);
        app.apply_local_filter();
        app.sync_list_states();
        app
    }

    fn rebuild_installed_index(&mut self, apps: Vec<InstalledApp>) {
        self.installed_map.clear();
        self.installed_by_name.clear();
        self.installed_entries.clear();

        for app in apps {
            let m = InstalledMatch {
                version: app.version.clone(),
                source: if app.source.is_empty() { "local".into() } else { app.source.clone() },
            };
            self.installed_map.insert(app.id.to_lowercase(), m.clone());
            let norm = normalize_name(&app.name);
            if !norm.is_empty() {
                self.installed_by_name.insert(norm.clone(), m.clone());
                self.installed_entries.push((norm, m));
            }
        }
    }

    #[allow(dead_code)]
    fn find_installed(&self, app: &InstalledApp) -> Option<&InstalledMatch> {
        find_installed_fields(
            &self.installed_map,
            &self.installed_by_name,
            &self.installed_entries,
            app,
        )
    }

    fn apply_local_filter(&mut self) {
        let q = self.filter_text.trim().to_lowercase();
        let mut apps: Vec<InstalledApp> = if q.is_empty() {
            self.all_apps.clone()
        } else {
            self.all_apps
                .iter()
                .filter(|a| {
                    a.name.to_lowercase().contains(&q) || a.id.to_lowercase().contains(&q)
                })
                .cloned()
                .collect()
        };

        if self.show_installed_only {
            let im = &self.installed_map;
            let ibn = &self.installed_by_name;
            let ie = &self.installed_entries;
            apps.retain(|a| find_installed_fields(im, ibn, ie, a).is_some());
        }

        self.filtered = apps;
        self.cursor_avail = self.cursor_avail.min(self.filtered.len().saturating_sub(1));
    }

    fn sync_list_states(&mut self) {
        self.list_avail.select(if self.filtered.is_empty() {
            None
        } else {
            Some(self.cursor_avail)
        });
        self.list_sel.select(if self.selected.is_empty() {
            None
        } else {
            Some(self.cursor_sel)
        });
    }

    fn merge_search_results(&mut self, query: &str, results: Vec<InstalledApp>) {
        let mut seen: HashMap<String, ()> = HashMap::new();
        let mut merged = self.all_apps.clone();
        for a in &merged {
            seen.insert(a.id.to_lowercase(), ());
        }
        for a in results {
            if !seen.contains_key(&a.id.to_lowercase()) {
                seen.insert(a.id.to_lowercase(), ());
                merged.push(a);
            }
        }
        self.all_apps = merged;

        // Re-apply if query still matches current filter
        if self.filter_text.trim().to_lowercase() == query {
            self.apply_local_filter();
            self.sync_list_states();
        }
    }

    fn toggle_app(&mut self, app: &InstalledApp) {
        let pos = self.selected.iter().position(|s| s.id == app.id);
        if let Some(i) = pos {
            self.selected.remove(i);
            self.cursor_sel = self.cursor_sel.min(self.selected.len().saturating_sub(1));
        } else {
            self.selected.push(AppConfig {
                id: app.id.clone(),
                name: app.name.clone(),
                version: app.version.clone(),
                available_in_winget: true,
                note: None,
            });
            self.cursor_sel = self.selected.len() - 1;
        }
    }

    fn save_preset(&self, filename: &str) -> Result<String, String> {
        if self.selected.is_empty() {
            return Err("No applications selected".into());
        }
        let name = filename.trim();
        if name.is_empty() {
            return Err("Filename cannot be empty".into());
        }
        let dir = Path::new(PRESET_DIR);
        if let Err(e) = fs::create_dir_all(dir) {
            return Err(format!("Cannot create preset dir: {e}"));
        }
        let fname = if name.ends_with(".json") {
            name.to_string()
        } else {
            format!("{name}.json")
        };
        let full = dir.join(&fname);
        let config = Config { apps: self.selected.clone() };
        let json = serde_json::to_string_pretty(&config)
            .map_err(|e| e.to_string())?;
        fs::write(&full, json).map_err(|e| e.to_string())?;
        Ok(full.to_string_lossy().into_owned())
    }

    fn set_status(&mut self, msg: impl Into<String>, color: Color) {
        self.status_msg = Some((msg.into(), color, Instant::now()));
    }

    // ─── drawing ─────────────────────────────────────────────────────────────

    fn draw(&mut self, frame: &mut Frame) {
        if matches!(self.mode, Mode::Progress { .. }) {
            self.draw_progress(frame);
        } else {
            self.draw_main(frame);
        }
    }

    fn draw_main(&mut self, frame: &mut Frame) {
        let area = frame.area();

        let layout = Layout::vertical([
            Constraint::Length(1), // title
            Constraint::Length(1), // filter bar
            Constraint::Length(1), // status / confirm message
            Constraint::Min(0),    // panels
            Constraint::Length(2), // help
        ])
        .split(area);

        // Title
        frame.render_widget(
            Paragraph::new("  Setup - App Manager")
                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            layout[0],
        );

        // Filter bar
        let installed_label = if self.show_installed_only { "  [Installed only]" } else { "" };
        let search_label = if self.is_searching { "  [Searching…]" } else { "" };
        let filter_line = Line::from(vec![
            Span::styled("Filter: ", Style::default().fg(Color::Gray)),
            Span::styled(
                &self.filter_text,
                Style::default().fg(Color::Yellow),
            ),
            Span::styled("_", Style::default().fg(Color::Yellow)),
            Span::styled(installed_label, Style::default().fg(Color::Magenta)),
            Span::styled(search_label, Style::default().fg(Color::Cyan)),
        ]);
        frame.render_widget(Paragraph::new(filter_line), layout[1]);

        // Status / confirm line
        self.draw_status_line(frame, layout[2]);

        // Panels
        let panels = Layout::horizontal([
            Constraint::Percentage(60),
            Constraint::Percentage(40),
        ])
        .split(layout[3]);

        self.draw_available_panel(frame, panels[0]);
        self.draw_selected_panel(frame, panels[1]);

        // Help
        self.draw_help(frame, layout[4]);

        // Overlays — determine variant first so the borrow on self.mode is dropped
        let overlay: u8 = match &self.mode {
            Mode::FilenameInput { .. } => 1,
            Mode::Confirm { .. } => 2,
            _ => 0,
        };
        if overlay == 1 {
            self.draw_filename_popup(frame);
        } else if overlay == 2 {
            self.draw_confirm_popup(frame);
        }
    }

    fn draw_status_line(&self, frame: &mut Frame, area: Rect) {
        if let Some((msg, color, _)) = &self.status_msg {
            frame.render_widget(
                Paragraph::new(msg.as_str()).style(Style::default().fg(*color)),
                area,
            );
        }
    }

    fn draw_available_panel(&mut self, frame: &mut Frame, area: Rect) {
        let is_active = self.panel == Panel::Available;
        let border_style = if is_active {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let total_pages = (self.filtered.len() + PAGE_SIZE - 1).max(1) / PAGE_SIZE;
        let current_page = self.cursor_avail / PAGE_SIZE + 1;
        let title = format!(
            " Available  ({} apps)  Page {}/{}",
            self.filtered.len(),
            current_page,
            total_pages
        );

        // Pre-compute item data with explicit field borrows to satisfy the borrow checker
        struct ItemData {
            is_sel: bool,
            is_inst: bool,
            name: String,
            id: String,
            source: String,
        }
        let items_data: Vec<ItemData> = {
            let filtered = &self.filtered;
            let selected = &self.selected;
            let installed_map = &self.installed_map;
            let installed_by_name = &self.installed_by_name;
            let installed_entries = &self.installed_entries;
            filtered.iter().map(|app| {
                let is_sel = selected.iter().any(|s| s.id == app.id);
                let is_inst = find_installed_fields(
                    installed_map,
                    installed_by_name,
                    installed_entries,
                    app,
                )
                .is_some();
                ItemData {
                    is_sel,
                    is_inst,
                    name: truncate(&app.name, 24),
                    id: truncate(&app.id, 22),
                    source: app.source.clone(),
                }
            })
            .collect()
        };

        let items: Vec<ListItem> = items_data
            .iter()
            .map(|d| {
                let (icon, icon_color) = match (d.is_sel, d.is_inst) {
                    (true, true) => ("[x]", Color::Green),
                    (true, false) => ("[x]", Color::Blue),
                    (false, true) => ("[i]", Color::Green),
                    (false, false) => ("[ ]", Color::DarkGray),
                };
                let src = if d.source.is_empty() { "" } else { d.source.as_str() };
                Line::from(vec![
                    Span::styled(icon.to_string(), Style::default().fg(icon_color)),
                    Span::raw(" "),
                    Span::raw(format!("{:<24}", d.name)),
                    Span::styled(format!(" {:<22}", d.id), Style::default().fg(Color::DarkGray)),
                    Span::styled(format!(" {src}"), Style::default().fg(Color::Gray)),
                ])
                .into()
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(border_style)
                    .title_style(border_style),
            )
            .highlight_style(Style::default().bg(Color::Cyan).fg(Color::Black))
            .highlight_symbol(if is_active { "> " } else { "  " });

        frame.render_stateful_widget(list, area, &mut self.list_avail);
    }

    fn draw_selected_panel(&mut self, frame: &mut Frame, area: Rect) {
        let is_active = self.panel == Panel::Selected;
        let border_style = if is_active {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let title = format!(" Selected ({}) ", self.selected.len());

        let items: Vec<ListItem> = self
            .selected
            .iter()
            .map(|app| {
                let name = truncate(&app.name, 22);
                let id = truncate(&app.id, 20);
                Line::from(vec![
                    Span::raw(format!("{:<22}", name)),
                    Span::styled(format!(" {}", id), Style::default().fg(Color::DarkGray)),
                ])
                .into()
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(border_style)
                    .title_style(border_style),
            )
            .highlight_style(Style::default().bg(Color::Cyan).fg(Color::Black))
            .highlight_symbol(if is_active { "> " } else { "  " });

        frame.render_stateful_widget(list, area, &mut self.list_sel);
    }

    fn draw_help(&self, frame: &mut Frame, area: Rect) {
        let lines = vec![
            Line::from(vec![
                kspan("[←→]"), Span::raw("Panel  "),
                kspan("[↑↓]"), Span::raw("Navigate  "),
                kspan("[Enter]"), Span::raw("Select/Remove  "),
                kspan("[Tab]"), Span::raw("Installed filter  "),
                kspan("[PgUp/PgDn]"), Span::raw("Jump"),
            ]),
            Line::from(vec![
                kspan("[F5]"), Span::raw("Install  "),
                kspan("[F6]"), Span::raw("Uninstall  "),
                kspan("[Ctrl+S]"), Span::raw("Save preset  "),
                kspan("[Ctrl+Q]"), Span::raw("Quit"),
                Span::raw("    "),
                Span::styled("[i]", Style::default().fg(Color::Green)),
                Span::raw("=installed  "),
                Span::styled("[x]", Style::default().fg(Color::Blue)),
                Span::raw("=selected"),
            ]),
        ];
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().fg(Color::DarkGray)),
            area,
        );
    }

    fn draw_filename_popup(&self, frame: &mut Frame) {
        let input = match &self.mode {
            Mode::FilenameInput { input } => input,
            _ => return,
        };
        let area = centered_rect(50, 5, frame.area());
        frame.render_widget(Clear, area);
        let content = Paragraph::new(vec![
            Line::from(vec![
                Span::styled("Filename: ", Style::default().fg(Color::Gray)),
                Span::styled(input.as_str(), Style::default().fg(Color::Yellow)),
                Span::styled("_", Style::default().fg(Color::Yellow)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "  Enter=save  Esc=cancel",
                Style::default().fg(Color::DarkGray),
            )),
        ])
        .block(
            Block::default()
                .title(" Save Preset ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        );
        frame.render_widget(content, area);
    }

    fn draw_confirm_popup(&self, frame: &mut Frame) {
        let message = match &self.mode {
            Mode::Confirm { message, .. } => message,
            _ => return,
        };
        let area = centered_rect(60, 5, frame.area());
        frame.render_widget(Clear, area);
        let content = Paragraph::new(vec![
            Line::from(Span::styled(message.as_str(), Style::default().fg(Color::White))),
            Line::from(""),
            Line::from(Span::styled(
                "  Enter=confirm  Esc=cancel",
                Style::default().fg(Color::DarkGray),
            )),
        ])
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .title(" Confirm ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        );
        frame.render_widget(content, area);
    }

    fn draw_progress(&self, frame: &mut Frame) {
        let state = match &self.mode {
            Mode::Progress { state } => state,
            _ => return,
        };

        let area = frame.area();
        let layout =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);

        let items: Vec<ListItem> = state.lines.iter().map(|l| ListItem::new(l.clone())).collect();
        let list = List::new(items).block(
            Block::default()
                .title(format!(" {} ", state.title))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );
        frame.render_widget(list, layout[0]);

        let footer_text = if state.done {
            format!("{}  Press any key to return…", state.result_msg)
        } else {
            "Running…".to_string()
        };
        frame.render_widget(
            Paragraph::new(footer_text).style(Style::default().fg(Color::DarkGray)),
            layout[1],
        );
    }
}

// ─── main event loop ─────────────────────────────────────────────────────────

pub fn run() -> Result<()> {
    // Load data before entering TUI
    eprint!("Loading installed apps… ");
    let winget = WingetService::new();
    eprintln!("done");
    eprint!("Loading winget catalog… ");
    let mut app = CreateApp::new(&winget);
    eprintln!("done");

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut CreateApp,
) -> Result<()> {
    loop {
        // Poll background search — use and_then so the borrow on search_rx is dropped
        // before we mutate app below
        let search_done = app.search_rx.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some((query, results)) = search_done {
            app.is_searching = false;
            app.search_rx = None;
            app.search_cache.insert(query.clone(), results.clone());
            app.merge_search_results(&query, results);
        }

        // Expire status message after 2 seconds
        if let Some((_, _, t)) = &app.status_msg {
            if t.elapsed() > Duration::from_secs(2) {
                app.status_msg = None;
            }
        }

        // Check debounce for winget search
        if let Some(t) = app.last_filter_change {
            if t.elapsed() >= Duration::from_millis(450) && !app.is_searching {
                app.last_filter_change = None;
                let q = app.filter_text.trim().to_string();
                if q.len() >= 3 && !app.search_cache.contains_key(&q) {
                    let (tx, rx) = mpsc::channel();
                    app.search_rx = Some(rx);
                    app.is_searching = true;
                    let q2 = q.clone();
                    thread::spawn(move || {
                        let winget = WingetService::new();
                        let results = winget.search_app(&q2);
                        let _ = tx.send((q2, results));
                    });
                }
            }
        }

        // Draw
        terminal.draw(|f| app.draw(f))?;

        // Poll events (short timeout so we keep processing background tasks)
        if !event::poll(Duration::from_millis(50))? {
            continue;
        }

        let ev = event::read()?;

        // Determine current mode without holding a borrow (matches! drops the borrow immediately)
        let in_progress = matches!(app.mode, Mode::Progress { .. });
        let in_confirm  = matches!(app.mode, Mode::Confirm { .. });
        let in_filename = matches!(app.mode, Mode::FilenameInput { .. });

        if in_progress {
            handle_progress_event(app, &ev);
        } else if in_confirm {
            handle_confirm_event(app, &ev, terminal)?;
        } else if in_filename {
            handle_filename_event(app, &ev);
        } else {
            handle_normal_event(app, &ev, terminal)?;
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

fn handle_normal_event(
    app: &mut CreateApp,
    ev: &Event,
    _terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<()> {
    let Event::Key(key) = ev else { return Ok(()) };
    if key.kind != KeyEventKind::Press {
        return Ok(());
    }

    match key.code {
        // Quit
        KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
        }

        // Save
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.selected.is_empty() {
                app.set_status("No applications selected!", Color::Red);
            } else {
                app.mode = Mode::FilenameInput { input: String::new() };
            }
        }

        // F5 – install selected
        KeyCode::F(5) => {
            if app.selected.is_empty() {
                app.set_status("Nothing selected.", Color::Yellow);
            } else {
                let names: Vec<_> = app.selected.iter().map(|a| a.name.as_str()).collect();
                let msg = format!("Install {} app(s)? ({})", app.selected.len(), names.join(", "));
                let action = PendingAction::Install(app.selected.clone());
                app.mode = Mode::Confirm { message: msg, action };
            }
        }

        // F6 – uninstall selected
        KeyCode::F(6) => {
            let installed_sel: Vec<AppConfig> = {
                let im = &app.installed_map;
                let ibn = &app.installed_by_name;
                let ie = &app.installed_entries;
                app.selected
                    .iter()
                    .filter(|s| {
                        let tmp = InstalledApp {
                            id: s.id.clone(),
                            name: s.name.clone(),
                            ..Default::default()
                        };
                        find_installed_fields(im, ibn, ie, &tmp).is_some()
                    })
                    .cloned()
                    .collect()
            };
            if installed_sel.is_empty() {
                app.set_status("No selected apps are installed.", Color::Yellow);
            } else {
                let names: Vec<_> = installed_sel.iter().map(|a| a.name.as_str()).collect();
                let msg = format!(
                    "Uninstall {} app(s)? ({})",
                    installed_sel.len(),
                    names.join(", ")
                );
                app.mode = Mode::Confirm { message: msg, action: PendingAction::Uninstall(installed_sel) };
            }
        }

        // Panel switch
        KeyCode::Left => {
            app.panel = Panel::Available;
            app.sync_list_states();
        }
        KeyCode::Right => {
            app.panel = Panel::Selected;
            app.sync_list_states();
        }

        // Navigation
        KeyCode::Up => {
            move_cursor(app, -1);
        }
        KeyCode::Down => {
            move_cursor(app, 1);
        }
        KeyCode::PageUp => {
            move_cursor(app, -(PAGE_SIZE as i32));
        }
        KeyCode::PageDown => {
            move_cursor(app, PAGE_SIZE as i32);
        }

        // Select / remove
        KeyCode::Enter => {
            if app.panel == Panel::Available {
                if let Some(app_item) = app.filtered.get(app.cursor_avail).cloned() {
                    app.toggle_app(&app_item);
                    app.sync_list_states();
                }
            } else if !app.selected.is_empty() {
                app.selected.remove(app.cursor_sel);
                app.cursor_sel = app.cursor_sel.min(app.selected.len().saturating_sub(1));
                app.sync_list_states();
            }
        }

        // Toggle installed filter
        KeyCode::Tab => {
            app.show_installed_only = !app.show_installed_only;
            app.apply_local_filter();
            app.sync_list_states();
        }

        // Text input (filter)
        KeyCode::Backspace => {
            app.filter_text.pop();
            app.apply_local_filter();
            app.sync_list_states();
            app.last_filter_change = Some(Instant::now());
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.filter_text.push(c);
            app.apply_local_filter();
            app.sync_list_states();
            app.last_filter_change = Some(Instant::now());
        }

        _ => {}
    }

    Ok(())
}

fn handle_filename_event(app: &mut CreateApp, ev: &Event) {
    let Event::Key(key) = ev else { return };
    if key.kind != KeyEventKind::Press {
        return;
    }

    let input = match &mut app.mode {
        Mode::FilenameInput { input } => input,
        _ => return,
    };

    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
        }
        KeyCode::Enter => {
            let name = input.trim().to_string();
            if name.is_empty() {
                app.mode = Mode::Normal;
                return;
            }
            match app.save_preset(&name) {
                Ok(path) => {
                    app.set_status(format!("Saved to {path}"), Color::Green);
                    app.mode = Mode::Normal;
                }
                Err(e) => {
                    app.set_status(format!("Error: {e}"), Color::Red);
                    app.mode = Mode::Normal;
                }
            }
        }
        KeyCode::Backspace => {
            input.pop();
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                input.push(c);
            }
        }
        _ => {}
    }
}

fn handle_confirm_event(
    app: &mut CreateApp,
    ev: &Event,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<()> {
    let Event::Key(key) = ev else { return Ok(()) };
    if key.kind != KeyEventKind::Press {
        return Ok(());
    }

    match key.code {
        KeyCode::Enter => {
            let action = match std::mem::replace(&mut app.mode, Mode::Normal) {
                Mode::Confirm { action, .. } => action,
                other => {
                    app.mode = other;
                    return Ok(());
                }
            };
            run_action(app, action, terminal)?;
        }
        KeyCode::Esc | KeyCode::Char('n') => {
            app.mode = Mode::Normal;
        }
        _ => {}
    }

    Ok(())
}

fn handle_progress_event(app: &mut CreateApp, ev: &Event) {
    // Phase 1: poll messages from background thread (scoped borrow of app.mode)
    let is_done = if let Mode::Progress { state } = &mut app.mode {
        loop {
            match state.rx.try_recv() {
                Ok(ProgressMsg::Line(line)) => state.lines.push(line),
                Ok(ProgressMsg::Done(msg)) => {
                    state.done = true;
                    state.result_msg = msg;
                }
                Err(_) => break,
            }
        }
        state.done
    } else {
        false
    }; // mutable borrow of app.mode ends here

    // Phase 2: if done, wait for any key then return to main view
    if is_done {
        if let Event::Key(key) = ev {
            if key.kind == KeyEventKind::Press {
                let winget = WingetService::new();
                let installed: Vec<InstalledApp> = winget
                    .get_installed_apps()
                    .into_iter()
                    .filter(|a| !winget.is_system_app(a))
                    .collect();
                app.rebuild_installed_index(installed);
                app.apply_local_filter();
                app.sync_list_states();
                app.mode = Mode::Normal;
            }
        }
    }
}

fn run_action(
    app: &mut CreateApp,
    action: PendingAction,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<()> {
    let (title, apps) = match &action {
        PendingAction::Install(a) => ("Installing", a.clone()),
        PendingAction::Uninstall(a) => ("Uninstalling", a.clone()),
    };

    let (tx, rx) = mpsc::channel::<ProgressMsg>();
    app.mode = Mode::Progress {
        state: ProgressState {
            title: title.to_string(),
            lines: vec![],
            done: false,
            result_msg: String::new(),
            rx,
        },
    };
    terminal.draw(|f| app.draw(f))?;

    let tx2 = tx.clone();
    thread::spawn(move || {
        let winget = WingetService::new();
        let mut ok = 0usize;
        let mut fail = 0usize;

        for a in &apps {
            let _ = tx2.send(ProgressMsg::Line(format!(">>> {}", a.name)));
            let result = match &action {
                PendingAction::Install(_) => {
                    winget.install_app(&a.id, |line| {
                        let _ = tx2.send(ProgressMsg::Line(format!("  {line}")));
                    })
                }
                PendingAction::Uninstall(_) => {
                    winget.uninstall_app(&a.id, |line| {
                        let _ = tx2.send(ProgressMsg::Line(format!("  {line}")));
                    })
                }
            };
            if result.success {
                ok += 1;
                let _ = tx2.send(ProgressMsg::Line(format!("  OK – {}", result.message)));
            } else {
                fail += 1;
                let _ = tx2.send(ProgressMsg::Line(format!("  FAIL – {}", result.message)));
            }
        }

        let summary = format!("Done: {ok} succeeded, {fail} failed.");
        let _ = tx2.send(ProgressMsg::Done(summary));
    });

    Ok(())
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn move_cursor(app: &mut CreateApp, delta: i32) {
    match app.panel {
        Panel::Available => {
            let len = app.filtered.len() as i32;
            if len > 0 {
                app.cursor_avail =
                    ((app.cursor_avail as i32 + delta).rem_euclid(len)) as usize;
            }
        }
        Panel::Selected => {
            let len = app.selected.len() as i32;
            if len > 0 {
                app.cursor_sel =
                    ((app.cursor_sel as i32 + delta).rem_euclid(len)) as usize;
            }
        }
    }
    app.sync_list_states();
}

fn normalize_name(name: &str) -> String {
    let lower = name.to_lowercase();
    let without_arch = lower
        .replace("x64", " ")
        .replace("x86", " ")
        .replace("win64", " ")
        .replace("win32", " ")
        .replace("stable", " ")
        .replace("browser", " ");
    without_arch
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

fn kspan(s: &str) -> Span<'static> {
    Span::styled(s.to_string(), Style::default().fg(Color::Yellow))
}

fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let vert = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vert[1])[1]
}
