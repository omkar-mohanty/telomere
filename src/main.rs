use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use grammers_client::peer::{Channel, Dialog, Group, User};
use tokio::fs;

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::{env, io};

use grammers_client::media::{Downloadable, Media};
use grammers_client::{Client, SignInError};
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use mime::Mime;
use mime_guess::mime;
use simple_logger::SimpleLogger;
use tokio::sync::Semaphore;

pub struct App {
    client: Client,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum PeerType {
    Group,
    Channel,
    Chat,
}

const SESSION_FILE: &str = "telomere.session";

#[derive(Parser)]
#[command(name = "tg-dl")]
#[command(about = "Telegram CLI Media Downloader", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all available Telegram peers/chats
    List {
        #[arg(short, long, value_enum)]
        filter: PeerType,
    },

    /// Download all media from a specific peer
    Download {
        /// The name or ID of the chat/peer
        #[arg(short, long)]
        name: String,

        ///Destination Path
        #[arg(short, long)]
        path: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .init()
        .unwrap();

    let api_id = env!("TG_ID").parse().expect("TG_ID invalid");

    let session = Arc::new(SqliteSession::open(SESSION_FILE).await?);

    let SenderPool { runner, handle, .. } = SenderPool::new(Arc::clone(&session), api_id);
    let client = Client::new(handle);
    let _ = tokio::spawn(runner.run());

    if !client.is_authorized().await? {
        println!("Signing in...");
        let phone = prompt("Enter your phone number (international format): ")?;
        let token = client.request_login_code(&phone, env!("TG_HASH")).await?;
        let code = prompt("Enter the code you received: ")?;
        let signed_in = client.sign_in(&token, &code).await;
        match signed_in {
            Err(SignInError::PasswordRequired(password_token)) => {
                // Note: this `prompt` method will echo the password in the console.
                //       Real code might want to use a better way to handle this.
                let hint = password_token.hint().unwrap();
                let prompt_message = format!("Enter the password (hint {}): ", &hint);
                let password = prompt(prompt_message.as_str())?;

                client
                    .check_password(password_token, password.trim())
                    .await?;
            }
            Ok(_) => (),
            Err(e) => panic!("{}", e),
        };
        println!("Signed in!");
    }

    let cli = Cli::parse();

    match &cli.command {
        Commands::List { filter } => {
            let mut dialog_iter = client.iter_dialogs();

            while let Some(dialog) = dialog_iter.next().await? {
                display_dialog(dialog, *filter);
            }
        }
        Commands::Download { name, path } => {
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
                download_media(&client, search_dialog.unwrap(), path.to_path_buf()).await?;
            } else {
                println!("Could not find peer!");
            }
        }
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

async fn download_media(client: &Client, dialog: Dialog, dst_path: PathBuf) -> Result<()> {
    let semaphore = Arc::new(Semaphore::new(5));
    let mut messages = client.iter_messages(dialog.peer_ref());

    println!(
        "Peer {:?} has {} total messages.",
        dialog.peer().name(),
        messages.total().await.unwrap()
    );

    while let Some(message) = messages.next().await? {
        let client = client.clone();
        let semaphore = Arc::clone(&semaphore);
        let dst_path = dst_path.clone();
        tokio::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();

            if let Some(media) = message.media() {
                let folder_path = PathBuf::from_str(
                    message
                        .reply_to_message_id()
                        .unwrap_or(50000)
                        .to_string()
                        .as_str(),
                )
                .unwrap();

                let folder_path = dst_path.join(folder_path);

                fs::create_dir_all(&folder_path).await.unwrap();

                match &media {
                    Media::Document(doc) => {
                        // This is the file name before downloading
                        let name = doc.name().unwrap_or("unnamed_file");
                        let size = doc.size(); // You can also see the size in bytes!
                        let path = folder_path.join(name);
                        println!(
                            "Found Document: {} ({:?} bytes) | Message ID reply {:?}",
                            name,
                            size,
                            message.reply_to_message_id()
                        );
                        println!("Downloading to : {:?}", path);
                        client.download_media(&media, path).await.unwrap();
                    }
                    Media::Photo(_) => {
                        println!("Found Photo: photo_{}.jpg", message.id());
                    }
                    _ => println!("Found other media type"),
                }
                // Determine the folder name
                // If it's a topic message, use the topic name; otherwise "General"
            }
        });
    }
    Ok(())
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
