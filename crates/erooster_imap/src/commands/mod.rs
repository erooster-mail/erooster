use crate::{
    commands::{
        append::Append,
        auth::{Authenticate, AuthenticationMethod},
        capability::Capability,
        check::Check,
        close::Close,
        create::Create,
        delete::Delete,
        enable::Enable,
        fetch::Fetch,
        list::{LSub, List},
        login::Login,
        logout::Logout,
        noop::Noop,
        rename::Rename,
        search::Search,
        select::{Examine, Select},
        status::Status,
        store::Store,
        subscribe::Subscribe,
        uid::Uid,
        unsubscribe::Unsubscribe,
    },
    servers::state::{Connection, State},
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

mod append;
pub mod auth;
pub mod capability;
mod check;
mod close;
mod create;
mod delete;
mod enable;
mod fetch;
mod list;
mod login;
mod logout;
mod noop;
pub mod parsers;
mod rename;
mod search;
mod select;
mod status;
mod store;
mod subscribe;
mod uid;
mod unsubscribe;

#[derive(Debug, Clone)]
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
        enum_iterator::Sequence,
        enum_display_derive::Display,
        Clone,
        Copy,
        PartialEq,
        Eq
    )
)]
pub enum Commands {
    Append,
    Authenticate,
    Capability,
    Check,
    Close,
    Create,
    Delete,
    Enable,
    Examine,
    Fetch,
    List,
    Login,
    Logout,
    LSub,
    Noop,
    Rename,
    Search,
    Select,
    Status,
    Starttls,
    Store,
    Subscribe,
    Unsubscribe,
    Uid,
}

impl TryFrom<&str> for Commands {
    type Error = String;

    #[instrument(skip(i))]
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
            "fetch" => Ok(Commands::Fetch),
            "store" => Ok(Commands::Store),
            "append" => Ok(Commands::Append),
            "enable" => Ok(Commands::Enable),
            "status" => Ok(Commands::Status),
            "search" => Ok(Commands::Search),
            "starttls" => Ok(Commands::Starttls),
            _ => {
                warn!("[IMAP] Got unknown command: {}", i);
                Err(String::from("no other commands supported"))
            }
        }
    }
}

type Res<'a, U> = IResult<&'a str, U, VerboseError<&'a str>>;

#[instrument(skip(c))]
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
#[instrument(skip(input))]
fn imaptag(input: &str) -> Res<&str> {
    //alphanumeric1, none_of("+(){ %\\\""), one_of("1*]")
    context(
        "imaptag",
        terminated(take_while1(is_imaptag_char), tag(" ")),
    )(input)
    .map(|(next_input, res)| (next_input, res))
}

/// Gets the input minus the tag
#[instrument(skip(input))]
fn command(input: &str) -> Res<Result<Commands, String>> {
    context("command", alt((terminated(alpha1, tag(" ")), alpha1)))(input)
        .map(|(next_input, res)| (next_input, res.try_into()))
}

/// Gets the input minus the tag and minus the command
#[instrument(skip(input))]
fn arguments(input: &str) -> Res<Vec<&str>> {
    debug!("parsing arguments");
    context(
        "arguments",
        many0(alt((
            terminated(take_while1(|c: char| !c.is_whitespace()), tag(" ")),
            take_while1(|c: char| c != ' '),
        ))),
    )(input)
    .map(|(x, y)| (x, y))
}

#[allow(clippy::upper_case_acronyms)]
pub enum Response {
    Exit,
    Continue,
    STARTTLS(String),
}

impl Data {
    #[instrument(skip(line))]
    fn parse_internal(line: &str) -> Res<(&str, Result<Commands, String>, Vec<&str>)> {
        context("parse_internal", tuple((imaptag, command, arguments)))(line)
    }

    #[allow(clippy::too_many_lines)]
    #[instrument(skip(self, lines, config, database, storage, line))]
    pub async fn parse<S, E>(
        &self,
        lines: &mut S,
        config: Arc<Config>,
        database: DB,
        storage: Arc<Storage>,
        line: String,
    ) -> color_eyre::eyre::Result<Response>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        debug!("Current state: {:?}", self.con_state.read().await.state);

