// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::commands::{CommandData, Data};
use erooster_deps::{
    color_eyre::{self, eyre::bail},
    futures::{Sink, SinkExt},
    mail_auth::{Resolver, SpfResult},
    tracing::{self, instrument},
};

pub struct Ehlo<'a> {
    pub data: &'a mut Data,
}

impl Ehlo<'_> {
    #[instrument(skip(self, hostname, lines, command_data))]
    pub async fn exec<S, E>(
        &mut self,
        hostname: &str,
        lines: &mut S,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        if command_data.arguments.is_empty() {
            bail!("Invalid EHLO arguments: {:?}", command_data.arguments);
        }

        // Create a resolver using Quad9 DNS
        let resolver = Resolver::new_quad9_tls()?;
        // Verify EHLO identity
        let result = resolver
            .verify_spf_helo(
                self.data.con_state.peer_addr.parse()?,
                command_data.arguments[0],
                hostname,
            )
            .await;
        if result.result() == SpfResult::Fail {
            lines
                .feed(String::from("550 5.7.1 SPF MAIL FROM check failed:"))
                .await?;
            // TODO: find a better url
            lines
                .feed(String::from("550 5.7.1 The domain example.com explains:"))
                .await?;
            lines
                .feed(String::from(
                    "550 5.7.1 Please see http://www.example.com/mailpolicy.html",
                ))
                .await?;
            lines.flush().await?;
            return Ok(());
        }
        self.data.con_state.spf_result = Some(result);

        self.data.con_state.ehlo = Some(command_data.arguments[0].to_string());
        lines.feed(format!("250-{hostname}")).await?;
        lines.feed(String::from("250-ENHANCEDSTATUSCODES")).await?;
        if !self.data.con_state.secure {
            lines.feed(String::from("250-STARTTLS")).await?;
        }
        if self.data.con_state.secure {
            lines.feed(String::from("250-AUTH LOGIN PLAIN")).await?;
        }
        lines.feed(String::from("250 SMTPUTF8")).await?;
        lines.flush().await?;
        Ok(())
    }
}
