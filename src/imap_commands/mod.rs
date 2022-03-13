use crate::{
    config::Config,
    imap_commands::{
        auth::{Authenticate, AuthenticationMethod},
        capability::Capability,
        check::Check,
        close::Close,
        create::Create,
        delete::Delete,
        list::{LSub, List},
        login::Login,
        logout::Logout,
        noop::Noop,
        select::{Examine, Select},
        subscribe::Subscribe,
    },
    servers::state::{Connection, State},
};
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
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, warn};

mod utils;

pub mod auth;
pub mod capability;
mod check;
mod close;
mod create;
mod delete;
mod list;
mod login;
mod logout;
mod noop;
mod select;
mod subscribe;

#[derive(Debug)]
pub struct Data {
    pub con_state: Arc<RwLock<Connection>>,
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
    Examine,
    Noop,
    Check,
    Create,
    Delete,
    Subscribe,
    Close,
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
            "examine" => Ok(Commands::Examine),
            "noop" => Ok(Commands::Noop),
            "check" => Ok(Commands::Check),
            "create" => Ok(Commands::Create),
            "delete" => Ok(Commands::Delete),
            "subscribe" => Ok(Commands::Subscribe),
            "close" => Ok(Commands::Close),
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

impl Data {
    fn parse_internal(line: &str) -> color_eyre::eyre::Result<CommandData> {
        match context("parse", tuple((imaptag, command, arguments)))(line).map(
            |(_, (tag, command, arguments))| match command {
                Ok(command) => Ok(CommandData {
                    tag: tag.to_string(),
                    command,
                    arguments,
                }),
                Err(e) => Err(color_eyre::eyre::Report::msg(e)),
            },
        ) {
            Ok(v) => v,
            Err(e) => Err(color_eyre::eyre::Report::msg(format!("{}", e))),
        }
    }
}

impl<'a> Data {
    #[allow(clippy::too_many_lines)]
    pub async fn parse<S>(
        &mut self,
        lines: &'a mut S,
        config: Arc<Config>,
        line: String,
    ) -> color_eyre::eyre::Result<bool>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        debug!("Current state: {:?}", self.con_state.read().await.state);

        let con_clone = Arc::clone(&self.con_state);
        let state = { con_clone.read().await.state.clone() };
        if let State::Authenticating(AuthenticationMethod::Plain, ref tag) = state {
            let command_data = CommandData {
                tag: tag.to_string(),
                // This is unused but needed. We just assume Authenticate here
                command: Commands::Authenticate,
                arguments: vec![],
            };
            Authenticate {
                data: self,
                auth_data: line,
            }
            .plain(lines, &command_data)
            .await?;
            // We are done here
            return Ok(false);
        };
        let parse_result = Data::parse_internal(&line);
        match parse_result {
            Ok(command_data) => {
                match command_data.command {
                    Commands::Capability => {
                        Capability.exec(lines, &command_data).await?;
                    }
                    Commands::Login => {
                        Login.exec(lines, &command_data).await?;
                    }
                    Commands::Logout => {
                        Logout.exec(lines, &command_data).await?;
                        // We return true here early as we want to make sure that this closes the connection
                        return Ok(true);
                    }
                    Commands::Authenticate => {
                        let auth_data = command_data.arguments.last().unwrap().to_string();
                        Authenticate {
                            data: self,
                            auth_data,
                        }
                        .exec(lines, config, &command_data)
                        .await?;
                    }
                    Commands::List => {
                        List { data: self }
                            .exec(lines, config, &command_data)
                            .await?;
                    }
                    Commands::LSub => {
                        LSub { data: self }
                            .exec(lines, config, &command_data)
                            .await?;
                    }
                    Commands::Select => {
                        Select { data: self }
                            .exec(lines, config, &command_data)
                            .await?;
                    }
                    Commands::Examine => {
                        Examine { data: self }
                            .exec(lines, config, &command_data)
                            .await?;
                    }
                    Commands::Create => {
                        Create { data: self }
                            .exec(lines, config, &command_data)
                            .await?;
                    }
                    Commands::Delete => {
                        Delete { data: self }
                            .exec(lines, config, &command_data)
                            .await?;
                    }
                    Commands::Subscribe => {
                        Subscribe { data: self }
                            .exec(lines, config, &command_data)
                            .await?;
                    }
                    Commands::Noop => {
                        Noop.exec(lines, &command_data).await?;
                    }
                    Commands::Check => {
                        Check { data: self }.exec(lines, &command_data).await?;
                    }
                    Commands::Close => {
                        Close { data: self }
                            .exec(lines, config, &command_data)
                            .await?;
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
        let mut con_state = super::Connection::new(true);
        let mut data = Data {
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
        let mut con_state = super::Connection::new(true);
        let mut data = Data {
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
        let mut con_state = super::Connection::new(true);
        let mut data = Data {
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
