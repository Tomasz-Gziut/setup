use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use regex::Regex;

use crate::types::InstalledApp;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x08000000;

static EXCLUDED_PATTERNS: &[&str] = &[
    r"(?i)^Microsoft\.NET",
    r"(?i)^Microsoft\.VCRedist",
    r"(?i)^Microsoft\.VC\+\+",
    r"(?i)^Microsoft\.UI\.Xaml",
    r"(?i)^Microsoft\.Windows",
    r"(?i)^Microsoft\.DirectX",
    r"(?i)^Microsoft\.GameInput",
    r"(?i)^Microsoft\.Update",
    r"(?i)^Microsoft\.VSS",
    r"(?i)^Microsoft\.ODBC",
    r"(?i)^Microsoft\.OLE",
    r"(?i)^Microsoft\.Help",
    r"(?i)^Microsoft\.SQL.*Setup",
    r"(?i)^Microsoft\.VisualStudio\.Installer",
    r"(?i)^Microsoft\.VisualStudio\.Tools",
    r"(?i)WindowsAppRuntime",
    r"(?i)WindowsDesktopRuntime",
    r"(?i)^ARP\\",
    r"(?i)^MSIX\\",
    r"(?i)Driver",
    r"(?i)^NVIDIA\.Control",
    r"(?i)^Realtek",
    r"(?i)^Synaptics",
    r"(?i)^Intel\.",
    r"(?i)^AMD\.",
    r"(?i)Redistributable",
    r"(?i)Runtime Package",
    r"(?i)\.Net.*Runtime",
    r"(?i)^dotnet",
    r"(?i)^Microsoft\.Advertising",
    r"(?i)^Microsoft\.Services",
    r"(?i)^Microsoft\.StorePurchase",
    r"(?i)^Microsoft\.VP9",
    r"(?i)^Microsoft\.HEVC",
    r"(?i)^Microsoft\.AV1",
    r"(?i)^Microsoft\.MPEG",
    r"(?i)^Microsoft\.WebMedia",
    r"(?i)^Microsoft\.WebP",
    r"(?i)^Microsoft\.Raw",
    r"(?i)^Microsoft\.HEIFImage",
    r"(?i)Local Experience Pack",
    r"(?i)Language Pack",
    r"(?i)Speech Pack",
    r"Pakiet lokalizacyjny",
    r"本地体验包",
    r"(?i)^Microsoft\.Xbox.*Provider",
    r"(?i)^Microsoft\.Xbox.*Plugin",
    r"(?i)^Microsoft\.Gaming",
    r"(?i)^Microsoft\.Wallet",
    r"(?i)^Microsoft\.People",
    r"(?i)^Microsoft\.GetHelp",
    r"(?i)^Microsoft\.Getstarted",
    r"(?i)^Microsoft\.MixedReality",
    r"(?i)^Microsoft\.549981",
    r"(?i)^Microsoft\.BingNews",
    r"(?i)^Microsoft\.BingWeather",
    r"(?i)^Microsoft\.ZuneMusic",
    r"(?i)^Microsoft\.ZuneVideo",
    r"(?i)Widget.*Runtime",
    r"Host środowiska",
    r"Usługi gier",
];

pub struct InstallResult {
    pub success: bool,
    pub message: String,
}

pub struct WingetService {
    excluded: Vec<Regex>,
}

impl WingetService {
    pub fn new() -> Self {
        let excluded = EXCLUDED_PATTERNS
            .iter()
            .filter_map(|p| Regex::new(p).ok())
            .collect();
        Self { excluded }
    }

    pub fn is_system_app(&self, app: &InstalledApp) -> bool {
        self.excluded
            .iter()
            .any(|re| re.is_match(&app.id) || re.is_match(&app.name))
    }

    fn run(&self, args: &[&str]) -> String {
        self.run_with_status(args).0
    }

    fn run_with_status(&self, args: &[&str]) -> (String, Option<i32>) {
        let mut cmd = Command::new("winget");
        cmd.args(args);
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        match cmd.output() {
            Ok(out) => (command_text(out.stdout, out.stderr), out.status.code()),
            Err(e) => (e.to_string(), None),
        }
    }

    fn run_str(&self, args: &[String]) -> String {
        let mut cmd = Command::new("winget");
        cmd.args(args);
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        match cmd.output() {
            Ok(out) => command_text(out.stdout, out.stderr),
            Err(_) => String::new(),
        }
    }

