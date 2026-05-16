use anyhow::Result;
use grammers_client::media::Media;
use grammers_client::message::Message;
use grammers_client::{Client, InvocationError};
use grammers_mtsender::RpcError;
use grammers_session::types::PeerRef;
use grammers_tl_types::enums::ForumTopic;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::{RwLock, Semaphore, mpsc};
use tokio::task::JoinSet;

#[derive(Debug)]
pub enum DownloadEvent {
    Started { file: String, total_bytes: u64 },
    Progress { file: String, chunk_bytes: u64 },
    Skipped { file: String },
    Finished { file: String },
    Error { file: String, err: String },
}

pub struct DownlaoderBuilder {
    peer: PeerRef,
    limit: Option<usize>,
    client: Client,
    dst_root: Option<PathBuf>,
    forum_topics: Option<HashMap<i32, ForumTopic>>,
    event_sender: Option<mpsc::UnboundedSender<DownloadEvent>>,
}

impl DownlaoderBuilder {
    pub fn new(client: Client, peer: PeerRef) -> Self {
        DownlaoderBuilder {
            limit: None,
            dst_root: None,
            client,
            peer,
            forum_topics: None,
            event_sender: None,
        }
    }

    pub fn set_event_sender(mut self, tx: mpsc::UnboundedSender<DownloadEvent>) -> Self {
        self.event_sender = Some(tx);
        self
    }

    pub fn set_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn set_dst(mut self, path: PathBuf) -> Self {
        self.dst_root = Some(path);
        self
    }

    pub fn set_forum_topics(mut self, forum_topics: HashMap<i32, ForumTopic>) -> Self {
        self.forum_topics = Some(forum_topics);
        self
    }

    pub fn build(self) -> Result<Downloader> {
        if self.event_sender.is_none() {
            anyhow::bail!("Event Sender cannot be None")
        }

        Ok(Downloader {
            client: self.client,
            peer: self.peer,
            dst_root: self.dst_root.unwrap_or_default(),
            semaphore: Arc::new(Semaphore::new(self.limit.unwrap_or(1))),
            tasks: JoinSet::new(),
            forum_topics: self.forum_topics.unwrap_or_default(),
            event_sender: self.event_sender.unwrap(),
        })
    }
}

pub struct Downloader {
    client: Client,
    peer: PeerRef,
    dst_root: PathBuf,
    semaphore: Arc<Semaphore>,
    tasks: JoinSet<Result<()>>,
    forum_topics: HashMap<i32, ForumTopic>,
    event_sender: mpsc::UnboundedSender<DownloadEvent>,
}

impl Downloader {
    pub async fn run(mut self) -> Result<()> {
        let mut messages_iter = self.client.iter_messages(self.peer);

        while let Some(message) = messages_iter.next().await? {
            let msg_hdr = message.reply_header();

            if let Some(grammers_tl_types::enums::MessageReplyHeader::Header(hdr)) = msg_hdr {
                let thread_root_id = hdr
                    .reply_to_top_id
                    .or(hdr.reply_to_msg_id)
                    .expect("forum topic but no message id");

                if let Some(media) = message.media() {
                    if !self.forum_topics.is_empty() {
                        if self.forum_topics.contains_key(&thread_root_id) {
                            self.initiate_download(message, media).await?;
                        }
                    } else {
                        self.initiate_download(message, media).await?;
                    }
                }
            }
        }

        Ok(())
    }

    async fn initiate_download(&mut self, message: Message, media: Media) -> Result<()> {
        let dst_root = self.dst_root.clone();
        let client = self.client.clone();
        let permit = self.semaphore.clone().acquire_owned().await?;
        let event_sender = self.event_sender.clone();

        let seen_files = Arc::new(RwLock::new(HashSet::new()));
        let seen_files = seen_files.clone();

        self.tasks.spawn(async move {
            let _permit = permit;
            Self::download_media(client, message, media, dst_root, seen_files, event_sender)
                .await?;
            Ok::<(), anyhow::Error>(())
        });

        if let Some(res) = self.tasks.try_join_next() {
            res??;
        }

        while let Some(res) = self.tasks.join_next().await {
            res??;
        }
        Ok(())
    }

    async fn download_media(
        client: Client,
        message: Message,
        media: Media,
        dst_root: PathBuf,
        seen_files: Arc<RwLock<HashSet<String>>>,
        event_sender: mpsc::UnboundedSender<DownloadEvent>,
    ) -> Result<()> {
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

        let path_string = path.to_string_lossy().to_string();
        // --- 1. The Duplicate/Integrity Check ---
        if path.exists() {
            let meta = tokio::fs::metadata(&path).await?;
            if meta.len() == total_size {
                // Use println via MultiProgress to avoid flickering
                event_sender
                    .send(DownloadEvent::Skipped { file: path_string })
                    .ok();
                return Ok(());
            }
        }

        // --- 3. Streamed Download ---
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .await?;

        let mut stream = client.iter_download(&media);

        loop {
            match stream.next().await {
                Ok(Some(chunk)) => {
                    file.write_all(&chunk).await?;
                    file.flush().await?;
                    event_sender
                        .send(DownloadEvent::Progress {
                            file: path_string.clone(),
                            chunk_bytes: chunk.len() as u64,
                        })
                        .ok();
                }

                //End Of Stream
                Ok(None) => {
                    break;
                }

                // Too many requests sleeping for x seconds before sending next request
                Err(InvocationError::Rpc(RpcError {
                    code: 420, value, ..
                })) => {
                    let secs = value.unwrap_or(20);
                    event_sender
                        .send(DownloadEvent::Error {
                            file: path_string.clone(),
                            err: String::from_str("Flood Wait Error").unwrap(),
                        })
                        .ok();
                    log::info!("Flood Wait Sleeping for {}s", secs);
                    tokio::time::sleep(Duration::from_secs(secs as u64)).await;
                }

                // Bad Request: FILE_REF_EXPIRED
                Err(InvocationError::Rpc(RpcError { code: 400, .. })) => {
                    event_sender
                        .send(DownloadEvent::Error {
                            file: path_string.clone(),
                            err: String::from_str("File Reference Expired").unwrap(),
                        })
                        .ok();
                    stream = client.iter_download(&media);
                }
                Err(err) => {
                    return Err(err.into());
                }
            }
        }

        Ok(())
    }
}
