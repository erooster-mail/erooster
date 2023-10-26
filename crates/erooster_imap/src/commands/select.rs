use crate::{
    commands::{CommandData, Data},
    servers::state::{Access, State},
};
use erooster_core::backend::storage::{MailStorage, Storage};
use erooster_deps::{
    color_eyre::{self, eyre::ContextCompat},
    futures::{Sink, SinkExt},
    tracing::{self, instrument},
};
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Select<'a> {
    pub data: &'a mut Data,
}

#[instrument(skip(data, lines, storage, rw, command_data))]
async fn select<S, E>(
    data: &mut Data,
    lines: &mut S,
    storage: &Storage,
    rw: bool,
    command_data: &CommandData<'_>,
) -> color_eyre::eyre::Result<()>
where
    E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
    S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
{
    let args = &command_data.arguments;

    assert!(args.len() == 1);
    let folder_arg = args.first().expect("server selects a folder");
    let folder = folder_arg.replace('"', "");
    let access = if rw {
        Access::ReadWrite
    } else {
        Access::ReadOnly
    };
    {
        data.con_state.state = State::Selected(folder.clone(), access);
    };

    let folder_on_disk = folder_arg;
    let mailbox_path = storage.to_ondisk_path(
        (*folder_on_disk).to_string(),
        data.con_state
            .username
            .clone()
            .context("Username missing in internal State")?,
    )?;
    // Special INBOX check to make sure we have a mailbox
    if folder == "INBOX" && !mailbox_path.exists() {
        storage.create_dirs(&mailbox_path)?;
        storage.add_flag(&mailbox_path, "\\Subscribed").await?;
        storage.add_flag(&mailbox_path, "\\NoInferiors").await?;
    }
    send_success(lines, folder, storage, mailbox_path, rw, command_data).await?;
    Ok(())
}

#[instrument(skip(lines, folder, storage, mailbox_path, rw, command_data))]
async fn send_success<S, E>(
    lines: &mut S,
    folder: String,
    storage: &Storage,
    mailbox_path: PathBuf,
    rw: bool,
    command_data: &CommandData<'_>,
) -> color_eyre::eyre::Result<()>
where
    E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
    S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
{
    let count = storage.count_cur(&mailbox_path) + storage.count_new(&mailbox_path);
    lines.feed(format!("* {count} EXISTS")).await?;
    // TODO: Also send UNSEEN
    // FIXME: This is fundamentally invalid and instead should refer to the timestamp a mailbox was created
    let current_time = SystemTime::now();
    let unix_timestamp = current_time.duration_since(UNIX_EPOCH)?;
    #[allow(clippy::cast_possible_truncation)]
    let timestamp = unix_timestamp.as_millis() as u32;
    lines
        .feed(format!("* OK [UIDVALIDITY {timestamp}] UIDs valid"))
        .await?;
    let current_uid = storage.get_uid_for_folder(&mailbox_path)?;
    lines
        .feed(format!(
            "* OK [UIDNEXT {}] Predicted next UID",
            current_uid + 1,
        ))
        .await?;
    lines
        .feed(String::from(
            "* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)",
        ))
        .await?;
    lines
        .feed(String::from(
            "* OK [PERMANENTFLAGS (\\Deleted \\Seen \\*)] Limited",
        ))
        .await?;
    // TODO: generate proper list command
    lines.feed(format!("* LIST () \".\" \"{folder}\"")).await?;
    let sub_folders = storage.list_subdirs(&mailbox_path)?;
    for sub_folder in sub_folders {
        let flags_raw = storage.get_flags(&sub_folder).await;
        let flags = if let Ok(flags_raw) = flags_raw {
            flags_raw
        } else {
            vec![]
        };
        let folder_name = sub_folder
            .file_name()
            .context("Unable to get file name")?
            .to_string_lossy();
        lines
            .feed(format!(
                "* LIST ({}) \".\" \"{}\"",
                flags.join(" "),
                folder_name.trim_start_matches('.')
            ))
            .await?;
    }

    let resp = if rw {
        format!("{} OK [READ-WRITE] SELECT completed", command_data.tag)
    } else {
        format!("{} OK [READ-ONLY] EXAMINE completed", command_data.tag)
    };
    lines.feed(resp).await?;
    lines.flush().await?;
    Ok(())
}

impl Select<'_> {
    #[instrument(skip(self, lines, storage, command_data))]
    pub async fn exec<S, E>(
        &mut self,
        lines: &mut S,
        storage: &Storage,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        if matches!(self.data.con_state.state, State::Authenticated)
            || matches!(self.data.con_state.state, State::Selected(_, _))
        {
            select(self.data, lines, storage, true, command_data).await?;
        } else {
            lines
                .send(format!("{} NO invalid state", command_data.tag))
                .await?;
        }
        Ok(())
    }
}

pub struct Examine<'a> {
    pub data: &'a mut Data,
}

impl Examine<'_> {
    #[instrument(skip(self, lines, storage, command_data))]
    pub async fn exec<S, E>(
        &mut self,
        lines: &mut S,
        storage: &Storage,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        if matches!(self.data.con_state.state, State::Authenticated)
            || matches!(self.data.con_state.state, State::Selected(_, _))
        {
            select(self.data, lines, storage, false, command_data).await?;
        } else {
            lines
                .send(format!("{} NO invalid state", command_data.tag))
                .await?;
        }
        Ok(())
    }
}
