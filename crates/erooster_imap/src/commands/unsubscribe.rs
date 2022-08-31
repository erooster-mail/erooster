use crate::commands::{CommandData, Data};
use color_eyre::eyre::ContextCompat;
use erooster_core::backend::storage::{MailStorage, Storage};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::sync::Arc;
use tracing::instrument;

pub struct Unsubscribe<'a> {
    pub data: &'a Data,
}

impl Unsubscribe<'_> {
    #[instrument(skip(self, lines, storage, command_data))]
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        storage: Arc<Storage>,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        let arguments = &command_data.arguments;
        assert!(arguments.len() == 1);
        if arguments.len() == 1 {
            let folder = arguments[0].replace('/', ".");
            let mailbox_path = storage.to_ondisk_path(
                folder.clone(),
                self.data
                    .con_state
                    .read()
                    .await
                    .username
                    .clone()
                    .context("Username missing in internal State")?,
            )?;
            // Note we deviate from spec here and actually do this automatically. So we can just return OK here.
            if !mailbox_path.exists() {
                lines
                    .send(format!("{} OK UNSUBSCRIBE completed", command_data.tag))
                    .await?;
                return Ok(());
            }
            storage.remove_flag(&mailbox_path, "\\Subscribed").await?;
            lines
                .send(format!("{} OK UNSUBSCRIBE completed", command_data.tag))
                .await?;
        } else {
            lines
                .send(format!(
                    "{} BAD [SERVERBUG] invalid arguments",
                    command_data.tag
                ))
                .await?;
        }
        Ok(())
    }
}
