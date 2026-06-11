mod file_selection;
mod forum_topic;
mod peer_selection;

use crate::app::{Context, ForumTopicSelection, StateWrapper};
use anyhow::Result;
use forum_topic::*;
use peer_selection::*;
use std::sync::Arc;

use ratatui::{crossterm::event::Event, prelude::*, widgets::StatefulWidget};

pub trait Controller {
    type State;
    type Output;
    fn handle(&self, event: &Event, state: &mut Self::State) -> Result<Option<Self::Output>>;
}

pub trait Tick {
    type State;
    async fn tick(&mut self, state: &mut Self::State) -> Result<()>;
}

pub struct TerminalUserInterface {
    ctx: Arc<Context>,
}

impl TerminalUserInterface {
    pub fn new(ctx: Arc<Context>) -> Self {
        Self { ctx }
    }
}

pub struct TerminalController;

enum TickerState {
    PeerSelection(PeerSelectionTicker),
    ForumTopicSelection(ForumTopicTicker),
}

pub struct TerminalTicker {
    ctx: Arc<Context>,
    ticker_state: TickerState,
}

impl TerminalTicker {
    pub fn new(ctx: Arc<Context>) -> Self {
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
            _ => todo!(),
        }
    }
}

impl Controller for TerminalController {
    type State = StateWrapper;
    type Output = StateWrapper;
    fn handle(&self, event: &Event, state: &mut Self::State) -> Result<Option<StateWrapper>> {
        use StateWrapper::*;
        let state = match state {
            PeerSelection(state) => {
                let controller = PeerSelectionController;
                let state = controller.handle(event, state)?;

                match state {
                    Some(state) => Some(state.try_into()?),
                    None => None,
                }
            }
            ForumTopicSelection(state) => {
                let controller = ForumTopicController;
                let state = controller.handle(event, state)?;

                match state {
                    Some(state) => Some(state.try_into()?),
                    None => None,
                }
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
                TickerState::PeerSelection(peer_ticker) => {
                    peer_ticker.tick(&mut peer_selection.0).await
                }
                _ => {
                    let mut ticker = PeerSelectionTicker::new(self.ctx.clone());
                    let res = ticker.tick(&mut peer_selection.0).await;
                    self.ticker_state = TickerState::PeerSelection(ticker);
                    res
                }
            },
            ForumTopicSelection(state) => {
                if let TickerState::ForumTopicSelection(forum_ticker) = &mut self.ticker_state {
                    forum_ticker.tick(state).await
                } else {
                    let peer = state.0.dialog.peer().to_ref().await;

                    match peer {
                        Some(peer) => {
                            let mut ticker = ForumTopicTicker::new(self.ctx.clone(), peer);
                            let res = ticker.tick(state).await;
                            self.ticker_state = TickerState::ForumTopicSelection(ticker);

                            res
                        }
                        None => {
                            anyhow::bail!("None Peer received")
                        }
                    }
                }
            }
            _ => {
                todo!()
            }
        }
    }
}
