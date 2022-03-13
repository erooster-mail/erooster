use crate::{
    config::Config,
    imap_commands::{Command, Data},
};
use async_trait::async_trait;
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::{fs, path::Path, sync::Arc};

pub struct Delete<'a> {
    pub data: &'a Data,
}

#[async_trait]
impl<S> Command<S> for Delete<'_>
where
    S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
{
    async fn exec(&mut self, lines: &mut S, config: Arc<Config>) -> anyhow::Result<()> {
        let arguments = &self.data.command_data.as_ref().unwrap().arguments;
        if arguments.len() == 1 {
            let mut folder = arguments[0].replace('/', ".");
            folder.insert(0, '.');
            folder.remove_matches('"');
            let mailbox_path = Path::new(&config.mail.maildir_folders)
                .join(self.data.con_state.read().await.username.clone().unwrap())
                .join(folder.clone());
            // TODO error handling
            // TODO all the extra rules when to not delete
            fs::remove_dir_all(mailbox_path)?;
            lines
                .send(format!(
                    "{} OK DELETE completed",
                    self.data.command_data.as_ref().unwrap().tag
                ))
                .await?;
        } else {
            lines
                .send(format!(
                    "{} BAD [SERVERBUG] invalid arguments",
                    self.data.command_data.as_ref().unwrap().tag
                ))
                .await?;
        }
        Ok(())
    }
}
