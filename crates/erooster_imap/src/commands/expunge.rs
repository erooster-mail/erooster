// SPDX-FileCopyrightText: 2026 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! EXPUNGE command (RFC 9051 §6.4.3).
//!
//! Permanently removes all messages with the \Deleted flag from the currently
//! selected mailbox. Sends `* N EXPUNGE` for each removed message where N is
//! the current (post-shift) sequence number at the time of removal.

use crate::{
    commands::{CommandData, Data},
    servers::state::{Access, State},
};
use erooster_core::backend::storage::{MailEntry, MailStorage, Storage};
use {
    color_eyre::{self, eyre::ContextCompat},
    futures::{Sink, SinkExt},
    tokio::fs,
    tracing::instrument,
};

pub struct Expunge<'a> {
    pub data: &'a Data,
}

impl Expunge<'_> {
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
        let State::Selected(folder, access) = &self.data.con_state.state else {
            lines
                .send(format!(
                    "{} BAD [CLIENTBUG] EXPUNGE requires a selected mailbox",
                    command_data.tag
                ))
                .await?;
            return Ok(());
        };

        if access == &Access::ReadOnly {
            lines
                .send(format!(
                    "{} NO [CANNOT] EXPUNGE not permitted in read-only mode",
                    command_data.tag
                ))
                .await?;
            return Ok(());
        }

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

        // Walk messages in order; sequence numbers start at 1. When a message
        // is deleted, all subsequent numbers shift down — so we do NOT advance
        // the counter for deleted messages.
        let mut seq = 1u32;
        for mail in mails {
            if mail.is_trashed() {
                fs::remove_file(mail.path()).await?;
                lines.feed(format!("* {seq} EXPUNGE")).await?;
                // seq stays the same — the next message slides into this slot
            } else {
                seq += 1;
            }
        }

        lines.flush().await?;
        lines
            .send(format!("{} OK EXPUNGE completed", command_data.tag))
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
    async fn test_expunge_not_selected() {
        let data = Data {
            con_state: Connection {
                state: State::NotAuthenticated,
                secure: true,
                username: None,
                active_capabilities: vec![],
            },
        };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Expunge,
            arguments: &[],
        };
        let (_config, storage) = erooster_core::test_helpers::setup_test_storage()
            .await
            .unwrap();
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = Expunge { data: &data }
            .exec(&mut tx, &storage, &cmd_data)
            .await;
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            rx.next().await,
            Some(String::from(
                "a1 BAD [CLIENTBUG] EXPUNGE requires a selected mailbox"
            ))
        );
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    #[tokio::test]
    async fn test_expunge_read_only() {
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
            command: Commands::Expunge,
            arguments: &[],
        };
        let (_config, storage) = erooster_core::test_helpers::setup_test_storage()
            .await
            .unwrap();
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = Expunge { data: &data }
            .exec(&mut tx, &storage, &cmd_data)
            .await;
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            rx.next().await,
            Some(String::from(
                "a1 NO [CANNOT] EXPUNGE not permitted in read-only mode"
            ))
        );
    }
}
