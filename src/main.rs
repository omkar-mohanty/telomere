use anyhow::Result;

use clap::{Parser, Subcommand, ValueEnum};
use env_logger::{Builder, Target};
use grammers_client::media::Media;
use grammers_client::message::Message;
use grammers_client::peer::{Channel, Dialog, Group, User};
use grammers_client::{Client, SignInError};
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use grammers_session::types::PeerRef;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use log::LevelFilter;
use std::collections::HashSet;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::{env, io};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::{RwLock, Semaphore};
use tokio::task::JoinSet;

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

        #[arg(short, long)]
        limit: usize,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let target = Box::new(std::fs::File::create("app.log").expect("Could not create file"));

    Builder::new()
        .filter_level(LevelFilter::Info)
        .target(Target::Pipe(target))
        .init();

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
        Commands::Download { name, path, limit } => {
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
                Downloader::new(
                    client,
                    search_dialog.unwrap().peer_ref(),
                    path.to_path_buf(),
                    limit.to_owned(),
                )
                .run()
                .await?;
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

struct Downloader {
    client: Client,
    peer: PeerRef,
    dst_root: PathBuf,
    semaphore: Arc<Semaphore>,
    tasks: JoinSet<Result<()>>,
}

impl Downloader {
    pub fn new(client: Client, peer: PeerRef, dst_root: PathBuf, limit: usize) -> Self {
        let semaphore = Arc::new(Semaphore::new(limit));
        Self {
            client,
            peer,
            dst_root,
            semaphore,
            tasks: JoinSet::new(),
        }
    }

    pub async fn run(mut self) -> Result<()> {
        let mut messages_iter = self.client.iter_messages(self.peer);

        let style = ProgressStyle::with_template(
            "[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta}) {msg}",
        )?
        .progress_chars("#>-");

        let seen_files = Arc::new(RwLock::new(HashSet::new()));

        let multi_bar = Arc::new(MultiProgress::new());
        while let Some(message) = messages_iter.next().await? {
            if let Some(media) = message.media() {
                let dst_root = self.dst_root.clone();
                let client = self.client.clone();
                let permit = self.semaphore.clone().acquire_owned().await?;
                let style = style.clone();
                let seen_files = seen_files.clone();

                let mb = multi_bar.clone();

                tokio::time::sleep(Duration::from_millis(500)).await;

                self.tasks.spawn(async move {
                    let _permit = permit;
                    Self::download_media(client, message, mb, media, dst_root, style, seen_files)
                        .await?;
                    Ok::<(), anyhow::Error>(())
                });
            }

            if let Some(res) = self.tasks.try_join_next() {
                res??;
            }
        }

        while let Some(res) = self.tasks.join_next().await {
            res??;
        }
        Ok(())
    }

    async fn download_media(
        client: Client,
        message: Message,
        multi_bar: Arc<MultiProgress>,
        media: Media,
        dst_root: PathBuf,
        style: ProgressStyle,
        seen_files: Arc<RwLock<HashSet<String>>>,
    ) -> Result<()> {
        let mb = Arc::clone(&multi_bar);

        let (file_name, total_size) = match &media {
            Media::Document(d) => (
                d.name().unwrap_or("file").to_string(),
                d.size().unwrap_or(100) as u64,
            ),
            Media::Photo(p) => (
                format!("photo_{}.jpg", message.id()),
                p.size().unwrap_or(100) as u64,
            ),
            _ => return Ok(()),
        };

        {
            let mut set_read = seen_files.write().await;

            if !set_read.insert(file_name.clone()) {
                return Ok(());
            }
        }

        let folder_name = message.reply_to_message_id().unwrap_or(10000).to_string();
        let folder = dst_root.join(folder_name);

        tokio::fs::create_dir_all(&folder).await?;

        let path = folder.join(&file_name);

        // --- 1. The Duplicate/Integrity Check ---
        if path.exists() {
            let meta = tokio::fs::metadata(&path).await?;
            if meta.len() == total_size {
                // Use println via MultiProgress to avoid flickering
                mb.println(format!("✔ Skipping {}: Already exists", file_name))?;
                return Ok(());
            }
        }

        // --- 2. Setup Progress Bar ---
        let pb = mb.add(ProgressBar::new(total_size));
        pb.set_style(style);
        pb.set_message(file_name.clone());

        // --- 3. Streamed Download ---
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .await?;

        let mut stream = client.iter_download(&media);

        while let Some(chunk) = stream.next().await? {
            file.write_all(&chunk).await?;
            file.flush().await?;
            pb.inc(chunk.len() as u64);
        }

        pb.finish_with_message(format!("✔ {}", file_name));
        Ok(())
    }
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
