use std::{borrow::Cow, io};

use crate::{
    commands::{
        auth::Plain,
        capability::Capability,
        list::{Basic, Extended},
    },
    line_codec::LinesCodecError,
};
use async_trait::async_trait;
use futures::{Sink, SinkExt};
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while1},
    character::complete::alpha1,
    combinator::opt,
    error::{context, VerboseError},
    multi::many0,
    sequence::{terminated, tuple},
    IResult,
};
use tracing::{debug, error, warn};

use crate::{
    commands::auth::AuthenticationMethod,
    servers::{ConnectionState, State},
};

pub mod auth;
pub mod capability;
pub mod list;

#[derive(Debug, PartialEq)]
pub struct Data<'a, 'b> {
    tag: String,
    command: Commands,
    arguments: Option<Vec<Cow<'a, str>>>,
    con_state: &'a mut ConnectionState<'b>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
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
}

impl TryFrom<&str> for Commands {
    type Error = &'static str;

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
            _ => {
                warn!("Got unknown command: {}", i);
                Err("no other commands supported")
            }
        }
    }
}

type Res<T, U> = IResult<T, U, VerboseError<T>>;

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
fn imaptag(input: &str) -> Res<&str, String> {
    //alphanumeric1, none_of("+(){ %\\\""), one_of("1*]")
    context(
        "imaptag",
        terminated(take_while1(is_imaptag_char), tag(" ")),
    )(input)
    .map(|(next_input, res)| (next_input, res.into()))
}

/// Gets the input minus the tag
fn command(input: &str) -> Res<&str, Result<Commands, &'static str>> {
    context("command", alt((terminated(alpha1, tag(" ")), alpha1)))(input)
        .map(|(next_input, res)| (next_input, res.try_into()))
}

/// Gets the input minus the tag and minus the command
fn arguments(input: &str) -> Res<&str, Vec<Cow<'_, str>>> {
    context(
        "arguments",
        many0(alt((
            terminated(take_while1(|c: char| c != ' '), tag(" ")),
            take_while1(|c: char| c != ' '),
        ))),
    )(input)
    .map(|(next_input, res)| (next_input, res.into_iter().map(Cow::Borrowed).collect()))
}

fn parse_internal<'a, 'b>(
    line: &'a str,
    con_state: &'a mut ConnectionState<'b>,
) -> anyhow::Result<Data<'a, 'b>> {
    match context("parse", tuple((imaptag, command, opt(arguments))))(line).map(
        |(_, (tag, command, mut arguments))| {
            if let Some(ref arguments_inner) = arguments {
                if arguments_inner.is_empty() {
                    arguments = None;
                }
            }
            match command {
                Ok(command) => Ok(Data {
                    tag,
                    command,
                    arguments,
                    con_state,
                }),
                Err(e) => Err(anyhow::Error::msg(e.to_string())),
            }
        },
    ) {
        Ok(v) => v,
        Err(e) => Err(anyhow::Error::msg(format!("{}", e))),
    }
}

