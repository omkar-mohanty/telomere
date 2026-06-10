use anyhow::Result;
use directories::ProjectDirs;
use grammers_client::Client;
use grammers_client::client::LoginToken;
use grammers_client::media::Media;
use grammers_client::message::Message;
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use grammers_session::types::PeerRef;
use grammers_tl_types::enums::FileHash;
use grammers_tl_types::types::ForumTopic;
use ratatui::crossterm::event::{self, KeyCode};
use ratatui::prelude::Backend;
use ratatui::{Terminal, crossterm::event::Event};
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::ui::{
    Controller, CurrentScreen, DownloadScreen, FileSelectionScreen, GroupDownloadScreen,
    PeerSelectionScreen, Screen, Tick,
};

pub struct StateMachine<S>(pub S);

impl TryFrom<StateMachine<PeerSelection>> for StateMachine<ForumTopicSelection> {
    type Error = anyhow::Error;
    fn try_from(value: StateMachine<PeerSelection>) -> Result<Self> {
        let peer_ref = value.0.peer_ref;
        match peer_ref {
            Some(peer_ref) => {
                let peer_selection = ForumTopicSelection {
                    peer_ref,
                    forum_topics: Vec::new(),
                    messages: None,
                };
                Ok(StateMachine(peer_selection))
            }
            None => Err(anyhow::Error::msg("Peer Cannot be None")),
        }
    }
}

impl TryFrom<StateMachine<ForumTopicSelection>> for StateMachine<FileSelection> {
    type Error = anyhow::Error;
    fn try_from(value: StateMachine<ForumTopicSelection>) -> Result<Self> {
        let StateMachine(inner) = value;
        let ForumTopicSelection {
            forum_topics,
            messages,
            ..
        } = inner;

        let inner = match messages {
            Some(messages) => FileSelection {
                forum_topics,
                messages,
            },
            None => return Err(anyhow::Error::msg("Messages cannot be empty")),
        };

        Ok(StateMachine(inner))
    }
}

impl TryFrom<StateMachine<FileSelection>> for StateMachine<DownloadState> {
    type Error = anyhow::Error;
    fn try_from(value: StateMachine<FileSelection>) -> Result<Self> {
        let StateMachine(inner) = value;
        let FileSelection { messages, .. } = inner;

        let medias = messages
            .iter()
            .filter(|message| message.media().is_some())
            .map(|message| message.media().unwrap())
            .collect();

        let download = DownloadState { medias };

        Ok(StateMachine(download))
    }
}

impl TryFrom<StateMachine<DownloadState>> for StateMachine<DownloadProgress> {
    type Error = anyhow::Error;
    fn try_from(value: StateMachine<DownloadState>) -> Result<Self> {
        Ok(StateMachine(DownloadProgress::InProgress))
    }
}

pub enum StateWrapper {
    PeerSelection(StateMachine<PeerSelection>),
    ForumTopicSelection(StateMachine<ForumTopicSelection>),
    FileSelection(StateMachine<FileSelection>),
    Download(StateMachine<DownloadState>),
    Progress(StateMachine<DownloadProgress>),
}

impl StateWrapper {
    pub fn step(self) -> Result<Option<StateWrapper>> {
        use StateWrapper::*;
        let next = match self {
            PeerSelection(state) => ForumTopicSelection(state.try_into()?),
            ForumTopicSelection(state) => FileSelection(state.try_into()?),
            FileSelection(state) => Download(state.try_into()?),
            Download(state) => Progress(state.try_into()?),
            Progress(state) => {
                let StateMachine(progress) = state;
                match progress {
                    DownloadProgress::InProgress => {
                        Progress(StateMachine(DownloadProgress::InProgress))
                    }
                    DownloadProgress::Finished => return Ok(None),
                }
            }
        };

        Ok(Some(next))
    }
}

impl Default for StateWrapper {
    fn default() -> Self {
        Self::PeerSelection(StateMachine(PeerSelection::default()))
    }
}

pub struct ForumTopicSelection {
    pub peer_ref: PeerRef,
    pub forum_topics: Vec<ForumTopic>,
    pub messages: Option<Vec<Message>>,
}

#[derive(Default)]
pub struct PeerSelection {
    peer_ref: Option<PeerRef>,
}

pub enum DownloadProgress {
    InProgress,
    Finished,
}

pub struct FileSelection {
    pub forum_topics: Vec<ForumTopic>,
    pub messages: Vec<Message>,
}

pub struct DownloadState {
    medias: Vec<Media>,
}

pub struct UnAuthenticated;

#[derive(Default)]
pub struct Authenticated {
    state: StateWrapper,
}

pub struct Application<S> {
    ctx: Arc<Context>,
    state: S,
}

impl Application<Authenticated> {
    pub async fn new() -> Result<Self> {
        let ctx = Arc::new(Context::new().await?);
        let state = Authenticated::default();
        Ok(Self { ctx, state })
    }

    pub async fn run<B: Backend>(mut self, terminal: &mut Terminal<B>) -> Result<bool>
    where
        B::Error: Sync + Send + 'static,
    {
        loop {}
    }
}

pub struct Context {
    pub client: Client,
    pub session: Arc<SqliteSession>,
}

const SESSION_FILE: &str = "telomere.session";

impl Context {
    pub async fn new() -> Result<Self> {
        let api_id_path = env::var("TG_ID_FILE")?
            .parse::<PathBuf>()
            .expect("TG_ID invalid");

        let api_id_raw = tokio::fs::read_to_string(api_id_path).await?;

        let api_id = api_id_raw.trim().parse()?;

        let project_dirs = ProjectDirs::from("org", "ultrainfinite", "telomere-cli")
            .expect("Project Directores coudld not eb fetched!");

        let data_dir = project_dirs.data_dir();

        if !data_dir.exists() {
            tokio::fs::create_dir_all(&data_dir).await?;
        }

        let session = Arc::new(SqliteSession::open(data_dir.join(SESSION_FILE)).await?);

        let SenderPool { runner, handle, .. } = SenderPool::new(Arc::clone(&session), api_id);
        let client = Client::new(handle);
        let _ = tokio::spawn(runner.run());

        Ok(Self { client, session })
    }

    pub async fn init_auth(&self, phone: &String) -> Result<Option<LoginToken>> {
        if self.client.is_authorized().await? {
            return Ok(None);
        }

        let tg_hash_path = env::var("TG_HASH_FILE")?
            .parse::<PathBuf>()
            .expect("TG_HASH_FILE invalid");

        let tg_hash_raw = tokio::fs::read_to_string(tg_hash_path).await?;

        let tg_hash = tg_hash_raw.trim().parse::<String>()?;

        let token = self.client.request_login_code(&phone, &tg_hash).await?;
        Ok(Some(token))
    }

    pub async fn finish_auth(&self, code: &String, token: LoginToken) -> Result<()> {
        let signed_in = self.client.sign_in(&token, &code).await;

        match signed_in {
            Ok(_) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
