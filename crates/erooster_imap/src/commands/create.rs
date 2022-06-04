use crate::commands::{CommandData, Data};
use erooster_core::{
    backend::storage::{MailStorage, Storage},
    config::Config,
};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::{path::Path, sync::Arc};
use tracing::{error, instrument};

pub struct Create<'a> {
    pub data: &'a Data,
}
impl Create<'_> {
    #[instrument(skip(self, lines, config, storage, command_data))]
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        config: Arc<Config>,
        storage: Arc<Storage>,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        let arguments = &command_data.arguments;
        assert!(arguments.len() == 1);
        if arguments.len() == 1 {
            let mut folder = arguments[0].replace('/', ".");
            folder.insert(0, '.');
            folder.remove_matches('"');
            let mailbox_path = Path::new(&config.mail.maildir_folders)
                .join(self.data.con_state.read().await.username.clone().unwrap())
                .join(folder.replace(".INBOX", "INBOX"));
            let mailbox_path_string = mailbox_path
                .clone()
                .into_os_string()
                .into_string()
                .expect("Failed to convert path. Your system may be incompatible");

            match storage.create_dirs(mailbox_path_string) {
                Ok(_) => {
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
