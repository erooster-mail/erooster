use crate::{
    commands::Data,
    servers::{
        sending::{send_email_job, EmailPayload},
        state::State,
    },
    utils::rspamd::Response,
};
use color_eyre::eyre::ContextCompat;
use erooster_core::{
    backend::{
        database::{Database, DB},
        storage::{MailStorage, Storage},
    },
    config::{Config, Rspamd},
};
use futures::{Sink, SinkExt};
use simdutf8::compat::from_utf8;
use std::io::Write;
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};
use time::{macros::format_description, OffsetDateTime};
use tracing::{debug, instrument};

#[allow(clippy::module_name_repetitions)]
pub struct DataCommand<'a> {
    pub data: &'a Data,
}

impl DataCommand<'_> {
    #[instrument(skip(self, lines))]
    pub async fn exec<S, E>(&self, lines: &mut S) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        debug!("Waiting for incoming data");
        {
            let mut write_lock = self.data.con_state.write().await;
            let username = if let State::Authenticated(username) = &write_lock.state {
                Some(username.clone())
            } else {
                None
            };
            write_lock.state = State::ReceivingData((username, Vec::new()));
        };
        lines
            .send(String::from("354 Start mail input; end with <CRLF>.<CRLF>"))
            .await?;
        Ok(())
    }

    #[instrument(skip(self, config, lines, line, database, storage))]
    pub async fn receive<S, E>(
        &self,
        config: Arc<Config>,
        lines: &mut S,
        line: &str,
        database: DB,
        storage: Arc<Storage>,
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
        {
            let write_lock = &mut self.data.con_state.write().await;
            if line == "." {
                debug!("Got end of line");
                let receipts = if let Some(receipts) = &write_lock.receipts {
                    receipts
                } else {
                    color_eyre::eyre::bail!("No receipts")
                };
                write_lock.state = if let State::ReceivingData((Some(username), data)) =
                    &write_lock.state
                {
                    debug!("Authenticated user: {}", username);

                    let mut inner_data = data.clone();
                    inner_data.truncate(inner_data.len() - 2);
                    for address in receipts {
                        let mut to: BTreeMap<String, Vec<String>> = BTreeMap::new();
                        let domain = address.split('@').collect::<Vec<&str>>()[1];
                        to.entry(domain.to_string())
                            .or_default()
                            .push(address.clone());
                        let received_header = format!(
                            "Received: from {} ({} [{}])\r\n	by {} (Erooster) with ESMTPS\r\n	id 00000001\r\n	(envelope-from <{}>)\r\n	for <{}>; {}\r\n",
                            write_lock.ehlo.as_ref().context("Missing ehlo")?,
                            write_lock.ehlo.as_ref().context("Missing ehlo")?,
                            write_lock.peer_addr,
                            config.mail.hostname,
                            write_lock.sender.as_ref().context("Missing sender")?,
                            address,
                            OffsetDateTime::now_utc().format(&date_format)?
                        );
                        let temp_data = [received_header.as_bytes(), &inner_data].concat();
                        let data = from_utf8(&temp_data)?;

                        let data = if let Some(rspamd_config) = &config.rspamd {
                            self.call_rspamd(
                                rspamd_config,
                                data,
                                write_lock.ehlo.as_ref().context("Missing ehlo")?,
                                &write_lock.peer_addr,
                                write_lock.sender.as_ref().context("Missing sender")?,
                                address,
                                Some(username.to_string()),
                            )
                            .await?
                        } else {
                            data
                        };

                        let email_payload = EmailPayload {
                            to,
                            from: write_lock
                                .sender
                                .clone()
                                .context("Missing sender in internal state")?,
                            body: data.to_string(),
                            sender_domain: config.mail.hostname.clone(),
                            dkim_key_path: config.mail.dkim_key_path.clone(),
                            dkim_key_selector: config.mail.dkim_key_selector.clone(),
                        };
                        let pool = database.get_pool();
                        send_email_job
                            .builder()
                            .set_json(&email_payload)?
                            .spawn(pool)
                            .await?;
                        debug!("Email added to queue");
                    }

                    lines
                        .send(String::from("250 2.6.0 Message accepted"))
                        .await?;

                    State::Authenticated(username.clone())
                } else if let State::ReceivingData((None, data)) = &write_lock.state {
                    debug!("No authenticated user");
                    for receipt in receipts {
                        let folder = "INBOX".to_string();
                        let mailbox_path = Path::new(&config.mail.maildir_folders)
                            .join(receipt.clone())
                            .join(folder.clone());
                        if !mailbox_path.exists() {
                            storage.create_dirs(&mailbox_path)?;
                            storage.add_flag(&mailbox_path, "\\Subscribed").await?;
                            storage.add_flag(&mailbox_path, "\\NoInferiors").await?;
                        }
                        let received_header = format!(
                            "Received: from {} ({} [{}])\r\n	by {} (Erooster) with ESMTPS\r\n	id 00000001\r\n	for <{}>; {}\r\n",
                            write_lock.ehlo.as_ref().context("Missing ehlo")?,
                            write_lock.ehlo.as_ref().context("Missing ehlo")?,
                            write_lock.peer_addr,
                            config.mail.hostname,
                            receipt,
                            OffsetDateTime::now_utc().format(&date_format)?,
                        );
                        let temp_data = [received_header.as_bytes(), data].concat();
                        let data = from_utf8(&temp_data)?;

                        let data = if let Some(rspamd_config) = &config.rspamd {
                            self.call_rspamd(
                                rspamd_config,
                                data,
                                write_lock.ehlo.as_ref().context("Missing ehlo")?,
                                &write_lock.peer_addr,
                                write_lock.sender.as_ref().context("Missing sender")?,
                                receipt,
                                None,
                            )
                            .await?
                        } else {
                            data
                        };

                        let message_id = storage.store_new(&mailbox_path, data.as_bytes()).await?;
                        debug!("Stored message: {}", message_id);
                    }
                    // TODO cleanup after we are done
                    lines
                        .send(String::from("250 2.6.0 Message accepted"))
                        .await?;
                    State::NotAuthenticated
                } else {
                    write_lock.state = State::NotAuthenticated;
                    lines
                        .send(String::from("250 2.6.0 Message accepted"))
                        .await?;
                    color_eyre::eyre::bail!("Invalid state");
                };
            } else if let State::ReceivingData((_, data)) = &mut write_lock.state {
                write!(data, "{}\r\n", line)?;
            }
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
            .header("IP", ip)
            .header("RCPT", rcpt);
        let req = if let Some(username) = username {
            base_req.header("User", username)
        } else {
            base_req
        };
        let rspamd_res = req.send().await?.json::<Response>().await?;
        debug!("{:?}", rspamd_res);
        // TODO apply rspamd actions

        Ok(data)
    }
}
