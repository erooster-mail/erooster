use std::str::FromStr;

use crate::{
    commands::{CommandData, Data},
    servers::state::{AuthState, State},
};
use erooster_core::backend::database::{Database, DB};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use secrecy::{ExposeSecret, SecretString, SecretVec};
use simdutf8::compat::from_utf8;
use tracing::{debug, error, instrument};
pub struct Auth<'a> {
    pub data: &'a Data,
}

impl Auth<'_> {
    #[instrument(skip(self, lines, command_data))]
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        //let secure = self.data.con_state.read().await.secure;
        let secure = true;
        if secure {
            assert!(command_data.arguments.len() == 1);
            if command_data.arguments[0] == "LOGIN" {
                {
                    self.data.con_state.write().await.state =
                        State::Authenticating(AuthState::Username);
                };
                lines.send(String::from("334 VXNlcm5hbWU6")).await?;
            } else if command_data.arguments[0] == "PLAIN" {
                {
                    self.data.con_state.write().await.state =
                        State::Authenticating(AuthState::Plain);
                };
                lines.send(String::from("+ \"\"")).await?;
            } else {
                lines
                    .send(String::from("504 Unrecognized authentication type."))
                    .await?;
            }
        } else {
            lines
                .send(String::from(
                    "538 Encryption required for requested authentication mechanism",
                ))
                .await?;
        }
        Ok(())
    }

    #[instrument(skip(self, lines, line))]
    pub async fn username<S>(&self, lines: &mut S, line: &str) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        let bytes = base64::decode(line.as_bytes());
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
                    .send(String::from("501 Syntax error in parameters or arguments"))
                    .await?;
            }
        }

        Ok(())
    }

    #[instrument(skip(self, lines, database, line))]
    pub async fn plain<S>(
        &self,
        lines: &mut S,
        database: DB,
        line: &str,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        let bytes = base64::decode(line.as_bytes());
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
                debug!("auth_data_vec: {:?}", auth_data_vec);

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
                        {
                            write_lock.state = State::Authenticated(username.to_string());
                        };
                        let secure = write_lock.secure;
                        if secure {
                            lines.send(String::from("235 ok")).await?;
                        } else {
                            lines
                                .send(String::from(
                                    "538 Encryption required for requested authentication mechanism",
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
                        .send(String::from("503 Bad sequence of commands"))
                        .await?;
                }
            }
            Err(e) => {
                {
                    write_lock.state = State::NotAuthenticated;
                };
                error!("Error logging in: {}", e);
                lines
                    .send(String::from("503 Bad sequence of commands"))
                    .await?;
            }
        }
        Ok(())
    }

    #[instrument(skip(self, lines, database, line))]
    pub async fn password<S>(
        &self,
        lines: &mut S,
        database: DB,
        line: &str,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        let bytes = base64::decode(line.as_bytes());
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
                            .send(String::from("503 Bad sequence of commands"))
                            .await?;
                        return Ok(());
                    }
                    if let State::Authenticating(AuthState::Password(username)) = &write_lock.state
                    {
                        write_lock.state = State::Authenticated(username.to_string());
                    }
                };
                lines.send(String::from("235 ok")).await?;
            }
            Err(_) => {
                lines
                    .send(String::from("501 Syntax error in parameters or arguments"))
                    .await?;
            }
        }

        Ok(())
    }
}
