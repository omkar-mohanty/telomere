use anyhow::Result;
use directories::ProjectDirs;
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
use std::collections::HashSet;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::ui::{Controller, TerminalController, TerminalTicker, TerminalUserInterface, Tick};

pub struct StateMachine<S>(pub S);

impl TryFrom<StateMachine<PeerSelection>> for StateMachine<ForumTopicSelection> {
    type Error = anyhow::Error;
    fn try_from(value: StateMachine<PeerSelection>) -> Result<Self> {
        todo!()
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
    fn try_from(_value: StateMachine<DownloadState>) -> Result<Self> {
        Ok(StateMachine(DownloadProgress::InProgress))
    }
}

impl TryFrom<StateMachine<PeerSelection>> for StateWrapper {
    type Error = anyhow::Error;
    fn try_from(value: StateMachine<PeerSelection>) -> Result<Self> {
        Ok(StateWrapper::ForumTopicSelection(value.try_into()?))
    }
}

impl TryFrom<StateMachine<ForumTopicSelection>> for StateWrapper {
    type Error = anyhow::Error;

    fn try_from(value: StateMachine<ForumTopicSelection>) -> Result<Self> {
        Ok(StateWrapper::FileSelection(value.try_into()?))
    }
}

impl TryFrom<StateMachine<FileSelection>> for StateWrapper {
    type Error = anyhow::Error;

    fn try_from(value: StateMachine<FileSelection>) -> Result<Self> {
        Ok(StateWrapper::Download(value.try_into()?))
    }
}

impl TryFrom<StateMachine<DownloadState>> for StateWrapper {
    type Error = anyhow::Error;

    fn try_from(value: StateMachine<DownloadState>) -> Result<Self> {
        Ok(StateWrapper::Progress(value.try_into()?))
    }
}

impl TryFrom<StateMachine<DownloadProgress>> for StateWrapper {
    type Error = anyhow::Error;

    fn try_from(value: StateMachine<DownloadProgress>) -> Result<Self> {
        let progress = value.0;

        let res = match progress {
            DownloadProgress::InProgress => {
                StateWrapper::Progress(StateMachine(DownloadProgress::InProgress))
            }
            DownloadProgress::Finished => {
                StateWrapper::Progress(StateMachine(DownloadProgress::Finished))
            }
        };

        Ok(res)
    }
}

pub enum StateWrapper {
    PeerSelection(StateMachine<PeerSelection>),
    ForumTopicSelection(StateMachine<ForumTopicSelection>),
    FileSelection(StateMachine<FileSelection>),
    Download(StateMachine<DownloadState>),
    Progress(StateMachine<DownloadProgress>),
}

impl Default for StateWrapper {
    fn default() -> Self {
        Self::PeerSelection(StateMachine(PeerSelection::default()))
    }
}

pub struct ForumTopicSelection {
    pub dialog: Dialog,
    pub list_state: ListState,
    pub selected_topics: HashSet<usize>,
    pub forum_topics: Vec<ForumTopic>,
    pub messages: Option<Vec<Message>>,
}

#[derive(Default)]
pub struct PeerSelection {
    pub list_state: ListState,
    pub dialogs: Vec<Dialog>,
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
        let mut ticker = TerminalTicker::new(self.ctx.clone());

        let controller = TerminalController;
        loop {
            let tui = TerminalUserInterface::new(self.ctx.clone());
            terminal.draw(|f| f.render_stateful_widget(tui, f.area(), &mut self.state.state))?;

            ticker.tick(&mut self.state.state).await?;

            if event::poll(Duration::from_millis(16))? {
                let event = event::read()?;
                controller.handle(&event, &mut self.state.state)?;
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
