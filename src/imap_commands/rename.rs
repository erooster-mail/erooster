use std::{fs, path::Path, sync::Arc};

use crate::{
    config::Config,
    imap_commands::{CommandData, Data},
};
use futures::{channel::mpsc::SendError, Sink, SinkExt};

pub struct Rename<'a> {
    pub data: &'a Data,
}

impl Rename<'_> {
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        command_data: &CommandData<'_>,
        config: Arc<Config>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        let args = &command_data.arguments;
        assert!(args.len() == 2);
        let mut old_folder = args[0].replace('/', ".");
        old_folder.insert(0, '.');
        old_folder.remove_matches('"');
        let old_mailbox_path = Path::new(&config.mail.maildir_folders)
            .join(self.data.con_state.read().await.username.clone().unwrap())
            .join(old_folder.clone());
        let mut new_folder = args[1].replace('/', ".");
        new_folder.insert(0, '.');
        new_folder.remove_matches('"');
        let new_folder_path = Path::new(&config.mail.maildir_folders)
            .join(self.data.con_state.read().await.username.clone().unwrap())
            .join(new_folder.clone());
        fs::rename(old_mailbox_path, new_folder_path)?;
        lines
            .send(format!("{} OK RENAME completed", command_data.tag))
            .await?;
        Ok(())
    }
}
