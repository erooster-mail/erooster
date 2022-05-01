use crate::{
    config::Config,
    imap_commands::{
        utils::{add_flag, get_uid_for_folder},
        CommandData, Data,
    },
    imap_servers::state::{Access, State},
};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use maildir::Maildir;
use std::{
    path::Path,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Select<'a> {
    pub data: &'a Data,
}

async fn select<S>(
    data: &Data,
    lines: &mut S,
    config: Arc<Config>,
    rw: bool,
    command_data: &CommandData<'_>,
) -> color_eyre::eyre::Result<()>
where
    S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
{
    let args = &command_data.arguments;

    assert!(args.len() == 1);
    let folder = args.first().expect("server selects a folder");
    let folder = folder.replace('"', "");
    let access = if rw {
        Access::ReadWrite
    } else {
        Access::ReadOnly
    };
    {
        data.con_state.write().await.state = State::Selected(folder.clone(), access);
    };

    // Special INBOX check to make sure we have a mailbox
    let mailbox_path = Path::new(&config.mail.maildir_folders)
        .join(data.con_state.read().await.username.clone().unwrap())
        .join(folder.clone());
    let maildir = Maildir::from(mailbox_path.clone());
    if folder == "INBOX" && !mailbox_path.exists() {
        maildir.create_dirs()?;
        add_flag(&mailbox_path, "\\Subscribed")?;
        add_flag(&mailbox_path, "\\NoInferiors")?;
    }
    send_success(lines, folder, maildir, rw, command_data).await?;
    Ok(())
}

async fn send_success<S>(
    lines: &mut S,
    folder: String,
    maildir: Maildir,
    rw: bool,
    command_data: &CommandData<'_>,
) -> color_eyre::eyre::Result<()>
where
    S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
{
    // TODO get flags and perma flags
    // TODO UIDNEXT and UIDVALIDITY
    let count = maildir.count_cur() + maildir.count_new();
    lines.feed(format!("* {} EXISTS", count)).await?;
    let current_time = SystemTime::now();
    let unix_timestamp = current_time.duration_since(UNIX_EPOCH)?;
    #[allow(clippy::cast_possible_truncation)]
    let timestamp = unix_timestamp.as_millis() as u32;
    lines
        .feed(format!("* OK [UIDVALIDITY {}] UIDs valid", timestamp))
        .await?;
    let current_uid = get_uid_for_folder(maildir).await?;
    lines
        .feed(format!("* OK [UIDNEXT {}] Predicted next UID", current_uid,))
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
        .feed(format!("* LIST () \"/\" \"{}\"", folder))
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
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        config: Arc<Config>,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        if self.data.con_state.read().await.state == State::Authenticated {
            select(self.data, lines, config, true, command_data).await?;
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
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        config: Arc<Config>,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        if self.data.con_state.read().await.state == State::Authenticated {
            select(self.data, lines, config, false, command_data).await?;
        } else {
            lines
                .send(format!("{} NO invalid state", command_data.tag))
                .await?;
        }
        Ok(())
    }
}
