use crate::{
    app::{StateMachine, StateWrapper},
    ui::Controller,
};
use anyhow::Error;
use ratatui::{
    crossterm::event::{Event, KeyCode},
    prelude::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, StatefulWidget, Widget, Wrap},
};

pub struct ErrorUI;
pub struct ErrorController;

impl StatefulWidget for ErrorUI {
    type State = StateMachine<Error>;

    fn render(
        self,
        area: ratatui::prelude::Rect,
        buf: &mut ratatui::prelude::Buffer,
        state: &mut Self::State,
    ) {
        // 1. Clear the area so previous frames don't bleed through the pop-up
        let clear = Clear;
        clear.render(area, buf);
        // 2. Center the error modal on the screen using a layout helper
        let modal_area = centered_rect(60, 40, area);
        // (Adjust this depending on how your StateMachine exposes the inner Error)
        let error_msg = format!("{}", state.to_string());

        // Collect full error chain if causes exist
        let text = vec![
            Line::from(vec![
                Span::styled(
                    "❌ Error: ",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::raw(&error_msg),
            ]),
            Line::from(""), // Spacer
        ];

        // 4. Build the modal block decoration
        let block = Block::bordered()
            .title(" Critical Error ")
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(Color::Red))
            .bg(Color::Black);

        // 5. Add instructions at the bottom of the modal
        let footer = Paragraph::new("Press [Esc] or [Enter] to return")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray).italic());

        // Split the modal area into content and footer
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .margin(1)
            .split(modal_area);

        // Render content paragraph with text wrapping
        Paragraph::new(text)
            .wrap(Wrap { trim: true })
            .block(block)
            .render(chunks[0], buf); // Passes block ownership to frame borders

        footer.render(chunks[1], buf);
    }
}

impl Controller for ErrorController {
    type State = StateMachine<Error>;
    type Output = StateWrapper;

    fn handle(
        &self,
        event: &ratatui::crossterm::event::Event,
        state: Self::State,
    ) -> anyhow::Result<Self::Output> {
        if let Event::Key(key_event) = event {
            match key_event.code {
                KeyCode::Enter => return Ok(StateWrapper::default()),
                _ => {}
            }
        }
        Ok(StateWrapper::from(state))
    }
}

/// Helper function to create a centered rect layout for popup modals
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
