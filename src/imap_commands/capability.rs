use std::{io, sync::Arc};

use async_trait::async_trait;
use futures::{Sink, SinkExt};

use crate::{
    imap_commands::{Command, Data},
    config::Config,
    line_codec::LinesCodecError,
};

pub struct Capability<'a> {
    pub data: &'a Data<'a>,
}

#[async_trait]
impl<S> Command<S> for Capability<'_>
where
    S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
    S::Error: From<io::Error>,
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
