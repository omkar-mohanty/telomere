use crate::app::{Context, ForumTopicSelection, StateMachine, StateWrapper};
use crate::ui::{Controller, Screen, Tick};
use anyhow::{Error, Result};
use grammers_client::peer::Dialog;
use ratatui::widgets::ListState;
use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode},
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style, Stylize},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};
use std::sync::Arc;
use tokio::task::JoinSet;

type PeerSelectionResult = Result<Vec<Dialog>, Error>;

pub struct PeerSelectionScreen {
    pub ctx: Arc<Context>,
    pub list_state: ListState,
    pub dialogs: Vec<Dialog>,
    pub join_set: JoinSet<PeerSelectionResult>,
}

impl PeerSelectionScreen {
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

            Ok::<Vec<Dialog>, anyhow::Error>(dialogs)
        });

        Self {
            ctx,
            list_state,
            dialogs,
            join_set,
        }
    }
}

impl Screen for PeerSelectionScreen {
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
        if self.dialogs.is_empty() {
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

impl Tick for PeerSelectionScreen {
    async fn tick(&mut self) -> Result<()> {
        while let Some(joined_task) = self.join_set.try_join_next() {
            let dialogs = joined_task??;
            self.dialogs.extend(dialogs);
        }

        Ok(())
    }
}

impl Controller for PeerSelectionScreen {
    async fn handle_event(&mut self, event: &Event) -> Result<Option<StateWrapper>> {
        if let Event::Key(key) = event {
            if self.dialogs.is_empty() {
                return Ok(None);
            }

            let current = self.list_state.selected().unwrap_or(0);

            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    let next = if current == 0 {
                        self.dialogs.len() - 1
                    } else {
                        current - 1
                    };

                    self.list_state.select(Some(next));
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let next = if current >= self.dialogs.len() - 1 {
                        0
                    } else {
                        current + 1
                    };

                    self.list_state.select(Some(next));
                }
                KeyCode::Enter => {
                    let dialog = self.dialogs.get(current).unwrap();
                    let peer_ref = dialog.peer_ref();
                    todo!()
                    return Ok(Some(StateWrapper::ForumTopicSelection(StateMachine(
                        ForumTopicSelection { peer_ref },
                    ))));
                }
                _ => {}
            }
        }
        Ok(None)
    }
}
