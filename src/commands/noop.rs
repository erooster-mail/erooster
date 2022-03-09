use std::io;

use async_trait::async_trait;
use futures::{Sink, SinkExt};

use crate::{
    commands::{Command, Data},
    line_codec::LinesCodecError,
};

pub struct Noop<'a> {
    pub data: &'a Data<'a>,
}

#[async_trait]
impl<S> Command<S> for Noop<'_>
where
    S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
    S::Error: From<io::Error>,
{
    async fn exec(&mut self, lines: &mut S) -> anyhow::Result<()> {
        // TODO return status as suggested in https://www.rfc-editor.org/rfc/rfc9051.html#name-noop-command
        lines
            .send(format!(
                "{} OK NOOP completed",
                self.data.command_data.as_ref().unwrap().tag
            ))
            .await?;
        Ok(())
    }
}
