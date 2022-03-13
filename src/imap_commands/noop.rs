use crate::{
    config::Config,
    imap_commands::{Command, Data},
};
use async_trait::async_trait;
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::sync::Arc;

pub struct Noop<'a> {
    pub data: &'a Data,
}

#[async_trait]
impl<S> Command<S> for Noop<'_>
where
    S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
{
    async fn exec(&mut self, lines: &mut S, _config: Arc<Config>) -> color_eyre::eyre::Result<()> {
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
