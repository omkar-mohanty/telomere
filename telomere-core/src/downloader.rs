use anyhow::{Error, Result};

use grammers_client::media::{Downloadable, Media};
use grammers_client::sender::RpcError;
use grammers_client::{Client, InvocationError};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::{
    Semaphore,
    mpsc::{UnboundedReceiver, UnboundedSender},
};
use tokio::task::{JoinHandle, JoinSet};

pub struct DownloadTask {
    pub media: Media,
    pub retries: usize,
    pub id: i32,
    pub filename: String,
    pub filepath: PathBuf,
}

pub struct Downloader {
    client: Client,
    semaphore: Arc<Semaphore>,
    tasks: HashMap<i32, JoinHandle<Result<()>>>,
}

impl Downloader {
    pub fn new(client: Client, limit: usize) -> Self {
        Self {
            client,
            semaphore: Arc::new(Semaphore::new(limit)),
            tasks: HashMap::new(),
        }
    }

    pub fn enqueue_task(&mut self, task: DownloadTask) -> UnboundedReceiver<DownloadEvent> {
        let (tx, rx) = unbounded_channel();
        let client = self.client.clone();
        let permit = self.semaphore.clone();
        let id = task.id;
        let handle = tokio::spawn(async move {
            let _permit = permit.acquire().await?;
            task.download_media(client, tx).await?;
            Ok::<(), Error>(())
        });
        self.tasks.insert(id, handle);
        rx
    }

    pub fn tasks(&mut self) -> &mut HashMap<i32, JoinHandle<Result<()>>> {
        &mut self.tasks
    }
}

impl DownloadTask {
    async fn download_media(
        self,
        client: Client,
        tx: UnboundedSender<DownloadEvent>,
    ) -> Result<i32> {
        use InvocationError::*;
        let mut total_retries = 0;

        let mut stream = client.iter_download(&self.media);

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.filepath)
            .await?;

        log::info!("Initializing Download for : {}", self.filename);

        loop {
            match stream.next().await {
                Ok(Some(chunk)) => {
                    file.write_all(&chunk).await?;
                    file.flush().await?;
                    tx.send(DownloadEvent::Progress(chunk.len()))?;
                }
                Ok(None) => {
                    tx.send(DownloadEvent::Finished)?;
                    return Ok(self.id);
                }
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
                            if total_retries < self.retries {
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
                            return Ok(self.id);
                        }
                    },
                    _ => {
                        tx.send(e.into())?;
                        return Ok(self.id);
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
    Progress(usize),
    Error(Error),
}
