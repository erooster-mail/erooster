use crate::{
    commands::{CommandData, Data},
    servers::state::{Access, State},
};
use erooster_core::backend::storage::{MailEntry, MailStorage, Storage};
use erooster_deps::{
    color_eyre::{self, eyre::ContextCompat},
    futures::{Sink, SinkExt},
    tokio::fs,
    tracing::{self, debug, instrument},
};

pub struct Close<'a> {
    pub data: &'a mut Data,
}

impl Close<'_> {
    #[instrument(skip(self, lines, storage, command_data))]
    pub async fn exec<S, E>(
        &mut self,
        lines: &mut S,
        storage: &Storage,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        if let State::Selected(folder, access) = &self.data.con_state.state {
            if access == &Access::ReadOnly {
                lines
                    .send(format!("{} NO in read-only mode", command_data.tag))
                    .await?;
                return Ok(());
            }

            let username = self
                .data
                .con_state
                .username
                .clone()
                .context("Username missing in internal State")?;
            let mailbox_path = storage.to_ondisk_path(folder.clone(), username.clone())?;

            // We need to check all messages it seems?
            let mails = storage
                .list_cur(format!("{username}/{folder}"), &mailbox_path)
                .await
                .into_iter()
                .chain(
                    storage
                        .list_new(format!("{username}/{folder}"), &mailbox_path)
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
                self.data.con_state.state = State::Authenticated;
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
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::commands::{CommandData, Commands};
    use crate::servers::state::{Access, Connection};
    use erooster_deps::futures::{channel::mpsc, StreamExt};
    use erooster_deps::tokio;

    #[tokio::test]
    async fn test_select_rw() {
        let mut caps = Close {
            data: &mut Data {
                con_state: Connection {
                    state: State::Selected("INBOX".to_string(), Access::ReadWrite),
                    secure: true,
                    username: Some(String::from("test")),
                    active_capabilities: vec![],
                },
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
        let database = erooster_core::backend::database::get_database(&config)
            .await
            .unwrap();
        let storage = erooster_core::backend::storage::get_storage(database, config);
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(rx.next().await, Some(String::from("1 OK CLOSE completed")));
    }

    #[tokio::test]
    async fn test_selected_ro() {
        let mut caps = Close {
            data: &mut Data {
                con_state: Connection {
                    state: State::Selected("INBOX".to_string(), Access::ReadOnly),
                    secure: true,
                    username: Some(String::from("test")),
                    active_capabilities: vec![],
                },
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
        let database = erooster_core::backend::database::get_database(&config)
            .await
            .unwrap();
        let storage = erooster_core::backend::storage::get_storage(database, config);
        let res = caps.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(
            rx.next().await,
            Some(String::from("1 NO in read-only mode"))
        );
    }

    #[tokio::test]
    async fn test_no_auth() {
        let mut caps = Close {
            data: &mut Data {
                con_state: Connection {
                    state: State::NotAuthenticated,
                    secure: true,
                    username: None,
                    active_capabilities: vec![],
                },
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
        let database = erooster_core::backend::database::get_database(&config)
            .await
            .unwrap();
        let storage = erooster_core::backend::storage::get_storage(database, config);
        let res = caps.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(rx.next().await, Some(String::from("1 NO invalid state")));
    }
}
