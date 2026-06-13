use crate::{
    app::{ContextThreadSafe, PhoneNumberAuthentication, StateMachine},
    ui::Tick,
};
use anyhow::{Error, Result};

pub struct AuthTick {
    ctx: ContextThreadSafe,
}

impl Tick for AuthTick {
    type State = StateMachine<PhoneNumberAuthentication>;
    async fn tick(&mut self, state: &mut Self::State) -> Result<()> {
        todo!()
    }
}
