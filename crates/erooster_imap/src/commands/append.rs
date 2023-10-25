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
use std::path::Path;
use tracing::{debug, error, instrument};

pub struct Append<'a> {
    pub data: &'a mut Data,
}

impl Append<'_> {
    #[instrument(skip(self, lines, storage, command_data))]
    pub async fn exec<S, E>(
        &mut self,
        lines: &mut S,
        storage: &Storage,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        if matches!(self.data.con_state.state, State::Authenticated)
            || matches!(self.data.con_state.state, State::Selected(_, _))
        {
            debug!("[Append] User is authenticated");
            debug!(
                "[Append] User added {} arguments",
                command_data.arguments.len()
            );
            if command_data.arguments.len() < 3 {
                lines
                    .send(format!("{} NO invalid argument count", command_data.tag))
                    .await?;
                return Ok(());
            }
            let folder = command_data.arguments[0].replace('"', "");
            debug!("[Append] User wants to append to folder: {}", folder);
            let mailbox_path = storage.to_ondisk_path(
                folder.clone(),
                self.data
                    .con_state
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
                    self.data.con_state.state = State::Appending(AppendingState {
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
        &mut self,
        lines: &mut S,
        storage: &Storage,
        append_data: &str,
        config: &Config,
        tag: String,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        let username = self
            .data
            .con_state
            .username
            .clone()
            .context("Username missing in internal State")?;
        if let State::Appending(state) = &mut self.data.con_state.state {
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
                    // TODO: verify that we need this
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
                    self.data.con_state.state = State::GotAppendData;
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

    #[allow(clippy::unwrap_used)]
    #[tokio::test]
    async fn test_not_authenticated_state() {
        let mut caps = Append {
            data: &mut Data {
                con_state: Connection {
                    state: State::NotAuthenticated,
                    secure: true,
                    username: None,
                    active_capabilities: vec![],
                },
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
        let database = erooster_core::backend::database::get_database(&config)
            .await
            .unwrap();
        let storage = erooster_core::backend::storage::get_storage(database, config);
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(rx.next().await, Some(String::from("a1 NO invalid state")));
    }

    #[allow(clippy::unwrap_used)]
    #[tokio::test]
    async fn test_not_enough_arguments() {
        let mut caps = Append {
            data: &mut Data {
                con_state: Connection {
                    state: State::Selected(String::from("INBOX"), Access::ReadOnly),
                    secure: true,
                    username: Some(String::from("meow")),
                    active_capabilities: vec![],
                },
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
        let database = erooster_core::backend::database::get_database(&config)
            .await
            .unwrap();
        let storage = erooster_core::backend::storage::get_storage(database, config);
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(
            rx.next().await,
            Some(String::from("a1 NO invalid argument count"))
        );
    }

    #[allow(clippy::unwrap_used)]
    #[tokio::test]
    async fn test_read_only_arguments() {
        let mut caps = Append {
            data: &mut Data {
                con_state: Connection {
                    state: State::Selected(String::from("INBOX"), Access::ReadOnly),
                    secure: true,
                    username: Some(String::from("meow")),
                    active_capabilities: vec![],
                },
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
        let database = erooster_core::backend::database::get_database(&config)
            .await
            .unwrap();
        let storage = erooster_core::backend::storage::get_storage(database, config);
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(
            rx.next().await,
            Some(String::from("+ Ready for literal data"))
        );
    }

    #[allow(clippy::unwrap_used)]
    #[allow(clippy::too_many_lines)]
    #[tokio::test]
    async fn test_append() {
        let mut caps = Append {
            data: &mut Data {
                con_state: Connection {
                    state: State::Selected(String::from("INBOX"), Access::ReadOnly),
                    secure: true,
                    username: Some(String::from("meow")),
                    active_capabilities: vec![],
                },
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
        let database = erooster_core::backend::database::get_database(&config)
            .await
            .unwrap();
        let storage = erooster_core::backend::storage::get_storage(database, config.clone());
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(
            rx.next().await,
            Some(String::from("+ Ready for literal data"))
        );

        // Append data
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps
            .append(
                &mut tx,
                &storage,
                "Date: Mon, 7 Feb 1994 21:52:25 -0800 (PST)",
                &config,
                cmd_data.tag.to_string(),
            )
            .await;
        assert!(res.is_ok());
        let res = caps
            .append(
                &mut tx,
                &storage,
                "From: Fred Foobar <foobar@Blurdybloop.example>",
                &config,
                cmd_data.tag.to_string(),
            )
            .await;
        assert!(res.is_ok());
        let res = caps
            .append(
                &mut tx,
                &storage,
                "Subject: afternoon meeting",
                &config,
                cmd_data.tag.to_string(),
            )
            .await;
        assert!(res.is_ok());
        let res = caps
            .append(
                &mut tx,
                &storage,
                "To: mooch@owatagu.siam.edu.example",
                &config,
                cmd_data.tag.to_string(),
            )
            .await;
        assert!(res.is_ok());
        let res = caps
            .append(
                &mut tx,
                &storage,
                "Message-Id: <B27397-0100000@Blurdybloop.example>",
                &config,
                cmd_data.tag.to_string(),
            )
            .await;
        assert!(res.is_ok());
        let res = caps
            .append(
                &mut tx,
                &storage,
                "MIME-Version: 1.0",
                &config,
                cmd_data.tag.to_string(),
            )
            .await;
        assert!(res.is_ok());
        let res = caps
            .append(
                &mut tx,
                &storage,
                "Content-Type: TEXT/PLAIN; CHARSET=US-ASCII",
                &config,
                cmd_data.tag.to_string(),
            )
            .await;
        assert!(res.is_ok());
        let res = caps
            .append(&mut tx, &storage, "", &config, cmd_data.tag.to_string())
            .await;
        assert!(res.is_ok());
        let res = caps
            .append(
                &mut tx,
                &storage,
                "Hello Joe, do you think we can meet at 3:30 tomorrow?",
                &config,
                cmd_data.tag.to_string(),
            )
            .await;
        assert!(res.is_ok());
        let res = caps
            .append(&mut tx, &storage, "", &config, cmd_data.tag.to_string())
            .await;
        assert!(res.is_ok());
        assert_eq!(
            rx.next().await,
            Some(String::from("a1 OK APPEND completed"))
        );
    }
}
