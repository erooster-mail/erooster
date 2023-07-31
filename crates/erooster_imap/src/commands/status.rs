use crate::commands::{CommandData, Data};
use color_eyre::eyre::ContextCompat;
use erooster_core::backend::storage::{MailEntry, MailStorage, Storage};
use futures::{Sink, SinkExt};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::instrument;

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
            .read()
            .await
            .username
            .clone()
            .context("Username missing in internal State")?;
        let mailbox_path =
            storage.to_ondisk_path((*folder_on_disk).to_string(), username.clone())?;

        let mut values = String::new();
        if responses.contains(&"MESSAGES") {
            let count = storage.count_cur(&mailbox_path) + storage.count_new(&mailbox_path);
            values.push_str(&format!("MESSAGES {count}"));
        }
        if responses.contains(&"UNSEEN") {
            let count = storage
                .list_cur(format!("{username}/{folder_on_disk}"), &mailbox_path)
                .await
                .iter()
                .filter(|mail| !mail.is_seen())
                .count()
                + storage.count_new(&mailbox_path);
            values.push_str(&format!("UNSEEN {count}"));
        }
        if responses.contains(&"UIDNEXT") {
            let current_uid = storage.get_uid_for_folder(&mailbox_path)?;
            values.push_str(&format!("UIDNEXT {}", current_uid + 1));
        }
        if responses.contains(&"UIDVALIDITY") {
            let current_time = SystemTime::now();
            let unix_timestamp = current_time.duration_since(UNIX_EPOCH)?;
            #[allow(clippy::cast_possible_truncation)]
            let timestamp = unix_timestamp.as_millis() as u32;

            values.push_str(&format!("UIDVALIDITY {timestamp}"));
        }
        if responses.contains(&"UNSEEN") {
            let mails = storage
                .list_cur(format!("{username}/{folder_on_disk}"), &mailbox_path)
                .await;
            let count =
                mails.iter().filter(|m| !m.is_seen()).count() + storage.count_new(&mailbox_path);
            values.push_str(&format!("UNSEEN {count}"));
        }
        if responses.contains(&"DELETED") {
            let mails = storage
                .list_cur(format!("{username}/{folder_on_disk}"), &mailbox_path)
                .await;
            let count = mails.iter().filter(|m| m.is_trashed()).count();
            values.push_str(&format!("DELETED {count}"));
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
            values.push_str(&format!("SIZE {size}"));
        }
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
