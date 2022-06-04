use crate::{
    commands::{
        fetch::generate_response, parsers::fetch_arguments, store::Store, CommandData, Data,
    },
    servers::state::State,
};
use erooster_core::backend::storage::{MailEntry, MailEntryType, MailStorage, Storage};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::sync::Arc;
use tracing::{debug, error, instrument};

pub struct Uid<'a> {
    pub data: &'a Data,
}

impl Uid<'_> {
    #[allow(clippy::too_many_lines)]
    #[instrument(skip(self, lines, command_data, storage))]
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        command_data: &CommandData<'_>,
        storage: Arc<Storage>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        if command_data.arguments[0].to_lowercase() == "fetch" {
            if let State::Selected(folder, _) = &self.data.con_state.read().await.state {
                let folder = folder.replace('/', ".");
                let mailbox_path = storage.to_ondisk_path(
                    folder.clone(),
                    self.data.con_state.read().await.username.clone().unwrap(),
                )?;
                let mails: Vec<MailEntryType> = storage
                    .list_all(
                        mailbox_path
                            .into_os_string()
                            .into_string()
                            .expect("Failed to convert path. Your system may be incompatible"),
                    )
                    .await;

                let mut filtered_mails: Vec<MailEntryType> =
                    if command_data.arguments[1].contains(':') {
                        let range = command_data.arguments[1].split(':').collect::<Vec<_>>();
                        let start = range[0].parse::<i64>().unwrap_or(1);
                        let end = range[1];
                        let end_int = end.parse::<i64>().unwrap_or(i64::max_value());
                        if end == "*" {
                            mails
                                .into_iter()
                                .filter(|mail| mail.uid() >= start)
                                .collect()
                        } else {
                            mails
                                .into_iter()
                                .filter(|mail| mail.uid() >= start && mail.uid() <= end_int)
                                .collect()
                        }
                    } else {
                        let wanted_id = command_data.arguments[1].parse::<i64>().unwrap_or(1);
                        mails
                            .into_iter()
                            .filter(|mail| mail.uid() == wanted_id)
                            .collect()
                    };

                let fetch_args = command_data.arguments[2..].to_vec().join(" ");
                let fetch_args_str = &fetch_args[1..fetch_args.len() - 1];
                debug!("Fetch args: {}", fetch_args_str);

                filtered_mails.sort_by_key(MailEntry::uid);
                match fetch_arguments(fetch_args_str) {
                    Ok((_, args)) => {
                        for mut mail in filtered_mails {
                            let uid = mail.uid();
                            if let Some(resp) = generate_response(args.clone(), &mut mail) {
                                lines
                                    .feed(format!("* {} FETCH (UID {} {})", uid, uid, resp))
                                    .await?;
                            }
                        }

                        lines
                            .feed(format!("{} Ok UID FETCH completed", command_data.tag))
                            .await?;
                        lines.flush().await?;
                    }
                    Err(e) => {
                        error!("Failed to parse fetch arguments: {}", e);
                        lines
                            .send(format!("{} BAD Unable to parse", command_data.tag))
                            .await?;
                    }
                }
            } else {
                lines
                    .feed(format!(
                        "{} NO [TRYCREATE] No mailbox selected",
                        command_data.tag
                    ))
                    .await?;
                lines.flush().await?;
            }
        } else if command_data.arguments[0].to_lowercase() == "copy" {
            // TODO implement other commands
            lines
                .send(format!("{} BAD Not supported", command_data.tag))
                .await?;
        } else if command_data.arguments[0].to_lowercase() == "move" {
            lines
                .send(format!("{} BAD Not supported", command_data.tag))
                .await?;
        } else if command_data.arguments[0].to_lowercase() == "expunge" {
            lines
                .send(format!("{} BAD Not supported", command_data.tag))
                .await?;
        } else if command_data.arguments[0].to_lowercase() == "search" {
            lines
                .send(format!("{} BAD Not supported", command_data.tag))
                .await?;
        } else if command_data.arguments[0].to_lowercase() == "store" {
            Store { data: self.data }
                .exec(lines, storage, command_data, true)
                .await?;
        }
        Ok(())
    }
}
