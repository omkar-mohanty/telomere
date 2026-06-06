use crate::app::{AuthPhoneNumber, AuthState, Context, StateMachine, StateWrapper};
use crate::ui::{Controller, Screen};
use anyhow::Result;
use grammers_client::client::LoginToken;
use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode},
    layout::{Constraint, Direction, Layout, Position},
    style::{Color, Style, Stylize},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use std::sync::Arc;
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
