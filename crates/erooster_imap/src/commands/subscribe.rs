use crate::commands::{CommandData, Data};
use color_eyre::eyre::ContextCompat;
use erooster_core::backend::storage::{MailStorage, Storage};
use futures::{Sink, SinkExt};
use std::sync::Arc;
use tracing::{debug, instrument};

pub struct Subscribe<'a> {
    pub data: &'a Data,
}

impl Subscribe<'_> {
    #[instrument(skip(self, lines, storage, command_data))]
    pub async fn exec<S, E>(
        &self,
        lines: &mut S,
        storage: Arc<Storage>,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
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

            // This is a spec violation. However we need to do this currently due to how the storage is set up
            debug!("mailbox_path: {:?}", &mailbox_path);
            if !mailbox_path.exists() {
                storage.create_dirs(&mailbox_path)?;
                if folder.to_lowercase() == ".sent" {
                    storage.add_flag(&mailbox_path, "\\Sent").await?;
                } else if folder.to_lowercase() == ".junk" {
                    storage.add_flag(&mailbox_path, "\\Junk").await?;
                } else if folder.to_lowercase() == ".drafts" {
                    storage.add_flag(&mailbox_path, "\\Drafts").await?;
                } else if folder.to_lowercase() == ".archive" {
                    storage.add_flag(&mailbox_path, "\\Archive").await?;
                } else if folder.to_lowercase() == ".trash" {
                    storage.add_flag(&mailbox_path, "\\Trash").await?;
                }
            }
            storage.add_flag(&mailbox_path, "\\Subscribed").await?;
            lines
                .send(format!("{} OK SUBSCRIBE completed", command_data.tag))
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
