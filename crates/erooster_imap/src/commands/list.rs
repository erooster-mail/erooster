// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::{
    commands::{CommandData, Commands, Data},
    servers::state::State,
};
use erooster_core::{
    backend::storage::{MailStorage, Storage},
    config::Config,
};
use std::path::Path;
use {
    color_eyre::{self, eyre::ContextCompat},
    futures::{Sink, SinkExt},
    tracing::{debug, instrument},
};

/// Returns the RFC 6154 special-use attribute for a folder, if any.
fn special_use_flag(display_name: &str) -> Option<&'static str> {
    match display_name.to_lowercase().as_str() {
        "inbox" => Some("\\Inbox"),
        "sent" | "sent items" | "sent messages" => Some("\\Sent"),
        "drafts" | "draft" => Some("\\Drafts"),
        "trash" | "deleted" | "deleted items" | "deleted messages" => Some("\\Trash"),
        "junk" | "spam" | "junk email" => Some("\\Junk"),
        "archive" | "archives" => Some("\\Archive"),
        "all" | "all mail" => Some("\\All"),
        _ => None,
    }
}

/// Builds the full flag list for a folder path.
///
/// Combines stored flags from the `.flags` file, `\HasChildren` / `\NoChildren`
/// derived from the directory structure, and RFC 6154 special-use attributes
/// derived from the display name.
async fn folder_flags(storage: &Storage, path: &Path, display_name: &str) -> Vec<String> {
    let mut flags: Vec<String> = storage.get_flags(path).await.unwrap_or_default();

    // HasChildren / NoChildren
    let has_children = storage
        .list_subdirs(path)
        .is_ok_and(|subs| !subs.is_empty());
    if has_children {
        if !flags.iter().any(|f| f == "\\HasChildren") {
            flags.push(String::from("\\HasChildren"));
        }
    } else if !flags.iter().any(|f| f == "\\NoChildren") {
        flags.push(String::from("\\NoChildren"));
    }

    // Special-use
    if let Some(attr) = special_use_flag(display_name) {
        if !flags.iter().any(|f| f == attr) {
            flags.push(attr.to_string());
        }
    }

    flags
}

