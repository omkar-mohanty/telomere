mod download;
mod error_screen;
mod file_selection;
mod forum_topic;
mod peer_selection;

use crate::app::{ContextThreadSafe, StateWrapper};
use anyhow::Result;
use download::*;
use file_selection::*;
use forum_topic::*;
use peer_selection::*;

use ratatui::{crossterm::event::Event, prelude::*, widgets::StatefulWidget};

pub trait Controller {
    type State;
    type Output;
    fn handle(&self, event: &Event, state: Self::State) -> Result<Self::Output>;
}

pub trait Tick {
    type State;
    async fn tick(&mut self, state: &mut Self::State) -> Result<()>;
}

pub struct TerminalUserInterface;

pub struct TerminalController;

enum TickerState {
    PeerSelection(PeerSelectionTicker),
    ForumTopicSelection(ForumTopicTicker),
    FileSelection(FileSelectionTicker),
    DownloadState(DownloadTicker),
}

pub struct TerminalTicker {
    ctx: ContextThreadSafe,
    ticker_state: TickerState,
}

impl TerminalTicker {
    pub fn new(ctx: ContextThreadSafe) -> Self {
        let ctx_clone = ctx.clone();
        Self {
            ctx,
            ticker_state: TickerState::PeerSelection(PeerSelectionTicker::new(ctx_clone)),
        }
    }
}

impl StatefulWidget for TerminalUserInterface {
    type State = StateWrapper;
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        match state {
            StateWrapper::PeerSelection(state) => {
                let f = PeerSelectionUI;
                f.render(area, buf, state);
            }
            StateWrapper::ForumTopicSelection(state) => {
                let f = ForumTopicUI;
                f.render(area, buf, state);
            }
            StateWrapper::FileSelection(state) => {
                let f = FileSelectionScreenUI;
                f.render(area, buf, state);
            }
            StateWrapper::Download(state) => {
                let f = DownloadUI;
                f.render(area, buf, state);
            }
            _ => todo!(),
        }
    }
}

impl Controller for TerminalController {
    type State = StateWrapper;
    type Output = StateWrapper;
    fn handle(&self, event: &Event, state: Self::State) -> Result<StateWrapper> {
        use StateWrapper::*;
        let state = match state {
            PeerSelection(state) => {
                let controller = PeerSelectionController;
                controller.handle(event, state)?
            }
            ForumTopicSelection(state) => {
                let controller = ForumTopicController;
                controller.handle(event, state)?
            }
            FileSelection(state) => {
                let controller = FileSelectionController;
                controller.handle(event, state)?
            }
            Download(state) => {
                let controller = DownloadContrller;
                controller.handle(event, state)?
            }
            _ => {
                todo!()
            }
        };

        Ok(state)
    }
}

impl Tick for TerminalTicker {
    type State = StateWrapper;
    async fn tick(&mut self, state: &mut Self::State) -> Result<()> {
        use StateWrapper::*;
        match state {
            PeerSelection(peer_selection) => match &mut self.ticker_state {
                TickerState::PeerSelection(peer_ticker) => peer_ticker.tick(peer_selection).await,
                _ => {
                    let mut ticker = PeerSelectionTicker::new(self.ctx.clone());
                    let res = ticker.tick(peer_selection).await;
                    self.ticker_state = TickerState::PeerSelection(ticker);
                    res
                }
            },
            ForumTopicSelection(state) => {
                if let TickerState::ForumTopicSelection(forum_ticker) = &mut self.ticker_state {
                    forum_ticker.tick(state).await
                } else {
                    let peer = state.dialog.peer_ref();

                    let mut ticker = ForumTopicTicker::new(self.ctx.clone(), peer);
                    let res = ticker.tick(state).await;
                    self.ticker_state = TickerState::ForumTopicSelection(ticker);

                    res
                }
            }
            FileSelection(state) => match &mut self.ticker_state {
                TickerState::FileSelection(file_ticker) => file_ticker.tick(state).await,
                _ => {
                    let peer_ref = state.dialog.peer_ref();

                    let mut ticker = FileSelectionTicker::new(
                        self.ctx.clone(),
                        peer_ref,
                        state.forum_topics.clone(),
                    );
                    let res = ticker.tick(state).await;
                    self.ticker_state = TickerState::FileSelection(ticker);

                    res
                }
            },
            Download(state) => match &mut self.ticker_state {
                TickerState::DownloadState(download_ticker) => download_ticker.tick(state).await,
                _ => {
                    let mut ticker = DownloadTicker::new(self.ctx.clone());
                    let res = ticker.tick(state).await;
                    self.ticker_state = TickerState::DownloadState(ticker);
                    res
                }
            },
            Error(_) => Ok(()),
            _ => {
                todo!()
            }
        }
    }
}
