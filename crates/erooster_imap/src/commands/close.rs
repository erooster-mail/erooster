use crate::{
    commands::{CommandData, Data},
    servers::state::{Access, State},
};
use erooster_core::{
    backend::storage::{MailEntry, MailStorage, Storage},
    config::Config,
};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::{path::Path, sync::Arc};
use tokio::fs;
use tracing::{debug, instrument};

pub struct Close<'a> {
    pub data: &'a Data,
}

impl Close<'_> {
    #[instrument(skip(self, lines, storage, config, command_data))]
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        storage: Arc<Storage>,
        config: Arc<Config>,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        let mut write_lock = self.data.con_state.write().await;

        if let State::Selected(folder, access) = &write_lock.state {
            if access == &Access::ReadOnly {
                lines
                    .send(format!("{} NO in read-only mode", command_data.tag))
                    .await?;
                return Ok(());
            }

            let mut folder = folder.replace('/', ".");
            folder.insert(0, '.');
            let mailbox_path = Path::new(&config.mail.maildir_folders)
                .join(write_lock.username.clone().unwrap())
                .join(folder.clone());

            // We need to check all messages it seems?
            let mails = storage
                .list_cur(
                    mailbox_path
                        .clone()
                        .into_os_string()
                        .into_string()
                        .expect("Failed to convert path. Your system may be incompatible"),
                )
                .await
                .into_iter()
                .chain(
                    storage
                        .list_new(
                            mailbox_path
                                .into_os_string()
                                .into_string()
                                .expect("Failed to convert path. Your system may be incompatible"),
                        )
                        .await,
                );
            for mail in mails {
                debug!("Checking mails");
                if mail.is_trashed() {
                    let path = mail.path();
                    fs::remove_file(path).await?;
                }
            }

            {
                write_lock.state = State::Authenticated;
            };
            lines
                .send(format!("{} OK CLOSE completed", command_data.tag))
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
mod tests {
    use super::*;
    use crate::commands::{CommandData, Commands};
    use crate::servers::state::{Access, Connection};
    use futures::{channel::mpsc, StreamExt};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn test_select_rw() {
        let caps = Close {
            data: &Data {
                con_state: Arc::new(RwLock::new(Connection {
                    state: State::Selected("INBOX".to_string(), Access::ReadWrite),
                    secure: true,
                    username: Some(String::from("test")),
                })),
            },
        };
        let cmd_data = CommandData {
            tag: "1",
            command: Commands::Close,
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
        let storage = Arc::new(erooster_core::backend::storage::get_storage(database));
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, storage, config, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(rx.next().await, Some(String::from("1 OK CLOSE completed")));
    }

    #[tokio::test]
    async fn test_selected_ro() {
        let caps = Close {
            data: &Data {
                con_state: Arc::new(RwLock::new(Connection {
                    state: State::Selected("INBOX".to_string(), Access::ReadOnly),
                    secure: true,
                    username: Some(String::from("test")),
                })),
            },
        };
        let cmd_data = CommandData {
            tag: "1",
            command: Commands::Close,
            arguments: &[],
        };
        let (mut tx, mut rx) = mpsc::unbounded();
        let config = erooster_core::get_config(String::from("./config.yml"))
            .await
            .unwrap();
        let database = Arc::new(
            erooster_core::backend::database::get_database(Arc::clone(&config))
                .await
                .unwrap(),
        );
        let storage = Arc::new(erooster_core::backend::storage::get_storage(database));
        let res = caps.exec(&mut tx, storage, config, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(
            rx.next().await,
            Some(String::from("1 NO in read-only mode"))
        );
    }

    #[tokio::test]
    async fn test_no_auth() {
        let caps = Close {
            data: &Data {
                con_state: Arc::new(RwLock::new(Connection {
                    state: State::NotAuthenticated,
                    secure: true,
                    username: None,
                })),
            },
        };
        let cmd_data = CommandData {
            tag: "1",
            command: Commands::Close,
            arguments: &[],
        };
        let (mut tx, mut rx) = mpsc::unbounded();
        let config = erooster_core::get_config(String::from("./config.yml"))
            .await
            .unwrap();
        let database = Arc::new(
            erooster_core::backend::database::get_database(Arc::clone(&config))
                .await
                .unwrap(),
        );
        let storage = Arc::new(erooster_core::backend::storage::get_storage(database));
        let res = caps.exec(&mut tx, storage, config, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(rx.next().await, Some(String::from("1 NO invalid state")));
    }
}
