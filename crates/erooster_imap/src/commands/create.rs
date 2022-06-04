use crate::commands::{CommandData, Data};
use erooster_core::backend::storage::{MailStorage, Storage};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::sync::Arc;
use tracing::{error, instrument};

pub struct Create<'a> {
    pub data: &'a Data,
}
impl Create<'_> {
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
                self.data.con_state.read().await.username.clone().unwrap(),
            )?;
            let folder = storage.to_ondisk_path_name(folder)?;

            let mailbox_path_string = mailbox_path
                .clone()
                .into_os_string()
                .into_string()
                .expect("Failed to convert path. Your system may be incompatible");

            match storage.create_dirs(mailbox_path_string) {
                Ok(_) => {
                    if folder.to_lowercase() == ".sent" {
                        storage.add_flag(&mailbox_path, "\\Sent").await?;
                        storage.add_flag(&mailbox_path, "\\Subscribed").await?;
                    } else if folder.to_lowercase() == ".junk" {
                        storage.add_flag(&mailbox_path, "\\Junk").await?;
                        storage.add_flag(&mailbox_path, "\\Subscribed").await?;
                    } else if folder.to_lowercase() == ".drafts" {
                        storage.add_flag(&mailbox_path, "\\Drafts").await?;
                        storage.add_flag(&mailbox_path, "\\Subscribed").await?;
                    } else if folder.to_lowercase() == ".archive" {
                        storage.add_flag(&mailbox_path, "\\Archive").await?;
                        storage.add_flag(&mailbox_path, "\\Subscribed").await?;
                    } else if folder.to_lowercase() == ".trash" {
                        storage.add_flag(&mailbox_path, "\\Trash").await?;
                        storage.add_flag(&mailbox_path, "\\Subscribed").await?;
                    }
                    lines
                        .send(format!("{} OK CREATE completed", command_data.tag))
                        .await?;
                }
                Err(e) => {
                    error!("Failed to create folder: {}", e);

                    lines
                        .send(format!("{} NO CREATE failure", command_data.tag))
                        .await?;
                }
            }
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
