use anyhow::Result;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
        let name = fit_cell(&app.name, w_name);
        let id = fit_cell(&app.id, w_id);
        let ver = fit_cell(&app.version, w_ver);
        let src = if app.source.is_empty() { "N/A" } else { &app.source };
        let src = fit_cell(src, w_src);
        println!("| {name} | {id} | {ver} | {src} |");
    }

    println!("{sep}");
    println!("\nTotal: {} applications", apps.len());

    Ok(())
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
