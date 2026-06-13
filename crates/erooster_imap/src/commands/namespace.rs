// SPDX-FileCopyrightText: 2026 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! NAMESPACE command (RFC 2342 / RFC 9051 §6.3.10) — mandatory in `IMAP4rev2`.

use crate::commands::CommandData;
use {
    color_eyre,
    futures::{Sink, SinkExt},
    tracing::instrument,
};

pub struct Namespace;

impl Namespace {
    #[instrument(skip(self, lines, command_data))]
    pub async fn exec<S, E>(
        &self,
        lines: &mut S,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        // Personal namespace with empty prefix and "/" delimiter.
        // No other-users or shared namespaces for a single-server setup.
        lines
            .feed(String::from("* NAMESPACE ((\"\" \"/\")) NIL NIL"))
            .await?;
        lines
            .feed(format!("{} OK NAMESPACE completed", command_data.tag))
            .await?;
        lines.flush().await?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::commands::{CommandData, Commands};
    use futures::{channel::mpsc, StreamExt};
    use tokio;

    #[cfg_attr(coverage_nightly, coverage(off))]
    #[tokio::test]
    async fn test_namespace_response() {
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Namespace,
            arguments: &[],
        };
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = Namespace.exec(&mut tx, &cmd_data).await;
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            rx.next().await,
            Some(String::from("* NAMESPACE ((\"\" \"/\")) NIL NIL"))
        );
        assert_eq!(
            rx.next().await,
            Some(String::from("a1 OK NAMESPACE completed"))
        );
    }
}
