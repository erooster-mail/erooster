use crate::{
    config::Config,
    imap_commands::{utils::add_flag, Command, CommandData, Data},
    servers::state::{Access, State},
};
use async_trait::async_trait;
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use maildir::Maildir;
use std::{path::Path, sync::Arc};

pub struct Select<'a> {
    pub data: &'a mut Data,
}

async fn select<S>(
    data: &mut Data,
    lines: &mut S,
    config: Arc<Config>,
    rw: bool,
    command_data: &CommandData,
) -> color_eyre::eyre::Result<()>
where
    S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
{
    let args = &command_data.arguments;

    assert!(args.len() == 1);
    let mut folder = args.first().expect("server selects a folder").to_string();
    folder.remove_matches('"');
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
    command_data: &CommandData,
) -> color_eyre::eyre::Result<()>
where
    S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
{
    // TODO get flags and perma flags
    // TODO UIDNEXT and UIDVALIDITY
    let count = maildir.count_cur() + maildir.count_new();
    lines.feed(format!("* {} EXISTS", count)).await?;
    lines
        .feed(String::from("* OK [UIDVALIDITY 3857529045] UIDs valid"))
        .await?;
    lines
        .feed(String::from("* OK [UIDNEXT 4392] Predicted next UID"))
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

#[async_trait]
impl<S> Command<S> for Select<'_>
where
    S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
{
    async fn exec(
        &mut self,
        lines: &mut S,
        config: Arc<Config>,
        command_data: &CommandData,
    ) -> color_eyre::eyre::Result<()> {
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
    pub data: &'a mut Data,
}

#[async_trait]
impl<S> Command<S> for Examine<'_>
where
    S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
{
    async fn exec(
        &mut self,
        lines: &mut S,
        config: Arc<Config>,
        command_data: &CommandData,
    ) -> color_eyre::eyre::Result<()> {
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
