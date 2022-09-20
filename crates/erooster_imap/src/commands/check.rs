use std::sync::Arc;

use crate::{
    commands::{CommandData, Data},
    servers::state::State,
};
use erooster_core::backend::storage::{MailEntryType, MailStorage, Storage};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use tracing::instrument;

pub struct Check<'a> {
    pub data: &'a Data,
}

impl Check<'_> {
    #[instrument(skip(self, lines, storage, command_data))]
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        storage: Arc<Storage>,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        // This is an Imap4rev1 feature. It does the same as Noop for us as we have no memory gc.
        // It also only is allowed in selected state
        if let State::Selected(folder, _) = &self.data.con_state.read().await.state {
            let folder = folder.replace('/', ".");
            let username = if let Some(username) = self.data.con_state.read().await.username.clone()
            {
                username
            } else {
                lines
                    .send(format!("{} NO invalid state", command_data.tag))
                    .await?;
                return Ok(());
            };
            let mailbox_path = storage.to_ondisk_path(folder.clone(), username)?;
            let mails: Vec<MailEntryType> = storage.list_new(&mailbox_path).await;
            let got_new = !mails.is_empty();
            if got_new {
                let mails: Vec<MailEntryType> = storage.list_all(&mailbox_path).await;
                lines.send(format!("* {} EXISTS", mails.len())).await?;
            }

            lines
                .send(format!("{} OK CHECK completed", command_data.tag))
                .await?;
        } else {
            lines
                .send(format!("{} NO invalid state", command_data.tag))
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::commands::{CommandData, Commands};
    use crate::state::{Access, Connection};
    use futures::{channel::mpsc, StreamExt};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn test_successful_check() {
        let caps = Check {
            data: &Data {
                con_state: Arc::new(RwLock::new(Connection {
                    state: State::Selected("INBOX".to_string(), Access::ReadWrite),
                    secure: true,
                    username: Some(String::from("test")),
                    active_capabilities: vec![],
                })),
            },
        };
        let cmd_data = CommandData {
            tag: "1",
            command: Commands::Check,
            arguments: &[],
        };
        let config = erooster_core::get_config(String::from("./config.yml"))
            .await
            .unwrap();
        let database = Arc::new(
            erooster_core::backend::database::get_database(Arc::clone(&config))
                .await
                .unwrap(),
        );
        let storage = Arc::new(erooster_core::backend::storage::get_storage(
            database,
            Arc::clone(&config),
        ));
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, storage, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(rx.next().await, Some(String::from("1 OK CHECK completed")));
    }

    #[tokio::test]
    async fn test_unsuccessful_check() {
        let caps = Check {
            data: &Data {
                con_state: Arc::new(RwLock::new(Connection {
                    state: State::NotAuthenticated,
                    secure: true,
                    username: None,
                    active_capabilities: vec![],
                })),
            },
        };
        let cmd_data = CommandData {
            tag: "1",
            command: Commands::Check,
            arguments: &[],
        };
        let config = erooster_core::get_config(String::from("./config.yml"))
            .await
            .unwrap();
        let database = Arc::new(
            erooster_core::backend::database::get_database(Arc::clone(&config))
                .await
                .unwrap(),
        );
        let storage = Arc::new(erooster_core::backend::storage::get_storage(
            database,
            Arc::clone(&config),
        ));
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, storage, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(rx.next().await, Some(String::from("1 NO invalid state")));
    }
}
