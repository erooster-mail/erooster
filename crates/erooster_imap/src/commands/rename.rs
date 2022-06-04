use crate::commands::{CommandData, Data};
use erooster_core::backend::storage::{MailStorage, Storage};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::sync::Arc;
use tokio::fs;
use tracing::instrument;

pub struct Rename<'a> {
    pub data: &'a Data,
}

impl Rename<'_> {
    #[instrument(skip(self, lines, command_data, storage))]
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        command_data: &CommandData<'_>,
        storage: Arc<Storage>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        let args = &command_data.arguments;
        assert!(args.len() == 2);
        let old_folder = args[0].replace('/', ".");
        let old_mailbox_path = storage.to_ondisk_path(
            old_folder.clone(),
            self.data.con_state.read().await.username.clone().unwrap(),
        )?;
        let new_folder = args[1].replace('/', ".");
        let new_mailbox_path = storage.to_ondisk_path(
            new_folder.clone(),
            self.data.con_state.read().await.username.clone().unwrap(),
        )?;
        fs::rename(old_mailbox_path, new_mailbox_path).await?;
        lines
            .send(format!("{} OK RENAME completed", command_data.tag))
            .await?;
        Ok(())
    }
}
