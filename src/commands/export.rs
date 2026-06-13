use std::fs;
use std::path::Path;

use anyhow::Result;

use crate::services::winget::WingetService;
use crate::types::{AppConfig, Config};

pub fn run(output_path: &str) -> Result<()> {
    let winget = WingetService::new();

    eprintln!("Fetching installed applications...");
    let all_apps = winget.get_installed_apps();

    if all_apps.is_empty() {
        println!("No applications found.");
        return Ok(());
    }

    let apps: Vec<_> = all_apps.iter().filter(|a| !winget.is_system_app(a)).collect();
    let excluded = all_apps.len() - apps.len();

    println!("Found {} installed applications", all_apps.len());
    println!("  Excluded {} system apps/drivers/runtimes", excluded);
    println!("  Processing {} user applications", apps.len());
    println!("\nChecking availability in winget (this may take a while)...\n");

    let mut config_apps: Vec<AppConfig> = vec![];
    let mut available_count = 0usize;
    let mut unavailable_count = 0usize;

    for (i, app) in apps.iter().enumerate() {
        let display = truncate(&app.name, 40);
        eprint!("\r  Checking {}/{}: {:<40}", i + 1, apps.len(), display);

        let from_winget = app.source.to_lowercase() == "winget";
        let available = if from_winget {
            true
        } else {
            winget.is_available_in_winget(&app.id)
        };

        if available {
            available_count += 1;
        } else {
            unavailable_count += 1;
        }

        let mut cfg = AppConfig {
            id: app.id.clone(),
            name: app.name.clone(),
            version: "latest".into(),
            available_in_winget: available,
            note: None,
        };
        if !available {
            cfg.note = Some("Not available via winget - install manually".into());
        }
        config_apps.push(cfg);
    }

    eprintln!();

    // Sort: available first, then by name
    config_apps.sort_by(|a, b| {
        b.available_in_winget
            .cmp(&a.available_in_winget)
            .then_with(|| a.name.cmp(&b.name))
    });

    let config = Config { apps: config_apps };

    let abs_path = Path::new(output_path);
    if let Some(parent) = abs_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let json = serde_json::to_string_pretty(&config)?;
    fs::write(abs_path, json)?;

    println!("\nConfig exported to: {}", abs_path.display());
    println!("\nSummary:");
    println!("  Available in winget: {}", available_count);
    println!("  Not available:       {}", unavailable_count);
    println!("  Excluded (system):   {}", excluded);
    println!("  Total exported:      {}", apps.len());

    if unavailable_count > 0 {
        println!("\nNote: Apps marked unavailable will be skipped during installation.");
    }

    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else if max == 0 {
        String::new()
    } else {
        let mut out = s.chars().take(max.saturating_sub(1)).collect::<String>();
        out.push('…');
        out
    }
}
