use std::io;

use async_trait::async_trait;
use futures::{Sink, SinkExt};
use tracing::debug;

use crate::{
    commands::{Command, Commands, Data},
    line_codec::LinesCodecError,
    servers::State,
};

pub struct Basic<'a, 'b> {
    pub data: Data<'a, 'b>,
}

#[async_trait]
impl<S> Command<S> for Basic<'_, '_>
where
    S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
    S::Error: From<io::Error>,
{
    // TODO parse all arguments
    async fn exec(&mut self, lines: &'async_trait mut S) -> anyhow::Result<()> {
        let state = { self.data.con_state.state.clone() };
        if state == State::NotAuthenticated {
            lines
                .send(format!("{} BAD Not Authenticated", self.data.tag))
                .await?;
            return Ok(());
        }

        let command_resp = if self.data.command == Commands::LSub {
            "LSUB"
        } else {
            "LIST"
        };

        let arguments = self.data.arguments.as_ref().unwrap();
        if let Some(first_arg) = arguments.first() {
            if first_arg == "\"\"" {
                if let Some(second_arg) = arguments.last() {
                    // TODO proper parse. This is obviously wrong and also fails for second arg == "INBOX"
                    if second_arg == "\"\"" {
                        lines
                            .feed(format!("* {} (\\Noselect) \"/\" \"\"", command_resp))
                            .await?;
                        lines
                            .feed(format!("{} OK {} Completed", self.data.tag, command_resp))
                            .await?;
                        lines.flush().await?;
                        return Ok(());
                    }
                    if second_arg == "\"*\"" {
                        lines
                            .feed(format!(
                                "* {} (\\NoInferiors) \"/\" \"INBOX\"",
                                command_resp,
                            ))
                            .await?;

                        lines.feed(format!("{} OK done", self.data.tag,)).await?;
                        lines.flush().await?;
                        return Ok(());
                    }
                }
            }
        }
        lines
            .send(format!(
                "{} BAD {} Arguments unknown",
                self.data.tag, command_resp
            ))
            .await?;
        Ok(())
    }
}

pub struct Extended<'a, 'b> {
    pub data: Data<'a, 'b>,
}

#[async_trait]
impl<S> Command<S> for Extended<'_, '_>
where
    S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
    S::Error: From<io::Error>,
{
    // TODO setup
    async fn exec(&mut self, lines: &'async_trait mut S) -> anyhow::Result<()> {
        debug!("extended");
        let state = { self.data.con_state.state.clone() };
        if state == State::NotAuthenticated {
            lines
                .send(format!("{} BAD Not Authenticated", self.data.tag))
                .await?;
        } else {
            lines
                .send(format!("{} BAD LIST Not supported", self.data.tag))
                .await?;
        }
        Ok(())
    }
}