    pub fn get_installed_apps(&self) -> Vec<InstalledApp> {
        let out = self.run(&["list", "--disable-interactivity"]);
        let apps = self.parse_table(&out);
        if apps.is_empty() {
            self.get_installed_apps_from_registry()
        } else {
            apps
        }
    }

    pub fn is_available_in_winget(&self, id: &str) -> bool {
        if self.is_available_in_index(id) {
            return true;
        }

        let out = self.run(&[
            "search",
            "--id",
            id,
            "--exact",
            "--source",
            "winget",
            "--accept-source-agreements",
            "--disable-interactivity",
        ]);
        out.to_lowercase().contains(&id.to_lowercase())
    }

    pub fn is_app_installed(&self, id: &str) -> bool {
        self.get_installed_apps()
            .iter()
            .any(|a| a.id.to_lowercase() == id.to_lowercase())
    }

    pub fn search_app(&self, query: &str) -> Vec<InstalledApp> {
        let query = query.trim();
        if query.is_empty() {
            return self.get_catalog_apps();
        }

        if let Ok(apps) = self.search_catalog_index(query, 100) {
            return apps;
        }

        let mut args = vec!["search".to_string()];
        args.push("--query".to_string());
        args.push(query.to_string());
        args.push("--source".to_string());
        args.push("winget".to_string());
        args.push("--accept-source-agreements".to_string());
        args.push("--disable-interactivity".to_string());

        let out = self.run_str(&args);
        self.parse_table(&out)
    }

    pub fn get_catalog_apps(&self) -> Vec<InstalledApp> {
        self.get_catalog_apps_with_error().0
    }

    pub fn get_catalog_apps_with_error(&self) -> (Vec<InstalledApp>, Option<String>) {
        match self.read_catalog_index(None, None) {
            Ok(apps) if !apps.is_empty() => return (apps, None),
            Ok(_) => {}
            Err(_) => {}
        }

        let attempts = [vec![
            "search",
            ".",
            "--source",
            "winget",
            "--accept-source-agreements",
            "--disable-interactivity",
        ]];

        let mut last_error = None;
        for args in attempts {
            let (out, code) = self.run_with_status(&args);
            let apps = self.parse_table(&out);
            if !apps.is_empty() {
                return (apps, None);
            }

            let command = format!("winget {}", args.join(" "));
            last_error = Some(match code {
                Some(0) => format!("{command} returned no packages"),
                Some(c) => format!("{command} failed with exit code {c}"),
                None => format!("{command} could not be started: {out}"),
            });
            if !out.trim().is_empty() {
                last_error = Some(format!(
                    "{}: {}",
                    last_error.unwrap(),
                    out.lines().next().unwrap_or_default()
                ));
            }
        }

        (vec![], last_error)
    }

    fn search_catalog_index(&self, query: &str, limit: usize) -> Result<Vec<InstalledApp>, String> {
        self.read_catalog_index(Some(query), Some(limit))
    }

    fn is_available_in_index(&self, id: &str) -> bool {
        let Ok(apps) = self.read_catalog_index(Some(id), Some(1)) else {
            return false;
        };
        apps.iter().any(|app| app.id.eq_ignore_ascii_case(id))
    }

