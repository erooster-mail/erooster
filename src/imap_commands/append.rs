use crate::{
    backend::storage::{MailStorage, Storage},
    imap_commands::{parsers::append_arguments, CommandData, Data},
    imap_servers::state::{AppendingState, State},
    Config,
};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::{path::Path, sync::Arc};
use tracing::{debug, instrument};

pub struct Append<'a> {
    pub data: &'a Data,
}
impl Append<'_> {
    #[instrument(skip(self, lines, config, command_data))]
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        config: Arc<Config>,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        let mut write_lock = self.data.con_state.write().await;
        debug!("Append command start");
        if write_lock.state == State::Authenticated {
            debug!("[Append] User is authenticated");
            debug!(
                "[Append] User added {} arguments",
                command_data.arguments.len()
            );
            assert!(command_data.arguments.len() >= 3);
            let folder = command_data.arguments[0].replace('"', "");
            debug!("[Append] User wants to append to folder: {}", folder);
            let mut folder = folder.replace('/', ".");
            folder.insert(0, '.');
            folder.remove_matches('"');
            folder = folder.replace(".INBOX", "INBOX");
            let mailbox_path = Path::new(&config.mail.maildir_folders)
                .join(write_lock.username.clone().unwrap())
                .join(folder.clone());
            debug!("Appending to folder: {:?}", mailbox_path);

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
                write_lock.state = State::Appending(AppendingState {
                    folder: folder.to_string(),
                    flags: flags.map(|x| x.iter().map(ToString::to_string).collect::<Vec<_>>()),
                    datetime,
                    data: None,
                    datalen: literal.length,
                    tag: command_data.tag.to_string(),
                });
                if !literal.continuation {
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

    #[instrument(skip(self, lines, storage, config, append_data))]
    pub async fn append<S>(
        &self,
        lines: &mut S,
        storage: Arc<Storage>,
        append_data: &str,
        config: Arc<Config>,
        tag: String,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        let mut write_lock = self.data.con_state.write().await;
        let username = write_lock.username.clone().unwrap();
        if let State::Appending(state) = &mut write_lock.state {
            if let Some(buffer) = &mut state.data {
                let mut bytes = append_data.as_bytes().to_vec();
                let buffer_length = bytes.len();
                buffer.append(&mut bytes);
                if buffer_length + bytes.len() > state.datalen {
                    let folder = &state.folder;
                    let mut folder = folder.replace('/', ".");
                    folder.insert(0, '.');
                    folder.remove_matches('"');
                    folder = folder.replace(".INBOX", "INBOX");
                    let mailbox_path = Path::new(&config.mail.maildir_folders)
                        .join(username)
                        .join(folder.clone());
                    if let Some(flags) = &state.flags {
                        let message_id = storage
                            .store_cur_with_flags(
                                mailbox_path.clone().into_os_string().into_string().expect(
                                    "Failed to convert path. Your system may be incompatible",
                                ),
                                buffer,
                                flags.clone(),
                            )
                            .await?;
                        debug!("Stored message via append: {}", message_id);
                    } else {
                        let message_id = storage
                            .store_cur_with_flags(
                                mailbox_path.clone().into_os_string().into_string().expect(
                                    "Failed to convert path. Your system may be incompatible",
                                ),
                                buffer,
                                vec![],
                            )
                            .await?;
                        debug!("Stored message via append: {}", message_id);
                    }
                    lines.send(format!("{} OK APPEND completed", tag)).await?;
                }
            } else {
                let mut buffer = Vec::with_capacity(state.datalen);
                let mut bytes = append_data.as_bytes().to_vec();
                buffer.append(&mut bytes);

                state.data = Some(buffer);
            }
        } else {
            lines.send(format!("{} NO invalid state", tag)).await?;
        }
        Ok(())
    }
}
