use crate::app::{Context, ForumTopicSelection, PeerSelection, StateMachine, StateWrapper};
use crate::ui::{Controller, Tick};
use anyhow::Result;
use grammers_client::peer::Dialog;
use std::sync::Arc;

use ratatui::{
    crossterm::event::{Event, KeyCode},
    prelude::*,
    widgets::{Block, Borders, List, ListItem, Paragraph, StatefulWidget, Widget},
};
use tokio::task::JoinSet;

pub struct PeerSelectionUI;

impl StatefulWidget for PeerSelectionUI {
    type State = StateMachine<PeerSelection>;
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        if state.dialogs.is_empty() {
            Paragraph::new("🔄 Loading Telegram Dialogs/Peers...\nPress [Esc] to exit.")
                .block(
                    Block::default()
                        .title(" Dialogs ")
                        .borders(Borders::ALL)
                        .fg(Color::White),
                )
                .render(area, buf);
        } else {
            let items: Vec<ListItem> = state
                .dialogs
                .iter()
                .map(|dialog| {
                    let name = dialog.peer().name().unwrap_or("Unknown Chat").to_owned();
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
}
pub struct PeerSelectionTicker {
    join_set: JoinSet<Result<Vec<Dialog>>>,
}

impl PeerSelectionTicker {
    pub fn new(ctx: Arc<Context>) -> Self {
        let mut join_set = JoinSet::new();

        join_set.spawn(async move {
            let mut dialogs = Vec::new();
            let client = &ctx.client;
            let mut stream = client.iter_dialogs();

            loop {
                let res = stream.next().await;

                match res {
                    Ok(Some(dialog)) => {
                        dialogs.push(dialog);
                    }
                    Ok(None) => {
                        break;
                    }
                    Err(e) => {
                        log::error!("{}", e);
                        break;
                    }
                }
            }

            Ok::<Vec<Dialog>, anyhow::Error>(dialogs)
        });

        PeerSelectionTicker { join_set }
    }
}

pub struct PeerSelectionController;

impl Controller for PeerSelectionController {
    type State = StateMachine<PeerSelection>;
    type Output = StateWrapper;
    fn handle(&self, event: &Event, mut state: Self::State) -> Result<Self::Output> {
        if let Event::Key(key) = event {
            if state.dialogs.is_empty() {
                return Ok(StateWrapper::from(state));
            }

            let current = state.list_state.selected().unwrap_or(0);

            match key.code {
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
                    let new_state = StateMachine::<ForumTopicSelection>::try_from(state)?;
                    let wrapper = StateWrapper::from(new_state);
                    return Ok(wrapper);
                }
                _ => {}
            }
        }
        Ok(StateWrapper::from(state))
    }
}

impl Tick for PeerSelectionTicker {
    type State = PeerSelection;
    async fn tick(&mut self, state: &mut Self::State) -> Result<()> {
        while let Some(res) = self.join_set.try_join_next() {
            let res = res??;
            state.dialogs.extend(res);
        }
        Ok(())
    }
}
