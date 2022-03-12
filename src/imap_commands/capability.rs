use std::{ sync::Arc};
use async_trait::async_trait;
use futures::{Sink, SinkExt, channel::mpsc::SendError};
use crate::{
    imap_commands::{Command, Data},
    config::Config,
};

pub struct Capability<'a> {
    pub data: &'a Data,
}

#[async_trait]
impl<S> Command<S> for Capability<'_>
where
    S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
{
    async fn exec(&mut self, lines: &mut S, _config: Arc<Config>) -> anyhow::Result<()> {
        let capabilities = get_capabilities();
        lines.feed(format!("* {}", capabilities)).await?;
        lines
            .feed(format!(
                "{} OK CAPABILITY completed",
                self.data.command_data.as_ref().unwrap().tag
            ))
            .await?;
        lines.flush().await?;
        Ok(())
    }
}

pub fn get_capabilities() -> String {
    String::from("CAPABILITY AUTH=PLAIN LOGINDISABLED IMAP4rev2")
}
