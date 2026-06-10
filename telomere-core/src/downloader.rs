use anyhow::{Error, Result};

use grammers_client::media::Media;
use grammers_client::sender::RpcError;
use grammers_client::{Client, InvocationError};
use grammers_session::types::PeerRef;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::{
    Semaphore,
    mpsc::{UnboundedReceiver, UnboundedSender},
};
use tokio::task::JoinSet;

pub struct DownlaoderBuilder {
    limit: Option<usize>,
    client: Client,
    dst_root: Option<PathBuf>,
}

impl DownlaoderBuilder {
    pub fn new(client: Client, peer: PeerRef) -> Self {
        DownlaoderBuilder {
            limit: None,
            dst_root: None,
            client,
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
    pub fn build(self) -> Result<Downloader> {
        Ok(Downloader {
            client: self.client,
            semaphore: Arc::new(Semaphore::new(self.limit.unwrap_or(1))),
            tasks: JoinSet::new(),
        })
    }
}

pub struct DownloadTask {
    medias: Vec<Media>,
    download_folder: PathBuf,
    retries: Option<usize>,
}

pub struct Downloader {
    client: Client,
    semaphore: Arc<Semaphore>,
    tasks: JoinSet<Result<()>>,
}

impl DownloadTask {
    async fn download_media(
        self,
        client: Client,
        media: Media,
        tx: UnboundedSender<DownloadEvent>,
        folder_path: PathBuf,
    ) -> Result<()> {
        use InvocationError::*;
        todo!("Implement getting filename from media");
        let path = folder_path.join("/filename.file");
        match media {
            Media::Photo(p) => {}
            Media::Document(doc) => {}
            _ => {
                return Err(anyhow::Error::msg("Unsupported Media Type"));
            }
        };
        let mut stream = client.iter_download(&media);
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .await?;

        loop {
            match stream.next().await {
                Ok(Some(chunk)) => {
                    file.write_all(&chunk).await?;
                    file.flush().await?;
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
                        }
                        _ => {
                            let _ = tx.send(e.into());
                            return Ok(());
                        }
                    },
                    _ => {
                        let _ = tx.send(e.into());
                        return Ok(());
                    }
                },
            }
        }
    }
}

impl Downloader {}

impl<E: std::error::Error + Send + Sync + 'static> From<E> for DownloadEvent {
    fn from(value: E) -> Self {
        DownloadEvent::Error(Error::new(value))
    }
}

pub enum DownloadEvent {
    Finished,
    Progress {
        filename: String,
        total_size: u64,
        total_downloaded: u64,
    },
    Error(Error),
}
