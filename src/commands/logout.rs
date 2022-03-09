use std::io;

use async_trait::async_trait;
use futures::{Sink, SinkExt};

use crate::{
    commands::{Command, Data},
    line_codec::LinesCodecError,
};

pub struct Logout<'a> {
    pub data: &'a Data<'a>,
}

#[async_trait]
impl<S> Command<S> for Logout<'_>
where
    S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
    S::Error: From<io::Error>,
{
    async fn exec(&mut self, lines: &mut S) -> anyhow::Result<()> {
        lines
            .feed(String::from("* BYE IMAP4rev2 Server logging out"))
            .await?;
        lines
            .feed(format!(
                "{} OK LOGOUT completed",
                self.data.command_data.as_ref().unwrap().tag
            ))
            .await?;
        lines.flush().await?;
        Ok(())
    }
}
