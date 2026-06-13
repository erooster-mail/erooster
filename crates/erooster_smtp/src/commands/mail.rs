// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::commands::{parsers::localpart_arguments, CommandData, Data};
use crate::servers::state::State;
use erooster_core::config::Config;
use {
    color_eyre::{
        self,
        eyre::{bail, ContextCompat},
    },
    futures::{Sink, SinkExt},
    mail_auth::{spf::verify::SpfParameters, MessageAuthenticator, SpfResult},
    tracing::{error, instrument, warn},
};

pub struct Mail<'a> {
    pub data: &'a mut Data,
}

impl Mail<'_> {
    #[instrument(skip(self, lines, command_data, config))]
    pub async fn exec<S, E>(
        &mut self,
        config: &Config,
        lines: &mut S,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        // RFC 6409 §4.3: submission ports must reject MAIL FROM from unauthenticated senders.
        if self.data.con_state.is_submission
            && !matches!(&self.data.con_state.state, State::Authenticated(_))
        {
            lines
                .send(String::from(
                    "530 5.7.0 Authentication required. \
                     Please authenticate before sending mail.",
                ))
                .await?;
            return Ok(());
        }

        if command_data.arguments.is_empty() {
            bail!("Failed to parse localpart arguments (no arguments)");
        }

        match localpart_arguments(command_data.arguments[0]).map(|(_, senders)| senders) {
            Ok(args) => {
                // Parse extra MAIL FROM parameters (SIZE=, BODY=, REQUIRETLS).
                let mut declared_size: Option<u64> = None;
                let mut require_tls = false;

                for param in &command_data.arguments[1..] {
                    let upper = param.to_uppercase();
                    if let Some(size_str) = upper.strip_prefix("SIZE=") {
                        match size_str.parse::<u64>() {
                            Ok(n) => declared_size = Some(n),
                            Err(_) => {
                                warn!("Ignoring malformed SIZE parameter: {param}");
                            }
                        }
                    } else if upper == "REQUIRETLS" {
                        require_tls = true;
                    }
                    // BODY= (8BITMIME, 7BIT, BINARYMIME) — accepted, not enforced
                }

                // Reject early if the declared size exceeds our limit.
                if let Some(size) = declared_size {
                    let max = config.mail.max_message_size.as_bytes();
                    if size > max {
                        let max_mb = max / 1_048_576;
                        let size_mb = size / 1_048_576;
                        lines
                            .send(format!(
                                "552 5.3.4 Message size ({size_mb} MB) exceeds the server \
                                 limit ({max_mb} MB). Please reduce attachments and try again."
                            ))
                            .await?;
                        return Ok(());
                    }
                }

                // Verify SPF for the sender address.
                let resolver = MessageAuthenticator::new_system_conf()?;
                for sender in &args {
                    let result = resolver
                        .verify_spf(SpfParameters::verify_mail_from(
                            self.data.con_state.peer_addr.parse()?,
                            self.data.con_state.ehlo.as_ref().context("Missing ehlo")?,
                            &config.mail.hostname,
                            sender,
                        ))
                        .await;

                    if result.result() == SpfResult::Fail {
                        lines
                            .send(format!(
                                "550 5.7.23 Message from <{sender}> rejected: \
                                 SPF check failed. The sending IP is not authorized \
                                 to send mail for this domain."
                            ))
                            .await?;
                        return Ok(());
                    }
                }

                let senders: Vec<_> = args.iter().map(ToString::to_string).collect();
                self.data.con_state.sender = Some(senders[0].clone());
                self.data.con_state.declared_size = declared_size;
                if require_tls {
                    self.data.con_state.require_tls = true;
                }

                lines
                    .send(format!(
                        "250 2.1.0 Originator {} OK",
                        command_data.arguments[0]
                    ))
                    .await?;
            }
            Err(e) => {
                error!("Failed to parse localpart arguments: {:?}", e);
                bail!("Failed to parse localpart arguments");
            }
        }

        Ok(())
    }
}
