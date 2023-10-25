use std::str::FromStr;

use crate::{
    commands::{CommandData, Data},
    servers::state::State,
};
use base64::Engine;
use erooster_core::{
    backend::database::{Database, DB},
    BASE64_DECODER,
};
use futures::{Sink, SinkExt};
use secrecy::SecretString;
use simdutf8::compat::from_utf8;
use tracing::{debug, error, instrument};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum AuthenticationMethod {
    Plain,
}

pub struct Authenticate<'a> {
    pub data: &'a mut Data,
    pub auth_data: &'a str,
}

impl Authenticate<'_> {
    #[instrument(skip(self, lines, database, command_data))]
    pub async fn plain<S, E>(
        &mut self,
        lines: &mut S,
        database: &DB,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        let bytes = BASE64_DECODER.decode(self.auth_data.as_bytes());
        match bytes {
            Ok(bytes) => {
                let auth_data_vec: Vec<&str> = bytes
                    .split(|x| *x == b'\0')
                    .filter_map(|string| {
                        let new_string = from_utf8(string);
                        if let Ok(new_string) = new_string {
                            if new_string.is_empty() {
                                return None;
                            }
                            return Some(new_string);
                        }
                        None
                    })
                    .collect();

                if auth_data_vec.len() == 2 {
                    let username = auth_data_vec[0];
                    let password = SecretString::from_str(auth_data_vec[1])?;

                    debug!("[IMAP] Making sure user exists");
                    if database.user_exists(username).await {
                        debug!("[IMAP] Verify credentials");
                        if !database.verify_user(username, password).await {
                            {
                                self.data.con_state.state = State::NotAuthenticated;
                            };
                            lines
                                .send(format!("{} NO Invalid user or password", command_data.tag))
                                .await?;
                            debug!("[IMAP] Invalid user or password");
                            return Ok(());
                        }
                        {
                            self.data.con_state.username = Some(username.to_string());
                            self.data.con_state.state = State::Authenticated;
                        };
                        let secure = self.data.con_state.secure;
                        if secure {
                            lines
                                .send(format!("{} OK Success (tls protection)", command_data.tag))
                                .await?;
                        } else {
                            lines
                                .send(format!("{} OK Success (unprotected)", command_data.tag))
                                .await?;
                        }
                    } else {
                        {
                            self.data.con_state.state = State::NotAuthenticated;
                        };
                        lines
                            .send(format!("{} NO Invalid user or password", command_data.tag))
                            .await?;
                    }
                } else {
                    {
                        self.data.con_state.state = State::NotAuthenticated;
                    };
                    lines
                        .send(format!("{} BAD Invalid arguments", command_data.tag))
                        .await?;
                }
            }
            Err(e) => {
                {
                    self.data.con_state.state = State::NotAuthenticated;
                };
                error!("Error logging in: {}", e);
                lines
                    .send(format!("{} BAD Invalid arguments", command_data.tag))
                    .await?;
            }
        }

        Ok(())
    }

    #[instrument(skip(self, lines, database, command_data))]
    pub async fn exec<S, E>(
        &mut self,
        lines: &mut S,
        database: &DB,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        if self.data.con_state.state == State::NotAuthenticated {
            let args = &command_data.arguments;
            assert!(args.len() == 1);
            if args.len() == 1 {
                if args[0].to_lowercase() == "plain" {
                    debug!("[IMAP] Update state to Authenticating");
                    {
                        let command_data = command_data;
                        self.data.con_state.state = State::Authenticating(
                            AuthenticationMethod::Plain,
                            command_data.tag.to_string(),
                        );
                    };
                    debug!("[IMAP] Sending continuation request");
                    lines.send(String::from("+ ")).await?;
                } else {
                    self.plain(lines, database, command_data).await?;
                }
            } else {
                lines
                    .send(format!(
                        "{} BAD [SERVERBUG] unable to parse command",
                        command_data.tag
                    ))
                    .await?;
            }
        } else {
            lines
                .send(format!("{} NO invalid state", command_data.tag))
                .await?;
        }
        Ok(())
    }
}
