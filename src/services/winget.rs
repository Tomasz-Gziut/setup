use std::io::{BufRead, BufReader};
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
        let mut cmd = Command::new("winget");
        cmd.args(args);
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        match cmd.output() {
            Ok(out) => String::from_utf8_lossy(&out.stdout).into_owned(),
            Err(_) => String::new(),
        }
    }

    fn run_str(&self, args: &[String]) -> String {
        let mut cmd = Command::new("winget");
        cmd.args(args);
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        match cmd.output() {
            Ok(out) => String::from_utf8_lossy(&out.stdout).into_owned(),
            Err(_) => String::new(),
        }
    }

    pub fn get_installed_apps(&self) -> Vec<InstalledApp> {
        let out = self.run(&["list", "--disable-interactivity"]);
        self.parse_table(&out)
    }

    pub fn is_available_in_winget(&self, id: &str) -> bool {
        let out = self.run(&["search", "--id", id, "--exact", "--disable-interactivity"]);
        out.to_lowercase().contains(&id.to_lowercase())
    }

    pub fn is_app_installed(&self, id: &str) -> bool {
        self.get_installed_apps()
            .iter()
            .any(|a| a.id.to_lowercase() == id.to_lowercase())
    }

    pub fn search_app(&self, query: &str) -> Vec<InstalledApp> {
        let args = vec![
            "search".to_string(),
            query.to_string(),
            "--disable-interactivity".to_string(),
        ];
        let out = self.run_str(&args);
        self.parse_table(&out)
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
            "--force",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);

        let child = match cmd.spawn() {
            Err(e) => return InstallResult { success: false, message: e.to_string() },
            Ok(c) => c,
        };

        let stdout = child.stdout.unwrap();
        let reader = BufReader::new(stdout);
        let mut full_output = String::new();

        for line in reader.lines().map_while(Result::ok) {
            on_line(&line);
            full_output.push_str(&line);
            full_output.push('\n');
        }

        let is_installed = self.is_app_installed(id);

        if is_installed {
            if was_installed && full_output.contains("already installed") {
                InstallResult { success: true, message: "Already installed".into() }
            } else {
                InstallResult { success: true, message: "Installed successfully".into() }
            }
        } else {
            InstallResult { success: false, message: full_output.lines().last().unwrap_or("Installation failed").to_string() }
        }
    }

    pub fn uninstall_app(&self, id: &str, mut on_line: impl FnMut(&str)) -> InstallResult {
        let mut cmd = Command::new("winget");
        cmd.args(["uninstall", "--id", id, "--disable-interactivity", "--force", "--purge"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);

        let child = match cmd.spawn() {
            Err(e) => return InstallResult { success: false, message: e.to_string() },
            Ok(c) => c,
        };

        let stdout = child.stdout.unwrap();
        let reader = BufReader::new(stdout);

        for line in reader.lines().map_while(Result::ok) {
            on_line(&line);
        }

        if self.is_app_installed(id) {
            InstallResult { success: false, message: "Uninstall failed – app still installed".into() }
        } else {
            InstallResult { success: true, message: "Uninstalled successfully".into() }
        }
    }

    pub fn parse_table(&self, raw: &str) -> Vec<InstalledApp> {
        let raw = raw.trim_start_matches('\u{feff}');

        let lines: Vec<String> = raw
            .split('\n')
            .map(|l| {
                if l.contains('\r') {
                    l.split('\r').last().unwrap_or(l).to_string()
                } else {
                    l.to_string()
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
        for (i, b) in sep.bytes().enumerate() {
            if b == b'-' && !in_dash {
                col_starts.push(i);
                in_dash = true;
            } else if b == b' ' {
                in_dash = false;
            }
        }

        if col_starts.len() < 2 {
            return vec![];
        }

        let mut apps = vec![];

        for line in &lines[sep_idx + 1..] {
            if line.trim().starts_with("---") {
                continue;
            }
            let lb = line.as_bytes();
            let ll = lb.len();

            let col = |s: usize, e: usize| -> String {
                let s = s.min(ll);
                let e = e.min(ll);
                String::from_utf8_lossy(&lb[s..e]).trim().to_string()
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
                apps.push(InstalledApp { name, id, version, source });
            }
        }

        apps
    }
}
