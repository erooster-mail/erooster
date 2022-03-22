use crate::imap_commands::CommandData;
use futures::{channel::mpsc::SendError, Sink, SinkExt};

pub struct Login;

impl Login {
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        command_data: &CommandData,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        lines
            .send(format!(
                "{} NO [PRIVACYREQUIRED] LOGIN COMMAND DISABLED FOR SECURITY. USE AUTH",
                command_data.tag
            ))
            .await?;
        Ok(())
    }
}
