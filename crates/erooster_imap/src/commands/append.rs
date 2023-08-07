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
                        .join(username.clone())
                        .join(folder.clone());
                    debug!("[Append] Mailbox path: {:?}", mailbox_path);
                    // TODO verify that we need this
                    buffer.truncate(buffer.len() - 2);
                    let message_id = storage
                        .store_cur_with_flags(
                            format!("{username}/{folder}"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{CommandData, Commands};
    use crate::servers::state::{Access, Connection};
    use futures::{channel::mpsc, StreamExt};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[allow(clippy::unwrap_used)]
    #[tokio::test]
    async fn test_not_authenticated_state() {
        let caps = Append {
            data: &Data {
                con_state: Arc::new(RwLock::new(Connection {
                    state: State::NotAuthenticated,
                    secure: true,
                    username: None,
                    active_capabilities: vec![],
                })),
            },
        };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Append,
            arguments: &[],
        };
        let config = erooster_core::get_config(String::from("./config.yml"))
            .await
            .unwrap();
        let database = erooster_core::backend::database::get_database(Arc::clone(&config))
            .await
            .unwrap();
        let storage = erooster_core::backend::storage::get_storage(database, Arc::clone(&config));
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(rx.next().await, Some(String::from("a1 NO invalid state")));
    }

    #[allow(clippy::unwrap_used)]
    #[tokio::test]
    async fn test_authenticated_not_selected_state() {
        let caps = Append {
            data: &Data {
                con_state: Arc::new(RwLock::new(Connection {
                    state: State::Authenticated,
                    secure: true,
                    username: None,
                    active_capabilities: vec![],
                })),
            },
        };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Append,
            arguments: &[],
        };
        let config = erooster_core::get_config(String::from("./config.yml"))
            .await
            .unwrap();
        let database = erooster_core::backend::database::get_database(Arc::clone(&config))
            .await
            .unwrap();
        let storage = erooster_core::backend::storage::get_storage(database, Arc::clone(&config));
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(rx.next().await, Some(String::from("a1 NO invalid state")));
    }

    #[allow(clippy::unwrap_used)]
    #[tokio::test]
    async fn test_not_enough_arguments() {
        let caps = Append {
            data: &Data {
                con_state: Arc::new(RwLock::new(Connection {
                    state: State::Selected(String::from("INBOX"), Access::ReadOnly),
                    secure: true,
                    username: Some(String::from("meow")),
                    active_capabilities: vec![],
                })),
            },
        };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Append,
            arguments: &[],
        };
        let config = erooster_core::get_config(String::from("./config.yml"))
            .await
            .unwrap();
        let database = erooster_core::backend::database::get_database(Arc::clone(&config))
            .await
            .unwrap();
        let storage = erooster_core::backend::storage::get_storage(database, Arc::clone(&config));
        let (mut tx, mut _rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_err());
    }

    #[allow(clippy::unwrap_used)]
    #[tokio::test]
    async fn test_read_only_arguments() {
        let caps = Append {
            data: &Data {
                con_state: Arc::new(RwLock::new(Connection {
                    state: State::Selected(String::from("INBOX"), Access::ReadOnly),
                    secure: true,
                    username: Some(String::from("meow")),
                    active_capabilities: vec![],
                })),
            },
        };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Append,
            arguments: &["INBOX", "(\\Seen)", "{326}"],
        };

        let config = erooster_core::get_config(String::from("./config.yml"))
            .await
            .unwrap();
        let database = erooster_core::backend::database::get_database(Arc::clone(&config))
            .await
            .unwrap();
        let storage = erooster_core::backend::storage::get_storage(database, Arc::clone(&config));
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(
            rx.next().await,
            Some(String::from("+ Ready for literal data"))
        );
    }
}
