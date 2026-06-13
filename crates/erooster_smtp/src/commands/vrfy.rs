// SPDX-FileCopyrightText: 2026 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! VRFY command (RFC 5321 §3.5) with anti-enumeration protection.
//!
//! Unauthenticated callers always receive 252 — never 250 or 550 — to prevent
//! user-existence harvesting. Authenticated callers on port 587 may receive the
//! real result in the future; for now the same safe response is returned for
//! all callers until port-587 submission is fully wired up.

use crate::commands::{CommandData, Data};
use erooster_core::backend::database::{Database, DB};
use {
    color_eyre,
    futures::{Sink, SinkExt},
    tracing::{info, instrument},
};

pub struct Vrfy<'a> {
    pub data: &'a Data,
}

impl Vrfy<'_> {
    #[instrument(skip(self, lines, database, command_data))]
    pub async fn exec<S, E>(
        &self,
        lines: &mut S,
        database: &DB,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        let address = command_data.arguments.first().copied().unwrap_or("");
        info!(
            peer = %self.data.con_state.peer_addr,
            address,
            "VRFY attempt"
        );

        // Check if the caller is authenticated (only relevant once port 587 exists).
        // Until then, treat every caller as unauthenticated for safety.
        let authenticated = matches!(
            self.data.con_state.state,
            crate::servers::state::State::Authenticated(_)
        );

        if authenticated && !address.is_empty() {
            // Authenticated caller: reveal real result.
            let username = address
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_lowercase();
            if database.user_exists(&username).await {
                lines
                    .send(format!("250 2.1.5 <{username}>"))
                    .await?;
            } else {
                lines
                    .send(format!(
                        "550 5.1.1 <{username}>: Recipient address rejected. The account does not exist on this server."
                    ))
                    .await?;
            }
        } else {
            // Unauthenticated: RFC-blessed refusal — never leaks whether user exists.
            lines
                .send(String::from(
                    "252 2.5.2 Cannot VRFY user, but will accept message and attempt delivery",
                ))
                .await?;
        }

        Ok(())
    }
}
