// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::{
    commands::{CommandData, Data},
    servers::state::State,
};
use erooster_core::backend::storage::{MailEntry, MailEntryType, MailStorage, Storage};
use erooster_deps::{
    color_eyre::{self, eyre::ContextCompat},
    futures::{Sink, SinkExt},
    tracing::{self, debug, error, instrument},
};

pub struct Store<'a> {
    pub data: &'a Data,
}
impl Store<'_> {
    #[instrument(skip(self, lines, storage, command_data))]
    pub async fn exec<S, E>(
        &self,
        lines: &mut S,
        storage: &Storage,
        command_data: &CommandData<'_>,
        uid: bool,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        let offset = usize::from(uid);
        let arguments = &command_data.arguments;
        assert!(arguments.len() >= 2 + offset);
        if arguments.len() >= 2 + offset {
            if let State::Selected(folder, _) = &self.data.con_state.state {
                let folder = folder.replace('/', ".");
                let username = self
                    .data
                    .con_state
                    .username
                    .clone()
                    .context("Username missing in internal State")?;
                let mailbox_path = storage.to_ondisk_path(folder.clone(), username.clone())?;
                let mails: Vec<MailEntryType> = storage
                    .list_all(format!("{username}/{folder}"), &mailbox_path)
                    .await;

                let filtered_mails: Vec<MailEntryType> =
                    if command_data.arguments[offset].contains(':') {
                        let range = command_data.arguments[offset]
                            .split(':')
                            .collect::<Vec<_>>();
                        let start = range[0].parse::<u32>().unwrap_or(1);
                        let end = range[1];
                        let end_int = end.parse::<u32>().unwrap_or(u32::max_value());
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
                        let wanted_id = command_data.arguments[offset].parse::<u32>().unwrap_or(1);
                        mails
                            .into_iter()
                            .filter(|mail| mail.uid() == wanted_id)
                            .collect()
                    };
                let action = arguments[1 + offset];

                let flags = command_data.arguments[2 + offset..].to_vec();
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
                            if let Err(e) =
                                storage.move_new_to_cur_with_flags(&mailbox_path, mail.id(), &flags)
                            {
                                error!("Failed to store flags or move email {}: {}", mail.id(), e);
                            }
                        } else if let Err(e) = storage.set_flags(&mailbox_path, mail.id(), &flags) {
                            error!("Failed to store flags or move email {}: {}", mail.id(), e);
                        }

                        if uid {
                            lines
                                .feed(format!(
                                    "* {} FETCH (UID {} FLAGS {})",
                                    mail.uid(),
                                    mail.uid(),
                                    flags_string
                                ))
                                .await?;
                        } else {
                            lines
                                .feed(format!("* {} FETCH (FLAGS {flags_string})", mail.uid()))
                                .await?;
                        }
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
                            if let Err(e) =
                                storage.move_new_to_cur_with_flags(&mailbox_path, mail.id(), &flags)
                            {
                                error!("Failed to store flags or move email {}: {}", mail.id(), e);
                            }
                        } else if let Err(e) = storage.set_flags(&mailbox_path, mail.id(), &flags) {
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
                        debug!("Path: {}", path);
                        if path.ends_with("new") {
                            if let Err(e) =
                                storage.move_new_to_cur_with_flags(&mailbox_path, mail.id(), &flags)
                            {
                                error!("Failed to store flags or move email {}: {}", mail.id(), e);
                            }
                        } else if let Err(e) = storage.add_flags(&mailbox_path, mail.id(), &flags) {
                            error!("Failed to store flags or move email {}: {}", mail.id(), e);
                        }

                        if uid {
                            lines
                                .feed(format!(
                                    "* {} FETCH (UID {} FLAGS {})",
                                    mail.uid(),
                                    mail.uid(),
                                    flags_string
                                ))
                                .await?;
                        } else {
                            lines
                                .feed(format!("* {} FETCH (FLAGS {flags_string})", mail.uid()))
                                .await?;
                        }
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
                            if let Err(e) =
                                storage.move_new_to_cur_with_flags(&mailbox_path, mail.id(), &flags)
                            {
                                error!("Failed to store flags or move email {}: {}", mail.id(), e);
                            }
                        } else if let Err(e) = storage.add_flags(&mailbox_path, mail.id(), &flags) {
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
                            .iter()
                            .copied()
                            .filter_map(|x| {
                                let x = x.replace(['(', ')'], "");
                                (!flags.contains(&x.as_str())).then_some(x)
                            })
                            .collect::<Vec<_>>();

                        if let Err(e) = storage.remove_flags(&mailbox_path, mail.id(), &flags) {
                            error!("Failed to store flags or move email {}: {}", mail.id(), e);
                        }

                        if uid {
                            lines
                                .feed(format!(
                                    "* {} FETCH (UID {} FLAGS ({}))",
                                    mail.uid(),
                                    mail.uid(),
                                    new_flags.join(" ")
                                ))
                                .await?;
                        } else {
                            lines
                                .feed(format!(
                                    "* {} FETCH (FLAGS ({}))",
                                    mail.uid(),
                                    new_flags.join(" ")
                                ))
                                .await?;
                        }
                    }
                } else if action.to_lowercase() == "-flags.silent" {
                    for mail in filtered_mails {
                        if let Err(e) = storage.remove_flags(&mailbox_path, mail.id(), &flags) {
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
                if uid {
                    lines
                        .feed(format!("{} Ok UID STORE completed", command_data.tag))
                        .await?;
                } else {
                    lines
                        .feed(format!("{} Ok STORE completed", command_data.tag))
                        .await?;
                }
                lines.flush().await?;
            } else {
                lines
                    .feed(format!(
                        "{} NO [TRYCREATE] No mailbox selected",
                        command_data.tag
                    ))
                    .await?;
                lines.flush().await?;
            }
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
