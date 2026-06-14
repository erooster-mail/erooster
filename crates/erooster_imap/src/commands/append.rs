// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::{
    commands::{parsers::append_arguments, select::get_or_create_uidvalidity, CommandData, Data},
    servers::state::{AppendingState, State},
};
use erooster_core::{
    backend::storage::{MailStorage, Storage},
    config::Config,
};
use std::{io::Write, path::Path};
use {
    color_eyre::{self, eyre::ContextCompat},
    futures::{Sink, SinkExt},
    nom::Finish,
    nom_language::error::convert_error,
    tracing::{debug, error, instrument},
};

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
            let is_new = !mailbox_path.exists();
            storage.create_dirs(&mailbox_path)?;
            if is_new {
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
                    let previous_state = self.data.con_state.state.clone();
                    self.data.con_state.state = State::Appending(AppendingState {
                        folder: folder.clone(),
                        flags: flags.map(|x| x.iter().map(ToString::to_string).collect::<Vec<_>>()),
                        datetime,
                        data: None,
                        datalen: literal.length,
                        tag: command_data.tag.to_string(),
                        previous_state: Box::new(previous_state),
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
        // Extract all needed data from Appending state before mutating.
        let completion = if let State::Appending(ref mut state) = self.data.con_state.state {
            if let Some(ref mut buffer) = state.data {
                write!(buffer, "{append_data}\r\n")?;
                debug!("Buffer length: {}", buffer.len());
                debug!("expected: {}", state.datalen);
                if buffer.len() >= state.datalen {
                    debug!("[Append] Saving data");
                    let folder = state.folder.clone();
                    let imap_flags = state.flags.clone().unwrap_or_default();
                    let previous_state = *state.previous_state.clone();
                    let completion_tag = state.tag.clone();
                    let mut buf = buffer.clone();
                    buf.truncate(buf.len() - 2);
                    Some((folder, imap_flags, previous_state, completion_tag, buf))
                } else {
                    None
                }
            } else {
                let mut buffer = Vec::with_capacity(state.datalen);
                write!(buffer, "{append_data}\r\n")?;
                state.data = Some(buffer);
                return Ok(());
            }
        } else {
            lines.send(format!("{tag} NO invalid state")).await?;
            return Ok(());
        };

        if let Some((folder, imap_flags, previous_state, completion_tag, buffer)) = completion {
            let mailbox_path = Path::new(&config.mail.maildir_folders)
                .join(username.clone())
                .join(folder.clone());
            debug!("[Append] Mailbox path: {:?}", mailbox_path);
            storage.create_dirs(&mailbox_path)?;
            // Use the IMAP folder name (no leading dot) as the DB key so it
            // matches what SELECT/FETCH use (State::Selected stores "Sent", not ".Sent").
            let db_folder = folder.trim_start_matches('.');
            let mailbox_id = format!("{username}/{db_folder}");
            let message_id = storage
                .store_cur_with_flags(mailbox_id.clone(), &mailbox_path, &buffer, imap_flags)
                .await?;
            debug!("Stored message via append: {}", message_id);
            // Restore the state we were in before APPEND (RFC 9051: APPEND does
            // not change the selected mailbox).
            self.data.con_state.state = previous_state;
            let uidvalidity = get_or_create_uidvalidity(&mailbox_path).await?;
            let uid = storage.get_uid_for_folder(&mailbox_id).await?;
            lines
                .send(format!(
                    "{completion_tag} OK [APPENDUID {uidvalidity} {uid}] APPEND completed"
                ))
                .await?;
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
    use tokio;

    #[allow(clippy::unwrap_used)]
    #[cfg_attr(coverage_nightly, coverage(off))]
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
        let (_config, storage) = erooster_core::test_helpers::setup_test_storage()
            .await
            .unwrap();
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(rx.next().await, Some(String::from("a1 NO invalid state")));
    }

    #[allow(clippy::unwrap_used)]
    #[cfg_attr(coverage_nightly, coverage(off))]
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
        let (_config, storage) = erooster_core::test_helpers::setup_test_storage()
            .await
            .unwrap();
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            rx.next().await,
            Some(String::from("a1 NO invalid argument count"))
        );
    }

    #[allow(clippy::unwrap_used)]
    #[cfg_attr(coverage_nightly, coverage(off))]
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

        let (_config, storage) = erooster_core::test_helpers::setup_test_storage()
            .await
            .unwrap();
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            rx.next().await,
            Some(String::from("+ Ready for literal data"))
        );
    }

    #[allow(clippy::unwrap_used)]
    #[allow(clippy::too_many_lines)]
    #[cfg_attr(coverage_nightly, coverage(off))]
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

        let (config, storage) = erooster_core::test_helpers::setup_test_storage()
            .await
            .unwrap();
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok(), "{:?}", res);
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
        assert!(res.is_ok(), "{:?}", res);
        let res = caps
            .append(
                &mut tx,
                &storage,
                "From: Fred Foobar <foobar@Blurdybloop.example>",
                &config,
                cmd_data.tag.to_string(),
            )
            .await;
        assert!(res.is_ok(), "{:?}", res);
        let res = caps
            .append(
                &mut tx,
                &storage,
                "Subject: afternoon meeting",
                &config,
                cmd_data.tag.to_string(),
            )
            .await;
        assert!(res.is_ok(), "{:?}", res);
        let res = caps
            .append(
                &mut tx,
                &storage,
                "To: mooch@owatagu.siam.edu.example",
                &config,
                cmd_data.tag.to_string(),
            )
            .await;
        assert!(res.is_ok(), "{:?}", res);
        let res = caps
            .append(
                &mut tx,
                &storage,
                "Message-Id: <B27397-0100000@Blurdybloop.example>",
                &config,
                cmd_data.tag.to_string(),
            )
            .await;
        assert!(res.is_ok(), "{:?}", res);
        let res = caps
            .append(
                &mut tx,
                &storage,
                "MIME-Version: 1.0",
                &config,
                cmd_data.tag.to_string(),
            )
            .await;
        assert!(res.is_ok(), "{:?}", res);
        let res = caps
            .append(
                &mut tx,
                &storage,
                "Content-Type: TEXT/PLAIN; CHARSET=US-ASCII",
                &config,
                cmd_data.tag.to_string(),
            )
            .await;
        assert!(res.is_ok(), "{:?}", res);
        let res = caps
            .append(&mut tx, &storage, "", &config, cmd_data.tag.to_string())
            .await;
        assert!(res.is_ok(), "{:?}", res);
        let res = caps
            .append(
                &mut tx,
                &storage,
                "Hello Joe, do you think we can meet at 3:30 tomorrow?",
                &config,
                cmd_data.tag.to_string(),
            )
            .await;
        assert!(res.is_ok(), "{:?}", res);
        let res = caps
            .append(&mut tx, &storage, "", &config, cmd_data.tag.to_string())
            .await;
        assert!(res.is_ok(), "{:?}", res);
        let reply = rx.next().await.unwrap_or_default();
        assert!(
            reply.starts_with("a1 OK [APPENDUID "),
            "expected APPENDUID reply, got: {reply}"
        );
    }

    /// Regression: APPEND must restore the previously-selected mailbox state.
    /// Before the fix, completing an APPEND unconditionally set state to
    /// `Authenticated`, which broke all subsequent FETCH/IDLE commands.
    #[allow(clippy::unwrap_used)]
    #[cfg_attr(coverage_nightly, coverage(off))]
    #[tokio::test]
    async fn test_append_restores_selected_state() {
        // Start in Selected("INBOX") — simulates Thunderbird appending to Sent
        // while the connection has INBOX selected.
        let connection = Connection {
            state: State::Selected(String::from("INBOX"), Access::ReadWrite),
            secure: true,
            username: Some(String::from("meow")),
            active_capabilities: vec![],
        };
        let mut data = Data {
            con_state: connection,
        };
        let mut caps = Append { data: &mut data };

        // "From: a@b.com\r\n" (15) + "Subject: test\r\n" (15) + "\r\n" (2) +
        // "test body\r\n" (11) = 43 bytes.
        let cmd_data = CommandData {
            tag: "t1",
            command: Commands::Append,
            arguments: &["INBOX", "(\\Seen)", "{43}"],
        };
        let (config, storage) = erooster_core::test_helpers::setup_test_storage()
            .await
            .unwrap();
        let (mut tx, _rx) = mpsc::unbounded();
        caps.exec(&mut tx, &storage, &cmd_data).await.unwrap();

        let (mut tx, mut rx) = mpsc::unbounded();
        for line in &["From: a@b.com", "Subject: test", "", "test body"] {
            caps.append(&mut tx, &storage, line, &config, "t1".to_string())
                .await
                .unwrap();
        }
        let reply = rx.next().await.unwrap_or_default();
        assert!(
            reply.starts_with("t1 OK [APPENDUID "),
            "expected APPENDUID reply, got: {reply}"
        );
        assert_eq!(
            caps.data.con_state.state,
            State::Selected(String::from("INBOX"), Access::ReadWrite),
            "APPEND must restore the previously selected state, not drop to Authenticated"
        );
    }

    /// Regression: APPEND must write the DB mailbox key without a leading dot
    /// so that FETCH (which uses the IMAP folder name from Selected state) can
    /// find the messages.  Before the fix, appending to "Sent" stored
    /// mailbox_id as "meow/.Sent" while FETCH queried "meow/Sent".
    #[allow(clippy::unwrap_used)]
    #[cfg_attr(coverage_nightly, coverage(off))]
    #[tokio::test]
    async fn test_append_stores_imap_folder_name_in_db() {
        let connection = Connection {
            state: State::Authenticated,
            secure: true,
            username: Some(String::from("meow")),
            active_capabilities: vec![],
        };
        let mut data = Data {
            con_state: connection,
        };
        let mut caps = Append { data: &mut data };

        let cmd_data = CommandData {
            tag: "t2",
            command: Commands::Append,
            arguments: &["Sent", "(\\Seen)", "{43}"],
        };
        let (config, storage) = erooster_core::test_helpers::setup_test_storage()
            .await
            .unwrap();
        let (mut tx, _rx) = mpsc::unbounded();
        caps.exec(&mut tx, &storage, &cmd_data).await.unwrap();

        let (mut tx, mut rx) = mpsc::unbounded();
        for line in &["From: a@b.com", "Subject: test", "", "test body"] {
            caps.append(&mut tx, &storage, line, &config, "t2".to_string())
                .await
                .unwrap();
        }
        let reply = rx.next().await.unwrap_or_default();
        assert!(
            reply.starts_with("t2 OK [APPENDUID "),
            "expected APPENDUID reply, got: {reply}"
        );
        // Querying "meow/Sent" (no dot) must return UID=1, proving the message
        // was stored under the IMAP name, not the ondisk ".Sent" name.
        let uid = storage.get_uid_for_folder("meow/Sent").await.unwrap();
        assert_eq!(
            uid, 1,
            "message should be findable via IMAP folder name meow/Sent"
        );
        let uid_dot = storage.get_uid_for_folder("meow/.Sent").await.unwrap();
        assert_eq!(
            uid_dot, 0,
            "nothing should be stored under ondisk name meow/.Sent"
        );
    }
}
