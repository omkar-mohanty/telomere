mod app;
mod ui;
use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
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

use crate::app::Application;

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
#[command(name = "tg-dl")]
#[command(about = "Telegram CLI & TUI Media Downloader", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Clone)]
enum Command {
    /// List all available Telegram peers/chats
    List {
        ///Filter by type e.g Groups, Channels, User
        #[arg(short, long, value_enum)]
        filter: PeerType,
    },

    Forum {
        ///Name of the peer
        name: String,
    },

    /// Download all media from a specific peer
    Download {
        /// The name or ID of the chat/peer
        #[arg(short, long)]
        name: String,

        ///Destination Path
        #[arg(short, long)]
        path: PathBuf,

        ///Number of downloads to do simultaneously. Recommened never to go above 2
        #[arg(short, long)]
        limit: usize,

        ///Forum topic i.e Group or Channel to download from if applicable
        #[arg(short, long)]
        forum: Option<Vec<String>>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize the native systemd journal logger
    JournalLog::new()?
        .with_extra_fields(vec![("VERSION", env!("CARGO_PKG_VERSION"))])
        .with_syslog_identifier("telomere".to_string())
        .install()?;

    log::set_max_level(LevelFilter::Info);

    log::info!("Telomere media downloader initializing natively inside systemd!");

    let app = Application::new().await?;

    let cli = Cli::parse();

    // setup terminal
    enable_raw_mode()?;
    let mut stderr = io::stderr(); // This is a special case. Normally using stdout is fine
    execute!(stderr, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stderr);
    let mut terminal = Terminal::new(backend)?;

    let res = if cli.command.is_none() {
        app.run(&mut terminal).await
    } else {
        todo!()
    };

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
