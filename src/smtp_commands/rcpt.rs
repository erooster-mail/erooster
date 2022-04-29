use futures::{channel::mpsc::SendError, Sink, SinkExt};
use tracing::info;

use crate::smtp_commands::CommandData;

pub struct Rcpt;

impl Rcpt {
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        info!("{:#?}", command_data.arguments);
        lines.send(String::from("250 OK")).await?;
        Ok(())
    }
}
