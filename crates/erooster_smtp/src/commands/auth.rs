use std::str::FromStr;

use crate::{
    commands::{CommandData, Data},
    servers::state::{AuthState, State},
};
use base64::Engine;
use erooster_core::{
    backend::database::{Database, DB},
    BASE64_DECODER,
};
use futures::{Sink, SinkExt};
use secrecy::{ExposeSecret, SecretString, SecretVec};
use simdutf8::compat::from_utf8;
use tracing::{debug, error, instrument};
pub struct Auth<'a> {
    pub data: &'a Data,
}

impl Auth<'_> {
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
        //let secure = self.data.con_state.read().await.secure;
        let secure = true;
        if secure {
            if command_data.arguments.is_empty() {
                lines
                    .send(String::from("454 4.7.0 Temporary authentication failure"))
                    .await?;
                return Ok(());
            }
            if command_data.arguments[0] == "LOGIN" {
                {
                    self.data.con_state.write().await.state =
                        State::Authenticating(AuthState::Username);
                };
                lines.send(String::from("334 VXNlcm5hbWU6")).await?;
            } else if command_data.arguments[0] == "PLAIN" {
                if command_data.arguments.len() == 2 {
                    self.plain(lines, database, command_data.arguments[1])
                        .await?;
                } else {
                    {
                        self.data.con_state.write().await.state =
                            State::Authenticating(AuthState::Plain);
                    };
                    lines.send(String::from("+ \"\"")).await?;
                }
            } else {
                lines
                    .send(String::from("535 4.7.8 Authentication credentials invalid"))
                    .await?;
            }
        } else {
            lines
                .send(String::from(
                    "538 5.7.11 Encryption required for requested authentication mechanism",
                ))
                .await?;
        }
        Ok(())
    }

    #[instrument(skip(self, lines, line))]
    pub async fn username<S, E>(&self, lines: &mut S, line: &str) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        let bytes = BASE64_DECODER.decode(line.as_bytes());
        match bytes {
            Ok(bytes) => {
                let username = from_utf8(&bytes)?;
                {
                    self.data.con_state.write().await.state =
                        State::Authenticating(AuthState::Password(username.to_string()));
                };
                lines.send(String::from("334 UGFzc3dvcmQ6")).await?;
            }
            Err(_) => {
                lines
                    .send(String::from("535 5.7.8 Authentication credentials invalid"))
                    .await?;
            }
        }

        Ok(())
    }

    #[instrument(skip(self, lines, database, line))]
    pub async fn plain<S, E>(
        &self,
        lines: &mut S,
        database: DB,
        line: &str,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        let bytes = BASE64_DECODER.decode(line.as_bytes());
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

                    debug!("[SMTP] Making sure user exists");
                    if database.user_exists(username).await {
                        debug!("[SMTP] Verify credentials");
                        if !database.verify_user(username, password).await {
                            {
                                write_lock.state = State::NotAuthenticated;
                            };
                            lines
                                .send(String::from("535 5.7.8 Authentication credentials invalid"))
                                .await?;
                            debug!("[SMTP] Invalid user or password");
                            return Ok(());
                        }
                        let secure = write_lock.secure;
                        if secure {
                            {
                                write_lock.state = State::Authenticated(username.to_string());
                                debug!("[SMTP] User authenticated");
                            };

                            lines
                                .send(String::from("235 2.7.0 Authentication Succeeded"))
                                .await?;
                        } else {
                            lines
                                .send(String::from(
                                    "538 5.7.11 Encryption required for requested authentication mechanism",
                                ))
                                .await?;
                        }
                    } else {
                        {
                            write_lock.state = State::NotAuthenticated;
                        };
                        lines
                            .send(String::from("535 5.7.8 Authentication credentials invalid"))
                            .await?;
                    }
                } else {
                    {
                        write_lock.state = State::NotAuthenticated;
                    };
                    lines
                        .send(String::from("432 4.7.12 A password transition is needed"))
                        .await?;
                }
            }
            Err(e) => {
                {
                    write_lock.state = State::NotAuthenticated;
                };
                error!("Error logging in: {}", e);
                lines
                    .send(String::from("432 4.7.12 A password transition is needed"))
                    .await?;
            }
        }
        Ok(())
    }

    #[instrument(skip(self, lines, database, line))]
    pub async fn password<S, E>(
        &self,
        lines: &mut S,
        database: DB,
        line: &str,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        let bytes = BASE64_DECODER.decode(line.as_bytes());
        match bytes {
            Ok(bytes) => {
                let password =
                    SecretString::from_str(from_utf8(SecretVec::new(bytes).expose_secret())?)?;

                {
                    let mut write_lock = self.data.con_state.write().await;
                    if let State::Authenticating(AuthState::Password(username)) = &write_lock.state
                    {
                        if database.user_exists(username).await {
                            let valid = database.verify_user(username, password).await;
                            if !valid {
                                write_lock.state = State::NotAuthenticated;
                                lines
                                    .send(String::from(
                                        "535 5.7.8 Authentication credentials invalid",
                                    ))
                                    .await?;
                                return Ok(());
                            }
                        } else {
                            write_lock.state = State::NotAuthenticated;
                            lines
                                .send(String::from("535 5.7.8 Authentication credentials invalid"))
                                .await?;
                            return Ok(());
                        }
                    } else {
                        write_lock.state = State::NotAuthenticated;
                        lines
                            .send(String::from("432 4.7.12 A password transition is needed"))
                            .await?;
                        return Ok(());
                    }
                    if let State::Authenticating(AuthState::Password(username)) = &write_lock.state
                    {
                        write_lock.state = State::Authenticated(username.to_string());
                    }
                };
                lines
                    .send(String::from("235 2.7.0  Authentication Succeeded"))
                    .await?;
            }
            Err(_) => {
                lines
                    .send(String::from("454 4.7.0 Temporary authentication failure"))
                    .await?;
            }
        }

        Ok(())
    }
}