#[async_trait]
pub trait Command<Lines> {
    async fn exec(&mut self, lines: &'async_trait mut Lines) -> anyhow::Result<()>;
}

#[async_trait]
pub trait Parser<Lines> {
    async fn parse(
        lines: &'async_trait mut Lines,
        line: String,
        con_state: &'async_trait mut ConnectionState<'_>,
    ) -> anyhow::Result<bool>;
}

#[async_trait]
impl<S> Parser<S> for Data<'_, '_>
where
    S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
    S::Error: From<io::Error>,
{
    #[allow(clippy::too_many_lines)]
    async fn parse(
        lines: &'async_trait mut S,
        line: String,
        con_state: &'async_trait mut ConnectionState<'_>,
    ) -> anyhow::Result<bool> {
        let cloned_line = line.clone();

        let state_read = { con_state.state.clone() };
        debug!("Current state: {:?}", state_read);
        if let State::Authenticating((AuthenticationMethod::Plain, ref tag)) = state_read {
            Plain {
                data: Data {
                    tag: tag.to_string(),
                    // This is unused but needed. We just assume Authenticate here
                    command: Commands::Authenticate,
                    arguments: None,
                    con_state,
                },
                auth_data: Cow::Owned(cloned_line),
            }
            .exec(lines)
            .await?;
            // We are done here
            return Ok(false);
        }
        let parse_result = parse_internal(&line, con_state);
        match parse_result {
            Ok(command) => {
                match command.command {
                    Commands::Capability => Capability { data: command }.exec(lines).await?,
                    Commands::Login => {
                        lines
                            .send(format!(
                                "{} NO LOGIN COMMAND DISABLED FOR SECURITY. USE AUTH",
                                command.tag
                            ))
                            .await?;
                    }
                    Commands::Logout => {
                        lines
                            .feed(String::from("* BYE IMAP4rev2 Server logging out"))
                            .await?;
                        lines
                            .feed(format!("{} OK LOGOUT completed", command.tag))
                            .await?;
                        lines.flush().await?;
                        // We return true here early as we want to make sure that this closes the connection
                        return Ok(true);
                    }
                    Commands::Authenticate => {
                        if state_read == State::NotAuthenticated && command.arguments.is_some() {
                            let args = command.arguments.unwrap();
                            if args.len() == 1 {
                                if args.first().unwrap().to_lowercase() == "plain" {
                                    {
                                        con_state.state = State::Authenticating((
                                            AuthenticationMethod::Plain,
                                            Cow::Owned(command.tag),
                                        ));
                                    };
                                    lines.send(String::from("+ ")).await?;
                                } else {
                                    Plain {
                                        data: command,
                                        auth_data: Cow::Borrowed(args.last().unwrap()),
                                    }
                                    .exec(lines)
                                    .await?;
                                }
                            } else {
                                lines
                                    .send(format!(
                                        "{} BAD [SERVERBUG] unable to parse command",
                                        command.tag
                                    ))
                                    .await?;
                            }
                        } else {
                            lines
                                .send(format!("{} NO invalid state", command.tag))
                                .await?;
                        }
                    }
                    Commands::List => {
                        if let Some(arguments) = command.arguments {
                            if arguments.len() == 2 {
                                Basic { data: command }.exec(lines).await?;
                            } else if arguments.len() == 4 {
                                Extended { data: command }.exec(lines).await?;
                            } else {
                                lines
                                    .send(format!(
                                        "{} BAD [SERVERBUG] invalid arguments",
                                        command.tag
                                    ))
                                    .await?;
                            }
                        } else {
                            lines
                                .send(format!("{} BAD [SERVERBUG] invalid arguments", command.tag))
                                .await?;
                        }
                    }
                    Commands::LSub => {
                        if let Some(arguments) = command.arguments {
                            if arguments.len() == 2 {
                                Basic { data: command }.exec(lines).await?;
                            } else {
                                lines
                                    .send(format!(
                                        "{} BAD [SERVERBUG] invalid arguments",
                                        command.tag
                                    ))
                                    .await?;
                            }
                        } else {
                            lines
                                .send(format!("{} BAD [SERVERBUG] invalid arguments", command.tag))
                                .await?;
                        }
                    }
                    Commands::Select => {
                        if state_read == State::Authenticated {
                            if let Some(args) = command.arguments {
                                let mut folder =
                                    args.first().expect("server selects a folder").to_string();
                                folder.remove_matches('"');
                                con_state.state = State::Selected(Cow::Owned(folder));
                                // TODO get count of mails
                                // TODO get flags and perma flags
                                // TODO get real list
                                // TODO UIDNEXT and UIDVALIDITY
                                lines.feed(String::from("* 0 EXISTS")).await?;
                                lines
                                    .feed(String::from("* OK [UIDVALIDITY 3857529045] UIDs valid"))
                                    .await?;
                                lines
                                    .feed(String::from("* OK [UIDNEXT 4392] Predicted next UID"))
                                    .await?;
                                lines
                                    .feed(String::from(
                                        "* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)",
                                    ))
                                    .await?;
                                lines
                                    .feed(String::from(
                                        "* OK [PERMANENTFLAGS (\\Deleted \\Seen \\*)] Limited",
                                    ))
                                    .await?;
                                lines
                                    .feed(String::from("* LIST () \"/\" \"INBOX\""))
                                    .await?;
                                lines
                                    .feed(format!(
                                        "{} OK [READ-WRITE] SELECT completed",
                                        command.tag
                                    ))
                                    .await?;
                                lines.flush().await?;
                            }
                        } else {
                            lines
                                .send(format!("{} NO invalid state", command.tag))
                                .await?;
                        }
                    }
                    Commands::Noop => {
                        // TODO return status as suggested in https://www.rfc-editor.org/rfc/rfc9051.html#name-noop-command
                        lines
                            .send(format!("{} OK NOOP completed", command.tag))
                            .await?;
                    }
                    Commands::Check => {
                        // This is an Imap4rev1 feature. It does the same as Noop for us as we have no memory gc.
                        // It also only is allowed in selected state
                        if matches!(state_read, State::Selected(_)) {
                            lines
                                .send(format!("{} OK CHECK completed", command.tag))
                                .await?;
                        } else {
                            lines
                                .send(format!("{} NO invalid state", command.tag))
                                .await?;
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
    use std::borrow::Cow;
    use std::net::IpAddr;
    use std::net::Ipv4Addr;

    #[test]
    fn test_parsing_imaptag() {
        assert_eq!(
            imaptag("abcd CAPABILITY"),
            Ok(("CAPABILITY", String::from("abcd")))
        );
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
            Ok(("", vec![Cow::Borrowed("PLAIN"), Cow::Borrowed("abd==")]))
        );
        assert_eq!(arguments("PLAIN"), Ok(("", vec![Cow::Borrowed("PLAIN")])));
    }

    #[test]
    fn test_parsing_authenticate_command() {
        let con_state = super::ConnectionState {
            state: super::State::NotAuthenticated,
            ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            secure: true,
        };
        let result = parse_internal("a AUTHENTICATE PLAIN abcde", &mut con_state);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            Data {
                tag: String::from("a"),
                command: Commands::Authenticate,
                arguments: Some(vec![Cow::Borrowed("PLAIN"), Cow::Borrowed("abcde")]),
                con_state: &mut con_state
            }
        );

        let result = parse_internal("a AUTHENTICATE PLAIN", &mut con_state);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            Data {
                tag: String::from("a"),
                command: Commands::Authenticate,
                arguments: Some(vec![Cow::Borrowed("PLAIN")]),
                con_state: &mut con_state
            }
        );
    }

    #[test]
    fn test_parsing_capability_command() {
        let con_state = super::ConnectionState {
            state: super::State::NotAuthenticated,
            ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            secure: true,
        };
        let result = parse_internal("a CAPABILITY", &mut con_state);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            Data {
                tag: String::from("a"),
                command: Commands::Capability,
                arguments: None,
                con_state: &mut con_state
            }
        );
    }

    #[test]
    fn test_parsing_list_command() {
        let con_state = super::ConnectionState {
            state: super::State::Authenticated,
            ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            secure: true,
        };
        let result = parse_internal("18 list \"\" \"*\"", &mut con_state);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            Data {
                tag: String::from("18"),
                command: Commands::List,
                arguments: Some(vec![Cow::Borrowed("\"\""), Cow::Borrowed("\"*\"")]),
                con_state: &mut con_state
            }
        );
        let result = parse_internal("18 list \"\" \"\"", &mut con_state);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            Data {
                tag: String::from("18"),
                command: Commands::List,
                arguments: Some(vec![Cow::Borrowed("\"\""), Cow::Borrowed("\"\"")]),
                con_state: &mut con_state
            }
        );
    }
}
