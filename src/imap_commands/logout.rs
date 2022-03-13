use crate::{
    config::Config,
    imap_commands::{Command, CommandData, Data},
};
use async_trait::async_trait;
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::sync::Arc;
pub struct Logout<'a> {
    pub data: &'a Data,
}

#[async_trait]
impl<S> Command<S> for Logout<'_>
where
    S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
{
    async fn exec(
        &mut self,
        lines: &mut S,
        _config: Arc<Config>,
        command_data: &CommandData,
    ) -> color_eyre::eyre::Result<()> {
        lines
            .feed(String::from("* BYE IMAP4rev2 Server logging out"))
            .await?;
        lines
            .feed(format!("{} OK LOGOUT completed", command_data.tag))
            .await?;
        lines.flush().await?;
        Ok(())
    }
}
