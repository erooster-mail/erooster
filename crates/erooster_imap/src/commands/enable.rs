// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::{
    commands::{CommandData, Data},
    servers::state::Capabilities,
};
use erooster_deps::{
    color_eyre,
    futures::{Sink, SinkExt},
    tracing::{self, instrument},
};

pub struct Enable<'a> {
    pub data: &'a mut Data,
}

impl Enable<'_> {
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
        for arg in command_data.arguments {
            if arg == &"UTF8=ACCEPT" {
                self.data
                    .con_state
                    .active_capabilities
                    .push(Capabilities::UTF8);
            } else {
                self.data
                    .con_state
                    .active_capabilities
                    .push(Capabilities::Other((*arg).to_string()));
            }
            lines.feed(format!("* ENABLED {arg}")).await?;
        }
        lines.feed(format!("{} OK", command_data.tag)).await?;
        lines.flush().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{CommandData, Commands};
    use crate::servers::state::{Connection, State};
    use erooster_deps::futures::{channel::mpsc, StreamExt};
    use erooster_deps::tokio;

    #[allow(clippy::unwrap_used)]
    #[cfg_attr(coverage_nightly, coverage(off))]
    #[tokio::test]
    async fn test_utf8() {
        let state = &mut Data {
            con_state: Connection {
                state: State::NotAuthenticated,
                secure: true,
                username: None,
                active_capabilities: vec![],
            },
        };
        let mut caps = Enable { data: state };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Enable,
            arguments: &["UTF8=ACCEPT"],
        };

        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &cmd_data).await;
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(rx.next().await, Some(String::from("* ENABLED UTF8=ACCEPT")));
        assert_eq!(rx.next().await, Some(String::from("a1 OK")));

        assert!(state
            .con_state
            .active_capabilities
            .contains(&Capabilities::UTF8));
    }

    #[allow(clippy::unwrap_used)]
    #[cfg_attr(coverage_nightly, coverage(off))]
    #[tokio::test]
    async fn test_custom() {
        let state = &mut Data {
            con_state: Connection {
                state: State::NotAuthenticated,
                secure: true,
                username: None,
                active_capabilities: vec![],
            },
        };
        let mut caps = Enable { data: state };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Enable,
            arguments: &["Random"],
        };

        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &cmd_data).await;
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(rx.next().await, Some(String::from("* ENABLED Random")));
        assert_eq!(rx.next().await, Some(String::from("a1 OK")));

        assert!(state
            .con_state
            .active_capabilities
            .contains(&Capabilities::Other(String::from("Random"))));
    }
}
