use crate::{
    backend::storage::{MailEntry, MailEntryType, MailStorage, Storage},
    config::Config,
    imap_commands::{
        parsers::{fetch_arguments, FetchArguments, FetchAttributes},
        CommandData, Data,
    },
    imap_servers::state::State,
};
use futures::{channel::mpsc::SendError, stream, Sink, SinkExt, StreamExt};
use std::{path::Path, sync::Arc};
use tracing::debug;

pub struct Uid<'a> {
    pub data: &'a Data,
}

impl Uid<'_> {
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        config: Arc<Config>,
        command_data: &CommandData<'_>,
        storage: Arc<Storage>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        debug!("Arguments: {:?}", command_data.arguments);
        if command_data.arguments[0].to_lowercase() == "fetch" {
            // TODO handle the various request types defined in https://www.rfc-editor.org/rfc/rfc9051.html#name-fetch-command
            // TODO handle * as "everything"
            // TODO make this code also available to the pure FETCH command
            if let State::Selected(folder, _) = &self.data.con_state.read().await.state {
                let mut folder = folder.replace('/', ".");
                folder.insert(0, '.');
                folder.remove_matches('"');
                folder = folder.replace(".INBOX", "INBOX");
                let mailbox_path = Path::new(&config.mail.maildir_folders)
                    .join(self.data.con_state.read().await.username.clone().unwrap())
                    .join(folder.clone());
                let mails: Vec<MailEntryType> = storage.list_all(
                    mailbox_path
                        .into_os_string()
                        .into_string()
                        .expect("Failed to convert path. Your system may be incompatible"),
                );

                let range = command_data.arguments[1].split(':').collect::<Vec<_>>();
                let start = range[0].parse::<i64>().unwrap_or(1);
                let end = range[1];
                let end_int = end.parse::<i64>().unwrap_or(i64::max_value());
                let filtered_mails: Vec<MailEntryType> = if end == "*" {
                    stream::iter(mails)
                        .filter_map(|mail: MailEntryType| async {
                            if let Ok(id) = mail.uid().await {
                                (id >= start).then(|| mail)
                            } else {
                                None
                            }
                        })
                        .collect()
                        .await
                } else {
                    stream::iter(mails)
                        .filter_map(|mail: MailEntryType| async move {
                            if let Ok(id) = mail.uid().await {
                                (id >= start && id <= end_int).then(|| mail)
                            } else {
                                None
                            }
                        })
                        .collect()
                        .await
                };

                let fetch_args = command_data.arguments[2..].to_vec().join(" ");
                let (_, args) =
                    fetch_arguments(fetch_args).expect("Failed to parse fetch arguments");
                debug!("Fetch args: {:?}", args);
                for mail in filtered_mails {
                    let uid = mail.uid().await?;
                    if let Some(resp) = generate_response(args.clone(), &mail).await {
                        lines
                            .feed(format!("* {} FETCH (UID {} {})", uid, uid, resp))
                            .await?;
                    }
                }

                lines
                    .feed(format!("{} Ok UID FETCH completed", command_data.tag))
                    .await?;
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
        }
        Ok(())
    }
}

async fn generate_response(args: Vec<FetchArguments>, mail: &MailEntryType) -> Option<String> {
    let mut resp = None;
    for arg in args {
        resp = match arg {
            FetchArguments::All => None,
            FetchArguments::Fast => None,
            FetchArguments::Full => None,
            FetchArguments::Single(single_arg) => {
                generate_response_for_attributes(single_arg, mail).await
            }
            FetchArguments::List(args) => {
                let mut resp = String::new();
                for arg in args {
                    if let Some(extra_resp) = generate_response_for_attributes(arg, mail).await {
                        resp = format!("{}\r\n{}", resp, extra_resp);
                    }
                }
                Some(resp)
            }
        }
    }
    resp
}

async fn generate_response_for_attributes(
    attr: FetchAttributes,
    mail: &MailEntryType,
) -> Option<String> {
    match attr {
        FetchAttributes::Envelope => None,
        FetchAttributes::Flags => {
            let mut flags = String::new();
            if mail.is_draft() {
                flags = format!("{} \\Draft", flags);
            }
            if mail.is_flagged() {
                flags = format!("{} \\Flagged", flags);
            }
            if mail.is_seen() {
                flags = format!("{} \\Seen", flags);
            }
            if mail.is_replied() {
                flags = format!("{} \\Answered", flags);
            }
            if mail.is_trashed() {
                flags = format!("{} \\Deleted", flags);
            }

            Some(format!("FLAGS ({})", flags))
        }
        FetchAttributes::InternalDate => None,
        FetchAttributes::RFC822Size => None,
        FetchAttributes::Uid => None,
        FetchAttributes::BodyStructure => None,
        FetchAttributes::BodySection(_, _) => None,
        FetchAttributes::BodyPeek(_, _) => None,
        FetchAttributes::Binary(_, _) => None,
        FetchAttributes::BinaryPeek(_, _) => None,
        FetchAttributes::BinarySize(_) => None,
    }
}
