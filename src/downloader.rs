use anyhow::Result;

use grammers_client::Client;
use grammers_client::media::Media;
use grammers_client::message::Message;
use grammers_session::types::PeerRef;
use grammers_tl_types::enums::ForumTopic;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::{RwLock, Semaphore};
use tokio::task::JoinSet;

pub struct DownlaoderBuilder {
    peer: PeerRef,
    limit: Option<usize>,
    client: Client,
    dst_root: Option<PathBuf>,
    forum_topics: Option<HashMap<i32, ForumTopic>>,
}

impl DownlaoderBuilder {
    pub fn new(client: Client, peer: PeerRef) -> Self {
        DownlaoderBuilder {
            limit: None,
            dst_root: None,
            client,
            peer,
            forum_topics: None,
        }
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
        Ok(Downloader {
            client: self.client,
            peer: self.peer,
            dst_root: self.dst_root.unwrap_or_default(),
            semaphore: Arc::new(Semaphore::new(self.limit.unwrap_or(1))),
            tasks: JoinSet::new(),
            forum_topics: self.forum_topics.unwrap_or_default(),
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

        let seen_files = Arc::new(RwLock::new(HashSet::new()));
        let multi_bar = Arc::new(MultiProgress::new());
        let style = ProgressStyle::with_template(
            "[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta}) {msg}",
        )?
        .progress_chars("#>-");
        let seen_files = seen_files.clone();

        let mb = multi_bar.clone();

        self.tasks.spawn(async move {
            let _permit = permit;
            Self::download_media(client, message, mb, media, dst_root, style, seen_files).await?;
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
