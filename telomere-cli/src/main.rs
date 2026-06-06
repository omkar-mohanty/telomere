mod app;
mod ui;
use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use grammers_client::peer::{Channel, Dialog, Group, User};
use grammers_client::{Client, SignInError};
use grammers_session::types::PeerRef;
use grammers_tl_types::enums::ForumTopic;
use grammers_tl_types::enums::messages::ForumTopics;
use grammers_tl_types::functions::messages::GetForumTopics;
use log::LevelFilter;
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::prelude::{Backend, CrosstermBackend};
use ratatui::{Terminal, TerminalOptions};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::{env, io};
use systemd_journal_logger::JournalLog;
use telomere_core::downloader::DownlaoderBuilder;

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
    /// Launch the interactive Terminal User Interface (TUI)
    #[arg(short, long)]
    interactive: bool,
}

#[derive(Subcommand)]
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

async fn get_forum_topics(client: &Client, peer: &PeerRef) -> Result<HashMap<i32, ForumTopic>> {
    let mut filtered_topics = HashMap::new();

    let forum_topic_res = client
        .invoke(&GetForumTopics {
            peer: peer.into(),
            q: None,
            offset_date: 0,
            offset_id: 0,
            offset_topic: 0,
            limit: 0,
        })
        .await?;
    let topics = {
        let ForumTopics::Topics(topics) = forum_topic_res;
        topics.topics
    };

    for topic in topics {
        filtered_topics.insert(topic.id(), topic);
    }

    Ok(filtered_topics)
}

async fn run_app_cli(app: Application, cli: Command) -> Result<()> {
    let client = app.client;
    match cli {
        Command::List { filter } => {
            let mut dialog_iter = client.iter_dialogs();

            while let Some(dialog) = dialog_iter.next().await? {
                display_dialog(dialog, filter);
            }
            Ok(())
        }
        Command::Forum { name } => {
            let mut dialog_iter = client.iter_dialogs();

            let mut search_dialog = None;

            while let Some(dialog) = dialog_iter.next().await? {
                if dialog
                    .peer()
                    .name()
                    .is_some_and(|search_name| search_name == name)
                {
                    search_dialog = Some(dialog);
                }
            }

            let peer_ref = search_dialog.unwrap().peer_ref();
            let forum_topics = get_forum_topics(&client, &peer_ref).await?;

            for (id, topic) in forum_topics {
                if let ForumTopic::Topic(topic) = topic {
                    println!("Title : {}\tID : {}", topic.title, id);
                }
            }

            Ok(())
        }
        Command::Download {
            name,
            path,
            limit,
            forum,
        } => {
            println!("Initializing download for peer: '{}'", name);

            let mut dialog_iter = client.iter_dialogs();

            let mut search_dialog = None;

            while let Some(dialog) = dialog_iter.next().await? {
                if dialog
                    .peer()
                    .name()
                    .is_some_and(|search_name| search_name == name)
                {
                    search_dialog = Some(dialog);
                }
            }

            if search_dialog.is_some() {
                let peer_ref = search_dialog.unwrap().peer_ref();
                let forum_topics = get_forum_topics(&client, &peer_ref).await?;
                let mut filtered_forum_topics = HashMap::new();

                for (id, value) in forum_topics {
                    if let ForumTopic::Topic(topic) = &value {
                        if let Some(forum_topic) = &forum {
                            if forum_topic.contains(&topic.title) {
                                filtered_forum_topics.insert(id, value);
                            }
                        }
                    }
                }
                log::debug!("Forum Topics {:?}", filtered_forum_topics);
                DownlaoderBuilder::new(client, peer_ref)
                    .set_limit(limit)
                    .set_dst(path)
                    .set_forum_topics(filtered_forum_topics)
                    .build()?
                    .run()
                    .await?;
            } else {
                println!("Could not find peer!");
            }
            Ok(())
        }
    }
}

pub async fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: Application) -> io::Result<bool>
where
    io::Error: From<B::Error>,
{
    loop {
        terminal.draw(|f| {
            ui::draw(f, &app);
        })?;
    }
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

    let phone = prompt("Enter Phone Number in international format")?;

    if let Some(token) = app.init_auth(phone).await? {
        let code = prompt("Enter code")?;

        app.finish_auth(code, token).await?;
    }

    let cli = Cli::parse();

    if cli.interactive {
        // setup terminal
        enable_raw_mode()?;
        let mut stderr = io::stderr(); // This is a special case. Normally using stdout is fine
        execute!(stderr, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stderr);
        let mut terminal = Terminal::new(backend)?;
        run_app(&mut terminal, app).await?;
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;
    } else {
    }

    Ok(())
}

fn display_dialog(dialog: Dialog, filter: PeerType) {
    use grammers_client::peer::Peer::*;
    let peer = dialog.peer();
    match peer {
        User(user) => {
            if filter == PeerType::Chat {
                display_user(user);
            }
        }
        Channel(channel) => {
            if filter == PeerType::Channel {
                display_channel(channel);
            }
        }
        Group(group) => {
            if filter == PeerType::Group {
                display_group(group);
            }
        }
    }
}

fn display_user(user: &User) {
    println!("User : {}", user.full_name());
}
fn display_channel(channel: &Channel) {
    println!("Channel : {}", channel.title());
}
fn display_group(group: &Group) {
    println!(
        "Group : {:?} | Username : {:?}",
        group.title(),
        group.username()
    );
}

fn prompt(message: &str) -> Result<String> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    stdout.write_all(message.as_bytes())?;
    stdout.flush()?;

    let stdin = io::stdin();
    let mut stdin = stdin.lock();

    let mut line = String::new();
    stdin.read_line(&mut line)?;
    Ok(line)
}
