use crate::{
    commands::{CommandData, Commands, Data},
    servers::state::State,
};
use erooster_core::{
    backend::storage::{MailStorage, Storage},
    config::Config,
};
use erooster_deps::{
    color_eyre::{self, eyre::ContextCompat},
    futures::{Sink, SinkExt},
    tracing::{self, debug, instrument},
};
use std::path::Path;

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
    assert!(arguments.len() == 2);

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
            reference_name_folder.remove_matches('"');
            folder = folder.join(reference_name_folder);
        }
        if mailbox_patterns != "*" {
            let mut mailbox_patterns_folder = mailbox_patterns.clone();
            mailbox_patterns_folder.insert(0, '.');
            mailbox_patterns_folder.remove_matches('"');
            mailbox_patterns_folder.remove_matches(".*");
            mailbox_patterns_folder.remove_matches('*');
            folder = folder.join(mailbox_patterns_folder);
        }

        let sub_folders = storage.list_subdirs(&folder)?;
        if reference_name.is_empty() && mailbox_patterns == "*" {
            lines
                .feed(format!("* {command_resp} (\\Subscribed) \".\" \"INBOX\"",))
                .await?;
        }
        for sub_folder in sub_folders {
            let flags_raw = storage.get_flags(&sub_folder).await;
            let flags = if let Ok(flags_raw) = flags_raw {
                flags_raw
            } else {
                vec![]
            };
            let folder_name = sub_folder
                .file_name()
                .context("Failed to get folder name")?
                .to_string_lossy();
            lines
                .feed(format!(
                    "* {} ({}) \".\" \"{}\"",
                    command_resp,
                    flags.join(" "),
                    folder_name.trim_start_matches('.')
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
            reference_name_folder.remove_matches('"');
            folder = folder.join(reference_name_folder);

            if mailbox_patterns != "%" {
                let mut mailbox_patterns_folder = mailbox_patterns.clone();
                mailbox_patterns_folder.insert(0, '.');
                mailbox_patterns_folder.remove_matches('"');
                mailbox_patterns_folder.remove_matches(".%");
                mailbox_patterns_folder.remove_matches('%');
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
            mailbox_patterns_folder.remove_matches('"');
            mailbox_patterns_folder.remove_matches(".%");
            mailbox_patterns_folder.remove_matches('%');
            folder = folder.join(mailbox_patterns_folder);
        }

        let sub_folders = storage.list_subdirs(&folder)?;
        if reference_name.is_empty() && mailbox_patterns == "%" {
            lines
                .feed(format!("* {command_resp} (\\Subscribed) \".\" \"INBOX\"",))
                .await?;
        }
        for sub_folder in sub_folders {
            // TODO: calc flags
            let flags_raw = storage.get_flags(&sub_folder).await;
            let flags = if let Ok(flags_raw) = flags_raw {
                flags_raw
            } else {
                vec![]
            };
            lines
                .feed(format!(
                    "* {} ({}) \".\" \"{}\"",
                    command_resp,
                    flags.join(" "),
                    sub_folder
                        .file_name()
                        .context("Failed to get file name")?
                        .to_string_lossy()
                        .trim_start_matches('.')
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
            reference_name_folder.remove_matches('"');
            reference_name_folder.insert(0, '.');
            folder = folder.join(reference_name_folder);
        }
        let mut mailbox_patterns_folder = mailbox_patterns.clone();
        mailbox_patterns_folder.remove_matches('"');
        if mailbox_patterns_folder != "INBOX" {
            mailbox_patterns_folder.insert(0, '.');
        }
        folder = folder.join(mailbox_patterns_folder.clone());

        // TODO: check for folder existence
        // TODO: calc flags
        let flags_raw = storage.get_flags(&folder).await;
        let mut flags = if let Ok(flags_raw) = flags_raw {
            if folder.exists() {
                flags_raw
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        if !folder.exists() {
            flags.push(String::from("\\NonExistent"));
        }
        lines
            .feed(format!(
                "* {} ({}) \".\" \"{}\"",
                command_resp,
                flags.join(" "),
                folder
                    .file_name()
                    .context("Failed to get file name")?
                    .to_string_lossy()
                    .trim_start_matches('.')
            ))
            .await?;
    }
    lines
        .feed(format!("{} OK {command_resp} completed", command_data.tag))
        .await?;
    lines.flush().await?;
    Ok(())
}
pub struct List<'a> {
    pub data: &'a Data,
}

impl List<'_> {
    // TODO parse all arguments

    // TODO setup
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
        debug!("extended");
        if self.data.con_state.state == State::NotAuthenticated {
            lines
                .send(format!("{} BAD Not Authenticated", command_data.tag))
                .await?;
        } else {
            let arguments = &command_data.arguments;
            if arguments[0].starts_with('(') && arguments[0].ends_with(')') {
                // TODO: handle selection options
            } else {
                // Cleanup args
                let reference_name = arguments[0];
                let _reference_name = reference_name.replace('"', "");
                let mailbox_patterns = arguments[1];
                let _mailbox_patterns = mailbox_patterns.replace('"', "");
            }
            lines
                .send(format!("{} BAD LIST Not supported", command_data.tag))
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
        assert!(arguments.len() >= 2);
        if arguments.len() == 2 {
            basic(self.data, lines, config, storage, command_data).await?;
        } else if arguments.len() == 4 {
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
        assert!(arguments.len() == 2);
        if arguments.len() == 2 {
            basic(self.data, lines, config, storage, command_data).await?;
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
