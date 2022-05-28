use crate::{backend::database::DB,
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
        rename::Rename,
        select::{Examine, Select},
        subscribe::Subscribe,
        uid::Uid,
        unsubscribe::Unsubscribe,
    },
    imap_servers::state::{Connection, State},
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

#[cfg(test)]
use std::fmt::Display;

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
mod rename;
mod select;
mod subscribe;
mod uid;
mod unsubscribe;
pub mod utils;

#[derive(Debug)]
pub struct Data {
    pub con_state: Arc<RwLock<Connection>>,
}

#[derive(Debug)]
pub struct CommandData<'a> {
    tag: &'a str,
    command: Commands,
    arguments: &'a [&'a str],
}

#[derive(Debug)]
#[cfg_attr(
    test,
    derive(
        enum_iterator::IntoEnumIterator,
        enum_display_derive::Display,
        Clone,
        Copy,
        PartialEq
    )
)]
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
    Unsubscribe,
    Close,
    Rename,
    Uid,
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
            "unsubscribe" => Ok(Commands::Unsubscribe),
            "close" => Ok(Commands::Close),
            "rename" => Ok(Commands::Rename),
            "uid" => Ok(Commands::Uid),
            _ => {
                warn!("[IMAP] Got unknown command: {}", i);
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
    fn parse_internal(line: &str) -> Res<(&str, Result<Commands, String>, Vec<&str>)> {
        context("parse", tuple((imaptag, command, arguments)))(line)
    }
    #[allow(clippy::too_many_lines)]
    pub async fn parse<S>(
        &self,
        lines: &mut S,
        config: Arc<Config>,
        database: DB,
        line: String,
    ) -> color_eyre::eyre::Result<bool>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        debug!("Current state: {:?}", self.con_state.read().await.state);

        let con_clone = Arc::clone(&self.con_state);
        let state = { con_clone.read().await.state.clone() };
        if let State::Authenticating(AuthenticationMethod::Plain, tag) = state {
            debug!("Second auth stage");
            let command_data = CommandData {
                tag: &tag,
                // This is unused but needed. We just assume Authenticate here
                command: Commands::Authenticate,
                arguments: &[],
            };
            Authenticate {
                data: self,
                auth_data: &line,
            }
            .plain(lines, database, &command_data)
            .await?;
            // We are done here
            return Ok(false);
        };
        debug!("Starting to parse");
        match Data::parse_internal(&line) {
            Ok((_, (tag, command, arguments))) => {
                let command_data = match command {
                    Ok(command) => CommandData {
                        tag,
                        command,
                        arguments: &arguments,
                    },
                    Err(e) => {
                        error!("[IMAP] Error parsing command: {}", e);
                        lines
                            .send(String::from("* BAD [SERVERBUG] unable to parse command"))
                            .await?;
                        return Ok(false);
                    }
                };
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
                        let auth_data = command_data.arguments.last().unwrap();
                        Authenticate {
                            data: self,
                            auth_data,
                        }
                        .exec(lines, database, &command_data)
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
                    Commands::Unsubscribe => {
                        Unsubscribe { data: self }
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
                        // See why in the command itself
                        cfg_if::cfg_if! {
                            if #[cfg(not(test))] {
                                    Close { data: self }
                                        .exec(lines, config, &command_data)
                                        .await?;
                            }
                        }
                    }
                    Commands::Rename => {
                        Rename { data: self }
                            .exec(lines, &command_data, config)
                            .await?;
                    }
                    Commands::Uid => {
                        Uid.exec(lines, &command_data, config).await?;
                    }
                }
            }
            Err(e) => {
                error!("[IMAP] Error parsing command: {}", e);
                lines
                    .send(String::from("* BAD [SERVERBUG] unable to parse command"))
                    .await?;
                return Ok(false);
            }
        }

        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convert_case::{Case, Casing};
    use enum_iterator::IntoEnumIterator;

    #[test]
    fn test_parsing_imaptag() {
        assert_eq!(imaptag("abcd CAPABILITY"), Ok(("CAPABILITY", "abcd")));
        assert_eq!(imaptag("12345 CAPABILITY"), Ok(("CAPABILITY", "12345")));
        assert_eq!(imaptag("abd124 CAPABILITY"), Ok(("CAPABILITY", "abd124")));
    }

    #[test]
    fn test_parsing_commands() {
        for command_variant in Commands::into_enum_iter() {
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
        assert_eq!(
            command("beeeeep"),
            Ok(("", Err(String::from("no other commands supported"))))
        );
    }

    #[test]
    fn test_parsing_arguments() {
        assert_eq!(arguments("PLAIN abd=="), Ok(("", vec!["PLAIN", "abd=="])));
        assert_eq!(arguments("PLAIN"), Ok(("", vec!["PLAIN"])));
    }

    #[test]
    fn test_parsing_authenticate_command() {
        let result = Data::parse_internal("a AUTHENTICATE PLAIN abcde");
        assert!(result.is_ok());
        let (_, (tag, command, arguments)) = result.unwrap();
        assert!(command.is_ok());
        assert_eq!(tag, "a");
        assert_eq!(command.unwrap(), Commands::Authenticate);
        assert_eq!(arguments, &["PLAIN", "abcde"]);

        let result = Data::parse_internal("a AUTHENTICATE PLAIN");
        assert!(result.is_ok());
        let (_, (tag, command, arguments)) = result.unwrap();
        assert!(command.is_ok());
        assert_eq!(tag, "a");
        assert_eq!(command.unwrap(), Commands::Authenticate);
        assert_eq!(arguments, &["PLAIN"]);
    }

    #[test]
    fn test_parsing_capability_command() {
        let result = Data::parse_internal("a CAPABILITY");
        assert!(result.is_ok());
        let (_, (tag, command, arguments)) = result.unwrap();
        assert!(command.is_ok());
        assert_eq!(tag, "a");
        assert_eq!(command.unwrap(), Commands::Capability);
        assert!(arguments.is_empty());
    }

    #[test]
    fn test_parsing_list_command() {
        let result = Data::parse_internal("18 list \"\" \"*\"");
        assert!(result.is_ok());
        let (_, (tag, command, arguments)) = result.unwrap();
        assert!(command.is_ok());
        assert_eq!(tag, "18");
        assert_eq!(command.unwrap(), Commands::List);
        assert_eq!(arguments, &["\"\"", "\"*\""]);

        let result = Data::parse_internal("18 list \"\" \"\"");
        assert!(result.is_ok());
        let (_, (tag, command, arguments)) = result.unwrap();
        assert!(command.is_ok());
        assert_eq!(tag, "18");
        assert_eq!(command.unwrap(), Commands::List);
        assert_eq!(arguments, &["\"\"", "\"\""]);
    }
}
