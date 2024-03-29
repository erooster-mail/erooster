// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::{
    commands::{
        parsers::{
            fetch_arguments, parse_selected_range, FetchArguments, FetchAttributes, SectionText,
        },
        CommandData, Data,
    },
    servers::state::State,
};
use erooster_core::backend::storage::{
    maildir::MaildirMailEntry, MailEntry, MailEntryType, MailStorage, Storage,
};
use erooster_deps::{
    color_eyre::{
        self,
        eyre::{eyre, Context, ContextCompat},
        Result,
    },
    futures::{Sink, SinkExt},
    nom::{error::convert_error, Finish},
    tracing::{self, debug, error, instrument, warn},
};

pub struct Fetch<'a> {
    pub data: &'a Data,
}

impl Fetch<'_> {
    #[allow(clippy::too_many_lines)]
    #[instrument(skip(self, lines, command_data, storage))]
    pub async fn exec<S, E>(
        &self,
        lines: &mut S,
        command_data: &CommandData<'_>,
        storage: &Storage,
        is_uid: bool,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        let offset = usize::from(is_uid);
        // TODO handle the various request types defined in https://www.rfc-editor.org/rfc/rfc9051.html#name-fetch-command
        if let State::Selected(folder, _) = &self.data.con_state.state {
            let folder = folder.replace('/', ".");
            let username = self
                .data
                .con_state
                .username
                .clone()
                .context("Username missing in internal State")?;
            let mailbox_path = storage.to_ondisk_path(folder.clone(), username.clone())?;
            let mut mails: Vec<MailEntryType> = storage
                .list_all(format!("{username}/{folder}"), &mailbox_path)
                .await;
            mails.sort_by_key(MaildirMailEntry::uid);

            let arguments_borrow = command_data.arguments[offset];
            let ranges = parse_selected_range(arguments_borrow).finish();
            debug!("Range: {:?}; Mails: {}", ranges, mails.len());

            match ranges {
                Ok((_, ranges)) => {
                    let mut filtered_mails: Vec<_> = mails
                        .iter_mut()
                        .enumerate()
                        .filter_map(|(index, mail)| {
                            if ranges.iter().any(|range| {
                                if is_uid {
                                    range.contains(&mail.uid())
                                } else {
                                    range.contains(
                                        &(u32::try_from(index)
                                            .expect("Unable to convert index to u32")
                                            + 1),
                                    )
                                }
                            }) {
                                mail.sequence_number = Some(
                                    u32::try_from(index).expect("Unable to convert index to u32")
                                        + 1,
                                );
                                return Some(mail);
                            }
                            None
                        })
                        .collect::<Vec<_>>();

                    let mut fetch_args: String = command_data.arguments[1 + offset..].join(" ");
                    fetch_args.pop(); // remove last
                    if !fetch_args.is_empty() {
                        fetch_args.remove(0); // remove first
                    }
                    let fetch_args = fetch_args.as_str();

                    filtered_mails.sort_by_key(|x| {
                        if let Some(number) = x.sequence_number() {
                            number
                        } else {
                            0
                        }
                    });
                    match fetch_arguments(fetch_args).finish() {
                        Ok((_, args)) => {
                            for mail in filtered_mails {
                                let uid = mail.uid();
                                let sequence =
                                    mail.sequence_number().context("Sequence number missing")?;
                                if let Some(resp) = generate_response(args.clone(), mail)? {
                                    if is_uid {
                                        // This deduplicates the UID command if needed
                                        if resp.contains("UID") {
                                            lines
                                                .feed(format!("* {sequence} FETCH ({resp})"))
                                                .await?;
                                        } else {
                                            lines
                                                .feed(format!(
                                                    "* {sequence} FETCH (UID {uid} {resp})"
                                                ))
                                                .await?;
                                        }
                                    } else {
                                        lines.feed(format!("* {sequence} FETCH ({resp})")).await?;
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
                            error!(
                                "Failed to parse fetch arguments: {}",
                                convert_error(fetch_args, e)
                            );
                            lines
                                .send(format!("{} BAD Unable to parse", command_data.tag))
                                .await?;
                        }
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to parse fetch arguments: {}",
                        convert_error(arguments_borrow, e)
                    );
                    lines
                        .send(format!("{} BAD Unable to parse", command_data.tag))
                        .await?;
                }
            }
        } else {
            lines
                .feed(format!(
                    "{} NO [TRYCREATE] No mailbox selected",
                    command_data.tag,
                ))
                .await?;
            error!(
                "State was {:#?} instead of selected",
                self.data.con_state.state
            );
            lines.flush().await?;
        }
        Ok(())
    }
}

#[instrument(skip(arg, mail))]
pub fn generate_response(arg: FetchArguments, mail: &mut MailEntryType) -> Result<Option<String>> {
    match arg {
        FetchArguments::Single(single_arg) => {
            Ok(generate_response_for_attributes(single_arg, mail)?)
        }
        FetchArguments::List(args) => {
            let mut resp = String::new();
            for arg in args {
                if let Some(extra_resp) = generate_response_for_attributes(arg, mail)? {
                    if resp.is_empty() {
                        resp = extra_resp;
                    } else {
                        resp = format!("{resp} {extra_resp}");
                    }
                }
            }
            //debug!("[Fetch] List Response: {}", resp);
            Ok(Some(resp))
        }
        _ => Ok(None),
    }
}

#[allow(clippy::too_many_lines)]
#[instrument(skip(attr, mail))]
fn generate_response_for_attributes(
    attr: FetchAttributes,
    mail: &mut MailEntryType,
) -> Result<Option<String>> {
    match attr {
        FetchAttributes::RFC822Header => {
            if let Ok(headers_vec) = mail.headers() {
                let headers = headers_vec
                    .iter()
                    .map(|header| format!("{}: {}", header.get_key(), header.get_value()))
                    .collect::<Vec<_>>()
                    .join("\r\n");

                Ok(Some(format!("RFC822.HEADER{headers}")))
            } else {
                Ok(Some(String::from("RFC822.HEADER\r\n")))
            }
        }
        FetchAttributes::Flags => {
            let mut flags = String::new();
            if mail
                .path()
                .clone()
                .into_os_string()
                .into_string()
                .map_err(|e| eyre!(e.to_string_lossy().to_string()))
                .wrap_err("Failed to convert OS String into String type")?
                .contains("new")
            {
                flags.push_str("\\Recent");
            }
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

            Ok(Some(format!("FLAGS ({flags})")))
        }
        FetchAttributes::RFC822Size => {
            if let Ok(parsed) = mail.parsed() {
                let size = parsed.raw_bytes.len();
                Ok(Some(format!("RFC822.SIZE {size}")))
            } else {
                Ok(Some(String::from("RFC822.SIZE 0")))
            }
        }
        FetchAttributes::Uid => Ok(Some(format!("UID {}", mail.uid()))),
        FetchAttributes::BodySection(section_text, range) => {
            Ok(Some(body(section_text, range, mail)))
        }
        FetchAttributes::BodyPeek(section_text, range) => Ok(Some(body(section_text, range, mail))),
        _ => Ok(None),
    }
}

#[allow(clippy::cast_possible_truncation)]
#[instrument(skip(section_text, range, mail))]
fn body(
    section_text: Option<SectionText>,
    range: Option<(u64, u64)>,
    mail: &mut MailEntryType,
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
                            format!("BODY[] {{{}}}\r\n{body_text}", body_text.as_bytes().len())
                        } else {
                            String::from("BODY[TEXT] NIL\r\n")
                        }
                    } else {
                        String::from("BODY[TEXT] NIL\r\n")
                    }
                } else if let Ok(body) = mail.parsed() {
                    if let Ok(body_text) = body.get_body_raw() {
                        let body_text = String::from_utf8_lossy(&body_text);
                        format!("BODY[] {{{}}}\r\n{body_text}", body_text.as_bytes().len())
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
                        "BODY[HEADER.FIELDS ({})] {{{}}}\r\n{}\r\n",
                        headers_requested_vec.join(" "),
                        headers.as_bytes().len() + 2,
                        headers
                    )
                } else {
                    format!(
                        "BODY[HEADER.FIELDS ({})] NIL\r\n",
                        headers_requested_vec.join(" "),
                    )
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
                    let data = format!("{headers}\r\n");
                    format!("BODY[HEADER] {{{}}}\r\n{data}", data.as_bytes().len())
                } else {
                    String::from("BODY[HEADER] NIL\r\n")
                }
            }
        }
    } else if let Ok(mail) = mail.parsed() {
        let mail = String::from_utf8_lossy(mail.raw_bytes);

        format!("BODY[] {{{}}}\r\n{mail}", mail.as_bytes().len())
    } else {
        String::from("BODY[] NIL\r\n")
    }
}
