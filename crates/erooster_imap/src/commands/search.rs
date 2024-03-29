// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::{
    commands::{parsers::search_arguments, CommandData, Data},
    servers::state::State,
};
use erooster_core::backend::storage::{maildir::MaildirMailEntry, MailEntry, MailStorage, Storage};
use erooster_deps::{
    color_eyre::{self, eyre::ContextCompat},
    futures::{Sink, SinkExt},
    nom::{error::convert_error, Finish},
    tracing::{self, debug, error, instrument},
};

use super::parsers::{parse_search_date, SearchProgram, SearchReturnOption};

pub struct Search<'a> {
    pub data: &'a Data,
}

impl Search<'_> {
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
        if let State::Selected(folder, _) = &self.data.con_state.state {
            let folder = folder.replace('/', ".");
            let username = self
                .data
                .con_state
                .username
                .clone()
                .context("Username missing in internal State")?;
            let mailbox_path = storage.to_ondisk_path(folder.clone(), username.clone())?;

            let offset = usize::from(is_uid);
            let arguments = &command_data.arguments[offset..];
            let search_args: String = arguments.join(" ");
            let search_args = search_args.as_str();
            debug!("Search arguments: {:?}", search_args);

            match search_arguments(search_args).finish() {
                Ok((_, args)) => {
                    debug!(
                        "Resulting parsed arguments of the search query: {:#?}",
                        args
                    );

                    let mails = storage
                        .list_all(format!("{username}/{folder}"), &mailbox_path)
                        .await;
                    let mut results = parse_search_program(mails, &args.program, is_uid);

                    let esearch_return_string = if results.is_empty() {
                        if is_uid {
                            format!("* ESEARCH (TAG \"{}\") UID", command_data.tag)
                        } else {
                            format!("* ESEARCH (TAG \"{}\")", command_data.tag)
                        }
                    } else {
                        match args.return_opts {
                            SearchReturnOption::SAVE => {
                                // Not implemented
                                lines
                                    .send(format!("{} BAD Not implemented", command_data.tag))
                                    .await?;
                                return Ok(());
                            }
                            SearchReturnOption::ALL => {
                                if is_uid {
                                    format!(
                                        "* ESEARCH (TAG \"{}\") UID ALL {}",
                                        command_data.tag,
                                        generate_ranges(&mut results)
                                    )
                                } else {
                                    format!(
                                        "* ESEARCH (TAG \"{}\") ALL {}",
                                        command_data.tag,
                                        generate_ranges(&mut results)
                                    )
                                }
                            }
                            SearchReturnOption::MIN => {
                                if is_uid {
                                    format!(
                                        "* ESEARCH (TAG \"{}\") UID MIN {}",
                                        command_data.tag,
                                        results.iter().min().expect("No minimum value found")
                                    )
                                } else {
                                    format!(
                                        "* ESEARCH (TAG \"{}\") MIN {}",
                                        command_data.tag,
                                        results.iter().min().expect("No minimum value found")
                                    )
                                }
                            }
                            SearchReturnOption::MAX => {
                                if is_uid {
                                    format!(
                                        "* ESEARCH (TAG \"{}\") UID MAX {}",
                                        command_data.tag,
                                        results.iter().max().expect("No maximum value found")
                                    )
                                } else {
                                    format!(
                                        "* ESEARCH (TAG \"{}\") MAX {}",
                                        command_data.tag,
                                        results.iter().max().expect("No maximum value found")
                                    )
                                }
                            }
                            SearchReturnOption::COUNT => {
                                if is_uid {
                                    format!(
                                        "* ESEARCH (TAG \"{}\") UID COUNT {}",
                                        command_data.tag,
                                        results.len()
                                    )
                                } else {
                                    format!(
                                        "* ESEARCH (TAG \"{}\") COUNT {}",
                                        command_data.tag,
                                        results.len()
                                    )
                                }
                            }
                            SearchReturnOption::Multiple(result_options) => {
                                let mut result_options_string = String::new();
                                for option in result_options {
                                    match option {
                                        SearchReturnOption::ALL => {
                                            result_options_string.push_str(
                                                format!("ALL {}", generate_ranges(&mut results))
                                                    .as_str(),
                                            );
                                        }
                                        SearchReturnOption::MIN => {
                                            result_options_string.push_str(
                                                format!(
                                                    "MIN {}",
                                                    results
                                                        .iter()
                                                        .min()
                                                        .expect("No minimum value found")
                                                )
                                                .as_str(),
                                            );
                                        }
                                        SearchReturnOption::MAX => {
                                            result_options_string.push_str(
                                                format!(
                                                    "MAX {}",
                                                    results
                                                        .iter()
                                                        .max()
                                                        .expect("No maximum value found")
                                                )
                                                .as_str(),
                                            );
                                        }
                                        SearchReturnOption::COUNT => {
                                            result_options_string.push_str(
                                                format!("COUNT {}", results.len()).as_str(),
                                            );
                                        }
                                        SearchReturnOption::Multiple(_) => {
                                            unreachable!("Multiple options cannot be nested. This type only exists for internal data structure and is unreachable.");
                                        }
                                        SearchReturnOption::SAVE => {
                                            // Not yet implemented
                                            lines
                                                .send(format!(
                                                    "{} BAD SAVE is not yet implemented",
                                                    command_data.tag
                                                ))
                                                .await?;
                                        }
                                    }
                                }
                                if is_uid {
                                    format!(
                                        "* ESEARCH (TAG \"{}\") UID {}",
                                        command_data.tag, result_options_string
                                    )
                                } else {
                                    format!(
                                        "* ESEARCH (TAG \"{}\") {}",
                                        command_data.tag, result_options_string
                                    )
                                }
                            }
                        }
                    };
                    if !results.is_empty() {
                        lines
                            .feed(format!(
                                "* SEARCH {}",
                                results
                                    .iter()
                                    .map(ToString::to_string)
                                    .collect::<Vec<String>>()
                                    .join(" ")
                            ))
                            .await?;
                    }

                    debug!("return_string: {:#?}", esearch_return_string);

                    lines.feed(esearch_return_string).await?;
                    lines
                        .feed(format!("{} OK SEARCH completed", command_data.tag,))
                        .await?;
                    lines.flush().await?;
                }
                Err(e) => {
                    error!(
                        "Failed to parse search arguments: {}",
                        convert_error(search_args, e)
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

fn parse_search_program(
    mut mails: Vec<MaildirMailEntry>,
    program: &SearchProgram,
    is_uid: bool,
) -> Vec<u32> {
    let results: Vec<_> = mails
        .iter_mut()
        .filter_map(|entry| {
            // This applies the rules given by the search program
            check_search_condition(program, entry, is_uid).then_some(entry)
        })
        .map(|x| {
            if is_uid {
                x.uid()
            } else {
                //TODO: This will crash currently because we dont actually build the sequence numbers yet
                x.sequence_number().expect("Sequence number not set")
            }
        })
        .collect();

    results
}

/// Generates a string where continuous numbers are represented in a string as `<start>:<end>`.
/// Singular numbers are represented as `<number>`.
/// If there are gaps then there should be a "," between the ranges.
/// Order is not required.
/// There MUST be no spaces before or after the returned string.
///
/// # Example
///
/// `1:3,5,7:9,11,13:15`
fn generate_ranges(results: &mut Vec<u32>) -> String {
    results.sort_unstable();
    let mut ranges = Vec::new();
    let mut current_range = (0, 0);
    for result in results {
        if current_range.1 == 0 {
            current_range.0 = *result;
            current_range.1 = *result;
        } else if current_range.1 + 1 == *result {
            current_range.1 = *result;
        } else {
            ranges.push(current_range);
            current_range = (0, 0);
        }
    }
    ranges.push(current_range);
    ranges
        .iter()
        .map(|range| {
            if range.0 == range.1 {
                format!("{}", range.0)
            } else {
                format!("{}:{}", range.0, range.1)
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[allow(clippy::too_many_lines)]
fn check_search_condition(
    program: &SearchProgram,
    entry: &mut impl MailEntry,
    is_uid: bool,
) -> bool {
    match program {
        SearchProgram::ALL => true,
        SearchProgram::ANSWERED => entry.is_replied(),
        SearchProgram::BCC(ref bcc) => {
            entry
                .headers()
                .expect("Failed to get headers")
                .iter()
                .any(|header| {
                    header.get_key_ref().to_lowercase() == "bcc" && header.get_value().contains(bcc)
                })
        }
        SearchProgram::BEFORE(ref date) => {
            let date = parse_search_date(date).finish();
            match date {
                Ok((_, date)) => match entry.received() {
                    Ok(received) => received < date.midnight().assume_utc().unix_timestamp(),
                    Err(_) => false,
                },
                Err(_) => false,
            }
        }
        SearchProgram::BODY(ref body) => entry.body_contains(body),
        SearchProgram::CC(ref cc) => {
            entry
                .headers()
                .expect("Failed to get headers")
                .iter()
                .any(|header| {
                    header.get_key_ref().to_lowercase() == "cc" && header.get_value().contains(cc)
                })
        }
        SearchProgram::DELETED => entry.is_trashed(),
        SearchProgram::DRAFT => entry.is_draft(),
        SearchProgram::FLAGGED => entry.is_flagged(),
        SearchProgram::FROM(ref from) => entry
            .headers()
            .expect("Failed to get headers")
            .iter()
            .any(|header| {
                header.get_key_ref().to_lowercase() == "from" && header.get_value().contains(from)
            }),
        SearchProgram::HEADER(ref header_query, ref value) => entry
            .headers()
            .expect("Failed to get headers")
            .iter()
            .any(|header| {
                header.get_key_ref().to_lowercase() == header_query.to_lowercase()
                    && header.get_value().contains(value)
            }),
        SearchProgram::KEYWORD(ref keyword) => todo!(),
        SearchProgram::LARGER(ref size) => entry.body_size() > *size,
        SearchProgram::NOT(program) => !check_search_condition(program, entry, is_uid),
        SearchProgram::ON(ref date) => {
            let date = parse_search_date(date).finish();
            match date {
                Ok((_, date)) => match entry.received() {
                    Ok(received) => received == date.midnight().assume_utc().unix_timestamp(),
                    Err(_) => false,
                },
                Err(_) => false,
            }
        }
        SearchProgram::SEEN => entry.is_seen(),
        SearchProgram::SENTBEFORE(ref date) => {
            let date = parse_search_date(date).finish();
            match date {
                Ok((_, date)) => match entry.sent() {
                    Ok(sent) => sent < date.midnight().assume_utc().unix_timestamp(),
                    Err(_) => false,
                },
                Err(_) => false,
            }
        }
        SearchProgram::SENTON(ref date) => {
            let date = parse_search_date(date).finish();
            match date {
                Ok((_, date)) => match entry.sent() {
                    Ok(sent) => sent == date.midnight().assume_utc().unix_timestamp(),
                    Err(_) => false,
                },
                Err(_) => false,
            }
        }
        SearchProgram::SENTSINCE(ref date) => {
            let date = parse_search_date(date).finish();
            match date {
                Ok((_, date)) => match entry.sent() {
                    Ok(sent) => sent > date.midnight().assume_utc().unix_timestamp(),
                    Err(_) => false,
                },
                Err(_) => false,
            }
        }
        SearchProgram::SINCE(ref date) => {
            let date = parse_search_date(date).finish();
            match date {
                Ok((_, date)) => match entry.received() {
                    Ok(received) => received > date.midnight().assume_utc().unix_timestamp(),
                    Err(_) => false,
                },
                Err(_) => false,
            }
        }
        SearchProgram::SMALLER(ref size) => entry.body_size() < *size,
        SearchProgram::SUBJECT(ref subject) => entry
            .headers()
            .expect("Failed to get headers")
            .iter()
            .any(|header| {
                header.get_key_ref().to_lowercase() == "subject"
                    && header.get_value().contains(subject)
            }),
        SearchProgram::TEXT(ref text) => entry.text_contains(text),
        SearchProgram::TO(ref to) => {
            entry
                .headers()
                .expect("Failed to get headers")
                .iter()
                .any(|header| {
                    header.get_key_ref().to_lowercase() == "to" && header.get_value().contains(to)
                })
        }
        SearchProgram::UID(ref uid) => uid.contains(&entry.uid()),
        SearchProgram::UNANSWERED => !entry.is_replied(),
        SearchProgram::UNDELETED => !entry.is_trashed(),
        SearchProgram::UNDRAFT => !entry.is_draft(),
        SearchProgram::UNFLAGGED => !entry.is_flagged(),
        SearchProgram::UNKEYWORD(ref keyword) => todo!(),
        SearchProgram::UNSEEN => !entry.is_seen(),
        SearchProgram::OR(a, b) => {
            check_search_condition(a, entry, is_uid) || check_search_condition(b, entry, is_uid)
        }
        SearchProgram::AND(programs) => programs
            .iter()
            .all(|program| check_search_condition(program, entry, is_uid)),
        SearchProgram::Range(range) => {
            if is_uid {
                range.contains(&entry.uid())
            } else {
                range.contains(
                    &entry
                        .sequence_number()
                        .expect("Failed to get sequence number"),
                )
            }
        }
    }
}
