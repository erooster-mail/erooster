use crate::imap_commands::CommandData;
use futures::{channel::mpsc::SendError, Sink, SinkExt};
pub struct Capability;

impl Capability {
    pub async fn exec<S>(
        &mut self,
        lines: &mut S,
        command_data: &CommandData,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
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
