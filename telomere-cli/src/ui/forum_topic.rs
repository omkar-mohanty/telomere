use std::sync::Arc;

use anyhow::{Error, Result};
use grammers_client::Client;
use grammers_session::types::PeerRef;
use grammers_tl_types::types::ForumTopic;
use grammers_tl_types::{enums::messages::ForumTopics, functions::messages::GetForumTopics};
use ratatui::widgets::{StatefulWidget, Widget};
use tokio::task::JoinSet;

use crate::app::{Context, FileSelection, ForumTopicSelection, StateMachine, StateWrapper};
use crate::ui::{Controller, Tick};
use ratatui::{
    crossterm::event::{Event, KeyCode},
    style::{Color, Modifier, Style, Stylize},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

pub struct ForumTopicController;

impl Controller for ForumTopicController {
    type State = StateMachine<ForumTopicSelection>;
    type Output = StateWrapper;

    fn handle(
        &self,
        event: &ratatui::crossterm::event::Event,
        mut state: Self::State,
    ) -> Result<Self::Output> {
        if let Event::Key(key) = event {
            let current = state.list_state.selected().unwrap_or(0);

            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    let next = if current == 0 {
                        state.forum_topics.len() - 1
                    } else {
                        current - 1
                    };
                    state.list_state.select(Some(next));
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let next = if current >= state.forum_topics.len() - 1 {
                        0
                    } else {
                        current + 1
                    };

                    state.list_state.select(Some(next));
                }
                KeyCode::Char('s') => {
                    if state.selected_topics.contains(&current) {
                        state.selected_topics.remove(&current);
                    } else {
                        state.selected_topics.insert(current);
                    }
                }
                KeyCode::Enter => {
                    let state = StateMachine::<FileSelection>::try_from(state)?;
                    return Ok(StateWrapper::from(state));
                }
                _ => {}
            }
        }
        Ok(StateWrapper::from(state))
    }
}

pub struct ForumTopicTicker {
    join_set: JoinSet<Result<Vec<ForumTopic>>>,
}

impl ForumTopicTicker {
    pub fn new(ctx: Arc<Context>, peer: PeerRef) -> Self {
        let mut join_set = JoinSet::new();

        join_set.spawn(async move {
            let res = get_forum_topics(&ctx.client, &peer).await?;
            Ok::<Vec<ForumTopic>, Error>(res)
        });

        Self { join_set }
    }
}

impl Tick for ForumTopicTicker {
    type State = StateMachine<ForumTopicSelection>;
    async fn tick(&mut self, state: &mut Self::State) -> Result<()> {
        while let Some(res) = self.join_set.try_join_next() {
            let res = res??;
            state.forum_topics.extend(res);
        }
        Ok(())
    }
}

pub struct ForumTopicUI;

impl StatefulWidget for ForumTopicUI {
    type State = StateMachine<ForumTopicSelection>;

    fn render(
        self,
        area: ratatui::prelude::Rect,
        buf: &mut ratatui::prelude::Buffer,
        state: &mut Self::State,
    ) {
        if state.forum_topics.is_empty() {
            let loading_pane =
                Paragraph::new("🔄 Loading Forum Topics/Peers...\nPress [Esc] to exit.").block(
                    Block::default()
                        .title(" Dialogs ")
                        .borders(Borders::ALL)
                        .fg(Color::White),
                );
            Widget::render(loading_pane, area, buf);
        } else {
            let items: Vec<ListItem> = state
                .forum_topics
                .iter()
                .enumerate()
                .map(|(index, forum_topic)| {
                    let name = forum_topic.title.clone();
                    let mut style = Style::default();
                    let mut prefix = "[ ]";
                    if state.selected_topics.contains(&index) {
                        prefix = "[X]";
                        style = style.fg(Color::Green).add_modifier(Modifier::BOLD);
                    }
                    ListItem::new(format!("{}{}", prefix, name)).style(style)
                })
                .collect();
            let list = List::new(items)
                .block(
                    Block::default()
                        .title(" Forum Topics ")
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
