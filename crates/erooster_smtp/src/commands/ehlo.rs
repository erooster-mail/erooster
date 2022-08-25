use futures::{channel::mpsc::SendError, Sink, SinkExt};
use tracing::instrument;

use crate::commands::Data;

pub struct Ehlo<'a> {
    pub data: &'a Data,
}

impl Ehlo<'_> {
    #[instrument(skip(self, hostname, lines))]
    pub async fn exec<S>(&self, hostname: String, lines: &mut S) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        lines.feed(format!("250-{}", hostname)).await?;
        lines.feed(String::from("250-ENHANCEDSTATUSCODES")).await?;
        lines.feed(String::from("250-STARTTLS")).await?;
        if self.data.con_state.read().await.secure {
            lines.feed(String::from("250-SMTPUTF8")).await?;
            lines.feed(String::from("250 AUTH LOGIN PLAIN")).await?;
        } else {
            lines.feed(String::from("250 SMTPUTF8")).await?;
        }
        lines.flush().await?;
        Ok(())
    }
}
