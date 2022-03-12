use crate::{
    config::Config,
    imap_commands::{utils::add_flag, Command, Data},
    servers::state::State,
};
use async_trait::async_trait;
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use maildir::Maildir;
use std::{path::Path, sync::Arc};

pub struct Select<'a> {
    pub data: &'a mut Data,
}

impl Select<'_> {
    async fn send_success<S>(
        &self,
        lines: &mut S,
        folder: String,
        maildir: Maildir,
    ) -> anyhow::Result<()>
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
        lines
            .feed(format!(
                "{} OK [READ-WRITE] SELECT completed",
                self.data.command_data.as_ref().unwrap().tag
            ))
            .await?;
        lines.flush().await?;
        Ok(())
    }
}

#[async_trait]
impl<S> Command<S> for Select<'_>
where
    S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
{
    async fn exec(&mut self, lines: &mut S, config: Arc<Config>) -> anyhow::Result<()> {
        if self.data.con_state.read().await.state == State::Authenticated {
            let args = &self.data.command_data.as_ref().unwrap().arguments;

            debug_assert_eq!(args.len(), 1);
            let mut folder = args.first().expect("server selects a folder").to_string();
            folder.remove_matches('"');
            {
                self.data.con_state.write().await.state = State::Selected(folder.clone());
            };

            // Special INBOX check to make sure we have a mailbox
            let mailbox_path = Path::new(&config.mail.maildir_folders)
                .join(self.data.con_state.read().await.username.clone().unwrap())
                .join(folder.clone());
            let maildir = Maildir::from(mailbox_path.clone());
            if folder == "INBOX" && !mailbox_path.exists() {
                maildir.create_dirs()?;
                add_flag(&mailbox_path, "\\Subscribed")?;
                add_flag(&mailbox_path, "\\NoInferiors")?;
            }
            self.send_success(lines, folder, maildir).await?;
        } else {
            lines
                .send(format!(
                    "{} NO invalid state",
                    self.data.command_data.as_ref().unwrap().tag
                ))
                .await?;
        }
        Ok(())
    }
}
