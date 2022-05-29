use std::sync::Arc;
use crate::{config::Config, imap_commands::CommandData};
use futures::{channel::mpsc::SendError, Sink, SinkExt};

pub struct Uid;

impl Uid {
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        command_data: &CommandData<'_>,
        _config: Arc<Config>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        assert!(command_data.arguments.len() <= 4);
        if command_data.arguments[0].to_lowercase() == "fetch" {
            // TODO make sure we are in selected state
            // TODO open mailbox
            // TODO fetch all the right mails
            // TODO handle the various request types defined in https://www.rfc-editor.org/rfc/rfc9051.html#name-fetch-command
            // TODO handle * as "everything"
            // TODO make this code also available to the pure FETCH command
            lines
                .feed(format!("{} Ok UID FETCH completed", command_data.tag))
                .await?;
            lines.flush().await?;
        } else if command_data.arguments[0].to_lowercase() == "copy" {
            // TODO implement other commands
            lines
                .send(format!("{} BAD Not supported", command_data.tag))
                .await?;
        } else if command_data.arguments[0].to_lowercase() == "move" {
            lines
                .send(format!("{} BAD Not supported", command_data.tag))
                .await?;
        } else if command_data.arguments[0].to_lowercase() == "expunge" {
            lines
                .send(format!("{} BAD Not supported", command_data.tag))
                .await?;
        } else if command_data.arguments[0].to_lowercase() == "search" {
            lines
                .send(format!("{} BAD Not supported", command_data.tag))
                .await?;
        }
        Ok(())
    }
}
