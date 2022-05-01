use std::sync::Arc;

use futures::{channel::mpsc::SendError, Sink, SinkExt};
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while1},
    character::complete::alpha1,
    error::{context, VerboseError},
    multi::many0,
    sequence::{terminated, tuple},
    IResult,
};
use tokio::sync::RwLock;
use tracing::{debug, error, warn};

use crate::{
    config::Config,
    smtp_commands::{
        auth::Auth, data::DataCommand, ehlo::Ehlo, mail::Mail, noop::Noop, quit::Quit, rcpt::Rcpt,
    },
    smtp_servers::state::{AuthState, Connection, State},
};

mod auth;
mod data;
mod ehlo;
mod mail;
mod noop;
mod parsers;
mod quit;
mod rcpt;

#[derive(Debug)]
pub struct Data {
    pub con_state: Arc<RwLock<Connection>>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct CommandData<'a> {
    command: Commands,
    arguments: &'a [&'a str],
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[allow(clippy::upper_case_acronyms)]
pub enum Commands {
    EHLO,
    QUIT,
    MAILFROM,
    RCPTTO,
    DATA,
    AUTH,
    NOOP,
}

impl TryFrom<&str> for Commands {
    type Error = String;

    fn try_from(i: &str) -> Result<Self, Self::Error> {
        match i.to_lowercase().as_str() {
            "ehlo" => Ok(Commands::EHLO),
            "quit" => Ok(Commands::QUIT),
            "mail from" => Ok(Commands::MAILFROM),
            "rcpt to" => Ok(Commands::RCPTTO),
            "data" => Ok(Commands::DATA),
            "auth" => Ok(Commands::AUTH),
            "noop" => Ok(Commands::NOOP),
            _ => {
                warn!("[SMTP⦘ Got unknown command: {}", i);
                Err(String::from("no other commands supported"))
            }
        }
    }
}

type Res<'a, U> = IResult<&'a str, U, VerboseError<&'a str>>;

/// Gets the command
fn command(input: &str) -> Res<Result<Commands, String>> {
    context(
        "command",
        alt((
            terminated(
                take_while1(|c: char| c.is_alphanumeric() || c.is_whitespace()),
                tag(":"),
            ),
            terminated(alpha1, tag(" ")),
            alpha1,
        )),
    )(input)
    .map(|(next_input, res)| (next_input, res.try_into()))
}

/// Gets the input arguments
fn arguments(input: &str) -> Res<Vec<&str>> {
    context(
        "arguments",
        many0(alt((
            terminated(take_while1(|c: char| c != ' '), tag(" ")),
            take_while1(|c: char| c != ' '),
        ))),
    )(input)
    .map(|(x, y)| (x, y))
}

impl Data {
    fn parse_internal(line: &str) -> Res<(Result<Commands, String>, Vec<&str>)> {
        context("parse", tuple((command, arguments)))(line)
    }

    #[allow(clippy::too_many_lines)]
    pub async fn parse<S>(
        &self,
        lines: &mut S,
        config: Arc<Config>,
        line: String,
    ) -> color_eyre::eyre::Result<bool>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        debug!("Current state: {:?}", self.con_state.read().await.state);

        let con_clone = Arc::clone(&self.con_state);
        let state = { con_clone.read().await.state.clone() };
        if let State::ReceivingData = state {
            DataCommand { data: self }
                .receive(config, lines, &line)
                .await?;
            // We are done here
            return Ok(false);
        } else if let State::Authenticating(auth_state) = state {
            match auth_state {
                AuthState::Username => {
                    Auth { data: self }.username(lines, &line).await?;
                }
                AuthState::Password(_) => {
                    Auth { data: self }.password(lines, &line).await?;
                }
            }
            // We are done here
            return Ok(false);
        };
        match Data::parse_internal(&line) {
            Ok((_, (command, arguments))) => {
                let command_data = match command {
                    Ok(command) => CommandData {
                        command,
                        arguments: &arguments,
                    },
                    Err(e) => {
                        error!("[SMTP] Error parsing command: {}", e);
                        lines
                            .send(String::from("500 unable to parse command"))
                            .await?;
                        return Ok(false);
                    }
                };

                match command_data.command {
                    Commands::EHLO => {
                        Ehlo.exec(config.mail.hostname.clone(), lines).await?;
                    }
                    Commands::QUIT => {
                        Quit.exec(lines).await?;
                        // We return true here early as we want to make sure that this closes the connection
                        return Ok(true);
                    }
                    Commands::MAILFROM => {
                        Mail { data: self }.exec(lines, &command_data).await?;
                    }
                    Commands::RCPTTO => {
                        Rcpt { data: self }.exec(lines, &command_data).await?;
                    }
                    Commands::DATA => {
                        DataCommand { data: self }.exec(lines).await?;
                    }
                    Commands::AUTH => {
                        Auth { data: self }.exec(lines, &command_data).await?;
                    }
                    Commands::NOOP => {
                        Noop.exec(lines).await?;
                    }
                }
            }
            Err(e) => {
                error!("[SMTP] Error parsing command: {}", e);
                lines
                    .send(String::from("500 unable to parse command"))
                    .await?;
                return Ok(false);
            }
        }
        Ok(false)
    }
}
