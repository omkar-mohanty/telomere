use anyhow::Result;
use directories::ProjectDirs;
use grammers_client::Client;
use grammers_client::client::LoginToken;
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

pub struct Application {
    pub client: Client,
    pub session: Arc<SqliteSession>,
    pub current_screen: CurrentScreen,
    pub input_buffer: String,
}

pub enum CurrentScreen {
    Auth(AuthScreen),
    Main,
}

pub enum AuthScreen {
    PhoneNumber,
    LoginCode,
}

const SESSION_FILE: &str = "telomere.session";

impl Application {
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

        Ok(Self {
            client,
            session,
            current_screen: CurrentScreen::Auth(AuthScreen::PhoneNumber),
            input_buffer: String::new(),
        })
    }

    pub async fn init_auth(&self, phone: String) -> Result<Option<LoginToken>> {
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

    pub async fn finish_auth(&self, code: String, token: LoginToken) -> Result<()> {
        let signed_in = self.client.sign_in(&token, &code).await;

        match signed_in {
            Ok(_) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