    fn read_catalog_index(
        &self,
        query: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<InstalledApp>, String> {
        let index_path = find_winget_source_index()
            .ok_or_else(|| "Winget source index was not found".to_string())?;
        let mut apps = read_winget_packages_from_sqlite(&index_path)?;
        if let Some(query) = query {
            let q = query.to_lowercase();
            apps.retain(|app| {
                app.name.to_lowercase().contains(&q)
                    || app.id.to_lowercase().contains(&q)
                    || app.source.to_lowercase().contains(&q)
            });
            if let Some(limit) = limit {
                apps.truncate(limit);
            }
        }

        Ok(apps)
    }

    pub fn install_app(&self, id: &str, mut on_line: impl FnMut(&str)) -> InstallResult {
        let was_installed = self.is_app_installed(id);

        let mut cmd = Command::new("winget");
        cmd.args([
            "install",
            "--id",
            id,
            "--accept-source-agreements",
            "--accept-package-agreements",
            "--disable-interactivity",
            "--silent",
            "--force",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);

        let mut child = match cmd.spawn() {
            Err(e) => {
                return InstallResult {
                    success: false,
                    message: e.to_string(),
                }
            }
            Ok(c) => c,
        };

        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);
        let mut full_output = String::new();

        for line in reader.lines().map_while(Result::ok) {
            on_line(&line);
            full_output.push_str(&line);
            full_output.push('\n');
        }

        let child_status = child.wait().ok();
        let is_installed = self.is_app_installed(id);

        if is_installed || child_status.map_or(false, |s| s.success()) {
            if was_installed && full_output.contains("already installed") {
                InstallResult {
                    success: true,
                    message: "Already installed".into(),
                }
            } else {
                InstallResult {
                    success: true,
                    message: "Installed successfully".into(),
                }
            }
        } else {
            InstallResult {
                success: false,
                message: full_output
                    .lines()
                    .last()
                    .unwrap_or("Installation failed")
                    .to_string(),
            }
        }
    }

    pub fn uninstall_app(&self, id: &str, mut on_line: impl FnMut(&str)) -> InstallResult {
        let mut cmd = Command::new("winget");
        cmd.args([
            "uninstall",
            "--id",
            id,
            "--disable-interactivity",
            "--silent",
            "--force",
            "--purge",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);

        let mut child = match cmd.spawn() {
            Err(e) => {
                return InstallResult {
                    success: false,
                    message: e.to_string(),
                }
            }
            Ok(c) => c,
        };

        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);
        let mut full_output = String::new();

        for line in reader.lines().map_while(Result::ok) {
            on_line(&line);
            full_output.push_str(&line);
            full_output.push('\n');
        }

        let child_status = child.wait().ok();
        let is_installed = self.is_app_installed(id);

        if !is_installed || child_status.map_or(false, |s| s.success()) {
            InstallResult {
                success: true,
                message: "Uninstalled successfully".into(),
            }
        } else {
            InstallResult {
                success: false,
                message: "Uninstall failed – app still installed".into(),
            }
        }
    }

    pub fn parse_table(&self, raw: &str) -> Vec<InstalledApp> {
        let raw = raw.trim_start_matches('\u{feff}');
        let ansi = Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").ok();

        let lines: Vec<String> = raw
            .split('\n')
            .map(|l| {
                let l = if l.contains('\r') {
                    l.split('\r').last().unwrap_or(l).to_string()
                } else {
                    l.to_string()
                };
                if let Some(ansi) = &ansi {
                    ansi.replace_all(&l, "").into_owned()
                } else {
                    l
                }
            })
            .filter(|l| !l.trim().is_empty())
            .collect();

        let sep_idx = match lines.iter().position(|l| {
            let t = l.trim();
            t.len() >= 3 && t.chars().all(|c| c == '-' || c == ' ')
        }) {
            Some(i) => i,
            None => return vec![],
        };

        let sep = &lines[sep_idx];

        // Column start positions from dash groups in separator
        let mut col_starts: Vec<usize> = vec![];
        let mut in_dash = false;
        for (i, c) in sep.chars().enumerate() {
            if c == '-' && !in_dash {
                col_starts.push(i);
                in_dash = true;
            } else if c == ' ' {
                in_dash = false;
            }
        }

        if col_starts.len() < 2 {
            return parse_table_by_spacing(&lines, sep_idx);
        }

        let mut apps = vec![];

        for line in &lines[sep_idx + 1..] {
            if line.trim().starts_with("---") {
                continue;
            }
            let ll = line.chars().count();

            let col = |s: usize, e: usize| -> String {
                let s = s.min(ll);
                let e = e.min(ll);
                line.chars()
                    .skip(s)
                    .take(e.saturating_sub(s))
                    .collect::<String>()
                    .trim()
                    .to_string()
            };

            let last = col_starts.len() - 1;
            let name = col(col_starts[0], col_starts.get(1).copied().unwrap_or(ll));
            let id = col(col_starts[1], col_starts.get(2).copied().unwrap_or(ll));
            let version = if col_starts.len() > 2 {
                col(col_starts[2], col_starts.get(3).copied().unwrap_or(ll))
            } else {
                String::new()
            };
            let source = col(col_starts[last], ll);

            if !name.is_empty() && !id.is_empty() {
                apps.push(InstalledApp {
                    name,
                    id,
                    version,
                    source,
                });
            }
        }

        if apps.is_empty() {
            parse_table_by_spacing(&lines, sep_idx)
        } else {
            apps
        }
    }

    fn get_installed_apps_from_registry(&self) -> Vec<InstalledApp> {
        let roots = [
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
            r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ];

        let mut apps = vec![];
        let mut seen = HashSet::new();

        for root in roots {
            let mut cmd = Command::new("reg");
            cmd.args(["query", root, "/s"]);
            #[cfg(windows)]
            cmd.creation_flags(CREATE_NO_WINDOW);

            let Ok(out) = cmd.output() else {
                continue;
            };

            let raw = String::from_utf8_lossy(&out.stdout);
            for app in parse_registry_uninstall_output(&raw) {
                let key = app.name.to_lowercase();
                if seen.insert(key) {
                    apps.push(app);
                }
            }
        }

        apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        apps
    }
}

fn command_text(stdout: Vec<u8>, stderr: Vec<u8>) -> String {
    let mut text = String::from_utf8_lossy(&stdout).into_owned();
    if !stderr.is_empty() {
        if !text.ends_with('\n') && !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&String::from_utf8_lossy(&stderr));
    }
    text
}

