use anyhow::Result;
use directories::ProjectDirs;
use grammers_client::Client;
use grammers_client::client::LoginToken;
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use grammers_session::types::PeerRef;
use ratatui::crossterm::event::{self, KeyCode};
use ratatui::prelude::Backend;
use ratatui::{Terminal, crossterm::event::Event};
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use crate::ui::{Controller, CurrentScreen, Screen};

pub struct StateMachine<S> {
    pub ctx: Arc<Context>,
    pub state: S,
}

pub enum StateWrapper {
    Init(StateMachine<InitState>),
    Auth(AuthState),
    PeerSelection(StateMachine<PeerSelection>),
    ForumTopicSelection,
    Download(StateMachine<DownloadState>),
    Done,
}

impl StateWrapper {
    pub fn new(ctx: Arc<Context>) -> Self {
        Self::Init(StateMachine {
            ctx,
            state: InitState,
        })
    }
}

pub struct PeerSelection {
    pub peer_ref: Option<PeerRef>,
}

pub enum AuthState {
    PhoneNumber(StateMachine<AuthPhoneNumber>),
    LoginCode(StateMachine<AuthLoginCode>),
}

pub struct InitState;
pub struct DownloadState {}

pub struct AuthPhoneNumber {
    pub phone: String,
}

pub struct AuthLoginToken {
    pub login_token: LoginToken,
}

pub struct AuthLoginCode {
    pub login_code: String,
}

pub struct Application {
    ctx: Arc<Context>,
    state_wrapper: StateWrapper,
    current_screen: CurrentScreen,
}

impl Application {
    pub async fn new() -> Result<Self> {
        let ctx = Arc::new(Context::new().await?);
        let current_screen = CurrentScreen::new(Arc::clone(&ctx)).await?;
        let state_wrapper = StateWrapper::new(Arc::clone(&ctx));
        Ok(Self {
            state_wrapper,
            ctx,
            current_screen,
        })
    }
}

impl Application {
    pub async fn run<B: Backend>(mut self, terminal: &mut Terminal<B>) -> Result<bool>
    where
        B::Error: Sync + Send + 'static,
    {
        loop {
            use StateWrapper::*;
            terminal.draw(|f| self.current_screen.draw(f))?;
            let event = event::read()?;
            if let Some(transition) = self.current_screen.handle_event(&event).await? {
                let current_state = self.state_wrapper;
                match (current_state, transition) {
                    (Auth(AuthState::PhoneNumber(_phone)), Auth(AuthState::LoginCode(_login))) => {
                        todo!()
                    }
                    (_, PeerSelection(peer)) => todo!(),
                    (PeerSelection(_), ForumTopicSelection) => todo!(),
                    (_, _) => todo!(),
                }
            }

            if let Event::Key(key) = event {
                match key.code {
                    KeyCode::Esc => return Ok(true),
                    _ => {}
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
