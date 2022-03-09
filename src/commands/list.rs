use std::io;

use async_trait::async_trait;
use futures::{Sink, SinkExt};
use tracing::debug;

use crate::{
    commands::{Command, Commands, Data},
    line_codec::LinesCodecError,
    state::State,
};

pub struct Basic<'a> {
    pub data: &'a Data<'a>,
}

#[async_trait]
impl<S> Command<S> for Basic<'_>
where
    S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
    S::Error: From<io::Error>,
{
    // TODO parse all arguments
    async fn exec(&mut self, lines: &mut S) -> anyhow::Result<()> {
        if self.data.con_state.state == State::NotAuthenticated {
            lines
                .send(format!(
                    "{} BAD Not Authenticated",
                    self.data.command_data.as_ref().unwrap().tag
                ))
                .await?;
            return Ok(());
        }

        let command_resp = if self.data.command_data.as_ref().unwrap().command == Commands::LSub {
            "LSUB"
        } else {
            "LIST"
        };

        if let Some(ref arguments) = self.data.command_data.as_ref().unwrap().arguments {
            if let Some(first_arg) = arguments.first() {
                if first_arg == "\"\"" {
                    if let Some(second_arg) = arguments.last() {
                        // TODO proper parse. This is obviously wrong and also fails for second arg == "INBOX"
                        if second_arg == "\"\"" {
                            lines
                                .feed(format!("* {} (\\Noselect) \"/\" \"\"", command_resp))
                                .await?;
                            lines
                                .feed(format!(
                                    "{} OK {} Completed",
                                    self.data.command_data.as_ref().unwrap().tag,
                                    command_resp
                                ))
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

                            lines
                                .feed(format!(
                                    "{} OK done",
                                    self.data.command_data.as_ref().unwrap().tag,
                                ))
                                .await?;
                            lines.flush().await?;
                            return Ok(());
                        }
                    }
                }
            }
            lines
                .send(format!(
                    "{} BAD {} Arguments unknown",
                    self.data.command_data.as_ref().unwrap().tag,
                    command_resp
                ))
                .await?;
        }

        Ok(())
    }
}

pub struct Extended<'a> {
    pub data: &'a Data<'a>,
}

#[async_trait]
impl<S> Command<S> for Extended<'_>
where
    S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
    S::Error: From<io::Error>,
{
    // TODO setup
    async fn exec(&mut self, lines: &mut S) -> anyhow::Result<()> {
        debug!("extended");
        if self.data.con_state.state == State::NotAuthenticated {
            lines
                .send(format!(
                    "{} BAD Not Authenticated",
                    self.data.command_data.as_ref().unwrap().tag
                ))
                .await?;
        } else {
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
