use crate::{
    backend::storage::Storage,
    imap_commands::{parsers::append_arguments, CommandData, Data},
    imap_servers::state::State,
    Config,
};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::{path::Path, sync::Arc};
use tracing::{debug, instrument};

pub struct Append<'a> {
    pub data: &'a Data,
}
impl Append<'_> {
    #[instrument(skip(self, lines, storage, command_data))]
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        storage: Arc<Storage>,
        config: Arc<Config>,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        let mut write_lock = self.data.con_state.write().await;
        if write_lock.state == State::Authenticated {
            assert!(command_data.arguments.len() >= 3);
            let folder = command_data.arguments[0];
            let mut folder = folder.replace('/', ".");
            folder.insert(0, '.');
            folder.remove_matches('"');
            folder = folder.replace(".INBOX", "INBOX");
            let mailbox_path = Path::new(&config.mail.maildir_folders)
                .join(self.data.con_state.read().await.username.clone().unwrap())
                .join(folder.clone());

            if !mailbox_path.exists() {
                lines
                    .send(format!(
                        "{} NO [TRYCREATE] folder is not yet created",
                        command_data.tag
                    ))
                    .await?;
            }

            let append_args = command_data.arguments[1..].join(" ");
            debug!("Append args: {}", append_args);
            if let Ok((_, (flags, datetime, literal))) = append_arguments(&append_args) {
                write_lock.state = State::Appending(
                    folder.to_string(),
                    flags.map(|x| x.iter().map(ToString::to_string).collect::<Vec<_>>()),
                    datetime,
                );
                if literal.contains('+') {
                } else {
                    lines.send(String::from("+ Ready for literal data")).await?;
                }
            } else {
                lines
                    .send(format!(
                        "{} BAD failed to parse arguments",
                        command_data.tag
                    ))
                    .await?;
            }
        } else {
            lines
                .send(format!("{} NO invalid state", command_data.tag))
                .await?;
        }
        Ok(())
    }

    #[instrument(skip(self, lines, storage, append_data, command_data))]
    pub async fn append<S>(
        &self,
        lines: &mut S,
        storage: Arc<Storage>,
        append_data: &str,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        Ok(())
    }
}
