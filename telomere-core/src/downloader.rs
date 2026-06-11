use anyhow::{Error, Result};

use grammers_client::media::{Downloadable, Media};
use grammers_client::sender::RpcError;
use grammers_client::{Client, InvocationError};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::{
    Semaphore,
    mpsc::{UnboundedReceiver, UnboundedSender},
};
use tokio::task::JoinSet;

pub struct DownloadTask {
    pub media: Media,
    pub retries: Option<usize>,
    pub filename: String,
    pub filepath: PathBuf,
}

pub struct Downloader {
    client: Client,
    semaphore: Arc<Semaphore>,
    tasks: JoinSet<Result<()>>,
}

impl Downloader {
    pub fn new(client: Client, limit: usize) -> Self {
        Self {
            client,
            semaphore: Arc::new(Semaphore::new(limit)),
            tasks: JoinSet::new(),
        }
    }

    pub async fn enqueue_task(&mut self, task: DownloadTask) -> UnboundedReceiver<DownloadEvent> {
        let (tx, rx) = unbounded_channel();
        let client = self.client.clone();
        let permit = self.semaphore.clone();
        self.tasks.spawn(async move {
            let _permit = permit.acquire().await?;
            task.download_media(client, tx).await?;
            Ok::<(), Error>(())
        });
        rx
    }

    pub async fn finish_all_tasks(&mut self) -> Vec<Result<()>> {
        let tasks = std::mem::take(&mut self.tasks);
        tasks.join_all().await
    }
}

impl DownloadTask {
    async fn download_media(
        self,
        client: Client,
        tx: UnboundedSender<DownloadEvent>,
    ) -> Result<()> {
        use InvocationError::*;
        let total_size = self.media.size().unwrap_or(100);
        let mut total_retries = 0;

        let mut stream = client.iter_download(&self.media);

        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .truncate(true)
            .open(&self.filepath)
            .await?;
        let mut total_downloaded = 0;

        loop {
            match stream.next().await {
                Ok(Some(chunk)) => {
                    file.write_all(&chunk).await?;
                    file.flush().await?;
                    total_downloaded += chunk.len();
                    tx.send(DownloadEvent::Progress {
                        total_size,
                        total_downloaded,
                    })?;
                }
                Ok(None) => return Ok(()),
                Err(e) => match &e {
                    Rpc(rpc_error) => match rpc_error {
                        RpcError {
                            code: 420, value, ..
                        } => {
                            log::error!("RPC Error Floor Wait : {:?}", value);
                        }
                        RpcError {
                            code: 400, value, ..
                        } => {
                            log::error!("RPC Error File Ref Expired : {:?}", value);
                            if let Some(retries) = self.retries
                                && total_retries < retries
                            {
                                total_retries += 1;
                                log::info!(
                                    "Retrying Download for : {:?} Attempt : {}",
                                    self.filepath,
                                    total_retries
                                );
                                stream = client.iter_download(&self.media);
                            }
                        }
                        _ => {
                            tx.send(e.into())?;
                            return Ok(());
                        }
                    },
                    _ => {
                        tx.send(e.into())?;
                        return Ok(());
                    }
                },
            }
        }
    }
}

impl<E: std::error::Error + Send + Sync + 'static> From<E> for DownloadEvent {
    fn from(value: E) -> Self {
        DownloadEvent::Error(Error::new(value))
    }
}

pub enum DownloadEvent {
    Finished,
    Progress {
        total_size: usize,
        total_downloaded: usize,
    },
    Error(Error),
}
