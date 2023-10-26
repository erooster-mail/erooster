use crate::{
    commands::Data,
    servers::{sending::EmailPayload, state::Data as StateData, state::State},
    utils::rspamd::Response,
};
use erooster_core::{
    backend::storage::{MailStorage, Storage},
    config::{Config, Rspamd},
};
use erooster_deps::{
    cfg_if::cfg_if,
    color_eyre::{self, eyre::ContextCompat},
    futures::{Sink, SinkExt},
    mail_auth::{AuthenticatedMessage, DkimResult, DmarcResult, Resolver},
    reqwest, serde_json,
    simdutf8::compat::from_utf8,
    tracing::{self, debug, instrument},
    uuid,
    yaque::Sender,
};
use std::{collections::BTreeMap, io::Write, path::Path, time::Duration};
use time::{macros::format_description, OffsetDateTime};

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

    #[instrument(skip(self, config, lines, line, storage))]
    pub async fn receive<S, E>(
        &mut self,
        config: &Config,
        lines: &mut S,
        line: &str,
        storage: &Storage,
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

                    let data = if let Some(rspamd_config) = &config.rspamd {
                        self.call_rspamd(
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
                            Some(username.to_string()),
                        )
                        .await?
                    } else {
                        data
                    };

                    let email_payload = EmailPayload {
                        id: uuid::Uuid::new_v4(),
                        to,
                        from: self
                            .data
                            .con_state
                            .sender
                            .clone()
                            .context("Missing sender in internal state")?,
                        body: data.to_string(),
                        sender_domain: config.mail.hostname.clone(),
                        dkim_key_path: config.mail.dkim_key_path.clone(),
                        dkim_key_selector: config.mail.dkim_key_selector.clone(),
                    };
                    let payload_as_json_bytes = serde_json::to_vec(&email_payload)?;
                    Sender::open(config.task_folder.clone())?
                        .send(&payload_as_json_bytes)
                        .await?;

                    debug!("Email added to queue");
                }

                lines
                    .send(String::from("250 2.6.0 Message accepted"))
                    .await?;

                State::Authenticated(username.clone())
            } else if let State::ReceivingData((None, data)) = &self.data.con_state.state {
                debug!("No authenticated user");
                for receipt in receipts {
                    let folder = "INBOX".to_string();
                    let mailbox_path = Path::new(&config.mail.maildir_folders)
                        .join(receipt.clone())
                        .join(folder.clone());
                    let db_foldername = format!("{}/{}", receipt, folder,);
                    if !mailbox_path.exists() {
                        storage.create_dirs(&mailbox_path)?;
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

                    let data = if let Some(rspamd_config) = &config.rspamd {
                        self.call_rspamd(
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
                    } else {
                        data
                    };

                    // Create a resolver using Quad9 DNS
                    let resolver = Resolver::new_quad9_tls()?;
                    // Parse message
                    let authenticated_message = AuthenticatedMessage::parse(data.as_bytes())
                        .context("Failed to parse email")?;

                    // Validate signature
                    let dkim_result = resolver.verify_dkim(&authenticated_message).await;

                    // Handle fail
                    // TODO: generate reports
                    cfg_if! {
                        if #[cfg(feature = "benchmarking")] {} else {
                            if !dkim_result
                                .iter()
                                .any(|s| matches!(s.result(), &DkimResult::Pass))
                            {
                                // 'Strict' mode violates the advice of Section 6.1 of RFC6376
                                if dkim_result
                                    .iter()
                                    .any(|d| matches!(d.result(), DkimResult::TempError(_)))
                                {
                                    lines
                                        .send(String::from(
                                            "451 4.7.20 No passing DKIM signatures found.\r\n",
                                        ))
                                        .await?;
                                } else {
                                    lines
                                        .send(String::from(
                                            "550 5.7.20 No passing DKIM signatures found.\r\n",
                                        ))
                                        .await?;
                                };
                                return Ok(());
                            }

                            // Verify DMARC
                            let dmarc_result = resolver
                                .verify_dmarc(
                                    &authenticated_message,
                                    &dkim_result,
                                    self.data.con_state
                                        .sender
                                        .as_ref()
                                        .context("Missing a MAIL-FROM sender")?,
                                    self.data.con_state
                                        .spf_result
                                        .as_ref()
                                        .context("Missing an SPF result")?,
                                )
                                .await;

                            // These should pass at this point
                            if matches!(dmarc_result.dkim_result(), &DmarcResult::Fail(_))
                                || matches!(dmarc_result.spf_result(), &DmarcResult::Fail(_))
                            {
                                lines
                                    .send(String::from(
                                        "550 5.7.1 Email rejected per DMARC policy.\r\n",
                                    ))
                                    .await?;
                                return Ok(());
                            } else if matches!(dmarc_result.dkim_result(), &DmarcResult::TempError(_))
                                || matches!(dmarc_result.spf_result(), &DmarcResult::TempError(_))
                            {
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
                        .store_new(db_foldername, &mailbox_path, data.as_bytes())
                        .await?;
                    debug!("Stored message: {}", message_id);
                }
                // TODO: cleanup after we are done
                lines
                    .send(String::from("250 2.6.0 Message accepted"))
                    .await?;
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
        };
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn call_rspamd<'a>(
        &self,
        rspamd_config: &Rspamd,
        data: &'a str,
        ehlo: &str,
        ip: &str,
        sender: &str,
        rcpt: &str,
        username: Option<String>,
    ) -> color_eyre::Result<&'a str> {
        let client = reqwest::Client::builder()
            .trust_dns(true)
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
        // TODO apply rspamd actions

        Ok(data)
    }
}
