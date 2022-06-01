use crate::{
    backend::storage::{MailStorage, Storage},
    config::Config,
    imap_commands::{CommandData, Data},
};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::{path::Path, sync::Arc};
use tracing::instrument;

pub struct Unsubscribe<'a> {
    pub data: &'a Data,
}

impl Unsubscribe<'_> {
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
