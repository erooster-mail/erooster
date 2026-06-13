// SPDX-FileCopyrightText: 2026 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! MOVE command (RFC 6851, folded into RFC 9051 §6.4.8).
//!
//! Atomically copies messages to a destination mailbox, sends
//! `* OK [COPYUID ...]` with source/destination UID mapping, then
//! expunges the source messages and sends the corresponding `* N EXPUNGE`
//! responses.

use crate::{
    commands::{
        copy::uid_set_string, parsers::parse_selected_range,
        select::get_or_create_uidvalidity, CommandData, Data,
    },
    servers::state::{Access, State},
};
use erooster_core::backend::storage::{maildir::MaildirMailEntry, MailEntry, MailStorage, Storage};
use {
    color_eyre::{self, eyre::ContextCompat},
    futures::{Sink, SinkExt},
    nom::Finish,
    tokio::fs,
    tracing::instrument,
};

pub struct Move<'a> {
    pub data: &'a Data,
}

impl Move<'_> {
    #[allow(clippy::too_many_lines)]
    #[instrument(skip(self, lines, storage, command_data))]
    pub async fn exec<S, E>(
        &self,
        lines: &mut S,
        storage: &Storage,
        command_data: &CommandData<'_>,
        is_uid: bool,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        let State::Selected(folder, access) = &self.data.con_state.state else {
            lines
                .send(format!(
                    "{} BAD [CLIENTBUG] MOVE requires a selected mailbox",
                    command_data.tag
                ))
                .await?;
            return Ok(());
        };

        if *access == Access::ReadOnly {
            lines
                .send(format!(
                    "{} NO [CANNOT] MOVE not permitted in read-only mode",
                    command_data.tag
                ))
                .await?;
            return Ok(());
        }

        let offset = usize::from(is_uid);
        if command_data.arguments.len() < 2 + offset {
            lines
                .send(format!(
                    "{} BAD [PARSE] MOVE requires a sequence set and destination mailbox",
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

        let src_path = storage.to_ondisk_path(folder.clone(), username.clone())?;

        let mut mails = storage
            .list_all(format!("{username}/{folder}"), &src_path)
            .await;
        mails.sort_by_key(MaildirMailEntry::uid);

        let seq_arg = command_data.arguments[offset];
        let Ok((_, ranges)) = parse_selected_range(seq_arg).finish() else {
            lines
                .send(format!(
                    "{} BAD [PARSE] Invalid sequence set: {seq_arg}",
                    command_data.tag
                ))
                .await?;
            return Ok(());
        };

        let dest_raw = command_data.arguments[1 + offset].replace('"', "");
        let dest_path = storage.to_ondisk_path(dest_raw.clone(), username.clone())?;

        if !dest_path.exists() {
            lines
                .send(format!(
                    "{} NO [TRYCREATE] Destination mailbox does not exist",
                    command_data.tag
                ))
                .await?;
            return Ok(());
        }

        let dest_db_name = format!("{username}/{dest_raw}");

        // Identify matching messages (collect paths for later deletion).
        let mut src_uids: Vec<u32> = Vec::new();
        let mut dest_uids: Vec<u32> = Vec::new();
        let mut src_indices: Vec<u32> = Vec::new(); // 1-based sequence numbers
        let mut paths_to_remove = Vec::new();

        for (index, mail) in mails.iter_mut().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let seq = (index as u32) + 1;
            let in_range = ranges.iter().any(|r| {
                if is_uid {
                    r.contains(&mail.uid())
                } else {
                    r.contains(&seq)
                }
            });
            if !in_range {
                continue;
            }

            let bytes = fs::read(mail.path()).await?;
            let flags_str = mail.flags().to_string();
            let imap_flags: Vec<String> = flags_str
                .split(',')
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .collect();

            storage
                .store_cur_with_flags(dest_db_name.clone(), &dest_path, &bytes, imap_flags)
                .await?;

            src_uids.push(mail.uid());
            dest_uids.push(storage.get_uid_for_folder(&dest_path)?);
            src_indices.push(seq);
            paths_to_remove.push(mail.path().clone());
        }

        if src_uids.is_empty() {
            lines
                .send(format!(
                    "{} OK MOVE completed (no messages matched)",
                    command_data.tag
                ))
                .await?;
            return Ok(());
        }

        let uidvalidity = get_or_create_uidvalidity(&dest_path).await?;
        let src_uid_str = uid_set_string(&src_uids);
        let dest_uid_str = uid_set_string(&dest_uids);

        // RFC 6851 §3: send COPYUID before EXPUNGE responses.
        lines
            .feed(format!(
                "* OK [COPYUID {uidvalidity} {src_uid_str} {dest_uid_str}]"
            ))
            .await?;

        // Delete source messages and send EXPUNGE.  Sequence numbers shift
        // down after each removal, so we use the enumerate index as the offset.
        for (offset, (path, seq)) in paths_to_remove.iter().zip(src_indices.iter()).enumerate() {
            fs::remove_file(path).await?;
            #[allow(clippy::cast_possible_truncation)]
            let adjusted_seq = seq - offset as u32;
            lines.feed(format!("* {adjusted_seq} EXPUNGE")).await?;
        }

        lines.flush().await?;
        lines
            .send(format!("{} OK MOVE completed", command_data.tag))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::commands::{CommandData, Commands};
    use crate::servers::state::{Connection, State};
    use futures::{channel::mpsc, StreamExt};
    use tokio;

    #[cfg_attr(coverage_nightly, coverage(off))]
    #[tokio::test]
    async fn test_move_not_selected() {
        let data = Data {
            con_state: Connection {
                state: State::Authenticated,
                secure: true,
                username: Some(String::from("test")),
                active_capabilities: vec![],
            },
        };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Move,
            arguments: &["1:3", "INBOX"],
        };
        let (_config, storage) = erooster_core::test_helpers::setup_test_storage()
            .await
            .unwrap();
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = Move { data: &data }
            .exec(&mut tx, &storage, &cmd_data, false)
            .await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(
            rx.next().await,
            Some(String::from(
                "a1 BAD [CLIENTBUG] MOVE requires a selected mailbox"
            ))
        );
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    #[tokio::test]
    async fn test_move_read_only() {
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
            command: Commands::Move,
            arguments: &["1", "Trash"],
        };
        let (_config, storage) = erooster_core::test_helpers::setup_test_storage()
            .await
            .unwrap();
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = Move { data: &data }
            .exec(&mut tx, &storage, &cmd_data, false)
            .await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(
            rx.next().await,
            Some(String::from(
                "a1 NO [CANNOT] MOVE not permitted in read-only mode"
            ))
        );
    }
}
