use std::io;

use async_trait::async_trait;
use futures::{Sink, SinkExt};

use crate::{
    commands::{Command, Data},
    line_codec::LinesCodecError,
};

pub struct Capability<'a, 'b> {
    pub data: Data<'a, 'b>,
}

#[async_trait]
impl<S> Command<S> for Capability<'_, '_>
where
    S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
    S::Error: From<io::Error>,
{
    async fn exec(&mut self, lines: &'async_trait mut S) -> anyhow::Result<()> {
        let capabilities = get_capabilities();
        lines.feed(format!("* {}", capabilities)).await?;
        lines
            .feed(format!("{} OK CAPABILITY completed", self.data.tag))
            .await?;
        lines.flush().await?;
        Ok(())
    }
}

pub fn get_capabilities() -> String {
    String::from("CAPABILITY AUTH=PLAIN LOGINDISABLED IMAP4rev2")
}
