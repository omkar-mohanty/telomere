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
    ctx: Arc<Context>,
    state: S,
}

impl StateMachine<InitState> {
    pub async fn check_session(self) -> Result<StateWrapper> {
        if self.ctx.client.is_authorized().await? {
            Ok(StateWrapper::PeerSelection(StateMachine {
                ctx: self.ctx,
                state: PeerSelection { peer_ref: None },
            }))
        } else {
            Ok(StateWrapper::Auth(AuthState::PhoneNumber(StateMachine {
                ctx: self.ctx,
                state: AuthPhoneNumber {
                    phone: String::new(),
                },
            })))
        }
    }
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

    pub async fn step(mut self) -> Result<Self> {
        use StateWrapper::*;
        match self {
            Init(state) => state.check_session().await,
            PeerSelection(state) => {
                if state.state.peer_ref.is_none() {
                    Ok(Self::PeerSelection(state))
                } else {
                    Ok(Self::ForumTopicSelection)
                }
            }
            Done => Ok(Self::Done),
            _ => todo!(),
        }
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
    phone: String,
}

pub struct AuthLoginToken {
    login_token: LoginToken,
}

pub struct AuthLoginCode {
    login_code: String,
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
            self.state_wrapper = self.state_wrapper.step().await?;
            use StateWrapper::*;
            match &self.state_wrapper {
                PeerSelection(peer) => {}
                Done => return Ok(true),
                _ => todo!(),
            };
            terminal.draw(|f| self.current_screen.draw(f))?;
            let event = event::read()?;
            self.current_screen.handle_event(&event).await?;

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
