use std::sync::Arc;

use anyhow::{Error, Result};
use grammers_client::message::Message;
use grammers_session::types::PeerRef;
use grammers_tl_types::enums::MessageReplyHeader;
use grammers_tl_types::types::ForumTopic;
use ratatui::widgets::StatefulWidget;
use ratatui::{
    crossterm::event::{Event, KeyCode},
    prelude::*,
    style::{Color, Modifier, Style, Stylize},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};
use tokio::sync::mpsc::Receiver;

use crate::app::{FileSelection, StateMachine, StateWrapper};
use crate::{
    app::Context,
    ui::{Controller, Tick},
};

pub struct FileSelectionScreenUI;

pub struct FileSelectionController;

pub struct FileSelectionTicker {
    rx: Receiver<Message>,
}

impl FileSelectionTicker {
    pub fn new(ctx: Arc<Context>, peer_ref: PeerRef, forum_topics: Vec<ForumTopic>) -> Self {
        let client = ctx.client.clone();
        let (tx, rx) = tokio::sync::mpsc::channel(50);
        let mut total_media = 0;
        let mut added_media = 0;

        tokio::spawn(async move {
            log::info!("Fetching Media Files");
            let mut message_iter = client.iter_messages(peer_ref);

            loop {
                match message_iter.next().await {
                    Ok(Some(message)) => {
                        if message.media().is_some() {
                            total_media += 1;
                            let thread_id = get_thread_root_id(&message);
                            if let Some(thread_id) = thread_id
                                && forum_topics
                                    .iter()
                                    .any(|forum_topic| forum_topic.id == thread_id)
                            {
                                added_media += 1;
                                let _ = tx.send(message);
                            }
                        }
                    }
                    Ok(None) => {
                        break;
                    }
                    Err(e) => {
                        log::error!("Error Received when fetching media : {}", e);
                        return Err(Error::from(e));
                    }
                }
            }

            log::info!("Total Media Received : {}", total_media);
            log::info!("Total Media Filtered : {}", added_media);

            Ok::<(), Error>(())
        });

        Self { rx }
    }
}

impl Tick for FileSelectionTicker {
    type State = StateMachine<FileSelection>;
    async fn tick(&mut self, state: &mut Self::State) -> Result<()> {
        while let Ok(res) = self.rx.try_recv() {
            state.messages.push(res);
        }
        Ok(())
    }
}

impl Controller for FileSelectionController {
    type State = StateMachine<FileSelection>;
    type Output = StateWrapper;
    fn handle(&self, event: &Event, mut state: Self::State) -> Result<Self::Output> {
        if let Event::Key(key) = event {
            let current = state.list_state.selected().unwrap_or(0);

            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    let next = if current == 0 {
                        state.messages.len().checked_sub(1).unwrap_or_default()
                    } else {
                        current - 1
                    };
                    state.list_state.select(Some(next));
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let next = if current >= state.messages.len().checked_sub(1).unwrap_or_default()
                    {
                        0
                    } else {
                        current + 1
                    };

                    state.list_state.select(Some(next));
                }
                KeyCode::Char('s') => {
                    if state.selected_files.contains(&current) {
                        state.selected_files.remove(&current);
                    } else {
                        state.selected_files.insert(current);
                    }
                }
                KeyCode::Char('S') => {}
                KeyCode::Enter => {}
                _ => {}
            }
        }
        Ok(StateWrapper::from(state))
    }
}

impl StatefulWidget for FileSelectionScreenUI {
    type State = StateMachine<FileSelection>;
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        if state.messages.is_empty() {
            let str = "🔄 Loading Files...\nPress [Esc] to exit.".to_string();
            let loading = Paragraph::new(str).block(
                Block::default()
                    .title(" Media Files ")
                    .borders(Borders::ALL)
                    .fg(Color::White),
            );
            // Render loading screen to the entire area
            Widget::render(loading, area, buf);
        } else {
            log::info!("Inside List Rendering. Count: {}", state.messages.len());

            let items: Vec<ListItem> = state
                .messages
                .iter()
                .filter(|msg| msg.media().is_some())
                .enumerate()
                .map(|(index, msg)| {
                    let is_selected = state.selected_files.contains(&index);

                    // Fallback if msg.text() is empty
                    let mut title = msg.text().to_owned();
                    if title.is_empty() {
                        title = format!("Media File #{}", index + 1);
                    }

                    // Apply green style + [X] if selected, default style + [ ] otherwise
                    if is_selected {
                        ListItem::new(format!("[X] {}", title)).fg(Color::Green)
                    } else {
                        ListItem::new(format!("[ ] {}", title)).fg(Color::White)
                    }
                })
                .collect();

            let list = List::new(items)
                .block(
                    Block::default()
                        .title(" Select Files to Process ")
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

            // Render the full list directly to the entire area
            StatefulWidget::render(list, area, buf, &mut state.list_state);
        }
    }
}

fn get_thread_root_id(msg: &Message) -> Option<i32> {
    let reply_header = msg.reply_header();
    if let Some(MessageReplyHeader::Header(hdr)) = reply_header {
        let thread_root_id = hdr
            .reply_to_top_id
            .or(hdr.reply_to_msg_id)
            .expect("forum topic but no msg id");
        return Some(thread_root_id);
    }
    None
}
