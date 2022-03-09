use async_trait::async_trait;
use futures::{Sink, SinkExt};
use simdutf8::compat::from_utf8;
use std::io;
use tracing::{debug, error};

use crate::{
    commands::{Command, Data},
    line_codec::LinesCodecError,
    state::State,
};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum AuthenticationMethod {
    Plain,
}

pub struct Authenticate<'a> {
    pub data: &'a mut Data<'a>,
    pub auth_data: String,
}

impl Authenticate<'_> {
    pub async fn plain<S>(&mut self, lines: &mut S) -> anyhow::Result<()>
    where
        S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
        S::Error: From<io::Error>,
    {
        debug!("auth_data: {}", self.auth_data);
        let bytes = base64::decode(self.auth_data.as_bytes());
        match bytes {
            Ok(bytes) => {
                let auth_data_vec: Vec<&str> = bytes
                    .split(|x| *x == b'\0')
                    .filter_map(|string| {
                        let new_string = from_utf8(string);
                        if let Ok(new_string) = new_string {
                            return Some(new_string);
                        }
                        None
                    })
                    .collect();
                if auth_data_vec.len() == 3 {
                    // TODO remove with actual implementation.
                    // This exists so the compiler knows that collect is justified here
                    let _guard = auth_data_vec.get(0);

                    // TODO check against DB
                    self.data.con_state.state = State::Authenticated;
                    let secure = { self.data.con_state.secure };
                    if secure {
                        lines
                            .send(format!(
                                "{} OK Success (tls protection)",
                                self.data.command_data.as_ref().unwrap().tag
                            ))
                            .await?;
                    } else {
                        lines
                            .send(format!(
                                "{} OK Success (unprotected)",
                                self.data.command_data.as_ref().unwrap().tag
                            ))
                            .await?;
                    }
                } else {
                    self.data.con_state.state = State::NotAuthenticated;
                    lines
                        .send(format!(
                            "{} BAD Invalid arguments",
                            self.data.command_data.as_ref().unwrap().tag
                        ))
                        .await?;
                }
            }
            Err(e) => {
                {
                    self.data.con_state.state = State::NotAuthenticated;
                };
                error!("Error logging in: {}", e);
                lines
                    .send(format!(
                        "{} BAD Invalid arguments",
                        self.data.command_data.as_ref().unwrap().tag
                    ))
                    .await?;
                return Ok(());
            }
        }

        Ok(())
    }
}

#[async_trait]
impl<S> Command<S> for Authenticate<'_>
where
    S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
    S::Error: From<io::Error>,
{
    async fn exec(&mut self, lines: &mut S) -> anyhow::Result<()> {
        if self.data.con_state.state == State::NotAuthenticated {
            if let Some(ref args) = self.data.command_data.as_ref().unwrap().arguments {
                if args.len() == 1 {
                    if args.first().unwrap().to_lowercase() == "plain" {
                        self.data.con_state.state = State::Authenticating((
                            AuthenticationMethod::Plain,
                            self.data.command_data.as_ref().unwrap().tag.clone(),
                        ));
                        lines.send(String::from("+ ")).await?;
                    } else {
                        self.plain(lines).await?;
                    }
                } else {
                    lines
                        .send(format!(
                            "{} BAD [SERVERBUG] unable to parse command",
                            self.data.command_data.as_ref().unwrap().tag
                        ))
                        .await?;
                }
            } else {
                lines
                    .send(format!(
                        "{} BAD [SERVERBUG] unable to parse command",
                        self.data.command_data.as_ref().unwrap().tag
                    ))
                    .await?;
            }
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
