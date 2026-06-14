// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::{
    commands::Data,
    servers::{
        sending::{dkim_sign, EmailPayload},
        state::Data as StateData,
        state::State,
    },
    utils::rspamd::{Action, Response},
};

enum RspamdDecision {
    Accept(String),
    TempReject,
    PermReject,
}
use cfg_if::cfg_if;
use color_eyre::{self, eyre::ContextCompat};
use erooster_core::{
    backend::{
        database::{Database, DB},
        queue,
        storage::{MailStorage, Storage},
    },
    config::{Config, Rspamd},
};
use futures::{Sink, SinkExt};
use mail_auth::DkimResult;
#[cfg(not(feature = "benchmarking"))]
use mail_auth::{dmarc::verify::DmarcParameters, DmarcResult};
use mail_auth::{AuthenticatedMessage, MessageAuthenticator};
use reqwest;
use simdutf8::compat::from_utf8;
use std::{collections::BTreeMap, io::Write, path::Path, time::Duration};
use time::{macros::format_description, OffsetDateTime};
use tracing::{debug, instrument, warn};
use uuid;

#[allow(clippy::module_name_repetitions)]
pub struct DataCommand<'a> {
    pub data: &'a mut Data,
}

impl DataCommand<'_> {
    #[instrument(skip(self, lines))]
    pub async fn exec<S, E>(&mut self, lines: &mut S) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        debug!("Waiting for incoming data");
        {
            let username = if let State::Authenticated(username) = &self.data.con_state.state {
                Some(username.clone())
            } else {
                None
            };
            self.data.con_state.state = State::ReceivingData((username, StateData(Vec::new())));
        };
        lines
            .send(String::from("354 Start mail input; end with <CRLF>.<CRLF>"))
            .await?;
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    #[instrument(skip(self, config, lines, line, storage, database))]
    pub async fn receive<S, E>(
        &mut self,
        config: &Config,
        lines: &mut S,
        line: &str,
        storage: &Storage,
        database: &DB,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        debug!("Reading incoming data");

        let date_format = format_description!(
            "[weekday repr:short], [day] [month] [year] [hour]:[minute]:[second] [offset_hour \
         sign:mandatory]"
        );
        if line == "." {
            debug!("Got end of line");
            let Some(receipts) = &self.data.con_state.receipts else {
                color_eyre::eyre::bail!("No receipts")
            };
            self.data.con_state.state = if let State::ReceivingData((Some(username), data)) =
                &self.data.con_state.state
            {
                debug!("Authenticated user: {}", username);

                let mut inner_data = data.clone();
                inner_data.0.truncate(inner_data.0.len() - 2);
                for address in receipts {
                    let mut to: BTreeMap<String, Vec<String>> = BTreeMap::new();
                    let domain = address.split('@').collect::<Vec<&str>>()[1];
                    to.entry(domain.to_string())
                        .or_default()
                        .push(address.clone());
                    let received_header = format!(
                        "Received: from {} ({} [{}])\r\n	by {} (Erooster) with ESMTPS\r\n	id 00000001\r\n	(envelope-from <{}>)\r\n	for <{}>; {}\r\n",
                        self.data.con_state.ehlo.as_ref().context("Missing ehlo")?,
                        self.data.con_state.ehlo.as_ref().context("Missing ehlo")?,
                        self.data.con_state.peer_addr,
                        config.mail.hostname,
                        self.data.con_state.sender.as_ref().context("Missing sender")?,
                        address,
                        OffsetDateTime::now_utc().format(&date_format)?
                    );
                    let temp_data = [received_header.as_bytes(), &inner_data.0].concat();
                    let data = from_utf8(&temp_data)?;

                    let data_owned: String;
                    let data = if let Some(rspamd_config) = &config.rspamd {
                        match self
                            .call_rspamd(
                                rspamd_config,
                                data,
                                self.data.con_state.ehlo.as_ref().context("Missing ehlo")?,
                                &self.data.con_state.peer_addr,
                                self.data
                                    .con_state
                                    .sender
                                    .as_ref()
                                    .context("Missing sender")?,
                                address,
                                Some(username.clone()),
                            )
                            .await?
                        {
                            RspamdDecision::Accept(modified) => {
                                data_owned = modified;
                                data_owned.as_str()
                            }
                            RspamdDecision::PermReject => {
                                lines
                                    .send(String::from("550 5.7.1 Message cannot be accepted."))
                                    .await?;
                                self.data.con_state.receipts = None;
                                self.data.con_state.sender = None;
                                self.data.con_state.state = State::Authenticated(username.clone());
                                return Ok(());
                            }
                            RspamdDecision::TempReject => {
                                lines
                                    .send(String::from(
                                        "451 4.7.1 Service temporarily unavailable, please try again later.",
                                    ))
                                    .await?;
                                self.data.con_state.receipts = None;
                                self.data.con_state.sender = None;
                                self.data.con_state.state = State::Authenticated(username.clone());
                                return Ok(());
                            }
                        }
                    } else {
                        data
                    };

                    if domain == config.mail.hostname {
                        // Local recipient — write directly to their maildir.
                        let folder = "INBOX".to_string();
                        let mailbox_path = Path::new(&config.mail.maildir_folders)
                            .join(address.clone())
                            .join(folder.clone());
                        let db_foldername = format!("{address}/{folder}");
                        let is_new = !mailbox_path.exists();
                        storage.create_dirs(&mailbox_path)?;
                        if is_new {
                            storage.add_flag(&mailbox_path, "\\Subscribed").await?;
                            storage.add_flag(&mailbox_path, "\\NoInferiors").await?;
                        }
                        let signed_data = match dkim_sign(
                            &config.mail.hostname,
                            data,
                            &config.mail.dkim_key_path,
                            &config.mail.dkim_key_selector,
                        ) {
                            Ok(s) => s,
                            Err(e) => {
                                tracing::warn!("DKIM signing failed for local delivery: {e}");
                                data.to_string()
                            }
                        };
                        let message_id = storage
                            .store_new(db_foldername, &mailbox_path, signed_data.as_bytes(), None)
                            .await?;
                        debug!("Stored local message: {}", message_id);
                        // Record in audit queue so postmasters can inspect local deliveries.
                        let audit_id = uuid::Uuid::new_v4();
                        let from = self
                            .data
                            .con_state
                            .sender
                            .clone()
                            .context("Missing sender in internal state")?;
                        let audit_payload = serde_json::json!({
                            "id": audit_id,
                            "to": { domain: [address] },
                            "from": from,
                            "local_delivery": true,
                        });
                        queue::push_local(
                            database.get_pool(),
                            audit_id,
                            audit_payload.to_string(),
                            &from,
                            &address,
                        )
                        .await?;
                    } else {
                        // Remote recipient — queue for outbound SMTP delivery.
                        let email_id = uuid::Uuid::new_v4();
                        let from = self
                            .data
                            .con_state
                            .sender
                            .clone()
                            .context("Missing sender in internal state")?;
                        let to_addrs_str = to
                            .values()
                            .flatten()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ");
                        let email_payload = EmailPayload {
                            id: email_id,
                            to,
                            from: from.clone(),
                            body: data.to_string(),
                            sender_domain: config.mail.hostname.clone(),
                            dkim_key_path: config.mail.dkim_key_path.clone(),
                            dkim_key_selector: config.mail.dkim_key_selector.clone(),
                            require_tls: self.data.con_state.require_tls,
                        };
                        let payload_json = serde_json::to_string(&email_payload)?;
                        queue::push(
                            database.get_pool(),
                            email_id,
                            payload_json,
                            &from,
                            &to_addrs_str,
                        )
                        .await?;
                        debug!("Email queued for outbound sending");
                    }
                }

                lines
                    .send(String::from("250 2.6.0 Message accepted"))
                    .await?;

                // RFC 5321: reset envelope state after DATA so the client
                // can send another message in this session without re-AUTH.
                self.data.con_state.receipts = None;
                self.data.con_state.sender = None;

                State::Authenticated(username.clone())
            } else if let State::ReceivingData((None, data)) = &self.data.con_state.state {
                debug!("No authenticated user");
                for receipt in receipts {
                    let folder = "INBOX".to_string();
                    let mailbox_path = Path::new(&config.mail.maildir_folders)
                        .join(receipt.clone())
                        .join(folder.clone());
                    let db_foldername = format!("{receipt}/{folder}");
                    let is_new = !mailbox_path.exists();
                    storage.create_dirs(&mailbox_path)?;
                    if is_new {
                        storage.add_flag(&mailbox_path, "\\Subscribed").await?;
                        storage.add_flag(&mailbox_path, "\\NoInferiors").await?;
                    }
                    let received_header = format!(
                        "Received: from {} ({} [{}])\r\n	by {} (Erooster) with ESMTPS\r\n	id 00000001\r\n	for <{}>; {}\r\n",
                        self.data.con_state.ehlo.as_ref().context("Missing ehlo")?,
                        self.data.con_state.ehlo.as_ref().context("Missing ehlo")?,
                        self.data.con_state.peer_addr,
                        config.mail.hostname,
                        receipt,
                        OffsetDateTime::now_utc().format(&date_format)?,
                    );
                    let temp_data = [received_header.as_bytes(), &data.0].concat();
                    let data = from_utf8(&temp_data)?;

                    let data_owned: String;
                    let data = if let Some(rspamd_config) = &config.rspamd {
                        match self
                            .call_rspamd(
                                rspamd_config,
                                data,
                                self.data.con_state.ehlo.as_ref().context("Missing ehlo")?,
                                &self.data.con_state.peer_addr,
                                self.data
                                    .con_state
                                    .sender
                                    .as_ref()
                                    .context("Missing sender")?,
                                receipt,
                                None,
                            )
                            .await?
                        {
                            RspamdDecision::Accept(modified) => {
                                data_owned = modified;
                                data_owned.as_str()
                            }
                            RspamdDecision::PermReject => {
                                lines
                                    .send(String::from("550 5.7.1 Message rejected by spam filter"))
                                    .await?;
                                self.data.con_state.receipts = None;
                                self.data.con_state.sender = None;
                                self.data.con_state.state = State::NotAuthenticated;
                                return Ok(());
                            }
                            RspamdDecision::TempReject => {
                                lines
                                    .send(String::from(
                                        "451 4.7.1 Spam check inconclusive, please try again later",
                                    ))
                                    .await?;
                                self.data.con_state.receipts = None;
                                self.data.con_state.sender = None;
                                self.data.con_state.state = State::NotAuthenticated;
                                return Ok(());
                            }
                        }
                    } else {
                        data
                    };

                    // Create a resolver using System DNS
                    let resolver = MessageAuthenticator::new_system_conf()?;
                    // Parse message
                    let authenticated_message = AuthenticatedMessage::parse(data.as_bytes())
                        .context("Failed to parse email")?;

                    // Validate signature
                    let dkim_result = resolver.verify_dkim(&authenticated_message).await;

                    // Summarise to a single status string for logging and DB storage.
                    // Priority: pass > temp_error > fail > neutral > none.
                    let dkim_status = {
                        let mut status = "none";
                        for r in &dkim_result {
                            match r.result() {
                                DkimResult::Pass => {
                                    status = "pass";
                                    break;
                                }
                                DkimResult::TempError(_) if status != "pass" => {
                                    status = "temp_error";
                                }
                                DkimResult::PermError(_) | DkimResult::Fail(_)
                                    if !matches!(status, "pass" | "temp_error") =>
                                {
                                    status = "fail";
                                }
                                DkimResult::Neutral(_) if status == "none" => {
                                    status = "neutral";
                                }
                                _ => {}
                            }
                        }
                        status
                    };
                    if !matches!(dkim_status, "pass" | "none") {
                        warn!("Incoming message DKIM status: {dkim_status}");
                    }

                    // Handle fail — reject in production, skip in benchmarking mode.
                    // TODO: generate reports
                    cfg_if! {
                        if #[cfg(feature = "benchmarking")] {} else {
                            for s in &dkim_result {
                                match s.result() {
                                    DkimResult::Pass | DkimResult::None => break,
                                    DkimResult::Neutral(_) => {
                                        // neutral: logged above, allow through
                                    },
                                    DkimResult::Fail(e) | DkimResult::PermError(e) => {
                                        warn!("Message rejected - no passing DKIM signatures: {}", e);
                                        lines
                                            .send(String::from(
                                                "550 5.7.20 No passing DKIM signatures found.\r\n",
                                            ))
                                            .await?;
                                        return Ok(());
                                    },
                                    DkimResult::TempError(e) => {
                                        warn!("Message rejected - DKIM temp error: {}", e);
                                        lines
                                            .send(String::from(
                                                "451 4.7.20 No passing DKIM signatures found.\r\n",
                                            ))
                                            .await?;
                                        return Ok(());
                                    },
                                }
                            }

                            // Verify DMARC
                            let sender_str = self.data.con_state
                                .sender
                                .as_ref()
                                .context("Missing a MAIL-FROM sender")?;
                            let sender_domain = sender_str.rsplit_once('@').map_or(sender_str.as_str(), |(_, d)| d);
                            let dmarc_result = resolver
                                .verify_dmarc(DmarcParameters {
                                    message: &authenticated_message,
                                    dkim_output: &dkim_result,
                                    rfc5321_mail_from_domain: sender_domain,
                                    spf_output: self.data.con_state
                                        .spf_result
                                        .as_ref()
                                        .context("Missing an SPF result")?,
                                    domain_suffix_fn: |domain| domain,
                                })
                                .await;

                            // These should pass at this point
                            if matches!(dmarc_result.dkim_result(), &DmarcResult::Fail(_))
                                || matches!(dmarc_result.spf_result(), &DmarcResult::Fail(_))
                            {
                                warn!("Message was rejected due to DMARC policy.");
                                lines
                                    .send(String::from(
                                        "550 5.7.1 Email rejected per DMARC policy.\r\n",
                                    ))
                                    .await?;
                                return Ok(());
                            } else if matches!(dmarc_result.dkim_result(), &DmarcResult::TempError(_))
                                || matches!(dmarc_result.spf_result(), &DmarcResult::TempError(_))
                            {
                                warn!("Message was rejected due to DMARC policy.");
                                lines
                                    .send(String::from(
                                        "451 4.7.1 Email temporarily rejected per DMARC policy.\r\n",
                                    ))
                                    .await?;
                                return Ok(());
                            }
                        }
                    }

                    let message_id = storage
                        .store_new(
                            db_foldername,
                            &mailbox_path,
                            data.as_bytes(),
                            Some(dkim_status.to_string()),
                        )
                        .await?;
                    debug!("Stored message: {}", message_id);
                }
                lines
                    .send(String::from("250 2.6.0 Message accepted"))
                    .await?;
                self.data.con_state.receipts = None;
                self.data.con_state.sender = None;
                State::NotAuthenticated
            } else {
                self.data.con_state.state = State::NotAuthenticated;
                lines
                    .send(String::from("250 2.6.0 Message accepted"))
                    .await?;
                color_eyre::eyre::bail!("Invalid state");
            };
        } else if let State::ReceivingData((_, data)) = &mut self.data.con_state.state {
            write!(data.0, "{line}\r\n")?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn call_rspamd(
        &self,
        rspamd_config: &Rspamd,
        data: &str,
        ehlo: &str,
        ip: &str,
        sender: &str,
        rcpt: &str,
        username: Option<String>,
    ) -> color_eyre::Result<RspamdDecision> {
        let client = reqwest::Client::builder()
            .hickory_dns(true)
            .timeout(Duration::from_secs(30))
            .user_agent("Erooster")
            .build()?;
        let base_req = client
            .post(format!("{}/checkv2", rspamd_config.address))
            .body(data.to_string())
            .header("From", sender)
            .header("HELO", ehlo)
            .header("RCPT", rcpt);
        let req = if let Some(username) = username {
            base_req.header("User", username)
        } else {
            base_req.header("IP", ip)
        };
        let rspamd_res = req.send().await?.json::<Response>().await?;
        debug!("{:?}", rspamd_res);

        let decision = match rspamd_res.action {
            Action::Reject => RspamdDecision::PermReject,
            Action::SoftReject | Action::Greylist => RspamdDecision::TempReject,
            Action::AddHeader | Action::RewriteSubject => {
                // X-Spam-Flag is the SpamAssassin-originated header that mail clients
                // and sieve filters universally recognise as the spam marker.
                let modified = format!("X-Spam-Flag: YES\r\n{data}");
                RspamdDecision::Accept(modified)
            }
            Action::NoAction => RspamdDecision::Accept(data.to_string()),
        };
        Ok(decision)
    }
}
