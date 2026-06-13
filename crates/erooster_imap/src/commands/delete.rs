// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::commands::{CommandData, Data};
use erooster_core::backend::storage::{MailStorage, Storage};
use {
    color_eyre::{self, eyre::ContextCompat},
    futures::{Sink, SinkExt},
    tokio::fs,
    tracing::instrument,
};

pub struct Delete<'a> {
    pub data: &'a Data,
}

impl Delete<'_> {
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
        let Some(&mailbox_arg) = command_data.arguments.first() else {
            lines
                .send(format!(
                    "{} BAD [PARSE] Expected exactly one argument (mailbox name)",
                    command_data.tag
                ))
                .await?;
            return Ok(());
        };

        // RFC 9051 §6.3.5: INBOX cannot be deleted.
        if mailbox_arg.to_uppercase() == "INBOX" {
            lines
                .send(format!(
                    "{} NO [CANNOT] INBOX cannot be deleted",
                    command_data.tag
                ))
                .await?;
            return Ok(());
        }

        let folder = mailbox_arg.replace('/', ".");
        let username = self
            .data
            .con_state
            .username
            .clone()
            .context("Username missing in internal State")?;
        let mailbox_path = storage.to_ondisk_path(folder.clone(), username)?;

        if !mailbox_path.exists() {
            lines
                .send(format!(
                    "{} NO [NONEXISTENT] Mailbox \"{}\" does not exist",
                    command_data.tag, mailbox_arg
                ))
                .await?;
            return Ok(());
        }

        // RFC 9051 §6.3.5: if the mailbox has inferior mailboxes it becomes
        // \Noselect; deleting it when it has children should be refused.
        let has_children = mailbox_path
            .read_dir()
            .is_ok_and(|mut d| d.next().is_some());
        if has_children {
            lines
                .send(format!(
                    "{} NO [CANNOT] Mailbox has inferior mailboxes; delete them first",
                    command_data.tag
                ))
                .await?;
            return Ok(());
        }

        fs::remove_dir_all(&mailbox_path).await?;
        lines
            .send(format!("{} OK DELETE completed", command_data.tag))
            .await?;
        Ok(())
    }
}
