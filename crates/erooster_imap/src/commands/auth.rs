use std::str::FromStr;

use crate::{
    commands::{CommandData, Data},
    servers::state::State,
};
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine};
use erooster_core::backend::database::{Database, DB};
use futures::{Sink, SinkExt};
use secrecy::SecretString;
use simdutf8::compat::from_utf8;
use tracing::{debug, error, instrument};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum AuthenticationMethod {
    Plain,
}

pub struct Authenticate<'a> {
    pub data: &'a Data,
    pub auth_data: &'a str,
}

impl Authenticate<'_> {
    #[instrument(skip(self, lines, database, command_data))]
    pub async fn plain<S, E>(
        &self,
        lines: &mut S,
        database: DB,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        let bytes = STANDARD_NO_PAD.decode(self.auth_data.as_bytes());
        let mut write_lock = self.data.con_state.write().await;
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
                                write_lock.state = State::NotAuthenticated;
                            };
                            lines
                                .send(format!("{} NO Invalid user or password", command_data.tag))
                                .await?;
                            debug!("[IMAP] Invalid user or password");
                            return Ok(());
                        }
                        {
                            write_lock.username = Some(username.to_string());
                            write_lock.state = State::Authenticated;
                        };
                        let secure = write_lock.secure;
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
                            write_lock.state = State::NotAuthenticated;
                        };
                        lines
                            .send(format!("{} NO Invalid user or password", command_data.tag))
                            .await?;
                    }
                } else {
                    {
                        write_lock.state = State::NotAuthenticated;
                    };
                    lines
                        .send(format!("{} BAD Invalid arguments", command_data.tag))
                        .await?;
                }
            }
            Err(e) => {
                {
                    write_lock.state = State::NotAuthenticated;
                };
                error!("Error logging in: {}", e);
                lines
                    .send(format!("{} BAD Invalid arguments", command_data.tag))
                    .await?;
            }
        }

        Ok(())
    }
}

impl Authenticate<'_> {
    #[instrument(skip(self, lines, database, command_data))]
    pub async fn exec<S, E>(
        &self,
        lines: &mut S,
        database: DB,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        if self.data.con_state.read().await.state == State::NotAuthenticated {
            let args = &command_data.arguments;
            assert!(args.len() == 1);
            if args.len() == 1 {
                if args[0].to_lowercase() == "plain" {
                    debug!("[IMAP] Update state to Authenticating");
                    {
                        let command_data = command_data;
                        self.data.con_state.write().await.state = State::Authenticating(
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
