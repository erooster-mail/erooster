use crate::{
    commands::{parsers::append_arguments, CommandData, Data},
    servers::state::{AppendingState, State},
};
use color_eyre::eyre::ContextCompat;
use erooster_core::{
    backend::storage::{MailStorage, Storage},
    config::Config,
};
use futures::{Sink, SinkExt};
use nom::{error::convert_error, Finish};
use std::io::Write;
use std::{path::Path, sync::Arc};
use tracing::{debug, error, instrument};

pub struct Append<'a> {
    pub data: &'a Data,
}

impl Append<'_> {
    #[instrument(skip(self, lines, storage, command_data))]
    pub async fn exec<S, E>(
        &self,
        lines: &mut S,
        storage: &Storage,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        let mut write_lock = self.data.con_state.write().await;
        if matches!(write_lock.state, State::Authenticated)
            || matches!(write_lock.state, State::Selected(_, _))
        {
            debug!("[Append] User is authenticated");
            debug!(
                "[Append] User added {} arguments",
                command_data.arguments.len()
            );
            assert!(command_data.arguments.len() >= 3);
            let folder = command_data.arguments[0].replace('"', "");
            debug!("[Append] User wants to append to folder: {}", folder);
            let mailbox_path = storage.to_ondisk_path(
                folder.clone(),
                write_lock
                    .username
                    .clone()
                    .context("Username missing in internal State")?,
            )?;
            let folder = storage.to_ondisk_path_name(folder)?;
            debug!("Appending to folder: {:?}", mailbox_path);
            // Spec violation but thunderbird would prompt a user error otherwise :/
            if !mailbox_path.exists() {
                storage.create_dirs(&mailbox_path)?;
                if folder.to_lowercase() == ".sent" {
                    storage.add_flag(&mailbox_path, "\\Sent").await?;
                    storage.add_flag(&mailbox_path, "\\Subscribed").await?;
                } else if folder.to_lowercase() == ".junk" {
                    storage.add_flag(&mailbox_path, "\\Junk").await?;
                    storage.add_flag(&mailbox_path, "\\Subscribed").await?;
                } else if folder.to_lowercase() == ".drafts" {
                    storage.add_flag(&mailbox_path, "\\Drafts").await?;
                    storage.add_flag(&mailbox_path, "\\Subscribed").await?;
                } else if folder.to_lowercase() == ".archive" {
                    storage.add_flag(&mailbox_path, "\\Archive").await?;
                    storage.add_flag(&mailbox_path, "\\Subscribed").await?;
                } else if folder.to_lowercase() == ".trash" {
                    storage.add_flag(&mailbox_path, "\\Trash").await?;
                    storage.add_flag(&mailbox_path, "\\Subscribed").await?;
                }
            }

            let append_args = command_data.arguments[1..].join(" ");
            let append_args_borrow: &str = &append_args;
            debug!("Append args: {}", append_args_borrow);
            match append_arguments(append_args_borrow).finish() {
                Ok((left, (flags, datetime, literal))) => {
                    debug!("[Append] leftover: {}", left);
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
                }
                Err(e) => {
                    error!(
                        "[Append] Error parsing arguments: {}",
                        convert_error(append_args_borrow, e)
                    );
                    lines
                        .send(format!(
                            "{} BAD failed to parse arguments",
                            command_data.tag
                        ))
                        .await?;
                }
            }
        } else {
            lines
                .send(format!("{} NO invalid state", command_data.tag))
                .await?;
        }
        Ok(())
    }

    #[instrument(skip(self, lines, storage, config, append_data))]
    pub async fn append<S, E>(
        &self,
        lines: &mut S,
        storage: &Storage,
        append_data: &str,
        config: Arc<Config>,
        tag: String,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        let mut write_lock = self.data.con_state.write().await;
        let username = write_lock
            .username
            .clone()
            .context("Username missing in internal State")?;
        if let State::Appending(state) = &mut write_lock.state {
            if let Some(buffer) = &mut state.data {
                write!(buffer, "{append_data}\r\n")?;
                debug!("Buffer length: {}", buffer.len());
                debug!("expected: {}", state.datalen);
                if buffer.len() >= state.datalen {
                    debug!("[Append] Saving data");
                    let folder = &state.folder;
                    let mailbox_path = Path::new(&config.mail.maildir_folders)
                        .join(username)
                        .join(folder.clone());
                    debug!("[Append] Mailbox path: {:?}", mailbox_path);
                    // TODO verify that we need this
                    buffer.truncate(buffer.len() - 2);
                    let message_id = storage
                        .store_cur_with_flags(
                            &mailbox_path,
                            buffer,
                            state.flags.clone().unwrap_or_default(),
                        )
                        .await?;
                    debug!("Stored message via append: {}", message_id);
                    write_lock.state = State::GotAppendData;
                    lines.send(format!("{tag} OK APPEND completed")).await?;
                }
            } else {
                let mut buffer = Vec::with_capacity(state.datalen);
                write!(buffer, "{append_data}\r\n")?;

                state.data = Some(buffer);
            }
        } else {
            lines.send(format!("{tag} NO invalid state")).await?;
        }
        Ok(())
    }
}
