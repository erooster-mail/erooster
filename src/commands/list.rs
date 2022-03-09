use std::{io, path::Path, sync::Arc};

use async_trait::async_trait;
use futures::{Sink, SinkExt};
use maildir::Maildir;
use tracing::debug;

use crate::{
    commands::{utils::get_flags, Command, Commands, Data},
    config::Config,
    line_codec::LinesCodecError,
    servers::state::State,
};

#[allow(clippy::too_many_lines)]
pub async fn basic<'a, S>(
    data: &'a Data<'a>,
    lines: &mut S,
    config: Arc<Config>,
) -> anyhow::Result<()>
where
    S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
    S::Error: From<io::Error>,
{
    if data.con_state.state == State::NotAuthenticated {
        lines
            .send(format!(
                "{} BAD Not Authenticated",
                data.command_data.as_ref().unwrap().tag
            ))
            .await?;
        return Ok(());
    }

    let command_resp = if data.command_data.as_ref().unwrap().command == Commands::LSub {
        "LSUB"
    } else {
        "LIST"
    };

    let arguments = &data.command_data.as_ref().unwrap().arguments;
    debug_assert_eq!(arguments.len(), 2);

    // Cleanup args
    let mut reference_name = arguments[0].clone();
    reference_name.remove_matches('"');
    let mut mailbox_patterns = arguments[1].clone();
    mailbox_patterns.remove_matches('"');

    if mailbox_patterns.is_empty() {
        lines
            .feed(format!("* {} (\\Noselect) \"/\" \"\"", command_resp))
            .await?;
    } else if mailbox_patterns.ends_with('*') {
        let mut folder =
            Path::new(&config.mail.maildir_folders).join(data.con_state.username.clone().unwrap());
        if !reference_name.is_empty() {
            let mut reference_name_folder = reference_name.clone().replace('/', ".");
            reference_name_folder.insert(0, '.');
            reference_name_folder.remove_matches('"');
            folder = folder.join(reference_name_folder);
        }
        if mailbox_patterns != "*" {
            let mut mailbox_patterns_folder = mailbox_patterns.clone().replace('/', ".");
            mailbox_patterns_folder.insert(0, '.');
            mailbox_patterns_folder.remove_matches('"');
            mailbox_patterns_folder.remove_matches(".*");
            mailbox_patterns_folder.remove_matches('*');
            folder = folder.join(mailbox_patterns_folder);
        }

        let maildir = Maildir::from(folder);
        let sub_folders = maildir.list_subdirs();
        if reference_name.is_empty() && mailbox_patterns == "*" {
            lines
                .feed(format!(
                    "* {} (\\NoInferiors) \"/\" \"INBOX\"",
                    command_resp,
                ))
                .await?;
        }
        for sub_folder in sub_folders.flatten() {
            // TODO calc flags
            let flags_raw = get_flags(sub_folder.path());
            let flags = if let Ok(flags_raw) = flags_raw {
                flags_raw
            } else {
                vec![]
            };
            let folder_name = sub_folder.path().file_name().unwrap().to_string_lossy();
            lines
                .feed(format!(
                    "* {} ({}) \"/\" \"{}\"",
                    command_resp,
                    flags.join(" "),
                    folder_name.trim_start_matches('.').replace('.', "/")
                ))
                .await?;
        }
    } else if mailbox_patterns.ends_with('%') {
        let mut folder =
            Path::new(&config.mail.maildir_folders).join(data.con_state.username.clone().unwrap());
        if !reference_name.is_empty() {
            let mut reference_name_folder = reference_name.clone().replace('/', ".");
            reference_name_folder.insert(0, '.');
            reference_name_folder.remove_matches('"');
            folder = folder.join(reference_name_folder);

            if mailbox_patterns != "%" {
                let mut mailbox_patterns_folder = mailbox_patterns.clone().replace('/', ".");
                mailbox_patterns_folder.insert(0, '.');
                mailbox_patterns_folder.remove_matches('"');
                mailbox_patterns_folder.remove_matches(".%");
                mailbox_patterns_folder.remove_matches('%');
                lines
                    .feed(format!(
                        "* {} () \"/\" \"{}\"",
                        command_resp, mailbox_patterns_folder
                    ))
                    .await?;
            }
        }
        if mailbox_patterns != "%" {
            let mut mailbox_patterns_folder = mailbox_patterns.clone().replace('/', ".");
            mailbox_patterns_folder.insert(0, '.');
            mailbox_patterns_folder.remove_matches('"');
            mailbox_patterns_folder.remove_matches(".%");
            mailbox_patterns_folder.remove_matches('%');
            folder = folder.join(mailbox_patterns_folder);
        }

        let maildir = Maildir::from(folder);
        let sub_folders = maildir.list_subdirs();
        if reference_name.is_empty() && mailbox_patterns == "%" {
            lines
                .feed(format!(
                    "* {} (\\NoInferiors) \"/\" \"INBOX\"",
                    command_resp,
                ))
                .await?;
        }
        for sub_folder in sub_folders.flatten() {
            // TODO calc flags
            let flags_raw = get_flags(sub_folder.path());
            let flags = if let Ok(flags_raw) = flags_raw {
                flags_raw
            } else {
                vec![]
            };
            lines
                .feed(format!(
                    "* {} ({}) \"/\" \"{}\"",
                    command_resp,
                    flags.join(" "),
                    sub_folder
                        .path()
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .trim_start_matches('.')
                        .replace('.', "/")
                ))
                .await?;
        }
    } else {
        let mut folder =
            Path::new(&config.mail.maildir_folders).join(data.con_state.username.clone().unwrap());
        if !reference_name.is_empty() {
            let mut reference_name_folder = reference_name.clone().replace('/', ".");
            reference_name_folder.remove_matches('"');
            reference_name_folder.insert(0, '.');
            folder = folder.join(reference_name_folder);
        }
        let mut mailbox_patterns_folder = mailbox_patterns.clone().replace('/', ".");
        mailbox_patterns_folder.remove_matches('"');
        if mailbox_patterns_folder != "INBOX" {
            mailbox_patterns_folder.insert(0, '.');
        }
        folder = folder.join(mailbox_patterns_folder.clone());

        // TODO check for folder existence
        // TODO calc flags
        let flags_raw = get_flags(&folder);
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
                "* {} ({}) \"/\" \"{}\"",
                command_resp,
                flags.join(" "),
                folder
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .trim_start_matches('.')
                    .replace('.', "/")
            ))
            .await?;
    }
    lines
        .feed(format!(
            "{} OK done",
            data.command_data.as_ref().unwrap().tag,
        ))
        .await?;
    lines.flush().await?;
    Ok(())
}
pub struct List<'a> {
    pub data: &'a Data<'a>,
}

