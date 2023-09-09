use crate::{
    commands::{
        auth::Auth, data::DataCommand, ehlo::Ehlo, mail::Mail, noop::Noop, quit::Quit, rcpt::Rcpt,
        rset::Rset,
    },
    servers::state::{AuthState, Connection, State},
};
use erooster_core::{
    backend::{database::DB, storage::Storage},
    config::Config,
};
use futures::{Sink, SinkExt};
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while1},
    character::complete::alpha1,
    error::{context, convert_error, VerboseError},
    multi::many0,
    sequence::{terminated, tuple},
    Finish, IResult,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, instrument, warn};

#[cfg(test)]
use std::fmt::Display;

mod auth;
mod data;
mod ehlo;
mod mail;
mod noop;
mod parsers;
mod quit;
mod rcpt;
mod rset;

#[derive(Debug, Clone)]
pub struct Data {
    pub con_state: Arc<RwLock<Connection>>,
}

#[derive(Debug)]
pub struct CommandData<'a> {
    command: Commands,
    arguments: &'a [&'a str],
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug)]
#[cfg_attr(
    test,
    derive(
        enum_iterator::Sequence,
        enum_display_derive::Display,
        Clone,
        Copy,
        PartialEq,
        Eq
    )
)]
pub enum Commands {
    AUTH,
    DATA,
    EHLO,
    MAILFROM,
    NOOP,
    QUIT,
    RCPTTO,
    RSET,
    STARTTLS,
}

impl TryFrom<&str> for Commands {
    type Error = String;

    #[instrument(skip(i))]
    fn try_from(i: &str) -> Result<Self, Self::Error> {
        match i.to_lowercase().as_str() {
            "ehlo" => Ok(Commands::EHLO),
            "quit" => Ok(Commands::QUIT),
            "mail from" => Ok(Commands::MAILFROM),
            "rcpt to" => Ok(Commands::RCPTTO),
            "data" => Ok(Commands::DATA),
            "auth" => Ok(Commands::AUTH),
            "noop" => Ok(Commands::NOOP),
            "rset" => Ok(Commands::RSET),
            "starttls" => Ok(Commands::STARTTLS),
            _ => {
                warn!("[SMTP⦘ Got unknown command: {}", i);
                Err(String::from("no other commands supported"))
            }
        }
    }
}

type Res<'a, U> = IResult<&'a str, U, VerboseError<&'a str>>;

/// Gets the command
#[instrument(skip(input))]
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
#[instrument(skip(input))]
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

#[allow(clippy::upper_case_acronyms)]
pub enum Response {
    Exit,
    Continue,
    STARTTLS,
}

impl Data {
    #[instrument(skip(line))]
    fn parse_internal(line: &str) -> Res<(Result<Commands, String>, Vec<&str>)> {
        context("parse_internal", tuple((command, arguments)))(line)
    }

    #[instrument(skip(self, lines, config, database, storage, line))]
    #[allow(clippy::too_many_lines)]
    pub async fn parse<S, E>(
        &self,
        lines: &mut S,
        config: Arc<Config>,
        database: &DB,
        storage: &Storage,
        line: String,
    ) -> color_eyre::eyre::Result<Response>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        debug!("Current state: {:?}", self.con_state.read().await.state);
        debug!("Current request: {}", line);

        let con_clone = Arc::clone(&self.con_state);
        let state = { con_clone.read().await.state.clone() };
        if matches!(state, State::ReceivingData(_)) {
            DataCommand { data: self }
                .receive(config, lines, &line, storage)
                .await?;
            // We are done here
            return Ok(Response::Continue);
        } else if let State::Authenticating(auth_state) = state {
            match auth_state {
                AuthState::Username => {
                    Auth { data: self }.username(lines, &line).await?;
                }
                AuthState::Plain => {
                    Auth { data: self }.plain(lines, database, &line).await?;
                }
                AuthState::Password(_) => {
                    Auth { data: self }.password(lines, database, &line).await?;
                }
            }
            // We are done here
            return Ok(Response::Continue);
        };
        let line_borrow: &str = &line;
        match Data::parse_internal(line_borrow).finish() {
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
                        return Ok(Response::Continue);
                    }
                };

                match command_data.command {
                    Commands::STARTTLS => {
                        // We need to accept tls from this point on
                        debug!("[SMTP] STARTTLS initiated");
                        return Ok(Response::STARTTLS);
                    }
                    Commands::RSET => {
                        Rset.exec(lines).await?;
                    }
                    Commands::EHLO => {
                        Ehlo { data: self }
                            .exec(&config.mail.hostname, lines, &command_data)
                            .await?;
                    }
                    Commands::QUIT => {
                        Quit.exec(lines).await?;
                        // We return true here early as we want to make sure that this closes the connection
                        return Ok(Response::Exit);
                    }
                    Commands::MAILFROM => {
                        Mail { data: self }
                            .exec(&config.mail.hostname, lines, &command_data)
                            .await?;
                    }
                    Commands::RCPTTO => {
                        Rcpt { data: self }
                            .exec(lines, database, &config.mail.hostname, &command_data)
                            .await?;
                    }
                    Commands::DATA => {
                        DataCommand { data: self }.exec(lines).await?;
                    }
                    Commands::AUTH => {
                        Auth { data: self }
                            .exec(lines, database, &command_data)
                            .await?;
                    }
                    Commands::NOOP => {
                        Noop.exec(lines).await?;
                    }
                }
            }
            Err(e) => {
                error!(
                    "[SMTP] Error parsing command: {}",
                    convert_error(line_borrow, e)
                );
                lines
                    .send(String::from("500 unable to parse command"))
                    .await?;
                return Ok(Response::Continue);
            }
        }
        Ok(Response::Continue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convert_case::{Case, Casing};
    use enum_iterator::all;

    #[test]
    fn test_parsing_commands() {
        for command_variant in all::<Commands>() {
            if let Commands::MAILFROM = command_variant {
                // This command has a space
                assert_eq!(
                    command(&"MAIL FROM:".to_string().to_uppercase()),
                    Ok(("", Ok(command_variant)))
                );
                assert_eq!(
                    command(&"MAIL FROM:".to_string().to_lowercase()),
                    Ok(("", Ok(command_variant)))
                );
                assert_eq!(
                    command(
                        &"MAIL FROM:"
                            .to_string()
                            .to_lowercase()
                            .to_case(Case::Alternating)
                    ),
                    Ok(("", Ok(command_variant)))
                );
            } else if let Commands::RCPTTO = command_variant {
                // This command has a space
                assert_eq!(
                    command(&"rcpt to:".to_string().to_uppercase()),
                    Ok(("", Ok(command_variant)))
                );
                assert_eq!(
                    command(&"rcpt to:".to_string().to_lowercase()),
                    Ok(("", Ok(command_variant)))
                );
                assert_eq!(
                    command(
                        &"rcpt to:"
                            .to_string()
                            .to_lowercase()
                            .to_case(Case::Alternating)
                    ),
                    Ok(("", Ok(command_variant)))
                );
            } else {
                assert_eq!(
                    command(&command_variant.to_string().to_uppercase()),
                    Ok(("", Ok(command_variant)))
                );
                assert_eq!(
                    command(&command_variant.to_string().to_lowercase()),
                    Ok(("", Ok(command_variant)))
                );
                assert_eq!(
                    command(
                        &command_variant
                            .to_string()
                            .to_lowercase()
                            .to_case(Case::Alternating)
                    ),
                    Ok(("", Ok(command_variant)))
                );
            }
        }
        assert_eq!(
            command("beeeeep"),
            Ok(("", Err(String::from("no other commands supported"))))
        );
    }
}
