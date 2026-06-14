// SPDX-FileCopyrightText: 2026 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! COPY command (RFC 9051 §6.4.7).
//!
//! Copies the specified messages from the currently selected mailbox to
//! a destination mailbox. Returns a `[COPYUID]` response code so clients
//! can correlate source and destination UIDs.

use crate::{
    commands::{
        parsers::parse_selected_range, select::get_or_create_uidvalidity, CommandData, Data,
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

pub struct Copy<'a> {
    pub data: &'a Data,
}

impl Copy<'_> {
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
                    "{} BAD [CLIENTBUG] COPY requires a selected mailbox",
                    command_data.tag
                ))
                .await?;
            return Ok(());
        };

        if *access == Access::ReadOnly {
            lines
                .send(format!(
                    "{} NO [CANNOT] COPY not permitted in read-only mode",
                    command_data.tag
                ))
                .await?;
            return Ok(());
        }

        let offset = usize::from(is_uid);
        if command_data.arguments.len() < 2 + offset {
            lines
                .send(format!(
                    "{} BAD [PARSE] COPY requires a sequence set and destination mailbox",
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

        let mut src_uids: Vec<u32> = Vec::new();
        let mut dest_uids: Vec<u32> = Vec::new();

        for (index, mail) in mails.iter_mut().enumerate() {
            let in_range = ranges.iter().any(|r| {
                if is_uid {
                    r.contains(&mail.uid())
                } else {
                    #[allow(clippy::cast_possible_truncation)]
                    r.contains(&((index as u32) + 1))
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
            let dest_uid = storage.get_uid_for_folder(&dest_path)?;
            dest_uids.push(dest_uid);
        }

        if src_uids.is_empty() {
            lines
                .send(format!(
                    "{} OK COPY completed (no messages matched)",
                    command_data.tag
                ))
                .await?;
            return Ok(());
        }

        let uidvalidity = get_or_create_uidvalidity(&dest_path).await?;
        let src_uid_str = uid_set_string(&src_uids);
        let dest_uid_str = uid_set_string(&dest_uids);

        lines
            .send(format!(
                "{} OK [COPYUID {uidvalidity} {src_uid_str} {dest_uid_str}] COPY completed",
                command_data.tag
            ))
            .await?;
        Ok(())
    }
}

/// Formats a sorted list of UIDs as a compact RFC sequence set string (e.g. `1,3:5,7`).
pub fn uid_set_string(uids: &[u32]) -> String {
    if uids.is_empty() {
        return String::new();
    }
    let mut parts: Vec<String> = Vec::new();
    let mut start = uids[0];
    let mut end = uids[0];
    for &uid in &uids[1..] {
        if uid != end + 1 {
            if start == end {
                parts.push(start.to_string());
            } else {
                parts.push(format!("{start}:{end}"));
            }
            start = uid;
        }
        end = uid;
    }
    if start == end {
        parts.push(start.to_string());
    } else {
        parts.push(format!("{start}:{end}"));
    }
    parts.join(",")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tokio;

    #[test]
    fn test_uid_set_string_empty() {
        assert_eq!(uid_set_string(&[]), "");
    }

    #[test]
    fn test_uid_set_string_single() {
        assert_eq!(uid_set_string(&[5]), "5");
    }

    #[test]
    fn test_uid_set_string_range() {
        assert_eq!(uid_set_string(&[1, 2, 3]), "1:3");
    }

    #[test]
    fn test_uid_set_string_mixed() {
        assert_eq!(uid_set_string(&[1, 3, 4, 5, 7]), "1,3:5,7");
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    #[tokio::test]
    async fn test_copy_not_selected() {
        let data = Data {
            con_state: crate::servers::state::Connection {
                state: State::Authenticated,
                secure: true,
                username: Some(String::from("test")),
                active_capabilities: vec![],
            },
        };
        let cmd_data = CommandData {
            tag: "a1",
            command: crate::commands::Commands::Copy,
            arguments: &["1:3", "INBOX"],
        };
        let (_config, storage) = erooster_core::test_helpers::setup_test_storage()
            .await
            .unwrap();
        let (mut tx, mut rx) = futures::channel::mpsc::unbounded();
        let res = Copy { data: &data }
            .exec(&mut tx, &storage, &cmd_data, false)
            .await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(
            rx.next().await,
            Some(String::from(
                "a1 BAD [CLIENTBUG] COPY requires a selected mailbox"
            ))
        );
    }

    use futures::StreamExt;
}
