use crate::imap_commands::CommandData;
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use tracing::instrument;
pub struct Logout;

impl Logout {
    #[instrument(skip(self, lines, command_data))]
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
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
