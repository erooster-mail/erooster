use crate::{
    commands::Data,
    servers::{
        sending::{send_email_job, EmailPayload},
        state::State,
    },
};
use erooster_core::{
    backend::{
        database::{Database, DB},
        storage::{MailStorage, Storage},
    },
    config::Config,
};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::{collections::HashMap, path::Path, sync::Arc};
use tracing::{debug, instrument};

#[allow(clippy::module_name_repetitions)]
pub struct DataCommand<'a> {
    pub data: &'a Data,
}

impl DataCommand<'_> {
    #[instrument(skip(self, lines))]
    pub async fn exec<S>(&self, lines: &mut S) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        debug!("Waiting for incoming data");
        {
            let mut write_lock = self.data.con_state.write().await;
            let username = if let State::Authenticated(username) = &write_lock.state {
                Some(username.clone())
            } else {
                None
            };
            write_lock.state = State::ReceivingData(username);
        };
        lines
            .send(String::from("354 Start mail input; end with <CRLF>.<CRLF>"))
            .await?;
        Ok(())
    }

    #[instrument(skip(self, config, lines, line, database, storage))]
    pub async fn receive<S>(
        &self,
        config: Arc<Config>,
        lines: &mut S,
        line: &str,
        database: DB,
        storage: Arc<Storage>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        debug!("Reading incoming data");
        {
            let mut write_lock = self.data.con_state.write().await;
            if line == "." {
                debug!("Got end of line");
                let receipts = if let Some(receipts) = write_lock.receipts.clone() {
                    receipts
                } else {
                    color_eyre::eyre::bail!("No receipts")
                };
                write_lock.state = if let State::ReceivingData(Some(username)) = &write_lock.state {
                    debug!("Authenticated user: {}", username);

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
                    debug!("Email added to queue");
                    lines.send(String::from("250 OK")).await?;

                    State::Authenticated(username.clone())
                } else if matches!(write_lock.state, State::ReceivingData(None)) {
                    debug!("No authenticated user");
                    for receipt in receipts {
                        let folder = "INBOX".to_string();
                        let mailbox_path = Path::new(&config.mail.maildir_folders)
                            .join(receipt)
                            .join(folder.clone());
                        if !mailbox_path.exists() {
                            storage.create_dirs(
                                mailbox_path.clone().into_os_string().into_string().expect(
                                    "Failed to convert path. Your system may be incompatible",
                                ),
                            )?;
                            storage.add_flag(&mailbox_path, "\\Subscribed").await?;
                            storage.add_flag(&mailbox_path, "\\NoInferiors").await?;
                        }
                        let data = if let Some(data) = write_lock.data.clone() {
                            data
                        } else {
                            color_eyre::eyre::bail!("No data")
                        };
                        let message_id = storage
                            .store_new(
                                mailbox_path.clone().into_os_string().into_string().expect(
                                    "Failed to convert path. Your system may be incompatible",
                                ),
                                data.as_bytes(),
                            )
                            .await?;
                        debug!("Stored message: {}", message_id);
                    }
                    // TODO cleanup after we are done
                    lines.send(String::from("250 OK")).await?;
                    State::NotAuthenticated
                } else {
                    write_lock.state = State::NotAuthenticated;
                    lines.send(String::from("250 OK")).await?;
                    color_eyre::eyre::bail!("Invalid state");
                };
            } else if let Some(data) = &write_lock.data {
                write_lock.data = Some(format!("{}\r\n{}", data, line));
            } else {
                write_lock.data = Some(line.to_string());
            }
        };
        Ok(())
    }
}
