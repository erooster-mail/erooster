// SPDX-FileCopyrightText: 2026 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! UNSELECT command (RFC 9051 §6.4.2) — mandatory in `IMAP4rev2`.
//!
//! Like CLOSE but does NOT expunge \Deleted messages. Transitions
//! Selected → Authenticated and sends the [CLOSED] response code.

use crate::{
    commands::{CommandData, Data},
    servers::state::State,
};
use {
    color_eyre,
    futures::{Sink, SinkExt},
    tracing::instrument,
};

pub struct Unselect<'a> {
    pub data: &'a mut Data,
}

impl Unselect<'_> {
    #[instrument(skip(self, lines, command_data))]
    pub async fn exec<S, E>(
        &mut self,
        lines: &mut S,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        if let State::Selected(_, _) = &self.data.con_state.state {
            self.data.con_state.state = State::Authenticated;
            lines
                .send(format!(
                    "{} OK [CLOSED] UNSELECT completed",
                    command_data.tag
                ))
                .await?;
        } else {
            lines
                .send(format!(
                    "{} BAD [CLIENTBUG] UNSELECT requires a selected mailbox",
                    command_data.tag
                ))
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::commands::{CommandData, Commands};
    use crate::servers::state::{Access, Connection, State};
    use futures::{channel::mpsc, StreamExt};
    use tokio;

    #[cfg_attr(coverage_nightly, coverage(off))]
    #[tokio::test]
    async fn test_unselect_when_selected() {
        let mut data = Data {
            con_state: Connection {
                state: State::Selected("INBOX".to_string(), Access::ReadWrite),
                secure: true,
                username: Some(String::from("test")),
                active_capabilities: vec![],
            },
        };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Unselect,
            arguments: &[],
        };
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = Unselect { data: &mut data }.exec(&mut tx, &cmd_data).await;
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            rx.next().await,
            Some(String::from("a1 OK [CLOSED] UNSELECT completed"))
        );
        assert_eq!(data.con_state.state, State::Authenticated);
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    #[tokio::test]
    async fn test_unselect_when_not_selected() {
        let mut data = Data {
            con_state: Connection {
                state: State::Authenticated,
                secure: true,
                username: Some(String::from("test")),
                active_capabilities: vec![],
            },
        };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Unselect,
            arguments: &[],
        };
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = Unselect { data: &mut data }.exec(&mut tx, &cmd_data).await;
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            rx.next().await,
            Some(String::from(
                "a1 BAD [CLIENTBUG] UNSELECT requires a selected mailbox"
            ))
        );
    }
}
