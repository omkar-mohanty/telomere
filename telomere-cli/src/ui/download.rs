use crate::app::{
    ContextThreadSafe, DownloadState, DownloadableFile, FileEntry, FileStatus, StateMachine,
    StateWrapper,
};
use crate::ui::{Controller, Tick};

use ratatui::widgets::StatefulWidget;
use ratatui::{
    crossterm::event::{Event, KeyCode},
    prelude::*,
    style::{Color, Modifier, Style, Stylize},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use telomere_core::downloader::{DownloadEvent, DownloadTask};
use tokio::sync::mpsc::error::TryRecvError;
pub struct DownloadUI;
pub struct DownloadTicker {
    ctx: ContextThreadSafe,
}

impl DownloadTicker {
    pub fn new(ctx: ContextThreadSafe) -> Self {
        Self { ctx }
    }
}

pub struct DownloadContrller;

impl StatefulWidget for DownloadUI {
    type State = StateMachine<DownloadState>;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        // 1. Define the layout: A top bar/header, and the main download list area.
        let chunks = Layout::vertical([
            Constraint::Length(3), // Header block
            Constraint::Min(0),    // Active downloads list
        ])
        .split(area);

        // 2. Render Header/Summary Panel
        let total_files = state.queued_downloads.len();
        let finished_files = state
            .queued_downloads
            .values()
            .filter(|f| matches!(f.file_status, FileStatus::Finished))
            .count();

        let header_text = format!(
            " 📥 Downloads Overview | Done: {}/{} ",
            finished_files, total_files
        );
        let header_block = Block::default()
            .borders(Borders::ALL)
            .title(header_text)
            .border_style(Style::default().fg(Color::Cyan));

        Paragraph::new("Press Esc to go back or manage settings.")
            .block(header_block)
            .alignment(Alignment::Center)
            .render(chunks[0], buf);

        // 3. Render Downloads List
        if state.queued_downloads.is_empty() {
            let empty_block = Block::default()
                .borders(Borders::ALL)
                .title(" Active Tasks ");
            Paragraph::new("No active or queued downloads.")
                .block(empty_block)
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray))
                .render(chunks[1], buf);
            return;
        }

        // Calculate a list box area
        let main_block = Block::default()
            .borders(Borders::ALL)
            .title(" Active Tasks ");
        let list_inner_area = main_block.inner(chunks[1]);
        main_block.render(chunks[1], buf);

        // Split the list inner area into vertical rows for each download entry
        // Each download entry will get 3 rows: 1 for filename/status, 1 for the Gauge, 1 for spacer
        let mut download_items_layout = Layout::vertical(
            std::iter::repeat(Constraint::Length(3))
                .take(state.queued_downloads.len())
                .collect::<Vec<_>>(),
        );

        // Ensure we don't crash if the constraints exceed area, clamp it using ratatui constraints
        let rows = download_items_layout.split(list_inner_area);

        for (idx, (id, file_entry)) in state.queued_downloads.iter().enumerate() {
            if idx >= rows.len() {
                break; // Screen is full, stop rendering further items
            }

            let row_area = rows[idx];

            // Sub-divide the single item row into: label info (line 1) and gauge (line 2)
            let item_chunks = Layout::vertical([
                Constraint::Length(1), // Filename & status text
                Constraint::Length(1), // Gauge
                Constraint::Length(1), // Padding space
            ])
            .split(row_area);

            // Determine status text details and styling colors
            let (status_str, status_color) = match &file_entry.file_status {
                FileStatus::Finished => ("FINISHED".to_string(), Color::Green),
                FileStatus::Error(e) => (format!("ERROR: {}", e), Color::Red),
                _ => ("DOWNLOADING".to_string(), Color::Blue), // Assuming default/running variants
            };

            // Build item metadata label
            let progress_mb = file_entry.total_downloaded as f64 / 1_048_576.0;
            let total_mb = file_entry.total_size as f64 / 1_048_576.0;

            let label_text = Line::from(vec![
                Span::styled(
                    format!(" Row #{}: ", idx + 1),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    &file_entry.filename,
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!(" ({:.2} MB / {:.2} MB) ", progress_mb, total_mb)),
                Span::styled(
                    format!("[{}]", status_str),
                    Style::default().fg(status_color),
                ),
            ]);

            Paragraph::new(label_text).render(item_chunks[0], buf);

            // Calculate exact ratio safely for the Gauge widget
            let ratio = if file_entry.total_size > 0 {
                (file_entry.total_downloaded as f64 / file_entry.total_size as f64).clamp(0.0, 1.0)
            } else {
                0.0
            };

            // Render progress gauge bar
            let gauge = ratatui::widgets::Gauge::default()
                .block(Block::default())
                .gauge_style(
                    Style::default()
                        .fg(status_color)
                        .bg(Color::Indexed(236)) // Subdued dark gray background
                        .add_modifier(Modifier::ITALIC),
                )
                .ratio(ratio)
                .label(format!("{:.1}%", ratio * 100.0));

            gauge.render(item_chunks[1], buf);
        }
    }
}

impl Controller for DownloadContrller {
    type State = StateMachine<DownloadState>;
    type Output = StateWrapper;

    fn handle(&self, event: &Event, mut state: Self::State) -> anyhow::Result<Self::Output> {
        if let Event::Key(key_event) = event {
            match key_event.code {
                KeyCode::Char('C') => {
                    let finished_ids: Vec<_> = state
                        .queued_downloads
                        .iter()
                        .filter(|(_, file_entry)| {
                            let file_status = &file_entry.file_status;
                            matches!(file_status, FileStatus::Finished)
                        })
                        .map(|(id, _)| id.to_owned())
                        .collect();
                    for id in finished_ids {
                        state.queued_downloads.remove_entry(&id);
                    }
                }
                _ => {}
            }
        }
        Ok(StateWrapper::from(state))
    }
}

impl Tick for DownloadTicker {
    type State = StateMachine<DownloadState>;

    async fn tick(&mut self, state: &mut Self::State) -> anyhow::Result<()> {
        if !state.files.is_empty() {
            if let Ok(mut ctx) = self.ctx.try_write() {
                let files = std::mem::take(&mut state.files);

                for file in files {
                    let DownloadableFile {
                        media,
                        filename,
                        size,
                        id,
                    } = file;

                    let retries = ctx.config.retries;
                    let filepath = ctx.config.output.join(&filename);

                    let rx = ctx.downloader.enqueue_task(DownloadTask {
                        id,
                        media,
                        filename: filename.clone(),
                        retries,
                        filepath,
                    });

                    let file_entry = FileEntry {
                        filename,
                        total_size: size,
                        total_downloaded: 0,
                        rx,
                        file_status: FileStatus::default(),
                    };

                    state.queued_downloads.insert(id, file_entry);
                }
            }
        }

        for (_id, file_entry) in &mut state.queued_downloads {
            match file_entry.rx.try_recv() {
                Ok(DownloadEvent::Progress(chunk)) => {
                    file_entry.total_downloaded += chunk;
                    file_entry.file_status = FileStatus::InProgress;
                }
                Ok(DownloadEvent::Error(e)) => {
                    file_entry.file_status = FileStatus::Error(e);
                }
                Ok(DownloadEvent::Finished) => {
                    file_entry.file_status = FileStatus::Finished;
                }
                Err(TryRecvError::Disconnected) => {
                    if let FileStatus::InProgress = file_entry.file_status {
                        file_entry.file_status =
                            FileStatus::Error(anyhow::Error::msg("Channel is disconnected"));
                    }
                }
                Err(TryRecvError::Empty) => {}
            }
        }

        Ok(())
    }
}
