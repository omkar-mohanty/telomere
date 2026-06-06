use crate::app::{Application, AuthScreen, CurrentScreen};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Position},
    style::{Color, Modifier, Style, Stylize},
    widgets::{Block, Borders, Paragraph, Wrap},
};

/// Main entry point for rendering called by `terminal.draw`
pub fn draw(f: &mut Frame, app: &Application) {
    match app.current_screen {
        CurrentScreen::Auth(ref auth_screen) => {
            draw_auth_screen(f, app, auth_screen);
        }
        CurrentScreen::Main => {
            draw_main_dashboard(f);
        }
    }
}

/// Renders the login/authentication flows centered on the screen
fn draw_auth_screen(f: &mut Frame, app: &Application, screen: &AuthScreen) {
    // Create a centered area for the authentication card
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Length(10), // Box height
            Constraint::Percentage(30),
        ])
        .split(f.area());

    let center_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(60), // Box width
            Constraint::Percentage(20),
        ])
        .split(chunks[1]);

    let area = center_chunks[1];

    // Determine instructional text and title based on the active enum variant
    let (title, instruction) = match screen {
        AuthScreen::PhoneNumber => (
            " Telegram Authentication: Phone Number ",
            "Type your phone number (international format, e.g., +1234567890) and hit [Enter]:",
        ),
        AuthScreen::LoginCode => (
            " Telegram Authentication: Verification Code ",
            "Type the verification code sent to your Telegram app and hit [Enter]:",
        ),
    };

    let auth_block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Cyan));

    // Nested input layout inside the auth card box
    let inner_chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([Constraint::Length(2), Constraint::Length(3)])
        .split(area);

    let instruction_p = Paragraph::new(instruction)
        .style(Style::default().fg(Color::Gray))
        .wrap(Wrap { trim: true });

    // Render whatever the user is actively typing from our input buffer
    let input_p = Paragraph::new(app.input_buffer.as_str())
        .block(Block::default().borders(Borders::ALL).fg(Color::Yellow));

    f.render_widget(auth_block, area);
    f.render_widget(instruction_p, inner_chunks[0]);
    f.render_widget(input_p, inner_chunks[1]);

    // Position the terminal cursor at the end of the text input box
    f.set_cursor_position(Position::new(
        inner_chunks[1].x + 1 + app.input_buffer.len() as u16,
        inner_chunks[1].y + 1,
    ));
}

/// Renders your core functional layout after validation checks pass
fn draw_main_dashboard(f: &mut Frame) {
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

    // Dummy placeholders to visualize your running dashboard layout
    let left_pane = Paragraph::new("🔄 Loading Telegram Dialogs/Peers...\nPress [Esc] to exit.")
        .block(
            Block::default()
                .title(" Dialogs ")
                .borders(Borders::ALL)
                .fg(Color::White),
        );

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

    f.render_widget(left_pane, workspace_chunks[0]);
    f.render_widget(right_pane, workspace_chunks[1]);

    // 3. Footer Widget
    let footer_text = "Quit: [Ctrl+C] or [Esc] | Toggle View: [Tab]";
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::Gray))
        .alignment(Alignment::Left)
        .block(Block::default().borders(Borders::ALL).fg(Color::DarkGray));
    f.render_widget(footer, chunks[2]);
}
