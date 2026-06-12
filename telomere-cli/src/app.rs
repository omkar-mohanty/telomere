use anyhow::{Error, Result};
use directories::{ProjectDirs, UserDirs};
use grammers_client::Client;
use grammers_client::client::LoginToken;
use grammers_client::media::Media;
use grammers_client::message::Message;
use grammers_client::peer::Dialog;
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use grammers_tl_types::types::ForumTopic;
use ratatui::crossterm::event::{self, KeyCode};
use ratatui::prelude::Backend;
use ratatui::widgets::ListState;
use ratatui::{Terminal, crossterm::event::Event};
use std::collections::{HashMap, HashSet};
use std::env;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use telomere_core::downloader::{DownloadEvent, Downloader};
use tokio::sync::RwLock;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::ui::{Controller, TerminalController, TerminalTicker, TerminalUserInterface, Tick};

#[derive(Debug)]
pub struct StateMachine<S>(S);

impl<S> Deref for StateMachine<S> {
    type Target = S;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S> DerefMut for StateMachine<S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<S> AsMut<S> for StateMachine<S> {
    fn as_mut(&mut self) -> &mut S {
        &mut self.0
    }
}

impl<S> AsRef<S> for StateMachine<S> {
    fn as_ref(&self) -> &S {
        &self.0
    }
}

impl TryFrom<StateMachine<PeerSelection>> for StateMachine<ForumTopicSelection> {
    type Error = anyhow::Error;
    fn try_from(value: StateMachine<PeerSelection>) -> Result<Self> {
        let StateMachine(mut inner) = value;

        if inner.list_state.selected().is_none() {
            anyhow::bail!("No Peer is selected");
        }

        let selected = inner.list_state.selected().unwrap();
        let dialog = inner.dialogs.swap_remove(selected);

        let forum_topc = ForumTopicSelection {
            dialog,
            list_state: ListState::default(),
            selected_topics: HashSet::default(),
            forum_topics: Vec::new(),
        };

        Ok(StateMachine(forum_topc))
    }
}

impl TryFrom<StateMachine<ForumTopicSelection>> for StateMachine<FileSelection> {
    type Error = anyhow::Error;
    fn try_from(value: StateMachine<ForumTopicSelection>) -> Result<Self> {
        let StateMachine(inner) = value;
        let ForumTopicSelection {
            forum_topics,
            selected_topics,
            dialog,
            ..
        } = inner;

        if selected_topics.is_empty() {
            anyhow::bail!("Cannot Proceed with no forum topic selected!")
        }

        let forum_topics = forum_topics
            .into_iter()
            .enumerate()
            .filter(|(index, _)| selected_topics.contains(&index))
            .map(|(_, topic)| topic)
            .collect();

        let inner = FileSelection {
            dialog,
            list_state: ListState::default(),
            selected_files: HashSet::new(),
            forum_topics,
            messages: Vec::new(),
        };

        Ok(StateMachine(inner))
    }
}

impl TryFrom<StateMachine<FileSelection>> for StateMachine<DownloadState> {
    type Error = anyhow::Error;
    fn try_from(value: StateMachine<FileSelection>) -> Result<Self> {
        let StateMachine(inner) = value;
        let FileSelection {
            messages,
            selected_files,
            ..
        } = inner;

        let files = messages
            .iter()
            .enumerate()
            .filter(|(index, _)| selected_files.contains(&index))
            .map(|(_, msg)| msg)
            .filter(|message| message.media().is_some())
            .map(|msg| (msg.id(), msg))
            .map(|(id, message)| (id, message.media().unwrap()))
            .map(|(id, media)| {
                let (filename, size) = match &media {
                    Media::Photo(p) => {
                        let name = format!("photo_{}.jpg", p.id());
                        let size = p.size().unwrap_or(100);
                        (name, size)
                    }
                    Media::Document(doc) => {
                        let name = doc.name().unwrap_or("unknown_file").to_owned();
                        let size = doc.size().unwrap_or(100);
                        (name, size)
                    }
                    _ => panic!("Unsupported File Type"),
                };

                DownloadableFile {
                    media,
                    filename,
                    id,
                    size,
                }
            })
            .collect();

        let download = DownloadState {
            files,
            queued_downloads: HashMap::new(),
        };

        Ok(StateMachine(download))
    }
}

impl From<Error> for StateWrapper {
    fn from(value: Error) -> Self {
        StateWrapper::Error(StateMachine(value))
    }
}

impl From<StateMachine<Error>> for StateWrapper {
    fn from(value: StateMachine<Error>) -> Self {
        StateWrapper::Error(value)
    }
}

impl From<StateMachine<PeerSelection>> for StateWrapper {
    fn from(value: StateMachine<PeerSelection>) -> Self {
        StateWrapper::PeerSelection(value)
    }
}

impl From<StateMachine<ForumTopicSelection>> for StateWrapper {
    fn from(value: StateMachine<ForumTopicSelection>) -> Self {
        StateWrapper::ForumTopicSelection(value)
    }
}

impl From<StateMachine<FileSelection>> for StateWrapper {
    fn from(value: StateMachine<FileSelection>) -> Self {
        StateWrapper::FileSelection(value)
    }
}

impl From<StateMachine<DownloadState>> for StateWrapper {
    fn from(value: StateMachine<DownloadState>) -> Self {
        StateWrapper::Download(value)
    }
}

impl std::fmt::Display for StateWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use StateWrapper::*;
        let str = match self {
            PeerSelection(_) => "Peer Selection",
            ForumTopicSelection(_) => "Forum Topic Selection",
            FileSelection(_) => "File Selection",
            Download(_) => "Download State",
            _ => "Default",
        };
        f.write_str(str)
    }
}

