mod app;
mod ui;
use anyhow::Result;
use clap::{Parser, ValueEnum};
use log::LevelFilter;
use ratatui::Terminal;
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::prelude::CrosstermBackend;
use std::path::PathBuf;
use std::{env, io};
use systemd_journal_logger::JournalLog;

use crate::app::{Application, Config};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum PeerType {
    Group,
    Channel,
    Chat,
}

#[derive(Debug, Clone, Copy)]
pub enum AppMode {
    Interactive,
    Cli,
}

#[derive(Parser)]
#[command(about = "Telegram CLI & TUI Media Downloader", long_about = None)]
struct Cli {
    #[arg(short, long)]
    output: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize the native systemd journal logger
    JournalLog::new()?
        .with_extra_fields(vec![("VERSION", env!("CARGO_PKG_VERSION"))])
        .with_syslog_identifier("telomere".to_string())
        .install()?;

    log::set_max_level(LevelFilter::Info);

    log::info!("Telomere media downloader initializing");

    let cli = Cli::parse();
    let mut config = Config::default();

    config.output = cli.output;

    let app = Application::new(config).await?;
    // setup terminal
    enable_raw_mode()?;
    let mut stderr = io::stderr(); // This is a special case. Normally using stdout is fine
    execute!(stderr, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stderr);
    let mut terminal = Terminal::new(backend)?;

    let res = app.run(&mut terminal).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    res?;

    Ok(())
}
