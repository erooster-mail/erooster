use std::str::FromStr;

use crate::{
    commands::{CommandData, Data},
    servers::state::{AuthState, State},
};
use erooster_core::backend::database::{Database, DB};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use secrecy::{ExposeSecret, SecretString, SecretVec};
use simdutf8::compat::from_utf8;
use tracing::instrument;
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
