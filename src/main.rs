use grammers_client::message::Message;
use grammers_session::types::PeerRef;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::task::JoinSet;

use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use grammers_client::peer::{Channel, Dialog, Group, User};

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::{env, io};

use grammers_client::media::Media;
use grammers_client::{Client, SignInError};
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use simple_logger::SimpleLogger;
use tokio::sync::Semaphore;

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
        limit: u64,
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
                download_media(
                    &client,
                    search_dialog.unwrap().peer_ref(),
                    path.to_path_buf(),
                )
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
    tasks: Option<JoinSet<Result<()>>>,
}

impl Downloader {
    pub fn new(client: Client, peer: PeerRef, dst_root: PathBuf, limit: usize) -> Self {
        let semaphore = Arc::new(Semaphore::new(limit));
        Self {
            client,
            peer,
            dst_root,
            semaphore,
            tasks: None,
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        self.tasks = Some(JoinSet::new());
        let mut messages_iter = self.client.iter_messages(self.peer);

        let multi_bar = Arc::new(MultiProgress::new());
        while let Some(message) = messages_iter.next().await? {
            if self.semaphore.available_permits() <= 0 {
                self.tasks.take().unwrap().join_all().await;
                self.tasks = Some(JoinSet::new());
            }

            if let Some(media) = message.media() {
                let _permit = self.semaphore.acquire().await;
                let dst_root = self.dst_root.clone();
                let client = self.client.clone();

                self.tasks.as_mut().unwrap().spawn(Self::download_media(
                    client,
                    message,
                    Arc::clone(&multi_bar),
                    media,
                    dst_root,
                ));
            }
        }
        Ok(())
    }

    async fn download_media(
        client: Client,
        message: Message,
        multi_bar: Arc<MultiProgress>,
        media: Media,
        dst_root: PathBuf,
    ) -> Result<()> {
        let mb = Arc::clone(&multi_bar);

        let style = ProgressStyle::with_template(
            "[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta}) {msg}",
        )?
        .progress_chars("#>-");
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

async fn download_media(client: &Client, peer: PeerRef, dst_root: PathBuf) -> Result<()> {
    let semaphore = Arc::new(Semaphore::new(8));
    let multi_bar = Arc::new(MultiProgress::new());
    let mut messages = client.iter_messages(peer);
    let mut tasks = JoinSet::new();

    let style = ProgressStyle::with_template(
        "[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta}) {msg}",
    )?
    .progress_chars("#>-");

    while let Some(message) = messages.next().await? {
        if let Some(media) = message.media() {
            let client = client.clone();
            let semaphore = Arc::clone(&semaphore);
            let mb = Arc::clone(&multi_bar);
            let dst_root = dst_root.clone();
            let style = style.clone();

            tasks.spawn(async move {
                let _permit = semaphore.acquire().await.map_err(|e| anyhow::anyhow!(e))?;

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
                Ok::<(), anyhow::Error>(())
            });
        }
    }

    while let Some(res) = tasks.join_next().await {
        if let Err(e) = res? {
            multi_bar.println(format!("✘ Error: {}", e))?;
        }
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
