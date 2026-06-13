use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;
mod constants;
mod services;
mod types;

#[derive(Parser)]
#[command(name = "setup", version = "1.0.0", about = "Manage Windows applications via winget")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(short = 'c', long, value_name = "PATH")]
    config: Option<String>,

    #[arg(short = 'e', long, value_name = "PATH")]
    export: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Display all installed applications
    Show,
    /// Install applications from a config file or choose a preset
    Install { path: Option<String> },
    /// Export installed applications to a config file
    Export { path: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            if let Some(config) = cli.config {
                commands::install::run(Some(config))
            } else if let Some(path) = cli.export {
                commands::export::run(&path)
            } else {
                commands::create::run()
            }
        }
        Some(Commands::Show) => commands::show::run(),
        Some(Commands::Install { path }) => commands::install::run(path),
        Some(Commands::Export { path }) => commands::export::run(&path),
    }
}
