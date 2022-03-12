use crate::{
    config::Config,
    imap_commands::{Command, Data},
};
use async_trait::async_trait;
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::sync::Arc;

pub struct Login<'a> {
    pub data: &'a Data,
}

#[async_trait]
impl<S> Command<S> for Login<'_>
where
    S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
{
    async fn exec(&mut self, lines: &mut S, _config: Arc<Config>) -> anyhow::Result<()> {
        lines
            .send(format!(
                "{} NO [PRIVACYREQUIRED] LOGIN COMMAND DISABLED FOR SECURITY. USE AUTH",
                self.data.command_data.as_ref().unwrap().tag
            ))
            .await?;
        Ok(())
    }
}
