use std::{io, sync::Arc};

use crate::{
    commands::{
        auth::{Authenticate, AuthenticationMethod},
        capability::Capability,
        check::Check,
        create::Create,
        list::{LSub, List},
        login::Login,
        logout::Logout,
        noop::Noop,
        select::Select,
    },
    config::Config,
    line_codec::LinesCodecError,
    servers::state::{Connection, State},
};
use async_trait::async_trait;
use futures::{Sink, SinkExt};
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while1},
    character::complete::alpha1,
    error::{context, VerboseError},
    multi::many0,
    sequence::{terminated, tuple},
    IResult,
};
use tracing::{debug, error, warn};

pub mod auth;
pub mod capability;
mod check;
mod create;
mod list;
mod login;
mod logout;
mod noop;
mod select;

#[derive(Debug, PartialEq)]
pub struct Data<'a> {
    pub command_data: Option<CommandData>,
    pub con_state: &'a mut Connection,
}

#[derive(Debug, PartialEq, Clone)]
pub struct CommandData {
    tag: String,
    command: Commands,
    arguments: Vec<String>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Commands {
    Capability,
    Login,
    Authenticate,
    List,
    LSub,
    Logout,
    Select,
    Noop,
    Check,
    Create,
}

impl TryFrom<&str> for Commands {
    type Error = String;

    fn try_from(i: &str) -> Result<Self, Self::Error> {
        match i.to_lowercase().as_str() {
            "capability" => Ok(Commands::Capability),
            "login" => Ok(Commands::Login),
            "authenticate" => Ok(Commands::Authenticate),
            "list" => Ok(Commands::List),
            "lsub" => Ok(Commands::LSub),
            "logout" => Ok(Commands::Logout),
            "select" => Ok(Commands::Select),
            "noop" => Ok(Commands::Noop),
            "check" => Ok(Commands::Check),
            "create" => Ok(Commands::Create),
            _ => {
                warn!("Got unknown command: {}", i);
                Err(String::from("no other commands supported"))
            }
        }
    }
}

type Res<'a, U> = IResult<&'a str, U, VerboseError<&'a str>>;

fn is_imaptag_char(c: char) -> bool {
    (c.is_ascii() || c == '1' || c == '*' || c == ']')
        && c != '+'
        && c != '('
        && c != ')'
        && c != '{'
        && c != ' '
        && c != '%'
        && c != '\\'
        && c != '"'
        && !c.is_control()
}

/// Takes as input the full string
fn imaptag(input: &str) -> Res<&str> {
    //alphanumeric1, none_of("+(){ %\\\""), one_of("1*]")
    context(
        "imaptag",
        terminated(take_while1(is_imaptag_char), tag(" ")),
    )(input)
    .map(|(next_input, res)| (next_input, res))
}

/// Gets the input minus the tag
fn command(input: &str) -> Res<Result<Commands, String>> {
    context("command", alt((terminated(alpha1, tag(" ")), alpha1)))(input)
        .map(|(next_input, res)| (next_input, res.try_into()))
}

/// Gets the input minus the tag and minus the command
fn arguments(input: &str) -> Res<Vec<String>> {
    context(
        "arguments",
        many0(alt((
            terminated(take_while1(|c: char| c != ' '), tag(" ")),
            take_while1(|c: char| c != ' '),
        ))),
    )(input)
    .map(|(x, y)| (x, y.iter().map(ToString::to_string).collect()))
}

impl Data<'_> {
    fn parse_internal(&mut self, line: &str) -> anyhow::Result<()> {
        match context("parse", tuple((imaptag, command, arguments)))(line).map(
            |(_, (tag, command, arguments))| match command {
                Ok(command) => {
                    self.command_data = Some(CommandData {
                        tag: tag.to_string(),
                        command,
                        arguments,
                    });
                    Ok(())
                }
                Err(e) => Err(anyhow::Error::msg(e)),
            },
        ) {
            Ok(v) => v,
            Err(e) => Err(anyhow::Error::msg(format!("{}", e))),
        }
    }
}

#[async_trait]
pub trait Command<Lines> {
    async fn exec(&mut self, lines: &mut Lines, config: Arc<Config>) -> anyhow::Result<()>;
}

#[async_trait]
pub trait Parser<Lines, 'a> {
    async fn parse(
        mut self,
        lines: &'a mut Lines,
        config: Arc<Config>,
        line: String,
    ) -> anyhow::Result<bool>;
}

