mod auth;
mod download;
mod peer_selection;
use crate::app::StateWrapper;
use anyhow::Result;
pub use auth::*;
pub use download::*;
pub use peer_selection::*;
use ratatui::{Frame, crossterm::event::Event};

pub trait Tick {
    async fn tick(&mut self) -> Result<()>;
}

pub trait Screen {
    fn draw(&self, f: &mut Frame);
}

pub trait Controller {
    async fn handle_event(&mut self, event: &Event) -> Result<Option<StateWrapper>>;
}

impl Screen for CurrentScreen {
    fn draw(&self, f: &mut Frame) {
        use CurrentScreen::*;
        match self {
            PeerSelectionScreen(page) => page.draw(f),
            AuthScreen(page) => page.draw(f),
            DownloadScreen(page) => page.draw(f),
        }
    }
}

impl Controller for CurrentScreen {
    async fn handle_event(&mut self, event: &Event) -> Result<Option<StateWrapper>> {
        use CurrentScreen::*;
        match self {
            PeerSelectionScreen(page) => page.handle_event(&event).await,
            AuthScreen(page) => page.handle_event(&event).await,
            DownloadScreen(page) => page.handle_event(&event).await,
        }
    }
}

impl Tick for CurrentScreen {
    async fn tick(&mut self) -> Result<()> {
        match self {
            CurrentScreen::PeerSelectionScreen(screen) => screen.tick().await?,
            CurrentScreen::DownloadScreen(screen) => screen.tick().await?,
            _ => {}
        }
        Ok(())
    }
}

pub enum CurrentScreen {
    PeerSelectionScreen(PeerSelectionScreen),
    AuthScreen(AuthScreen),
    DownloadScreen(DownloadScreen),
}
