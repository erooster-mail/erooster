use std::{path::Path, sync::Arc};

use futures::{channel::mpsc::SendError, Sink, SinkExt};
use maildir::Maildir;
use tracing::info;

use crate::{
    config::Config, imap_commands::utils::add_flag, smtp_commands::Data, smtp_servers::state::State,
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
            self.data.con_state.write().await.state = State::ReceivingData;
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
                write_lock.state = State::NotAuthenticated;
                for receipt in receipts {
                    let mut folder = "INBOX".to_string();
                    folder.insert(0, '.');
                    folder.remove_matches('"');
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
            }
        };
        Ok(())
    }
}
