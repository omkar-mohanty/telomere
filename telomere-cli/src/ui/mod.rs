use std::{marker::PhantomData, sync::Arc};

use crate::app::{Context, ForumTopicSelection, PeerSelection, StateMachine, StateWrapper};
use anyhow::{Ok, Result};
use grammers_client::{Client, peer::Dialog};

use grammers_session::types::PeerRef;
use grammers_tl_types::types::ForumTopic;
use grammers_tl_types::{enums::messages::ForumTopics, functions::messages::GetForumTopics};
use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, KeyEvent},
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget},
};
use tokio::task::JoinSet;

pub trait Controller {
    type State;
    type AppState;
    fn handle(
        &self,
        event: Event,
        state: &mut Self::State,
    ) -> Result<Option<StateMachine<Self::AppState>>>;
}

pub trait Tick {
    type State;
    async fn tick(&mut self, state: &mut Self::State) -> Result<()>;
}

pub struct TerminalUserInterface;

pub struct TerminalController;

pub struct TerminalTicker;

pub struct UIPeerSelectionState {
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
}

impl TerminalUserInterface {
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
}

impl StatefulWidget for TerminalUserInterface {
    type State = UIPeerSelectionState;
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        if state.dialogs.is_empty() {
            self.render_loading_peers(area, buf);
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

impl TerminalController<PeerSelection> {
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

impl Controller for TerminalController<PeerSelection> {
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
                    let forum_topic_selection = ForumTopicSelection {
                        peer_ref,
                        forum_topics: Vec::new(),
                        messages: None,
                    };
                    return Ok(Some(StateMachine(forum_topic_selection)));
                }
                _ => {}
            }
        }
        Ok(None)
    }
}

impl Tick for TerminalTicker<PeerSelection> {
    type State = UIPeerSelectionState;
    async fn tick(&mut self, state: &mut Self::State) -> Result<()> {
        while let Some(res) = state.join_set.try_join_next() {
            let res = res??;
            state.dialogs.extend(res);
        }
        Ok(())
    }
}

impl TerminalTicker<PeerSelection> {
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

async fn get_forum_topics(client: &Client, peer: &PeerRef) -> Result<Vec<ForumTopic>> {
    let mut filtered_topics = Vec::new();

    // Tracking markers for API pagination chunk offsets
    let mut current_offset_date = 0;
    let mut current_offset_id = 0;
    let mut current_offset_topic = 0;

    loop {
        let forum_topic_res = client
            .invoke(&GetForumTopics {
                peer: peer.into(),
                q: None,
                offset_date: current_offset_date,
                offset_id: current_offset_id,
                offset_topic: current_offset_topic,
                limit: 100, // Request healthy chunk page boundaries
            })
            .await?;

        let ForumTopics::Topics(topics_payload) = forum_topic_res;

        if topics_payload.topics.is_empty() {
            break; // Reached the bottom of the group layout history
        }

        // Keep track of the last element's positions to pass into the next pagination step
        let mut last_topic_id = None;

        for topic in topics_payload.topics {
            if let grammers_tl_types::enums::ForumTopic::Topic(t) = topic {
                last_topic_id = Some(t.id);
                filtered_topics.push(t);
            }
        }

        // If your group has a total item count, you can also break early when match lengths line up
        if filtered_topics.len() >= topics_payload.count as usize {
            break;
        }

        // Update tracking markers based on the last processed element
        if let Some(id) = last_topic_id {
            // Adjust markers using payload fields to request subsequent records safely
            current_offset_topic = id;

            // Fallback safety to prevent infinite loops if values stall out
            if let Some(last_item) = filtered_topics.last() {
                current_offset_date = last_item.date;
                current_offset_id = last_item.id; // Set relative to message reference bounds
            }
        } else {
            break;
        }
    }

    Ok(filtered_topics)
}
