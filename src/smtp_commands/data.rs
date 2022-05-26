use std::{collections::HashMap, path::Path, sync::Arc};

use futures::{channel::mpsc::SendError, Sink, SinkExt};
use maildir::Maildir;
use tracing::info;

use crate::{
    config::Config,
    database::{Database, DB},
    imap_commands::utils::add_flag,
    smtp_commands::Data,
    smtp_servers::{send_email_job, state::State, EmailPayload},
};

#[allow(clippy::module_name_repetitions)]
pub struct DataCommand<'a> {
    pub data: &'a Data,
}

impl DataCommand<'_> {
    pub async fn exec<S>(&self, lines: &mut S) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        info!("Waiting for incoming data");
        {
            let write_lock = self.data.con_state.write().await;
            let username = if let State::Authenticated(username) = &write_lock.state {
                Some(username.clone())
            } else {
                None
            };
            self.data.con_state.write().await.state = State::ReceivingData(username);
        };
        lines
            .send(String::from("354 Start mail input; end with <CRLF>.<CRLF>"))
            .await?;
        Ok(())
    }

    pub async fn receive<S>(
        &self,
        config: Arc<Config>,
        lines: &mut S,
        line: &str,
        database: DB,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        {
            let mut write_lock = self.data.con_state.write().await;
            if let Some(data) = &write_lock.data {
                write_lock.data = Some(format!("{}\r\n{}", data, line));
            } else {
                write_lock.data = Some(line.to_string());
            }
            if line == "." {
                let receipts = if let Some(receipts) = write_lock.receipts.clone() {
                    receipts
                } else {
                    color_eyre::eyre::bail!("No receipts")
                };
                write_lock.state = if let State::ReceivingData(Some(username)) = &write_lock.state {
                    for receipt in receipts {
                        let folder = "INBOX".to_string();
                        let mailbox_path = Path::new(&config.mail.maildir_folders)
                            .join(receipt)
                            .join(folder.clone());
                        let maildir = Maildir::from(mailbox_path.clone());
                        if !mailbox_path.exists() {
                            maildir.create_dirs()?;
                            add_flag(&mailbox_path, "\\Subscribed")?;
                            add_flag(&mailbox_path, "\\NoInferiors")?;
                        }
                        let data = if let Some(data) = write_lock.data.clone() {
                            data
                        } else {
                            color_eyre::eyre::bail!("No data")
                        };
                        let message_id = maildir.store_new(data.as_bytes())?;
                        info!("Stored message: {}", message_id);
                        // TODO cleanup after we are done
                        lines.send(String::from("250 OK")).await?;
                    }
                    State::Authenticated(username.clone())
                } else if matches!(write_lock.state, State::ReceivingData(None)) {
                    let data = if let Some(data) = write_lock.data.clone() {
                        data
                    } else {
                        color_eyre::eyre::bail!("No data")
                    };
                    let mut to = HashMap::new();
                    for address in receipts {
                        let domain = address.split('@').collect::<Vec<&str>>()[1];
                        to.entry(domain.to_string())
                            .or_insert(Vec::new())
                            .push(address);
                    }
                    let email_payload = EmailPayload {
                        to,
                        from: write_lock.sender.clone().unwrap(),
                        body: data,
                        sender_domain: config.mail.hostname.clone(),
                    };
                    let pool = database.get_pool();
                    send_email_job
                        .builder()
                        .set_json(&email_payload)?
                        .spawn(pool)
                        .await?;
                    lines.send(String::from("250 OK")).await?;
                    State::NotAuthenticated
                } else {
                    write_lock.state = State::NotAuthenticated;
                    lines.send(String::from("250 OK")).await?;
                    color_eyre::eyre::bail!("Invalid state");
                };
            }
        };
        Ok(())
    }
}
