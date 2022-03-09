use std::io;

use async_trait::async_trait;
use futures::{Sink, SinkExt};
use tracing::debug;

use crate::{
    commands::{Command, Commands, Data},
    line_codec::LinesCodecError,
    state::State,
};

pub async fn basic<'a, S>(data: &'a Data<'a>, lines: &mut S) -> anyhow::Result<()>
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

    if let Some(ref arguments) = data.command_data.as_ref().unwrap().arguments {
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
                                data.command_data.as_ref().unwrap().tag,
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
                                data.command_data.as_ref().unwrap().tag,
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
                data.command_data.as_ref().unwrap().tag,
                command_resp
            ))
            .await?;
    }

    Ok(())
}
pub struct List<'a> {
    pub data: &'a Data<'a>,
}

impl List<'_> {
    // TODO parse all arguments

    // TODO setup
    pub async fn extended<S>(&mut self, lines: &mut S) -> anyhow::Result<()>
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
    async fn exec(&mut self, lines: &mut S) -> anyhow::Result<()> {
        if let Some(ref arguments) = self.data.command_data.as_ref().unwrap().arguments {
            if arguments.len() == 2 {
                basic(self.data, lines).await?;
            } else if arguments.len() == 4 {
                self.extended(lines).await?;
            } else {
                lines
                    .send(format!(
                        "{} BAD [SERVERBUG] invalid arguments",
                        self.data.command_data.as_ref().unwrap().tag
                    ))
                    .await?;
            }
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
    async fn exec(&mut self, lines: &mut S) -> anyhow::Result<()> {
        if let Some(ref arguments) = self.data.command_data.as_ref().unwrap().arguments {
            if arguments.len() == 2 {
                basic(self.data, lines).await?;
            } else {
                lines
                    .send(format!(
                        "{} BAD [SERVERBUG] invalid arguments",
                        self.data.command_data.as_ref().unwrap().tag
                    ))
                    .await?;
            }
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
