use crate::{
    commands::{
        parsers::{
            fetch_arguments, parse_selected_range, FetchArguments, FetchAttributes, Range,
            RangeEnd, SectionText,
        },
        CommandData, Data,
    },
    servers::state::State,
};
use erooster_core::backend::storage::{MailEntry, MailEntryType, MailStorage, Storage};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::sync::Arc;
use tracing::{debug, error, instrument};

pub struct Fetch<'a> {
    pub data: &'a Data,
}

impl Fetch<'_> {
    #[allow(clippy::too_many_lines)]
    #[instrument(skip(self, lines, command_data, storage))]
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        command_data: &CommandData<'_>,
        storage: Arc<Storage>,
        is_uid: bool,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        let offset = if is_uid { 1 } else { 0 };
        // TODO handle the various request types defined in https://www.rfc-editor.org/rfc/rfc9051.html#name-fetch-command
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

            let range = parse_selected_range(command_data.arguments[offset]);
            debug!("Range: {:?}", range);
            match range {
                Ok((_, range)) => {
                    let mut filtered_mails: Vec<MailEntryType> = mails
                        .into_iter()
                        .filter(|mail| {
                            for range in &range {
                                match range {
                                    Range::Single(id) => {
                                        if &mail.uid() == id {
                                            return true;
                                        }
                                    }
                                    Range::Range(start, end) => match end {
                                        RangeEnd::End(end_int) => {
                                            if &mail.uid() >= start && &mail.uid() <= end_int {
                                                return true;
                                            }
                                        }
                                        RangeEnd::All => {
                                            if &mail.uid() >= start {
                                                return true;
                                            }
                                        }
                                    },
                                }
                            }
                            false
                        })
                        .collect::<Vec<MailEntryType>>();

                    let fetch_args = command_data.arguments[1 + offset..].to_vec().join(" ");
                    let fetch_args_str = &fetch_args[1..fetch_args.len() - 1];
                    debug!("Fetch args: {}", fetch_args_str);

                    filtered_mails.sort_by_key(MailEntry::uid);
                    match fetch_arguments(fetch_args_str) {
                        Ok((_, args)) => {
                            for mut mail in filtered_mails {
                                let uid = mail.uid();
                                if let Some(resp) = generate_response(args.clone(), &mut mail) {
                                    if is_uid {
                                        if resp.contains("UID") {
                                            lines
                                                .feed(format!("* {} FETCH ({})", uid, resp))
                                                .await?;
                                        } else {
                                            lines
                                                .feed(format!(
                                                    "* {} FETCH (UID {} {})",
                                                    uid, uid, resp
                                                ))
                                                .await?;
                                        }
                                    } else {
                                        lines.feed(format!("* {} FETCH ({})", uid, resp)).await?;
                                    }
                                }
                            }

                            if is_uid {
                                lines
                                    .feed(format!("{} Ok UID FETCH completed", command_data.tag))
                                    .await?;
                            } else {
                                lines
                                    .feed(format!("{} Ok FETCH completed", command_data.tag))
                                    .await?;
                            }
                            lines.flush().await?;
                        }
                        Err(e) => {
                            error!("Failed to parse fetch arguments: {}", e);
                            lines
                                .send(format!("{} BAD Unable to parse", command_data.tag))
                                .await?;
                        }
                    }
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
        Ok(())
    }
}

#[instrument(skip(arg, mail))]
pub fn generate_response(arg: FetchArguments, mail: &mut MailEntryType) -> Option<String> {
    match arg {
        FetchArguments::All => None,
        FetchArguments::Fast => None,
        FetchArguments::Full => None,
        FetchArguments::Single(single_arg) => generate_response_for_attributes(single_arg, mail),
        FetchArguments::List(args) => {
            let mut resp = String::new();
            for arg in args {
                if let Some(extra_resp) = generate_response_for_attributes(arg, mail) {
                    if resp.is_empty() {
                        resp = extra_resp;
                    } else {
                        resp.push_str(&format!(" {}", extra_resp));
                    }
                }
            }
            Some(resp)
        }
    }
}

