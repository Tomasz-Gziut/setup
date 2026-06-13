use anyhow::Result;

use crate::services::winget::WingetService;

pub fn run() -> Result<()> {
    let winget = WingetService::new();

    eprintln!("Fetching installed applications...");

    let apps = winget.get_installed_apps();

    if apps.is_empty() {
        println!("No applications found.");
        return Ok(());
    }

    // Column widths
    let w_name = 40usize;
    let w_id = 45usize;
    let w_ver = 18usize;
    let w_src = 10usize;

    let sep = format!(
        "+-{}-+-{}-+-{}-+-{}-+",
        "-".repeat(w_name),
        "-".repeat(w_id),
        "-".repeat(w_ver),
        "-".repeat(w_src)
    );

    println!("{sep}");
    println!(
        "| {:<w_name$} | {:<w_id$} | {:<w_ver$} | {:<w_src$} |",
        "Name", "ID", "Version", "Source"
    );
    println!("{sep}");

    for app in &apps {
        let name = truncate(&app.name, w_name);
        let id = truncate(&app.id, w_id);
        let ver = truncate(&app.version, w_ver);
        let src = if app.source.is_empty() { "N/A" } else { &app.source };
        let src = truncate(src, w_src);
        println!("| {name:<w_name$} | {id:<w_id$} | {ver:<w_ver$} | {src:<w_src$} |");
    }

    println!("{sep}");
    println!("\nTotal: {} applications", apps.len());

    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}