fn find_winget_source_index() -> Option<PathBuf> {
    let from_appx = winget_source_index_from_appx_package();
    if from_appx.is_some() {
        return from_appx;
    }

    winget_source_index_from_windows_apps()
}

fn winget_source_index_from_appx_package() -> Option<PathBuf> {
    let mut cmd = Command::new("powershell");
    cmd.args([
            "-NoProfile",
            "-Command",
            "Get-AppxPackage Microsoft.Winget.Source | Sort-Object Version -Descending | Select-Object -First 1 -ExpandProperty InstallLocation",
        ]);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let output = cmd.output().ok()?;

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return None;
    }

    let index = PathBuf::from(path).join("Public").join("index.db");
    index.exists().then_some(index)
}

fn winget_source_index_from_windows_apps() -> Option<PathBuf> {
    let root = Path::new(r"C:\Program Files\WindowsApps");
    let entries = std::fs::read_dir(root).ok()?;
    let mut matches: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| name.starts_with("Microsoft.Winget.Source_"))
        })
        .map(|path| path.join("Public").join("index.db"))
        .filter(|path| path.exists())
        .collect();

    matches.sort();
    matches.pop()
}

fn read_winget_packages_from_sqlite(path: &Path) -> Result<Vec<InstalledApp>, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    if data.len() < 100 || &data[..16] != b"SQLite format 3\0" {
        return Err("Invalid SQLite index header".into());
    }

    let page_size = match read_u16(&data, 16)? {
        1 => 65_536,
        n => n as usize,
    };

    let mut rows = Vec::new();
    read_sqlite_table_page(&data, page_size, 3, &mut rows)?;
    rows.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.to_lowercase().cmp(&b.id.to_lowercase()))
    });
    Ok(rows)
}

fn read_sqlite_table_page(
    data: &[u8],
    page_size: usize,
    page_no: u32,
    rows: &mut Vec<InstalledApp>,
) -> Result<(), String> {
    let page_start = (page_no as usize)
        .checked_sub(1)
        .and_then(|n| n.checked_mul(page_size))
        .ok_or_else(|| "Invalid SQLite page number".to_string())?;
    let header_start = if page_no == 1 {
        page_start + 100
    } else {
        page_start
    };
    if header_start >= data.len() {
        return Err("SQLite page outside file".into());
    }

    let page_type = data[header_start];
    let cell_count = read_u16(data, header_start + 3)? as usize;
    let cell_ptr_start = header_start + if page_type == 0x05 { 12 } else { 8 };

    match page_type {
        0x05 => {
            for i in 0..cell_count {
                let ptr = read_u16(data, cell_ptr_start + (i * 2))? as usize;
                let cell = page_start + ptr;
                let child = read_u32(data, cell)?;
                read_sqlite_table_page(data, page_size, child, rows)?;
            }
            let right_child = read_u32(data, header_start + 8)?;
            read_sqlite_table_page(data, page_size, right_child, rows)
        }
        0x0d => {
            for i in 0..cell_count {
                let ptr = read_u16(data, cell_ptr_start + (i * 2))? as usize;
                let cell = page_start + ptr;
                if let Some(app) = parse_sqlite_package_cell(data, cell)? {
                    rows.push(app);
                }
            }
            Ok(())
        }
        _ => Err(format!("Unsupported SQLite page type {page_type:#x}")),
    }
}