#[allow(clippy::too_many_lines)]
#[instrument(skip(attr, mail))]
fn generate_response_for_attributes(
    attr: FetchAttributes,
    mail: &mut MailEntryType,
) -> Option<String> {
    match attr {
        FetchAttributes::Envelope => None,
        FetchAttributes::RFC822Header => {
            if let Ok(headers_vec) = mail.headers() {
                let headers = headers_vec
                    .iter()
                    .map(|header| format!("{}: {}", header.get_key(), header.get_value()))
                    .collect::<Vec<_>>()
                    .join("\r\n");

                Some(format!("RFC822.HEADER{}", headers))
            } else {
                Some(String::from("RFC822.HEADER\r\n"))
            }
        }
        FetchAttributes::Flags => {
            let mut flags = String::new();
            if mail.is_draft() {
                if flags.is_empty() {
                    flags = String::from("\\Draft");
                } else {
                    flags.push_str(" \\Draft");
                }
            }
            if mail.is_flagged() {
                if flags.is_empty() {
                    flags = String::from("\\Flagged");
                } else {
                    flags.push_str(" \\Flagged");
                }
            }
            if mail.is_seen() {
                if flags.is_empty() {
                    flags = String::from("\\Seen");
                } else {
                    flags.push_str(" \\Seen");
                }
            }
            if mail.is_replied() {
                if flags.is_empty() {
                    flags = String::from("\\Answered");
                } else {
                    flags.push_str(" \\Answered");
                }
            }
            if mail.is_trashed() {
                if flags.is_empty() {
                    flags = String::from("\\Deleted");
                } else {
                    flags.push_str(" \\Deleted");
                }
            }

            Some(format!("FLAGS ({})", flags))
        }
        FetchAttributes::InternalDate => None,
        FetchAttributes::RFC822Size => {
            if let Ok(parsed) = mail.parsed() {
                let size = parsed.raw_bytes.len();
                Some(format!("RFC822.SIZE {}", size))
            } else {
                Some(String::from("RFC822.SIZE 0"))
            }
        }
        FetchAttributes::Uid => Some(format!("UID {}", mail.uid())),
        FetchAttributes::BodyStructure => None,
        FetchAttributes::BodySection(section_text, range) => {
            Some(body(section_text, range, mail, true))
        }
        FetchAttributes::BodyPeek(section_text, range) => {
            Some(body(section_text, range, mail, false))
        }
        FetchAttributes::Binary(_, _) => None,
        FetchAttributes::BinaryPeek(_, _) => None,
        FetchAttributes::BinarySize(_) => None,
    }
}

#[allow(clippy::cast_possible_truncation)]
#[instrument(skip(section_text, range, mail))]
fn body(
    section_text: Option<SectionText>,
    range: Option<(u64, u64)>,
    mail: &mut MailEntryType,
    seen: bool,
) -> String {
    if let Some(section_text) = section_text {
        match section_text {
            super::parsers::SectionText::Header => {
                // TODO implement
                String::from("BODY[HEADER] NIL\r\n")
            }
            super::parsers::SectionText::Text => {
                if let Some((start, end)) = range {
                    if let Ok(body) = mail.parsed() {
                        if let Ok(body_text) = body.get_body_raw() {
                            let end = if body_text.len() < (end as usize) {
                                body_text.len()
                            } else {
                                end as usize
                            };
                            let body_text = body_text.get((start as usize)..end).unwrap_or(&[]);
                            let body_text = String::from_utf8_lossy(body_text);
                            format!("BODY[] {{{}}}\r\n{}", body_text.as_bytes().len(), body_text)
                        } else {
                            String::from("BODY[TEXT] NIL\r\n")
                        }
                    } else {
                        String::from("BODY[TEXT] NIL\r\n")
                    }
                } else if let Ok(body) = mail.parsed() {
                    if let Ok(body_text) = body.get_body_raw() {
                        let body_text = String::from_utf8_lossy(&body_text);
                        format!("BODY[] {{{}}}\r\n{}", body_text.as_bytes().len(), body_text)
                    } else {
                        String::from("BODY[TEXT] NIL\r\n")
                    }
                } else {
                    String::from("BODY[TEXT] NIL\r\n")
                }
            }
            super::parsers::SectionText::HeaderFields(headers_requested_vec) => {
                if let Ok(headers_vec) = mail.headers() {
                    let lower_headers_requested_vec: Vec<_> = headers_requested_vec
                        .iter()
                        .map(|header| header.to_lowercase())
                        .collect();
                    let headers = headers_vec
                        .iter()
                        .filter(|header| {
                            lower_headers_requested_vec.contains(&header.get_key().to_lowercase())
                        })
                        .map(|header| format!("{}: {}", header.get_key(), header.get_value()))
                        .collect::<Vec<_>>()
                        .join("\r\n");
                    format!(
                        "BODY[HEADER] {{{}}}\r\n{}",
                        headers.as_bytes().len(),
                        headers
                    )
                } else {
                    String::from("BODY[HEADER] NIL\r\n")
                }
            }
            super::parsers::SectionText::HeaderFieldsNot(headers_requested_vec) => {
                if let Ok(headers_vec) = mail.headers() {
                    let lower_headers_requested_vec: Vec<_> = headers_requested_vec
                        .iter()
                        .map(|header| header.to_lowercase())
                        .collect();

                    let headers = headers_vec
                        .iter()
                        .filter(|header| {
                            !lower_headers_requested_vec.contains(&header.get_key().to_lowercase())
                        })
                        .map(|header| format!("{}: {}", header.get_key(), header.get_value()))
                        .collect::<Vec<_>>()
                        .join("\r\n");
                    let data = format!("{}\r\n", headers);
                    format!("BODY[HEADER] {{{}}}\r\n{}", data.as_bytes().len(), data)
                } else {
                    String::from("BODY[HEADER] NIL\r\n")
                }
            }
        }
    } else if let Ok(mail) = mail.parsed() {
        let mail = String::from_utf8_lossy(mail.raw_bytes);

        format!("BODY[] {{{}}}\r\n{}", mail.as_bytes().len(), mail)
    } else {
        String::from("BODY[] NIL\r\n")
    }
}
