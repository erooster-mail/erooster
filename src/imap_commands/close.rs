use crate::{
    backend::storage::{MailEntry, MailStorage, Storage},
    config::Config,
    imap_commands::{CommandData, Data},
    imap_servers::state::{Access, State},
};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::{path::Path, sync::Arc};
use tokio::fs;
use tracing::debug;

pub struct Close<'a> {
    pub data: &'a Data,
}

impl Close<'_> {
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
                .join(self.data.con_state.read().await.username.clone().unwrap())
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
                .into_iter()
                .chain(
                    storage.list_new(
                        mailbox_path
                            .into_os_string()
                            .into_string()
                            .expect("Failed to convert path. Your system may be incompatible"),
                    ),
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
    use crate::imap_commands::{CommandData, Commands};
    use crate::imap_servers::state::{Access, Connection};
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
                    // TODO this may be invalid actuallly
                    username: None,
                })),
            },
        };
        let cmd_data = CommandData {
            tag: "1",
            command: Commands::Close,
            arguments: &[],
        };
        let config = Arc::new(crate::get_config("./config.yml"));
        let database = Arc::new(get_database(Arc::clone(&config)).await?);
        let storage = Arc::new(crate::backend::storage::get_storage(database));
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
                    // TODO this may be invalid actuallly
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
        let config = Arc::new(crate::get_config("./config.yml"));
        let database = Arc::new(get_database(Arc::clone(&config)).await?);
        let storage = Arc::new(crate::backend::storage::get_storage(database));
        let res = caps.exec(&mut tx, storage, config, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(
            rx.next().await,
            Some(String::from("1 NO in read-only mode"))
        );
    }

    #[tokio::test]
    async fn test_invalid_state() {
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
        let config = Arc::new(crate::get_config("./config.yml"));
        let database = Arc::new(get_database(Arc::clone(&config)).await?);
        let storage = Arc::new(crate::backend::storage::get_storage(database));
        let res = caps.exec(&mut tx, storage, config, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(rx.next().await, Some(String::from("1 NO invalid state")));
    }
}
