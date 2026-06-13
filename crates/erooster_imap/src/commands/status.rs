// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::commands::select::get_or_create_uidvalidity;
use crate::commands::{CommandData, Data};
use erooster_core::backend::storage::{MailEntry, MailStorage, Storage};
use {
    color_eyre::{self, eyre::ContextCompat},
    futures::{Sink, SinkExt},
    tracing::instrument,
};

pub struct Status<'a> {
    pub data: &'a Data,
}

impl Status<'_> {
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
        let requests = command_data.arguments[1].replace(['(', ')'], "");
        let responses: Vec<_> = requests.split_whitespace().collect();

        let folder_on_disk = command_data.arguments[0];
        let username = self
            .data
            .con_state
            .username
            .clone()
            .context("Username missing in internal State")?;
        let mailbox_path =
            storage.to_ondisk_path((*folder_on_disk).to_string(), username.clone())?;

        let mut parts: Vec<String> = Vec::new();

        if responses.contains(&"MESSAGES") {
            let count = storage.count_cur(&mailbox_path) + storage.count_new(&mailbox_path);
            parts.push(format!("MESSAGES {count}"));
        }
        if responses.contains(&"UNSEEN") {
            let count = storage
                .list_cur(format!("{username}/{folder_on_disk}"), &mailbox_path)
                .await
                .iter()
                .filter(|mail| !mail.is_seen())
                .count()
                + storage.count_new(&mailbox_path);
            parts.push(format!("UNSEEN {count}"));
        }
        if responses.contains(&"UIDNEXT") {
            let current_uid = storage.get_uid_for_folder(&mailbox_path)?;
            parts.push(format!("UIDNEXT {}", current_uid + 1));
        }
        if responses.contains(&"UIDVALIDITY") {
            let uidvalidity = get_or_create_uidvalidity(&mailbox_path).await?;
            parts.push(format!("UIDVALIDITY {uidvalidity}"));
        }
        if responses.contains(&"DELETED") {
            let mails = storage
                .list_cur(format!("{username}/{folder_on_disk}"), &mailbox_path)
                .await;
            let count = mails.iter().filter(|m| m.is_trashed()).count();
            parts.push(format!("DELETED {count}"));
        }
        if responses.contains(&"SIZE") {
            let size: usize = storage
                .list_all(format!("{username}/{folder_on_disk}"), &mailbox_path)
                .await
                .iter_mut()
                .map(|mail| {
                    if let Ok(parsed) = mail.parsed() {
                        parsed.raw_bytes.len()
                    } else {
                        0
                    }
                })
                .sum();
            parts.push(format!("SIZE {size}"));
        }

        let values = parts.join(" ");
        lines
            .feed(format!("* STATUS {folder_on_disk} ({values})"))
            .await?;
        lines
            .feed(format!("{} OK STATUS completed", command_data.tag))
            .await?;
        lines.flush().await?;
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
    async fn test_status_messages_and_unseen() {
        let data = Data {
            con_state: Connection {
                state: State::Selected("INBOX".to_string(), Access::ReadWrite),
                secure: true,
                username: Some(String::from("test_status_user")),
                active_capabilities: vec![],
            },
        };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Status,
            arguments: &["INBOX", "(MESSAGES UNSEEN UIDNEXT)"],
        };
        let (_config, storage) = erooster_core::test_helpers::setup_test_storage()
            .await
            .unwrap();
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = Status { data: &data }.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok(), "{:?}", res);

        let untagged = rx.next().await.unwrap_or_default();
        // Response must include all three items with spaces between them and no duplicates
        assert!(
            untagged.starts_with("* STATUS INBOX ("),
            "unexpected response: {untagged}"
        );
        assert!(untagged.contains("MESSAGES "), "MESSAGES missing: {untagged}");
        assert!(untagged.contains("UNSEEN "), "UNSEEN missing: {untagged}");
        assert!(untagged.contains("UIDNEXT "), "UIDNEXT missing: {untagged}");
        // Ensure items are space-separated (no two keywords directly adjacent)
        let inner = untagged
            .trim_start_matches("* STATUS INBOX (")
            .trim_end_matches(')');
        let parts: Vec<_> = inner.split_whitespace().collect();
        // parts should alternate keyword/value: ["MESSAGES","0","UNSEEN","0","UIDNEXT","1"]
        assert_eq!(parts.len(), 6, "wrong item count: {inner}");

        let tagged = rx.next().await.unwrap_or_default();
        assert_eq!(tagged, "a1 OK STATUS completed");
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    #[tokio::test]
    async fn test_status_uidvalidity_stable() {
        let data = Data {
            con_state: Connection {
                state: State::Selected("INBOX".to_string(), Access::ReadWrite),
                secure: true,
                username: Some(String::from("test_status_uid_user")),
                active_capabilities: vec![],
            },
        };
        let cmd_data = CommandData {
            tag: "b1",
            command: Commands::Status,
            arguments: &["INBOX", "(UIDVALIDITY)"],
        };
        let (_config, storage) = erooster_core::test_helpers::setup_test_storage()
            .await
            .unwrap();

        // Call STATUS twice — UIDVALIDITY must be the same both times.
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = Status { data: &data }.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok(), "{:?}", res);
        let first = rx.next().await.unwrap_or_default();

        let (mut tx2, mut rx2) = mpsc::unbounded();
        let res = Status { data: &data }.exec(&mut tx2, &storage, &cmd_data).await;
        assert!(res.is_ok(), "{:?}", res);
        let second = rx2.next().await.unwrap_or_default();

        assert_eq!(first, second, "UIDVALIDITY must be stable across calls");
    }
}
