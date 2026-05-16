use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use env_logger::{Builder, Target};
use grammers_client::peer::{Channel, Dialog, Group, User};
use grammers_client::{Client, SignInError};
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use grammers_session::types::PeerRef;
use grammers_tl_types::enums::ForumTopic;
use grammers_tl_types::enums::messages::ForumTopics;
use grammers_tl_types::functions::messages::GetForumTopics;
use log::LevelFilter;
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::{env, io};
use telomere_core::downloader::DownlaoderBuilder;

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

#[tokio::main]
async fn main() -> Result<()> {
    let target = Box::new(std::fs::File::create("app.log").expect("Could not create file"));

    Builder::new()
        .filter_level(LevelFilter::Info)
        .target(Target::Pipe(target))
        .init();

    let api_id_path = env::var("TG_ID_FILE")?
        .parse::<PathBuf>()
        .expect("TG_ID invalid");
    let tg_hash_path = env::var("TG_ID_FILE")?
        .parse::<PathBuf>()
        .expect("TG_ID_FILE invalid");

    let api_id_raw = tokio::fs::read_to_string(api_id_path).await?;
    let tg_hash_raw = tokio::fs::read_to_string(tg_hash_path).await?;

    let api_id = api_id_raw.trim().parse()?;
    let tg_hash = tg_hash_raw.trim().parse::<String>()?;

    let session = Arc::new(SqliteSession::open(SESSION_FILE).await?);

    let SenderPool { runner, handle, .. } = SenderPool::new(Arc::clone(&session), api_id);
    let client = Client::new(handle);
    let _ = tokio::spawn(runner.run());

    if !client.is_authorized().await? {
        println!("Signing in...");
        let phone = prompt("Enter your phone number (international format): ")?;
        let token = client.request_login_code(&phone, &tg_hash).await?;
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

    match cli.command {
        Commands::List { filter } => {
            let mut dialog_iter = client.iter_dialogs();

            while let Some(dialog) = dialog_iter.next().await? {
                display_dialog(dialog, filter);
            }
        }
        Commands::Forum { name } => {
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
        }
        Commands::Download {
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