fn parse_sqlite_package_cell(data: &[u8], offset: usize) -> Result<Option<InstalledApp>, String> {
    let (payload_len, n1) = read_varint(data, offset)?;
    let (_, n2) = read_varint(data, offset + n1)?;
    let payload_start = offset + n1 + n2;
    let payload_end = payload_start
        .checked_add(payload_len as usize)
        .ok_or_else(|| "Invalid SQLite payload length".to_string())?;
    if payload_end > data.len() {
        return Err("SQLite payload outside file".into());
    }

    let payload = &data[payload_start..payload_end];
    let (header_len, h_used) = read_varint(payload, 0)?;
    let header_len = header_len as usize;
    if header_len > payload.len() || h_used > header_len {
        return Err("Invalid SQLite record header".into());
    }

    let mut serials = Vec::new();
    let mut pos = h_used;
    while pos < header_len {
        let (serial, used) = read_varint(payload, pos)?;
        serials.push(serial);
        pos += used;
    }

    let mut values = Vec::new();
    let mut body_pos = header_len;
    for serial in serials {
        let len = sqlite_serial_len(serial)?;
        let end = body_pos
            .checked_add(len)
            .ok_or_else(|| "Invalid SQLite field length".to_string())?;
        if end > payload.len() {
            return Err("SQLite field outside payload".into());
        }
        values.push(sqlite_serial_text(serial, &payload[body_pos..end])?);
        body_pos = end;
    }

    let id = values.get(1).and_then(|v| v.clone()).unwrap_or_default();
    let name = values.get(2).and_then(|v| v.clone()).unwrap_or_default();
    let version = values.get(4).and_then(|v| v.clone()).unwrap_or_default();
    if id.is_empty() || name.is_empty() {
        return Ok(None);
    }

    Ok(Some(InstalledApp {
        name,
        id,
        version,
        source: "winget".into(),
    }))
}

fn sqlite_serial_len(serial: u64) -> Result<usize, String> {
    match serial {
        0 | 8 | 9 => Ok(0),
        1 => Ok(1),
        2 => Ok(2),
        3 => Ok(3),
        4 => Ok(4),
        5 => Ok(6),
        6 | 7 => Ok(8),
        n if n >= 12 => Ok(((n - 12) / 2) as usize),
        _ => Err(format!("Unsupported SQLite serial type {serial}")),
    }
}

fn sqlite_serial_text(serial: u64, bytes: &[u8]) -> Result<Option<String>, String> {
    if serial == 0 {
        return Ok(None);
    }
    if serial >= 13 && serial % 2 == 1 {
        return Ok(Some(String::from_utf8_lossy(bytes).into_owned()));
    }
    Ok(Some(String::new()))
}