#[async_trait]
impl<S, 'a> Parser<S, 'a> for Data<'a>
where
    S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
    S::Error: From<io::Error>,
{
    async fn parse(
        mut self,
        lines: &'a mut S,
        config: Arc<Config>,
        line: String,
    ) -> anyhow::Result<bool> {
        debug!("Current state: {:?}", self.con_state.state);
        if let State::Authenticating((AuthenticationMethod::Plain, tag)) = &self.con_state.state {
            self.command_data = Some(CommandData {
                tag: tag.to_string(),
                // This is unused but needed. We just assume Authenticate here
                command: Commands::Authenticate,
                arguments: vec![],
            });
            Authenticate {
                data: &mut self,
                auth_data: line,
            }
            .plain(lines, config)
            .await?;
            // We are done here
            return Ok(false);
        }
        let parse_result = self.parse_internal(&line);
        match parse_result {
            Ok(_) => {
                if let Some(CommandData {
                    command,
                    ref arguments,
                    ..
                }) = self.command_data
                {
                    match command {
                        Commands::Capability => {
                            Capability { data: &self }.exec(lines, config).await?;
                        }
                        Commands::Login => Login { data: &self }.exec(lines, config).await?,
                        Commands::Logout => {
                            Logout { data: &self }.exec(lines, config).await?;
                            // We return true here early as we want to make sure that this closes the connection
                            return Ok(true);
                        }
                        Commands::Authenticate => {
                            let auth_data = arguments.last().unwrap().to_string();
                            Authenticate {
                                data: &mut self,
                                auth_data,
                            }
                            .exec(lines, config)
                            .await?;
                        }
                        Commands::List => {
                            List { data: &self }.exec(lines, config).await?;
                        }
                        Commands::LSub => {
                            LSub { data: &self }.exec(lines, config).await?;
                        }
                        Commands::Select => {
                            Select { data: &mut self }.exec(lines, config).await?;
                        }
                        Commands::Create => Create { data: &self }.exec(lines, config).await?,
                        Commands::Noop => {
                            Noop { data: &self }.exec(lines, config).await?;
                        }
                        Commands::Check => {
                            Check { data: &self }.exec(lines, config).await?;
                        }
                    }
                }
            }
            Err(error) => {
                error!("Error parsing command: {}", error);
                lines
                    .send(String::from("* BAD [SERVERBUG] unable to parse command"))
                    .await?;
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;
    use std::net::Ipv4Addr;

    #[test]
    fn test_parsing_imaptag() {
        assert_eq!(imaptag("abcd CAPABILITY"), Ok(("CAPABILITY", "abcd")));
    }

    #[test]
    fn test_parsing_command() {
        assert_eq!(command("CAPABILITY"), Ok(("", Ok(Commands::Capability))));
        assert_eq!(command("LOGOUT"), Ok(("", Ok(Commands::Logout))));
    }

    #[test]
    fn test_parsing_arguments() {
        assert_eq!(
            arguments("PLAIN abd=="),
            Ok(("", vec![String::from("PLAIN"), String::from("abd==")]))
        );
        assert_eq!(arguments("PLAIN"), Ok(("", vec![String::from("PLAIN")])));
    }

    #[test]
    fn test_parsing_authenticate_command() {
        let mut con_state = super::Connection {
            state: super::State::NotAuthenticated,
            ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            secure: true,
            username: None,
        };
        let mut data = Data {
            command_data: None,
            con_state: &mut con_state,
        };
        let result = data.parse_internal("a AUTHENTICATE PLAIN abcde");
        assert!(result.is_ok());
        assert!(data.command_data.is_some());
        assert_eq!(
            data.command_data.unwrap(),
            CommandData {
                tag: String::from("a"),
                command: Commands::Authenticate,
                arguments: vec![String::from("PLAIN"), String::from("abcde")],
            }
        );

        data.command_data = None;
        let result = data.parse_internal("a AUTHENTICATE PLAIN");
        assert!(result.is_ok());
        assert!(data.command_data.is_some());
        assert_eq!(
            data.command_data.unwrap(),
            CommandData {
                tag: String::from("a"),
                command: Commands::Authenticate,
                arguments: vec![String::from("PLAIN")],
            }
        );
    }

    #[test]
    fn test_parsing_capability_command() {
        let mut con_state = super::Connection {
            state: super::State::NotAuthenticated,
            ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            secure: true,
            username: None,
        };
        let mut data = Data {
            command_data: None,
            con_state: &mut con_state,
        };
        let result = data.parse_internal("a CAPABILITY");
        assert!(result.is_ok());
        assert!(data.command_data.is_some());
        assert_eq!(
            data.command_data.unwrap(),
            CommandData {
                tag: String::from("a"),
                command: Commands::Capability,
                arguments: vec![],
            }
        );
    }

    #[test]
    fn test_parsing_list_command() {
        let mut con_state = super::Connection {
            state: super::State::Authenticated,
            ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            secure: true,
            username: None,
        };
        let mut data = Data {
            command_data: None,
            con_state: &mut con_state,
        };
        let result = data.parse_internal("18 list \"\" \"*\"");
        assert!(result.is_ok());
        assert!(data.command_data.is_some());
        assert_eq!(
            data.command_data.unwrap(),
            CommandData {
                tag: String::from("18"),
                command: Commands::List,
                arguments: vec![String::from("\"\""), String::from("\"*\"")],
            }
        );
        data.command_data = None;
        let result = data.parse_internal("18 list \"\" \"\"");
        assert!(result.is_ok());
        assert!(data.command_data.is_some());
        assert_eq!(
            data.command_data.unwrap(),
            CommandData {
                tag: String::from("18"),
                command: Commands::List,
                arguments: vec![String::from("\"\""), String::from("\"\"")],
            }
        );
    }
}