        let con_clone = Arc::clone(&self.con_state);
        let state = { con_clone.read().await.state.clone() };
        let secure = { con_clone.read().await.secure.clone() };
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
            return Ok(Response::Continue);
        } else if let State::Appending(state) = state {
            Append { data: self }
                .append(lines, storage, &line, config, state.tag)
                .await?;
            // We are done here
            return Ok(Response::Continue);
        } else if let State::GotAppendData = state {
            if line == ")" {
                con_clone.write().await.state = State::Authenticated;
                // We are done here
                return Ok(Response::Continue);
            }
        }
        debug!("Starting to parse");
        let line_borrow: &str = &line;
        match Data::parse_internal(line_borrow).finish() {
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
                        return Ok(Response::Continue);
                    }
                };
                debug!("Command data: {:?}", command_data);
                match command_data.command {
                    Commands::Starttls => {
                        return Ok(Response::STARTTLS(tag.to_string()));
                    }
                    Commands::Enable => {
                        Enable { data: self }.exec(lines, &command_data).await?;
                    }
                    Commands::Capability => {
                        Capability.exec(lines, &command_data, reader.secure).await?;
                    }
                    Commands::Login => {
                        Login.exec(lines, &command_data).await?;
                    }
                    Commands::Logout => {
                        Logout.exec(lines, &command_data).await?;
                        // We return true here early as we want to make sure that this closes the connection
                        return Ok(Response::Exit);
                    }
                    Commands::Authenticate => {
                        let auth_data = command_data.arguments[command_data.arguments.len() - 1];
                        Authenticate {
                            data: self,
                            auth_data,
                        }
                        .exec(lines, database, &command_data)
                        .await?;
                    }
                    Commands::List => {
                        List { data: self }
                            .exec(lines, config, storage, &command_data)
                            .await?;
                    }
                    Commands::LSub => {
                        LSub { data: self }
                            .exec(lines, config, storage, &command_data)
                            .await?;
                    }
                    Commands::Select => {
                        Select { data: self }
                            .exec(lines, storage, &command_data)
                            .await?;
                    }
                    Commands::Store => {
                        Store { data: self }
                            .exec(lines, storage, &command_data, false)
                            .await?;
                    }
                    Commands::Examine => {
                        Examine { data: self }
                            .exec(lines, storage, &command_data)
                            .await?;
                    }
                    Commands::Create => {
                        Create { data: self }
                            .exec(lines, storage, &command_data)
                            .await?;
                    }
                    Commands::Delete => {
                        Delete { data: self }
                            .exec(lines, storage, &command_data)
                            .await?;
                    }
                    Commands::Subscribe => {
                        Subscribe { data: self }
                            .exec(lines, storage, &command_data)
                            .await?;
                    }
                    Commands::Unsubscribe => {
                        Unsubscribe { data: self }
                            .exec(lines, storage, &command_data)
                            .await?;
                    }
                    Commands::Noop => {
                        Noop { data: self }
                            .exec(lines, storage, &command_data)
                            .await?;
                    }
                    Commands::Check => {
                        Check { data: self }
                            .exec(lines, storage, &command_data)
                            .await?;
                    }
                    Commands::Close => {
                        Close { data: self }
                            .exec(lines, storage, &command_data)
                            .await?;
                    }
                    Commands::Rename => {
                        Rename { data: self }
                            .exec(lines, &command_data, storage)
                            .await?;
                    }
                    Commands::Uid => {
                        Uid { data: self }
                            .exec(lines, &command_data, storage)
                            .await?;
                    }
                    Commands::Fetch => {
                        Fetch { data: self }
                            .exec(lines, &command_data, storage, false)
                            .await?;
                    }
                    Commands::Append => {
                        Append { data: self }
                            .exec(lines, storage, &command_data)
                            .await?;
                    }
                    Commands::Status => {
                        Status { data: self }
                            .exec(lines, storage, &command_data)
                            .await?;
                    }
                    Commands::Search => {
                        Search { data: self }
                            .exec(lines, storage, &command_data)
                            .await?;
                    }
                }
            }
            Err(e) => {
                error!(
                    "[IMAP] Error parsing command: {}",
                    convert_error(line_borrow, e)
                );
                lines
                    .send(String::from("* BAD [SERVERBUG] unable to parse command"))
                    .await?;
                return Ok(Response::Continue);
            }
        }

        Ok(Response::Continue)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use convert_case::{Case, Casing};
    use enum_iterator::all;

    #[test]
    fn test_parsing_imaptag() {
        assert_eq!(imaptag("abcd CAPABILITY"), Ok(("CAPABILITY", "abcd")));
        assert_eq!(imaptag("12345 CAPABILITY"), Ok(("CAPABILITY", "12345")));
        assert_eq!(imaptag("abd124 CAPABILITY"), Ok(("CAPABILITY", "abd124")));
    }

    #[test]
    fn test_parsing_commands() {
        for command_variant in all::<Commands>() {
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
