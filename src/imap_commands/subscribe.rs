use crate::{
    backend::storage::{MailStorage, Storage},
    config::Config,
    imap_commands::{CommandData, Data},
};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::{path::Path, sync::Arc};
use tracing::{debug, instrument};

pub struct Subscribe<'a> {
    pub data: &'a Data,
}

impl Subscribe<'_> {
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
                .join(folder.clone());
            let mailbox_path_string = mailbox_path
                .clone()
                .into_os_string()
                .into_string()
                .expect("Failed to convert path. Your system may be incompatible");

            // This is a spec violation. However we need to do this currently due to how the storage is set up
            debug!("mailbox_path: {}", mailbox_path_string);
            if !mailbox_path.exists() {
                storage.create_dirs(mailbox_path_string)?;
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
