use crate::{
    config::Config,
    imap_commands::{utils::add_flag, CommandData, Data},
};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use maildir::Maildir;
use std::{path::Path, sync::Arc};
use tracing::error;

pub struct Create<'a> {
    pub data: &'a Data,
}
impl Create<'_> {
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        config: Arc<Config>,
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
            let maildir = Maildir::from(mailbox_path.clone());
            match maildir.create_dirs() {
                Ok(_) => {
                    if folder.to_lowercase() == ".trash" {
                        add_flag(&mailbox_path, "\\Trash")?;
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
