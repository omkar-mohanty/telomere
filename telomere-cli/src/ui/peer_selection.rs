use crate::app::{Context, ForumTopicSelection, PeerSelection, StateMachine};
use crate::ui::{Controller, Tick};
use anyhow::Result;
use grammers_client::peer::Dialog;
use ratatui::widgets::ListState;
use std::collections::HashSet;
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
        let state = &mut state.0;
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
                        log::info!("Recived Dialog");
                        dialogs.push(dialog);
                    }
                    Ok(None) => {
                        log::info!("End of Dialog Stream");
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
    type Output = StateMachine<ForumTopicSelection>;
    fn handle(&self, event: &Event, state: &mut Self::State) -> Result<Option<Self::Output>> {
        let state = &mut state.0;
        if let Event::Key(key) = event {
            if state.dialogs.is_empty() {
                return Ok(None);
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
                    let dialog = state.dialogs.swap_remove(current);
                    let res = StateMachine(ForumTopicSelection {
                        dialog,
                        forum_topics: Vec::new(),
                        selected_topics: HashSet::default(),
                        messages: None,
                        list_state: ListState::default(),
                    });

                    return Ok(Some(res));
                }
                _ => {}
            }
        }
        Ok(None)
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
