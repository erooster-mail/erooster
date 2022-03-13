use crate::{
    config::Config,
    imap_commands::{utils::add_flag, Command, Data},
};
use async_trait::async_trait;
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::{path::Path, sync::Arc};

pub struct Subscribe<'a> {
    pub data: &'a Data,
}

#[async_trait]
impl<S> Command<S> for Subscribe<'_>
where
    S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
{
    async fn exec(&mut self, lines: &mut S, config: Arc<Config>) -> color_eyre::eyre::Result<()> {
        let arguments = &self.data.command_data.as_ref().unwrap().arguments;
        if arguments.len() == 1 {
            let mut folder = arguments[0].replace('/', ".");
            folder.insert(0, '.');
            folder.remove_matches('"');
            let mailbox_path = Path::new(&config.mail.maildir_folders)
                .join(self.data.con_state.read().await.username.clone().unwrap())
                .join(folder.clone());
            add_flag(&mailbox_path, "\\Subscribed")?;
            lines
                .send(format!(
                    "{} OK SUBSCRIBE completed",
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
