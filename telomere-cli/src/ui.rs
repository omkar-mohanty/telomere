use crate::app::{AuthPhoneNumber, AuthState, Context, StateMachine, StateWrapper};
use anyhow::Result;
use grammers_client::client::LoginToken;
use grammers_session::types::PeerRef;
use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode},
    layout::{Alignment, Constraint, Direction, Layout, Position},
    style::{Color, Modifier, Style, Stylize},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use std::sync::Arc;

pub trait Screen {
    fn draw(&self, f: &mut Frame);
}

pub trait Controller {
    async fn handle_event(&mut self, event: &Event) -> Result<Option<StateWrapper>>;
}

impl Screen for CurrentScreen {
    fn draw(&self, f: &mut Frame) {
        use CurrentScreen::*;
        match self {
            PeerSelectionScreen(page) => page.draw(f),
            AuthScreen(page) => page.draw(f),
        }
    }
}

impl Controller for CurrentScreen {
    async fn handle_event(&mut self, event: &Event) -> Result<Option<StateWrapper>> {
        use CurrentScreen::*;
        match self {
            PeerSelectionScreen(page) => page.handle_event(&event).await,
            AuthScreen(page) => page.handle_event(&event).await,
        }
    }
}

pub enum CurrentScreen {
    PeerSelectionScreen(PeerSelectionScreen),
    AuthScreen(AuthScreen),
}

impl CurrentScreen {
    pub async fn new(ctx: Arc<Context>) -> Result<Self> {
        if ctx.client.is_authorized().await? {
            return Ok(Self::PeerSelectionScreen(PeerSelectionScreen::new(ctx)));
        }

        Ok(Self::AuthScreen(AuthScreen::PhoneNumber(
            PhoneNumberScreen::new(ctx),
        )))
    }
}

pub enum AuthScreen {
    PhoneNumber(PhoneNumberScreen),
    LoginCode(LoginScreen),
}

impl Screen for AuthScreen {
    fn draw(&self, f: &mut Frame) {
        use AuthScreen::*;
        match self {
            PhoneNumber(page) => page.draw(f),
            LoginCode(page) => page.draw(f),
        }
    }
}

impl Controller for AuthScreen {
    async fn handle_event(&mut self, event: &Event) -> Result<Option<StateWrapper>> {
        use AuthScreen::*;
        match self {
            PhoneNumber(page) => page.handle_event(event).await,
            LoginCode(page) => page.handle_event(event).await,
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

        // Dummy placeholders to visualize your running dashboard layout
        let left_pane =
            Paragraph::new("🔄 Loading Telegram Dialogs/Peers...\nPress [Esc] to exit.").block(
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
}

impl Controller for PeerSelectionScreen {
    async fn handle_event(&mut self, event: &Event) -> Result<Option<StateWrapper>> {
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {}
                KeyCode::Down | KeyCode::Char('j') => {}
                _ => {}
            }
        }
        Ok(None)
    }
}

pub struct PeerSelectionScreen {
    pub ctx: Arc<Context>,
    pub selected_peer: usize,
    pub peers: Vec<PeerRef>,
}

impl PeerSelectionScreen {
    pub fn new(ctx: Arc<Context>) -> Self {
        Self {
            ctx,
            selected_peer: 0,
            peers: Vec::new(),
        }
    }
}

pub struct LoginScreen {
    pub input_phone_number: String,
    login_token: Option<LoginToken>,
    ctx: Arc<Context>,
}

impl LoginScreen {
    pub fn new(ctx: Arc<Context>) -> Self {
        Self {
            input_phone_number: String::new(),
            ctx,
            login_token: None,
        }
    }
}

impl Controller for LoginScreen {
    async fn handle_event(&mut self, event: &Event) -> Result<Option<StateWrapper>> {
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Enter => {
                    let token = self.ctx.init_auth(&self.input_phone_number).await?;
                    self.login_token = token;
                    let phone_number_state = AuthPhoneNumber {
                        phone: self.input_phone_number.clone(),
                    };
                    let state = StateMachine {
                        ctx: self.ctx.clone(),
                        state: phone_number_state,
                    };
                    return Ok(Some(StateWrapper::Auth(AuthState::PhoneNumber(state))));
                }
                KeyCode::Backspace => {
                    if !self.input_phone_number.is_empty() {
                        self.input_phone_number.pop();
                    }
                }
                KeyCode::Char(c) => self.input_phone_number.push(c),
                _ => {}
            }
        }
        Ok(None)
    }
}

impl Screen for LoginScreen {
    fn draw(&self, f: &mut Frame) {
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
        let title = "Telegram Authentication: Phone Number";
        let instruction =
            "Type your phone number (international format, e.g., +1234567890) and hit [Enter]:";

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
        let input_p = Paragraph::new(self.input_phone_number.as_str())
            .block(Block::default().borders(Borders::ALL).fg(Color::Yellow));

        f.render_widget(auth_block, area);
        f.render_widget(instruction_p, inner_chunks[0]);
        f.render_widget(input_p, inner_chunks[1]);

        // Position the terminal cursor at the end of the text input box
        f.set_cursor_position(Position::new(
            inner_chunks[1].x + 1 + self.input_phone_number.len() as u16,
            inner_chunks[1].y + 1,
        ));
    }
}
pub struct PhoneNumberScreen {
    pub input_phone_number: String,
    login_token: Option<LoginToken>,
    app: Arc<Context>,
}

impl PhoneNumberScreen {
    pub fn new(app: Arc<Context>) -> Self {
        Self {
            input_phone_number: String::new(),
            app,
            login_token: None,
        }
    }
}

impl Controller for PhoneNumberScreen {
    async fn handle_event(&mut self, event: &Event) -> Result<Option<StateWrapper>> {
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Enter => {
                    let token = self.app.init_auth(&self.input_phone_number).await?;
                    self.login_token = token;
                }
                KeyCode::Backspace => {
                    if !self.input_phone_number.is_empty() {
                        self.input_phone_number.pop();
                    }
                }
                KeyCode::Char(c) => self.input_phone_number.push(c),
                _ => {}
            }
        }
        Ok(None)
    }
}

impl Screen for PhoneNumberScreen {
    fn draw(&self, f: &mut Frame) {
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
        let title = "Telegram Authentication: Phone Number";
        let instruction =
            "Type your phone number (international format, e.g., +1234567890) and hit [Enter]:";

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
        let input_p = Paragraph::new(self.input_phone_number.as_str())
            .block(Block::default().borders(Borders::ALL).fg(Color::Yellow));

        f.render_widget(auth_block, area);
        f.render_widget(instruction_p, inner_chunks[0]);
        f.render_widget(input_p, inner_chunks[1]);

        // Position the terminal cursor at the end of the text input box
        f.set_cursor_position(Position::new(
            inner_chunks[1].x + 1 + self.input_phone_number.len() as u16,
            inner_chunks[1].y + 1,
        ));
    }
}
