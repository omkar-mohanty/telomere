mod auth;
mod download;
mod peer_selection;
use crate::app::StateWrapper;
use anyhow::Result;
pub use auth::*;
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
        }
    }
}

impl Controller for CurrentScreen {
    async fn handle_event(&mut self, event: &Event) -> Result<Option<StateWrapper>> {
        use CurrentScreen::*;
        match self {
            PeerSelectionScreen(page) => page.handle_event(&event).await,
            AuthScreen(page) => page.handle_event(&event).await,
        }
    }
}

impl Tick for CurrentScreen {
    async fn tick(&mut self) -> Result<()> {
        if let CurrentScreen::PeerSelectionScreen(screen) = self {
            screen.tick().await?;
        }
        Ok(())
    }
}

pub enum CurrentScreen {
    PeerSelectionScreen(PeerSelectionScreen),
    AuthScreen(AuthScreen),
}
