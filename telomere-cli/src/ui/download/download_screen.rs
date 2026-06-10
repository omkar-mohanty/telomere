use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Error;
use grammers_client::media::Media;
use grammers_client::message::Message;
use grammers_session::types::PeerRef;
use grammers_tl_types::enums::MessageReplyHeader;
use grammers_tl_types::types::ForumTopic;
use ratatui::widgets::ListState;
use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode},
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style, Stylize},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};
use tokio::sync::mpsc::Receiver;

use crate::{
    app::Context,
    ui::{Controller, Screen, Tick},
};

pub struct DownloadScreen {
    ctx: Arc<Context>,
    documents: Vec<Media>,
    selected_documents: HashSet<usize>,
    rx: Receiver<Vec<Media>>,
    list_state: ListState,
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

impl DownloadScreen {
    pub fn new(ctx: Arc<Context>, peer_ref: PeerRef, forum_topics: Vec<ForumTopic>) -> Self {
        let client = ctx.client.clone();
        let (tx, rx) = tokio::sync::mpsc::channel(50);
        let list_state = ListState::default();
        let mut total_media = 0;
        let mut added_media = 0;

        tokio::spawn(async move {
            log::info!("Fetching Media Files");
            let mut message_iter = client.iter_messages(peer_ref);
            let mut res = Vec::new();

            loop {
                match message_iter.next().await {
                    Ok(Some(message)) => {
                        if let Some(media) = message.media() {
                            total_media += 1;
                            let thread_id = get_thread_root_id(&message);
                            if let Some(thread_id) = thread_id
                                && forum_topics
                                    .iter()
                                    .any(|forum_topic| forum_topic.id == thread_id)
                            {
                                added_media += 1;
                                res.push(media);
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

                if res.len() >= 50 {
                    let send = std::mem::take(&mut res);
                    let _ = tx.send(send).await;
                }
            }

            log::info!("Total Media Received : {}", total_media);
            log::info!("Total Media Filtered : {}", added_media);

            if !res.is_empty() {
                let _ = tx.send(res).await;
            }

            Ok::<(), Error>(())
        });

        Self {
            ctx,
            rx,
            selected_documents: HashSet::new(),
            documents: Vec::new(),
            list_state,
        }
    }
}

impl Controller for DownloadScreen {
    async fn handle_event(
        &mut self,
        event: &Event,
    ) -> anyhow::Result<Option<crate::app::StateWrapper>> {
        if let Event::Key(key) = event {
            let current = self.list_state.selected().unwrap_or(0);

            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    let next = if current == 0 {
                        self.documents.len().checked_sub(1).unwrap_or_default()
                    } else {
                        current - 1
                    };
                    self.list_state.select(Some(next));
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let next = if current >= self.documents.len().checked_sub(1).unwrap_or_default()
                    {
                        0
                    } else {
                        current + 1
                    };

                    self.list_state.select(Some(next));
                }
                KeyCode::Char('s') => {
                    if self.selected_documents.contains(&current) {
                        self.selected_documents.remove(&current);
                    } else {
                        self.selected_documents.insert(current);
                    }
                }
                KeyCode::Char('S') => {
                    if self.selected_documents.len() == self.documents.len() - 1 {
                        self.selected_documents.drain();
                    } else {
                        self.selected_documents = (0..=self.documents.len() - 1).collect();
                    }
                }
                KeyCode::Enter => {}
                _ => {}
            }
        }
        Ok(None)
    }
}

impl Screen for DownloadScreen {
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
        if self.documents.is_empty() {
            let str = format!("🔄 Loading Telegram Dialogs/Peers...\nPress [Esc] to exit.",);
            let left_pane = Paragraph::new(str).block(
                Block::default()
                    .title(" Dialogs ")
                    .borders(Borders::ALL)
                    .fg(Color::White),
            );
            f.render_widget(left_pane, workspace_chunks[0]);
        } else {
            let items: Vec<ListItem> = self
                .documents
                .iter()
                .enumerate()
                .map(|(index, forum_topic)| {
                    let name = match forum_topic {
                        Media::Document(d) => d.name().unwrap_or("Unknown").to_string(),
                        Media::Photo(p) => format!("photo_{}.jpg", p.id()),
                        _ => format!("unknown.file"),
                    };
                    let mut style = Style::default();
                    let mut prefix = "[ ]";
                    if self.selected_documents.contains(&index) {
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
        let footer_text = "Quit: [Ctrl+C] or [Esc] | Toggle View: [Tab] | Select: s";
        let footer = Paragraph::new(footer_text)
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Left)
            .block(Block::default().borders(Borders::ALL).fg(Color::DarkGray));
        f.render_widget(footer, chunks[2]);
    }
}

impl Tick for DownloadScreen {
    async fn tick(&mut self) -> anyhow::Result<()> {
        while let Ok(res) = self.rx.try_recv() {
            self.documents.extend(res);
        }
        Ok(())
    }
}
