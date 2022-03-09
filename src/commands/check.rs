use std::io;

use async_trait::async_trait;
use futures::{Sink, SinkExt};

use crate::{
    commands::{Command, Data},
    line_codec::LinesCodecError,
    state::State,
};

pub struct Check<'a> {
    pub data: &'a Data<'a>,
}

#[async_trait]
impl<S> Command<S> for Check<'_>
where
    S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
    S::Error: From<io::Error>,
{
    async fn exec(&mut self, lines: &mut S) -> anyhow::Result<()> {
        // This is an Imap4rev1 feature. It does the same as Noop for us as we have no memory gc.
        // It also only is allowed in selected state
        if matches!(self.data.con_state.state, State::Selected(_)) {
            lines
                .send(format!(
                    "{} OK CHECK completed",
                    self.data.command_data.as_ref().unwrap().tag
                ))
                .await?;
        } else {
            lines
                .send(format!(
                    "{} NO invalid state",
                    self.data.command_data.as_ref().unwrap().tag
                ))
                .await?;
        }
        Ok(())
    }
}
