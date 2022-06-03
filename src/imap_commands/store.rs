use crate::{
    backend::storage::{MailEntry, MailEntryType, MailStorage, Storage},
    config::Config,
    imap_commands::{CommandData, Data},
};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::{path::Path, sync::Arc};
use tracing::{error, instrument};

pub struct Store<'a> {
    pub data: &'a Data,
}
impl Store<'_> {
    #[instrument(skip(self, lines, config, storage, command_data))]
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        config: Arc<Config>,
        storage: Arc<Storage>,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        let arguments = &command_data.arguments;
        assert!(arguments.len() >= 2);
        if arguments.len() >= 2 {
            let mut folder = arguments[0].replace('/', ".");
            folder.insert(0, '.');
            folder.remove_matches('"');
            let mailbox_path = Path::new(&config.mail.maildir_folders)
                .join(self.data.con_state.read().await.username.clone().unwrap())
                .join(folder.clone());
            let mailbox_path_string = mailbox_path
                .into_os_string()
                .into_string()
                .expect("Failed to convert path. Your system may be incompatible");
            let mails: Vec<MailEntryType> = storage.list_all(mailbox_path_string.clone()).await;

            let filtered_mails: Vec<MailEntryType> = if command_data.arguments[0].contains(':') {
                let range = command_data.arguments[0].split(':').collect::<Vec<_>>();
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
                let wanted_id = command_data.arguments[0].parse::<i64>().unwrap_or(1);
                mails
                    .into_iter()
                    .filter(|mail| mail.uid() == wanted_id)
                    .collect()
            };
            let action = arguments[1];

            let flags = command_data.arguments[2..].to_vec();
            let flags_string = flags.join(" ");
            if action.to_lowercase() == "flags" {
                for mail in filtered_mails {
                    let mut path = mail.path().clone();
                    path.pop();
                    let path = path
                        .into_os_string()
                        .into_string()
                        .expect("Failed to convert path. Your system may be incompatible");
                    if path.ends_with("new") {
                        if let Err(e) = storage.move_new_to_cur_with_flags(
                            mailbox_path_string.clone(),
                            mail.id(),
                            &flags,
                        ) {
                            error!("Failed to store flags or move email {}: {}", mail.id(), e);
                        }
                    } else if let Err(e) =
                        storage.set_flags(mailbox_path_string.clone(), mail.id(), &flags)
                    {
                        error!("Failed to store flags or move email {}: {}", mail.id(), e);
                    }

                    lines
                        .feed(format!("* {} FETCH (FLAGS ({}))", mail.uid(), flags_string))
                        .await?;
                }
            } else if action.to_lowercase() == "flags.silent" {
                for mail in filtered_mails {
                    let mut path = mail.path().clone();
                    path.pop();
                    let path = path
                        .into_os_string()
                        .into_string()
                        .expect("Failed to convert path. Your system may be incompatible");
                    if path.ends_with("new") {
                        if let Err(e) = storage.move_new_to_cur_with_flags(
                            mailbox_path_string.clone(),
                            mail.id(),
                            &flags,
                        ) {
                            error!("Failed to store flags or move email {}: {}", mail.id(), e);
                        }
                    } else if let Err(e) =
                        storage.set_flags(mailbox_path_string.clone(), mail.id(), &flags)
                    {
                        error!("Failed to store flags or move email {}: {}", mail.id(), e);
                    }
                }
            } else if action.to_lowercase() == "+flags" {
                for mail in filtered_mails {
                    let mut path = mail.path().clone();
                    path.pop();
                    let path = path
                        .into_os_string()
                        .into_string()
                        .expect("Failed to convert path. Your system may be incompatible");
                    if path.ends_with("new") {
                        if let Err(e) = storage.move_new_to_cur_with_flags(
                            mailbox_path_string.clone(),
                            mail.id(),
                            &flags,
                        ) {
                            error!("Failed to store flags or move email {}: {}", mail.id(), e);
                        }
                    } else if let Err(e) =
                        storage.add_flags(mailbox_path_string.clone(), mail.id(), &flags)
                    {
                        error!("Failed to store flags or move email {}: {}", mail.id(), e);
                    }

                    lines
                        .feed(format!("* {} FETCH (FLAGS ({}))", mail.uid(), flags_string))
                        .await?;
                }
            } else if action.to_lowercase() == "+flags.silent" {
                for mail in filtered_mails {
                    let mut path = mail.path().clone();
                    path.pop();
                    let path = path
                        .into_os_string()
                        .into_string()
                        .expect("Failed to convert path. Your system may be incompatible");
                    if path.ends_with("new") {
                        if let Err(e) = storage.move_new_to_cur_with_flags(
                            mailbox_path_string.clone(),
                            mail.id(),
                            &flags,
                        ) {
                            error!("Failed to store flags or move email {}: {}", mail.id(), e);
                        }
                    } else if let Err(e) =
                        storage.add_flags(mailbox_path_string.clone(), mail.id(), &flags)
                    {
                        error!("Failed to store flags or move email {}: {}", mail.id(), e);
                    }
                }
            } else if action.to_lowercase() == "-flags" {
                for mail in filtered_mails {
                    let mut current_flags = vec![];
                    if mail.is_replied() {
                        current_flags.push("\\Answered");
                    } else if mail.is_trashed() {
                        current_flags.push("\\Deleted");
                    } else if mail.is_draft() {
                        current_flags.push("\\Draft");
                    } else if mail.is_seen() {
                        current_flags.push("\\Seen");
                    } else if mail.is_flagged() {
                        current_flags.push("\\Flagged");
                    }

                    let new_flags = current_flags
                        .into_iter()
                        .filter(|x| !flags.contains(x))
                        .collect::<Vec<_>>();

                    if let Err(e) =
                        storage.remove_flags(mailbox_path_string.clone(), mail.id(), &flags)
                    {
                        error!("Failed to store flags or move email {}: {}", mail.id(), e);
                    }

                    lines
                        .feed(format!(
                            "* {} FETCH (FLAGS ({}))",
                            mail.uid(),
                            new_flags.join(" ")
                        ))
                        .await?;
                }
            } else if action.to_lowercase() == "-flags.silent" {
                for mail in filtered_mails {
                    if let Err(e) =
                        storage.remove_flags(mailbox_path_string.clone(), mail.id(), &flags)
                    {
                        error!("Failed to store flags or move email {}: {}", mail.id(), e);
                    }
                }
            } else {
                lines
                    .send(format!(
                        "{} BAD [SERVERBUG] invalid arguments",
                        command_data.tag
                    ))
                    .await?;
                return Ok(());
            }
            lines
                .feed(format!("{} Ok STORE completed", command_data.tag))
                .await?;
            lines.flush().await?;
        } else {
            lines
                .send(format!(
                    "{} BAD [SERVERBUG] invalid arguments",
                    command_data.tag
                ))
                .await?;
        }
        Ok(())
    }
}
