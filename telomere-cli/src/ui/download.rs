use std::sync::Arc;

use anyhow::{Error, Ok, Result};
use grammers_client::Client;
use grammers_session::types::PeerRef;
use grammers_tl_types::enums::messages::ForumTopics;
use grammers_tl_types::functions::messages::GetForumTopics;
use grammers_tl_types::types::ForumTopic;
use ratatui::widgets::ListState;
use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode},
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style, Stylize},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};
use tokio::task::JoinSet;

use crate::app::{Context, ForumTopicSelection, StateMachine};
use crate::ui::Tick;
use crate::{
    app::StateWrapper,
    ui::{Controller, Screen},
};

pub enum DownloadScreen {
    Group(GroupDownloadScreen),
    Direct,
}

impl Controller for DownloadScreen {
    async fn handle_event(&mut self, event: &Event) -> Result<Option<StateWrapper>> {
        use DownloadScreen::*;
        match self {
            Group(group) => group.handle_event(event).await,
            Direct => todo!(),
        }
    }
}

impl Screen for DownloadScreen {
    fn draw(&self, f: &mut Frame) {
        use DownloadScreen::*;
        match self {
            Group(screen) => screen.draw(f),
            Direct => todo!(),
        }
    }
}

type GroupSelectionResult = Result<Vec<ForumTopic>, Error>;

pub struct GroupDownloadScreen {
    ctx: Arc<Context>,
    list_state: ListState,
    peer_ref: PeerRef,
    forum_topics: Vec<ForumTopic>,
    join_set: JoinSet<GroupSelectionResult>,
}

impl GroupDownloadScreen {
    pub fn new(ctx: Arc<Context>, peer_ref: PeerRef) -> Self {
        let mut join_set = JoinSet::new();
        let ctx_clone = ctx.clone();
        let peer_clone = peer_ref.clone();
        let list_state = ListState::default();

        join_set.spawn(async move {
            let res = get_forum_topics(&ctx_clone.client, &peer_clone).await?;
            Ok::<Vec<ForumTopic>>(res)
        });

        Self {
            ctx,
            peer_ref,
            join_set,
            list_state,
            forum_topics: Vec::new(),
        }
    }
}

impl Screen for GroupDownloadScreen {
    fn draw(&self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header bar
                Constraint::Min(10),   // Main workspace
                Constraint::Length(3), // Help/Status footer
            ])
            .split(f.area());

        // 1. Header Widget
        let header = Paragraph::new("🧬 TELOMERE Media Downloader")
            .style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).fg(Color::DarkGray));
        f.render_widget(header, chunks[0]);

        // 2. Core Workspace Layout (Split split Left/Right for info panels)
        let workspace_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(40), // Left Pane: Chat List / Peers
                Constraint::Percentage(60), // Right Pane: Tasks / Media / Details
            ])
            .split(chunks[1]);

        // Left pane: show loading message or list of peers when ready
        if self.forum_topics.is_empty() {
            let left_pane =
                Paragraph::new("🔄 Loading Telegram Dialogs/Peers...\nPress [Esc] to exit.").block(
                    Block::default()
                        .title(" Dialogs ")
                        .borders(Borders::ALL)
                        .fg(Color::White),
                );
            f.render_widget(left_pane, workspace_chunks[0]);
        } else {
            let items: Vec<ListItem> = self
                .forum_topics
                .iter()
                .map(|dialog| {
                    let name = dialog.title.clone();
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
            let mut state = self.list_state.clone();
            f.render_stateful_widget(list, workspace_chunks[0], &mut state);
        }

        let right_pane = Paragraph::new(
            "Select a dialog to inspect forum topics or manage active background media downloads.",
        )
        .block(
            Block::default()
                .title(" Operations Panel ")
                .borders(Borders::ALL)
                .fg(Color::White),
        )
        .wrap(Wrap { trim: true });

        // Right pane
        f.render_widget(right_pane, workspace_chunks[1]);

        // 3. Footer Widget
        let footer_text = "Quit: [Ctrl+C] or [Esc] | Toggle View: [Tab]";
        let footer = Paragraph::new(footer_text)
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Left)
            .block(Block::default().borders(Borders::ALL).fg(Color::DarkGray));
        f.render_widget(footer, chunks[2]);
    }
}

impl Tick for GroupDownloadScreen {
    async fn tick(&mut self) -> Result<()> {
        while let Some(res) = self.join_set.try_join_next() {
            let res = res??;
            self.forum_topics.extend(res);
        }
        Ok(())
    }
}

impl Controller for GroupDownloadScreen {
    async fn handle_event(&mut self, event: &Event) -> Result<Option<StateWrapper>> {
        if let Event::Key(key) = event {
            let current = self.list_state.selected().unwrap_or(0);

            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    let next = if current == 0 {
                        self.forum_topics.len() - 1
                    } else {
                        current - 1
                    };
                    self.list_state.select(Some(next));
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let next = if current >= self.forum_topics.len() - 1 {
                        0
                    } else {
                        current + 1
                    };

                    self.list_state.select(Some(next));
                }
                KeyCode::Enter => {}
                _ => {}
            }
        }
        Ok(None)
    }
}

async fn get_forum_topics(client: &Client, peer: &PeerRef) -> Result<Vec<ForumTopic>> {
    let mut filtered_topics = Vec::new();

    let forum_topic_res = client
        .invoke(&GetForumTopics {
            peer: peer.into(),
            q: None,
            offset_date: 0,
            offset_id: 0,
            offset_topic: 0,
            limit: 0,
        })
        .await?;
    let topics = {
        let ForumTopics::Topics(topics) = forum_topic_res;
        topics.topics
    };

    for topic in topics {
        match topic {
            grammers_tl_types::enums::ForumTopic::Topic(topic) => filtered_topics.push(topic),
            _ => {}
        }
    }

    Ok(filtered_topics)
}
