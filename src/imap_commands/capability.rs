use crate::{
    config::Config,
    imap_commands::{Command, CommandData, Data},
};
use async_trait::async_trait;
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::sync::Arc;

pub struct Capability<'a> {
    pub data: &'a Data,
}

#[async_trait]
impl<S> Command<S> for Capability<'_>
where
    S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
{
    async fn exec(
        &mut self,
        lines: &mut S,
        _config: Arc<Config>,
        command_data: &CommandData,
    ) -> color_eyre::eyre::Result<()> {
        let capabilities = get_capabilities();
        lines.feed(format!("* {}", capabilities)).await?;
        lines
            .feed(format!("{} OK CAPABILITY completed", command_data.tag))
            .await?;
        lines.flush().await?;
        Ok(())
    }
}

pub const fn get_capabilities() -> &'static str {
    "CAPABILITY AUTH=PLAIN LOGINDISABLED IMAP4rev2"
}