impl List<'_> {
    // TODO parse all arguments

    // TODO setup
    pub async fn extended<S>(&mut self, lines: &mut S, _config: Arc<Config>) -> anyhow::Result<()>
    where
        S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
        S::Error: From<io::Error>,
    {
        debug!("extended");
        if self.data.con_state.state == State::NotAuthenticated {
            lines
                .send(format!(
                    "{} BAD Not Authenticated",
                    self.data.command_data.as_ref().unwrap().tag
                ))
                .await?;
        } else {
            let arguments = &self.data.command_data.as_ref().unwrap().arguments;
            if arguments[0].starts_with('(') && arguments[0].ends_with(')') {
                // TODO handle selection options
            } else {
                // Cleanup args
                let mut reference_name = arguments[0].clone();
                reference_name.remove_matches('"');
                let mut mailbox_patterns = arguments[1].clone();
                mailbox_patterns.remove_matches('"');
            }
            lines
                .send(format!(
                    "{} BAD LIST Not supported",
                    self.data.command_data.as_ref().unwrap().tag
                ))
                .await?;
        }
        Ok(())
    }
}

#[async_trait]
impl<S> Command<S> for List<'_>
where
    S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
    S::Error: From<io::Error>,
{
    async fn exec(&mut self, lines: &mut S, config: Arc<Config>) -> anyhow::Result<()> {
        let arguments = &self.data.command_data.as_ref().unwrap().arguments;
        debug_assert_eq!(arguments.len(), 2);
        if arguments.len() == 2 {
            basic(self.data, lines, config).await?;
        } else if arguments.len() == 4 {
            self.extended(lines, config).await?;
        } else {
            lines
                .send(format!(
                    "{} BAD [SERVERBUG] invalid arguments",
                    self.data.command_data.as_ref().unwrap().tag
                ))
                .await?;
        }
        Ok(())
    }
}

pub struct LSub<'a> {
    pub data: &'a Data<'a>,
}

#[async_trait]
impl<S> Command<S> for LSub<'_>
where
    S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
    S::Error: From<io::Error>,
{
    async fn exec(&mut self, lines: &mut S, config: Arc<Config>) -> anyhow::Result<()> {
        let arguments = &self.data.command_data.as_ref().unwrap().arguments;
        debug_assert_eq!(arguments.len(), 2);
        if arguments.len() == 2 {
            basic(self.data, lines, config).await?;
        } else {
            lines
                .send(format!(
                    "{} BAD [SERVERBUG] invalid arguments",
                    self.data.command_data.as_ref().unwrap().tag
                ))
                .await?;
        }
        Ok(())
    }
}
