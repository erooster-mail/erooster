use std::io;

use async_trait::async_trait;
use futures::{Sink, SinkExt};

use crate::{
    commands::{Command, Data},
    line_codec::LinesCodecError,
    state::State,
};

pub struct Select<'a> {
    pub data: &'a mut Data<'a>,
}

#[async_trait]
impl<S> Command<S> for Select<'_>
where
    S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
    S::Error: From<io::Error>,
{
    async fn exec(&mut self, lines: &mut S) -> anyhow::Result<()> {
        if self.data.con_state.state == State::Authenticated {
            let args = &self.data.command_data.as_ref().unwrap().arguments;
            
            debug_assert_eq!(args.len(), 1);
            let mut folder = args.first().expect("server selects a folder").to_string();
            folder.remove_matches('"');
            self.data.con_state.state = State::Selected(folder);
            // TODO get count of mails
            // TODO get flags and perma flags
            // TODO get real list
            // TODO UIDNEXT and UIDVALIDITY
            lines.feed(String::from("* 0 EXISTS")).await?;
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
            lines
                .feed(String::from("* LIST () \"/\" \"INBOX\""))
                .await?;
            lines
                .feed(format!(
                    "{} OK [READ-WRITE] SELECT completed",
                    self.data.command_data.as_ref().unwrap().tag
                ))
                .await?;
            lines.flush().await?;
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
