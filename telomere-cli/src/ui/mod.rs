mod auth;
mod download;
mod peer_selection;
use std::{marker::PhantomData, sync::Arc};

use crate::app::{Context, ForumTopicSelection, PeerSelection, StateMachine, StateWrapper};
use anyhow::{Ok, Result};
pub use download::*;
use grammers_client::peer::Dialog;
pub use peer_selection::*;
use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, KeyEvent},
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget},
};
use tokio::task::JoinSet;

pub struct TerminalUserInterface<S> {
    _phantom: PhantomData<S>,
}

struct UIPeerSelectionState {
    ctx: Arc<Context>,
    list_state: ListState,
    dialogs: Vec<Dialog>,
    join_set: JoinSet<Result<Vec<Dialog>>>,
}

impl UIPeerSelectionState {
    pub fn new(ctx: Arc<Context>) -> Self {
        let list_state = ListState::default();
        let dialogs = Vec::new();
        let mut iter_dialogs = ctx.client.iter_dialogs();
        let mut join_set = JoinSet::new();

        join_set.spawn(async move {
            let mut dialogs = Vec::new();
            while let Some(dialog) = iter_dialogs.next().await? {
                dialogs.push(dialog);
            }

            Ok::<Vec<Dialog>>(dialogs)
        });

        Self {
            ctx,
            list_state,
            dialogs,
            join_set,
        }
    }

    pub fn refresh(&mut self) -> Result<()> {
        while let Some(joined_task) = self.join_set.try_join_next() {
            let dialogs = joined_task??;
            self.dialogs.extend(dialogs);
        }

        Ok(())
    }
}

impl TerminalUserInterface<PeerSelection> {
    fn render_loading_peers(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new("🔄 Loading Telegram Dialogs/Peers...\nPress [Esc] to exit.")
            .block(
                Block::default()
                    .title(" Dialogs ")
                    .borders(Borders::ALL)
                    .fg(Color::White),
            )
            .render(area, buf);
    }

    fn render_peers(&self, area: Rect, buf: &mut Buffer, state: &mut UIPeerSelectionState) {
        let items: Vec<ListItem> = state
            .dialogs
            .iter()
            .map(|dialog| {
                let name = dialog.peer().name().unwrap_or("Unknown Chat");
                ListItem::new(name)
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .title(" Dialogs ")
                    .borders(Borders::ALL)
                    .fg(Color::White),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        StatefulWidget::render(list, area, buf, &mut state.list_state);
    }
}

impl StatefulWidget for TerminalUserInterface<PeerSelection> {
    type State = UIPeerSelectionState;
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        if state.dialogs.is_empty() {
            self.render_loading_peers(area, buf);
        } else {
            self.render_peers(area, buf, state);
        }
    }
}

pub struct TerminalController<S> {
    _phantom: PhantomData<S>,
}

impl Controller2 for TerminalController<PeerSelection> {
    type State = UIPeerSelectionState;
    type AppState = ForumTopicSelection;

    fn handle(
        &self,
        event: Event,
        state: &mut Self::State,
    ) -> Result<Option<StateMachine<Self::AppState>>> {
        let current = state.list_state.selected().unwrap_or(0);
        if let Event::Key(key_event) = event {
            match key_event.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    let next = if current == 0 {
                        state.dialogs.len() - 1
                    } else {
                        current - 1
                    };

                    state.list_state.select(Some(next));
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let next = if current >= state.dialogs.len() - 1 {
                        0
                    } else {
                        current + 1
                    };

                    state.list_state.select(Some(next));
                }
                KeyCode::Enter => {
                    let dialog = state.dialogs.get(current).unwrap();
                    let peer_ref = dialog.peer_ref();
                    let forum_topic_selection = ForumTopicSelection { peer_ref };
                    todo!()
                }
                _ => {}
            }
        }
        Ok(None)
    }
}

pub trait Controller2 {
    type State;
    type AppState;
    fn handle(
        &self,
        event: Event,
        state: &mut Self::State,
    ) -> Result<Option<StateMachine<Self::AppState>>>;
}

pub trait Tick2 {
    type State;
    async fn tick(&mut self, state: &mut Self::State) -> Result<()>;
}

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
            DownloadScreen(page) => page.draw(f),
        }
    }
}

impl Controller for CurrentScreen {
    async fn handle_event(&mut self, event: &Event) -> Result<Option<StateWrapper>> {
        use CurrentScreen::*;
        match self {
            PeerSelectionScreen(page) => page.handle_event(&event).await,
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
    DownloadScreen(DownloadScreen),
}