#[allow(clippy::too_many_lines)]
#[instrument(skip(data, lines, config, storage, command_data))]
pub async fn basic<S, E>(
    data: &Data,
    lines: &mut S,
    config: &Config,
    storage: &Storage,
    command_data: &CommandData<'_>,
) -> color_eyre::eyre::Result<()>
where
    E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
    S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
{
    if matches!(data.con_state.state, State::NotAuthenticated) {
        lines
            .send(format!("{} BAD Not Authenticated", command_data.tag))
            .await?;
        return Ok(());
    }

    let command_resp = if matches!(command_data.command, Commands::LSub) {
        "LSUB"
    } else {
        "LIST"
    };

    let arguments = &command_data.arguments;
    if arguments.len() < 2 {
        lines
            .send(format!(
                "{} BAD [PARSE] LIST requires reference and mailbox pattern arguments",
                command_data.tag
            ))
            .await?;
        return Ok(());
    }

    // Cleanup args
    let reference_name = arguments[0];
    let reference_name = reference_name.replace('"', "");
    let mailbox_patterns = arguments[1];
    let mailbox_patterns = mailbox_patterns.replace('"', "");

    if mailbox_patterns.is_empty() {
        lines
            .feed(format!("* {command_resp} (\\Noselect) \".\" \"\""))
            .await?;
    } else if mailbox_patterns.ends_with('*') {
        let mut folder = Path::new(&config.mail.maildir_folders).join(
            data.con_state
                .username
                .clone()
                .context("Username missing in internal State")?,
        );
        if !reference_name.is_empty() {
            let mut reference_name_folder = reference_name.clone();
            reference_name_folder.insert(0, '.');
            reference_name_folder.retain(|c| c != '"');
            folder = folder.join(reference_name_folder);
        }
        if mailbox_patterns != "*" {
            let mut mailbox_patterns_folder = mailbox_patterns.clone();
            mailbox_patterns_folder.insert(0, '.');
            mailbox_patterns_folder.retain(|c| c != '"');
            mailbox_patterns_folder = mailbox_patterns_folder.replace(".*", "");
            mailbox_patterns_folder.retain(|c| c != '*');
            folder = folder.join(mailbox_patterns_folder);
        }

        let sub_folders = storage.list_subdirs(&folder)?;
        if reference_name.is_empty() && mailbox_patterns == "*" {
            let inbox_flags = folder_flags(storage, &folder, "INBOX").await;
            lines
                .feed(format!(
                    "* {command_resp} ({}) \".\" \"INBOX\"",
                    inbox_flags.join(" ")
                ))
                .await?;
        }
        for sub_folder in sub_folders {
            let display = display_name_for(&sub_folder);
            let flags = folder_flags(storage, &sub_folder, &display).await;
            lines
                .feed(format!(
                    "* {command_resp} ({}) \".\" \"{display}\"",
                    flags.join(" "),
                ))
                .await?;
        }
    } else if mailbox_patterns.ends_with('%') {
        let mut folder = Path::new(&config.mail.maildir_folders).join(
            data.con_state
                .username
                .clone()
                .context("Username missing in internal State")?,
        );
        if !reference_name.is_empty() {
            let mut reference_name_folder = reference_name.clone();
            reference_name_folder.insert(0, '.');
            reference_name_folder.retain(|c| c != '"');
            folder = folder.join(reference_name_folder);

            if mailbox_patterns != "%" {
                let mut mailbox_patterns_folder = mailbox_patterns.clone();
                mailbox_patterns_folder.insert(0, '.');
                mailbox_patterns_folder.retain(|c| c != '"');
                mailbox_patterns_folder = mailbox_patterns_folder.replace(".%", "");
                mailbox_patterns_folder.retain(|c| c != '%');
                lines
                    .feed(format!(
                        "* {command_resp} () \".\" \"{mailbox_patterns_folder}\""
                    ))
                    .await?;
            }
        }
        if mailbox_patterns != "%" {
            let mut mailbox_patterns_folder = mailbox_patterns.clone();
            mailbox_patterns_folder.insert(0, '.');
            mailbox_patterns_folder.retain(|c| c != '"');
            mailbox_patterns_folder = mailbox_patterns_folder.replace(".%", "");
            mailbox_patterns_folder.retain(|c| c != '%');
            folder = folder.join(mailbox_patterns_folder);
        }

        let sub_folders = storage.list_subdirs(&folder)?;
        if reference_name.is_empty() && mailbox_patterns == "%" {
            let inbox_flags = folder_flags(storage, &folder, "INBOX").await;
            lines
                .feed(format!(
                    "* {command_resp} ({}) \".\" \"INBOX\"",
                    inbox_flags.join(" ")
                ))
                .await?;
        }
        for sub_folder in sub_folders {
            let display = display_name_for(&sub_folder);
            let flags = folder_flags(storage, &sub_folder, &display).await;
            lines
                .feed(format!(
                    "* {command_resp} ({}) \".\" \"{display}\"",
                    flags.join(" "),
                ))
                .await?;
        }
    } else {
        let mut folder = Path::new(&config.mail.maildir_folders).join(
            data.con_state
                .username
                .clone()
                .context("Username missing in internal State")?,
        );
        if !reference_name.is_empty() {
            let mut reference_name_folder = reference_name.clone();
            reference_name_folder.retain(|c| c != '"');
            reference_name_folder.insert(0, '.');
            folder = folder.join(reference_name_folder);
        }
        let mut mailbox_patterns_folder = mailbox_patterns.clone();
        mailbox_patterns_folder.retain(|c| c != '"');
        if mailbox_patterns_folder != "INBOX" {
            mailbox_patterns_folder.insert(0, '.');
        }
        folder = folder.join(mailbox_patterns_folder.clone());

        let display = mailbox_patterns_folder.trim_start_matches('.').to_string();
        let mut flags = if folder.exists() {
            folder_flags(storage, &folder, &display).await
        } else {
            vec![String::from("\\NonExistent")]
        };

        if !folder.exists() && !flags.contains(&String::from("\\NonExistent")) {
            flags.push(String::from("\\NonExistent"));
        }

        lines
            .feed(format!(
                "* {command_resp} ({}) \".\" \"{display}\"",
                flags.join(" "),
            ))
            .await?;
    }
    lines
        .feed(format!("{} OK {command_resp} completed", command_data.tag))
        .await?;
    lines.flush().await?;
    Ok(())
}

