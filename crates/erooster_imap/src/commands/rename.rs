use crate::commands::{CommandData, Data};
use color_eyre::eyre::ContextCompat;
use erooster_core::backend::storage::{MailStorage, Storage};
use futures::{Sink, SinkExt};
use tokio::fs;
use tracing::instrument;

pub struct Rename<'a> {
    pub data: &'a mut Data,
}

impl Rename<'_> {
    #[instrument(skip(self, lines, command_data, storage))]
    pub async fn exec<S, E>(
        &mut self,
        lines: &mut S,
        command_data: &CommandData<'_>,
        storage: &Storage,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        let args = &command_data.arguments;
        assert!(args.len() == 2);
        let old_folder = args[0].replace('/', ".");
        let old_mailbox_path = storage.to_ondisk_path(
            old_folder.clone(),
            self.data
                .con_state
                .username
                .clone()
                .context("Username missing in internal State")?,
        )?;
        let new_folder = args[1].replace('/', ".");
        let new_mailbox_path = storage.to_ondisk_path(
            new_folder.clone(),
            self.data
                .con_state
                .username
                .clone()
                .context("Username missing in internal State")?,
        )?;
        fs::rename(old_mailbox_path, new_mailbox_path).await?;
        lines
            .send(format!("{} OK RENAME completed", command_data.tag))
            .await?;
        Ok(())
    }
}
