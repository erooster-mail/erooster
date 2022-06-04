use crate::{
    commands::{CommandData, Data},
    servers::state::{Access, State},
};
use erooster_core::backend::storage::{MailStorage, Storage};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::instrument;

pub struct Select<'a> {
    pub data: &'a Data,
}

#[instrument(skip(data, lines, storage, rw, command_data))]
async fn select<S>(
    data: &Data,
    lines: &mut S,
    storage: Arc<Storage>,
    rw: bool,
    command_data: &CommandData<'_>,
) -> color_eyre::eyre::Result<()>
where
    S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
{
    let args = &command_data.arguments;
    let mut write_lock = data.con_state.write().await;

    assert!(args.len() == 1);
    let folder_arg = args.first().expect("server selects a folder");
    let folder = folder_arg.replace('"', "");
    let access = if rw {
        Access::ReadWrite
    } else {
        Access::ReadOnly
    };
    {
        write_lock.state = State::Selected(folder.clone(), access);
    };

    let folder_on_disk = folder_arg;
    let mailbox_path = storage.to_ondisk_path(
        (*folder_on_disk).to_string(),
        write_lock.username.clone().unwrap(),
    )?;
    // Special INBOX check to make sure we have a mailbox
    if folder == "INBOX" && !mailbox_path.exists() {
        storage.create_dirs(
            mailbox_path
                .clone()
                .into_os_string()
                .into_string()
                .expect("Failed to convert path. Your system may be incompatible"),
        )?;
        storage.add_flag(&mailbox_path, "\\Subscribed").await?;
        storage.add_flag(&mailbox_path, "\\NoInferiors").await?;
    }
    send_success(
        lines,
        folder,
        storage,
        mailbox_path
            .into_os_string()
            .into_string()
            .expect("Failed to convert path. Your system may be incompatible"),
        rw,
        command_data,
    )
    .await?;
    Ok(())
}

#[instrument(skip(lines, folder, storage, mailbox_path, rw, command_data))]
async fn send_success<S>(
    lines: &mut S,
    folder: String,
    storage: Arc<Storage>,
    mailbox_path: String,
    rw: bool,
    command_data: &CommandData<'_>,
) -> color_eyre::eyre::Result<()>
where
    S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
{
    let count = storage.count_cur(mailbox_path.clone()) + storage.count_new(mailbox_path.clone());
    lines.feed(format!("* {} EXISTS", count)).await?;
    let current_time = SystemTime::now();
    let unix_timestamp = current_time.duration_since(UNIX_EPOCH)?;
    #[allow(clippy::cast_possible_truncation)]
    let timestamp = unix_timestamp.as_millis() as u32;
    lines
        .feed(format!("* OK [UIDVALIDITY {}] UIDs valid", timestamp))
        .await?;
    let current_uid = storage.get_uid_for_folder(mailbox_path)?;
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
    // TODO generate proper list command
    lines
        .feed(format!("* LIST () \".\" \"{}\"", folder))
        .await?;

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
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        storage: Arc<Storage>,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        if self.data.con_state.read().await.state == State::Authenticated {
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
    pub data: &'a Data,
}

impl Examine<'_> {
    #[instrument(skip(self, lines, storage, command_data))]
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        storage: Arc<Storage>,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        if self.data.con_state.read().await.state == State::Authenticated {
            select(self.data, lines, storage, false, command_data).await?;
        } else {
            lines
                .send(format!("{} NO invalid state", command_data.tag))
                .await?;
        }
        Ok(())
    }
}