/// Returns the human-readable display name for a maildir subdirectory path.
fn display_name_for(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().trim_start_matches('.').to_string())
        .unwrap_or_default()
}

pub struct List<'a> {
    pub data: &'a Data,
}

impl List<'_> {
    #[instrument(skip(self, lines, command_data))]
    pub async fn extended<S, E>(
        &self,
        lines: &mut S,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        debug!("extended LIST not yet implemented");
        if self.data.con_state.state == State::NotAuthenticated {
            lines
                .send(format!("{} BAD Not Authenticated", command_data.tag))
                .await?;
        } else {
            lines
                .send(format!(
                    "{} BAD LIST extended form not supported",
                    command_data.tag
                ))
                .await?;
        }
        Ok(())
    }
}

impl List<'_> {
    #[instrument(skip(self, lines, config, storage, command_data))]
    pub async fn exec<S, E>(
        &mut self,
        lines: &mut S,
        config: &Config,
        storage: &Storage,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        let arguments = &command_data.arguments;
        if arguments.len() < 2 {
            lines
                .send(format!(
                    "{} BAD [PARSE] LIST requires at least two arguments",
                    command_data.tag
                ))
                .await?;
            return Ok(());
        }
        if arguments.len() == 2 {
            basic(self.data, lines, config, storage, command_data).await?;
        } else if arguments.len() >= 4 {
            self.extended(lines, command_data).await?;
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

pub struct LSub<'a> {
    pub data: &'a mut Data,
}

impl LSub<'_> {
    #[instrument(skip(self, lines, config, storage, command_data))]
    pub async fn exec<S, E>(
        &mut self,
        lines: &mut S,
        config: &Config,
        storage: &Storage,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        let arguments = &command_data.arguments;
        if arguments.len() < 2 {
            lines
                .send(format!(
                    "{} BAD [PARSE] LSUB requires two arguments",
                    command_data.tag
                ))
                .await?;
            return Ok(());
        }
        basic(self.data, lines, config, storage, command_data).await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tokio;

    #[test]
    fn test_special_use_flag_inbox() {
        assert_eq!(special_use_flag("INBOX"), Some("\\Inbox"));
        assert_eq!(special_use_flag("inbox"), Some("\\Inbox"));
    }

    #[test]
    fn test_special_use_flag_sent() {
        assert_eq!(special_use_flag("Sent"), Some("\\Sent"));
        assert_eq!(special_use_flag("sent items"), Some("\\Sent"));
    }

    #[test]
    fn test_special_use_flag_trash() {
        assert_eq!(special_use_flag("Trash"), Some("\\Trash"));
        assert_eq!(special_use_flag("Deleted"), Some("\\Trash"));
    }

    #[test]
    fn test_special_use_flag_junk() {
        assert_eq!(special_use_flag("Junk"), Some("\\Junk"));
        assert_eq!(special_use_flag("Spam"), Some("\\Junk"));
    }

    #[test]
    fn test_special_use_flag_drafts() {
        assert_eq!(special_use_flag("Drafts"), Some("\\Drafts"));
    }

    #[test]
    fn test_special_use_flag_archive() {
        assert_eq!(special_use_flag("Archive"), Some("\\Archive"));
    }

    #[test]
    fn test_special_use_flag_unknown() {
        assert_eq!(special_use_flag("My Custom Folder"), None);
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    #[tokio::test]
    async fn test_list_not_authenticated() {
        let mut data = Data {
            con_state: crate::servers::state::Connection {
                state: State::NotAuthenticated,
                secure: true,
                username: None,
                active_capabilities: vec![],
            },
        };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::List,
            arguments: &["\"\"", "\"*\""],
        };
        let (config, storage) = erooster_core::test_helpers::setup_test_storage()
            .await
            .unwrap();
        let (mut tx, mut rx) = futures::channel::mpsc::unbounded();
        let res = List { data: &mut data }
            .exec(&mut tx, &config, &storage, &cmd_data)
            .await;
        assert!(res.is_ok(), "{res:?}");
        use futures::StreamExt;
        let reply = rx.next().await.unwrap_or_default();
        assert!(
            reply.contains("BAD"),
            "expected BAD for unauthenticated LIST, got: {reply}"
        );
    }
}
