// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::commands::{
    copy::Copy,
    fetch::Fetch,
    move_::Move,
    parsers::parse_selected_range,
    store::Store,
    CommandData, Data,
};
use erooster_core::backend::storage::{MailEntry, MailStorage, Storage};
use {
    color_eyre::{self, eyre::ContextCompat},
    futures::{Sink, SinkExt},
    tokio::fs,
    tracing::instrument,
};

use crate::servers::state::{Access, State};
use super::search::Search;

pub struct Uid<'a> {
    pub data: &'a Data,
}

impl Uid<'_> {
    #[instrument(skip(self, lines, command_data, storage))]
    pub async fn exec<S, E>(
        &self,
        lines: &mut S,
        command_data: &CommandData<'_>,
        storage: &Storage,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        if command_data.arguments[0].to_lowercase() == "fetch" {
            Fetch { data: self.data }
                .exec(lines, command_data, storage, true)
                .await?;
        } else if command_data.arguments[0].to_lowercase() == "copy" {
            Copy { data: self.data }
                .exec(lines, storage, command_data, true)
                .await?;
        } else if command_data.arguments[0].to_lowercase() == "move" {
            Move { data: self.data }
                .exec(lines, storage, command_data, true)
                .await?;
        } else if command_data.arguments[0].to_lowercase() == "expunge" {
            self.uid_expunge(lines, command_data, storage).await?;
        } else if command_data.arguments[0].to_lowercase() == "search" {
            Search { data: self.data }
                .exec(lines, storage, command_data, true)
                .await?;
        } else if command_data.arguments[0].to_lowercase() == "store" {
            Store { data: self.data }
                .exec(lines, storage, command_data, true)
                .await?;
        }
        Ok(())
    }

    #[instrument(skip(self, lines, storage, command_data))]
    async fn uid_expunge<S, E>(
        &self,
        lines: &mut S,
        command_data: &CommandData<'_>,
        storage: &Storage,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        let State::Selected(folder, access) = &self.data.con_state.state else {
            lines
                .send(format!(
                    "{} BAD [CLIENTBUG] UID EXPUNGE requires a selected mailbox",
                    command_data.tag
                ))
                .await?;
            return Ok(());
        };

        if access == &Access::ReadOnly {
            lines
                .send(format!(
                    "{} NO [CANNOT] UID EXPUNGE not permitted in read-only mode",
                    command_data.tag
                ))
                .await?;
            return Ok(());
        }

        // argument[0] is "EXPUNGE", argument[1] is the UID set
        let uid_set_str = if command_data.arguments.len() > 1 {
            command_data.arguments[1]
        } else {
            lines
                .send(format!(
                    "{} BAD [CLIENTBUG] UID EXPUNGE requires a UID set argument",
                    command_data.tag
                ))
                .await?;
            return Ok(());
        };

        let uid_ranges = match parse_selected_range(uid_set_str) {
            Ok(("", ranges)) if !ranges.is_empty() => ranges,
            _ => {
                lines
                    .send(format!(
                        "{} BAD [CLIENTBUG] Invalid UID set: {uid_set_str}",
                        command_data.tag
                    ))
                    .await?;
                return Ok(());
            }
        };

        let username = self
            .data
            .con_state
            .username
            .clone()
            .context("Username missing in internal State")?;
        let mailbox_path = storage.to_ondisk_path(folder.clone(), username.clone())?;

        let mails: Vec<_> = storage
            .list_cur(format!("{username}/{folder}"), &mailbox_path)
            .await
            .into_iter()
            .chain(
                storage
                    .list_new(format!("{username}/{folder}"), &mailbox_path)
                    .await,
            )
            .collect();

        let mut seq = 1u32;
        for mail in mails {
            let uid_matches = uid_ranges.iter().any(|r| r.contains(&mail.uid()));
            if uid_matches && mail.is_trashed() {
                fs::remove_file(mail.path()).await?;
                lines.feed(format!("* {seq} EXPUNGE")).await?;
            } else {
                seq += 1;
            }
        }

        lines.flush().await?;
        lines
            .send(format!("{} OK UID EXPUNGE completed", command_data.tag))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::commands::{CommandData, Commands};
    use crate::servers::state::{Access, Connection, State};
    use futures::{channel::mpsc, StreamExt};
    use tokio;

    #[cfg_attr(coverage_nightly, coverage(off))]
    #[tokio::test]
    async fn test_uid_expunge_not_selected() {
        let data = Data {
            con_state: Connection::new(true),
        };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Uid,
            arguments: &["EXPUNGE", "1:*"],
        };
        let (_config, storage) = erooster_core::test_helpers::setup_test_storage()
            .await
            .unwrap();
        let (mut tx, mut rx) = mpsc::unbounded();
        let uid = Uid { data: &data };
        let res = uid.uid_expunge(&mut tx, &cmd_data, &storage).await;
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            rx.next().await,
            Some(String::from(
                "a1 BAD [CLIENTBUG] UID EXPUNGE requires a selected mailbox"
            ))
        );
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    #[tokio::test]
    async fn test_uid_expunge_read_only() {
        let data = Data {
            con_state: Connection {
                state: State::Selected("INBOX".to_string(), Access::ReadOnly),
                secure: true,
                username: Some(String::from("test")),
                active_capabilities: vec![],
            },
        };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Uid,
            arguments: &["EXPUNGE", "1:*"],
        };
        let (_config, storage) = erooster_core::test_helpers::setup_test_storage()
            .await
            .unwrap();
        let (mut tx, mut rx) = mpsc::unbounded();
        let uid = Uid { data: &data };
        let res = uid.uid_expunge(&mut tx, &cmd_data, &storage).await;
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            rx.next().await,
            Some(String::from(
                "a1 NO [CANNOT] UID EXPUNGE not permitted in read-only mode"
            ))
        );
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    #[tokio::test]
    async fn test_uid_expunge_no_uid_set() {
        let data = Data {
            con_state: Connection {
                state: State::Selected("INBOX".to_string(), Access::ReadWrite),
                secure: true,
                username: Some(String::from("test")),
                active_capabilities: vec![],
            },
        };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Uid,
            arguments: &["EXPUNGE"],
        };
        let (_config, storage) = erooster_core::test_helpers::setup_test_storage()
            .await
            .unwrap();
        let (mut tx, mut rx) = mpsc::unbounded();
        let uid = Uid { data: &data };
        let res = uid.uid_expunge(&mut tx, &cmd_data, &storage).await;
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            rx.next().await,
            Some(String::from(
                "a1 BAD [CLIENTBUG] UID EXPUNGE requires a UID set argument"
            ))
        );
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    #[tokio::test]
    async fn test_uid_expunge_invalid_uid_set() {
        let data = Data {
            con_state: Connection {
                state: State::Selected("INBOX".to_string(), Access::ReadWrite),
                secure: true,
                username: Some(String::from("test")),
                active_capabilities: vec![],
            },
        };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Uid,
            arguments: &["EXPUNGE", "notauidset!!!"],
        };
        let (_config, storage) = erooster_core::test_helpers::setup_test_storage()
            .await
            .unwrap();
        let (mut tx, mut rx) = mpsc::unbounded();
        let uid = Uid { data: &data };
        let res = uid.uid_expunge(&mut tx, &cmd_data, &storage).await;
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            rx.next().await,
            Some(String::from(
                "a1 BAD [CLIENTBUG] Invalid UID set: notauidset!!!"
            ))
        );
    }
}
