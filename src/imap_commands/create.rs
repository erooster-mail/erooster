use std::{io, path::Path, sync::Arc};

use async_trait::async_trait;
use futures::{Sink, SinkExt};
use maildir::Maildir;
use tracing::error;

use crate::{
    imap_commands::{utils::add_flag, Command, Data},
    config::Config,
    line_codec::LinesCodecError,
};

pub struct Create<'a> {
    pub data: &'a Data<'a>,
}

#[async_trait]
impl<S> Command<S> for Create<'_>
where
    S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
    S::Error: From<io::Error>,
{
    async fn exec(&mut self, lines: &mut S, config: Arc<Config>) -> anyhow::Result<()> {
        let arguments = &self.data.command_data.as_ref().unwrap().arguments;
        if arguments.len() == 1 {
            let mut folder = arguments[0].replace('/', ".");
            folder.insert(0, '.');
            folder.remove_matches('"');
            let mailbox_path = Path::new(&config.mail.maildir_folders)
                .join(self.data.con_state.username.clone().unwrap())
                .join(folder.clone());
            let maildir = Maildir::from(mailbox_path.clone());
            match maildir.create_dirs() {
                Ok(_) => {
                    if folder.to_lowercase() == ".trash" {
                        add_flag(&mailbox_path, "\\Trash")?;
                    }
                    lines
                        .send(format!(
                            "{} OK CREATE completed",
                            self.data.command_data.as_ref().unwrap().tag
                        ))
                        .await?;
                }
                Err(e) => {
                    error!("Failed to create folder: {}", e);

                    lines
                        .send(format!(
                            "{} NO CREATE failure",
                            self.data.command_data.as_ref().unwrap().tag
                        ))
                        .await?;
                }
            }
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