pub enum StateWrapper {
    PeerSelection(StateMachine<PeerSelection>),
    ForumTopicSelection(StateMachine<ForumTopicSelection>),
    FileSelection(StateMachine<FileSelection>),
    Download(StateMachine<DownloadState>),
    Error(StateMachine<Error>),
}

impl Default for StateWrapper {
    fn default() -> Self {
        Self::PeerSelection(StateMachine(PeerSelection::default()))
    }
}

#[derive(Debug)]
pub struct DownloadableFile {
    pub media: Media,
    pub filename: String,
    pub size: usize,
    pub id: i32,
}

#[derive(Debug)]
pub struct ForumTopicSelection {
    pub dialog: Dialog,
    pub list_state: ListState,
    pub selected_topics: HashSet<usize>,
    pub forum_topics: Vec<ForumTopic>,
}

#[derive(Default, Debug)]
pub struct PeerSelection {
    pub list_state: ListState,
    pub dialogs: Vec<Dialog>,
}

#[derive(Debug)]
pub struct FileSelection {
    pub dialog: Dialog,
    pub list_state: ListState,
    pub selected_files: HashSet<usize>,
    pub forum_topics: Vec<ForumTopic>,
    pub messages: Vec<Message>,
}

pub enum FileStatus {
    InProgress,
    Error(Error),
    Finished,
}

impl Default for FileStatus {
    fn default() -> Self {
        Self::InProgress
    }
}

pub struct FileEntry {
    pub filename: String,
    pub total_size: usize,
    pub total_downloaded: usize,
    pub rx: UnboundedReceiver<DownloadEvent>,
    pub file_status: FileStatus,
}

pub struct DownloadState {
    pub files: Vec<DownloadableFile>,
    pub queued_downloads: HashMap<i32, FileEntry>,
}

pub struct Authenticated {
    ctx: Arc<RwLock<Context>>,
    state: StateWrapper,
}

impl Authenticated {
    async fn new(config: Config) -> Result<Self> {
        let ctx = Arc::new(RwLock::new(Context::new(config).await?));
        Ok(Self {
            ctx,
            state: StateWrapper::default(),
        })
    }
}

pub struct Config {
    pub output: PathBuf,
    pub limit: usize,
    pub retries: usize,
}

impl Default for Config {
    fn default() -> Self {
        let path = UserDirs::new().unwrap();
        let output = path.download_dir().unwrap().to_owned();

        Self {
            output,
            limit: 1,
            retries: 1,
        }
    }
}

pub struct Application<S>(S);

impl<S> AsRef<S> for Application<S> {
    fn as_ref(&self) -> &S {
        &self.0
    }
}

impl<S> AsMut<S> for Application<S> {
    fn as_mut(&mut self) -> &mut S {
        &mut self.0
    }
}

impl<S> Deref for Application<S> {
    type Target = S;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S> DerefMut for Application<S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Application<Authenticated> {
    pub async fn new(config: Config) -> Result<Self> {
        Ok(Self(Authenticated::new(config).await?))
    }

    pub async fn run<B: Backend>(mut self, terminal: &mut Terminal<B>) -> Result<bool>
    where
        B::Error: Sync + Send + 'static,
    {
        let mut ticker = TerminalTicker::new(self.ctx.clone());

        let controller = TerminalController;
        loop {
            let tui = TerminalUserInterface;
            terminal.draw(|f| f.render_stateful_widget(tui, f.area(), &mut self.state))?;

            ticker.tick(&mut self.state).await?;

            if event::poll(Duration::from_millis(16))? {
                let event = event::read()?;
                let prev_state = std::mem::take(&mut self.state);
                self.state = match controller.handle(&event, prev_state) {
                    Ok(state) => state,
                    Err(e) => StateWrapper::from(e),
                };
                if let Event::Key(key_event) = event {
                    match key_event.code {
                        KeyCode::Esc => return Ok(true),
                        _ => {}
                    }
                }
            }
        }
    }
}

pub type ContextThreadSafe = Arc<RwLock<Context>>;

pub struct Context {
    pub config: Config,
    pub downloader: Downloader,
    pub client: Client,
    pub session: Arc<SqliteSession>,
}

const SESSION_FILE: &str = "telomere.session";

impl Context {
    pub async fn new(config: Config) -> Result<Self> {
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
        let downloader = Downloader::new(client.clone(), config.limit);

        Ok(Self {
            client,
            session,
            config,
            downloader,
        })
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