fn read_varint(data: &[u8], offset: usize) -> Result<(u64, usize), String> {
    let mut value = 0u64;
    for i in 0..9 {
        let byte = *data
            .get(offset + i)
            .ok_or_else(|| "Unexpected end of varint".to_string())?;
        if i == 8 {
            value = (value << 8) | byte as u64;
            return Ok((value, 9));
        }
        value = (value << 7) | (byte & 0x7f) as u64;
        if byte & 0x80 == 0 {
            return Ok((value, i + 1));
        }
    }
    Err("Invalid SQLite varint".into())
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16, String> {
    let bytes = data
        .get(offset..offset + 2)
        .ok_or_else(|| "Unexpected end reading u16".to_string())?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32, String> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or_else(|| "Unexpected end reading u32".to_string())?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn parse_table_by_spacing(lines: &[String], sep_idx: usize) -> Vec<InstalledApp> {
    let splitter = Regex::new(r"\s{2,}").ok();
    let Some(splitter) = splitter else {
        return vec![];
    };

    let mut apps = vec![];
    for line in &lines[sep_idx + 1..] {
        let trimmed = line.trim();
        if trimmed.starts_with("---") {
            continue;
        }

        let parts: Vec<&str> = splitter
            .split(trimmed)
            .filter(|p| !p.trim().is_empty())
            .collect();

        if parts.len() < 3 {
            continue;
        }

        let source = if parts.len() > 3 {
            parts.last().copied().unwrap_or_default()
        } else {
            ""
        };

        apps.push(InstalledApp {
            name: parts[0].trim().to_string(),
            id: parts[1].trim().to_string(),
            version: parts[2].trim().to_string(),
            source: source.trim().to_string(),
        });
    }

    apps
}

fn parse_registry_uninstall_output(raw: &str) -> Vec<InstalledApp> {
    let mut apps = vec![];
    let mut current_key = String::new();
    let mut package_id = String::new();
    let mut name = String::new();
    let mut version = String::new();
    let mut system_component = false;

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("HKEY_") {
            push_registry_app(
                &mut apps,
                &current_key,
                &package_id,
                &name,
                &version,
                system_component,
            );
            current_key = trimmed.to_string();
            package_id.clear();
            name.clear();
            version.clear();
            system_component = false;
            continue;
        }

        if let Some(value) = parse_reg_value(trimmed, "WinGetPackageIdentifier") {
            package_id = value;
        } else if let Some(value) = parse_reg_value(trimmed, "DisplayName") {
            name = value;
        } else if let Some(value) = parse_reg_value(trimmed, "DisplayVersion") {
            version = value;
        } else if let Some(value) = parse_reg_value(trimmed, "SystemComponent") {
            system_component = value.trim() == "0x1";
        }
    }

    push_registry_app(
        &mut apps,
        &current_key,
        &package_id,
        &name,
        &version,
        system_component,
    );
    apps
}

fn parse_reg_value(line: &str, name: &str) -> Option<String> {
    if !line.starts_with(name) {
        return None;
    }

    let rest = line[name.len()..].trim_start();
    let value_start = rest.find("    ").map(|idx| idx + 4)?;
    Some(rest[value_start..].trim().to_string())
}

fn push_registry_app(
    apps: &mut Vec<InstalledApp>,
    key: &str,
    package_id: &str,
    name: &str,
    version: &str,
    system_component: bool,
) {
    if name.trim().is_empty() || system_component {
        return;
    }

    let id = if package_id.trim().is_empty() {
        key.rsplit('\\')
            .next()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(name)
            .to_string()
    } else {
        package_id.trim().to_string()
    };

    apps.push(InstalledApp {
        name: name.trim().to_string(),
        id,
        version: version.trim().to_string(),
        source: if package_id.trim().is_empty() {
            "registry".into()
        } else {
            "winget".into()
        },
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_winget_tables_with_non_ascii_names() {
        let raw = "\
Name                 Id                         Version Source
-------------------- -------------------------- ------- ------
Zażółć gęślą jaźń    Example.Polish             1.0.0   winget
Visual Studio Code   Microsoft.VisualStudioCode 1.2.3   winget
";

        let apps = WingetService::new().parse_table(raw);

        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].name, "Zażółć gęślą jaźń");
        assert_eq!(apps[0].id, "Example.Polish");
        assert_eq!(apps[0].version, "1.0.0");
        assert_eq!(apps[0].source, "winget");
        assert_eq!(apps[1].id, "Microsoft.VisualStudioCode");
    }

    #[test]
    fn parses_winget_tables_with_ansi_and_spacing_fallback() {
        let raw = "\
\x1b[?25lName              Id                          Version Match        Source
---------------------------------------------------------------------------
Visual Studio Code  Microsoft.VisualStudioCode  1.2.3   Moniker: code winget
Docker Desktop      Docker.DockerDesktop        4.0.0   Tag: docker   winget
\x1b[?25h";

        let apps = WingetService::new().parse_table(raw);

        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].name, "Visual Studio Code");
        assert_eq!(apps[0].id, "Microsoft.VisualStudioCode");
        assert_eq!(apps[1].source, "winget");
    }

    #[test]
    fn reads_local_winget_source_index_when_present() {
        let Some(path) = find_winget_source_index() else {
            return;
        };

        let apps = read_winget_packages_from_sqlite(&path).expect("read winget source index");

        assert!(apps
            .iter()
            .any(|app| app.id == "Microsoft.VisualStudioCode"));
    }

    #[test]
    fn parses_registry_uninstall_entries() {
        let raw = r"
HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Example
    DisplayName    REG_SZ    Example App
    DisplayVersion    REG_SZ    2.3.4

HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Hidden
    DisplayName    REG_SZ    Hidden Runtime
    SystemComponent    REG_DWORD    0x1
";

        let apps = parse_registry_uninstall_output(raw);

        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Example App");
        assert_eq!(apps[0].id, "Example");
        assert_eq!(apps[0].version, "2.3.4");
        assert_eq!(apps[0].source, "registry");
    }
}
